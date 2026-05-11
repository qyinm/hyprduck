use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Context, Result};
use duckdocs_engine_client::{EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    BrainActor, BrainActorType, BrainProposalKind, BrainReadScope, BrainRelationKind,
    GetBrainHealthRequest, GetContextPackRequest, ProposeBrainUpdateRequest, ReadNodeRequest,
    ReadRecentEventsRequest, ReadSourceRequest, ReadWikiPageRequest, SearchBrainRequest,
};
use serde_json::{json, Map, Value};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let client = SubprocessEngineClient::default();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&client, &line) {
            serde_json::to_writer(&mut stdout, &response)
                .context("failed to encode MCP response")?;
            stdout
                .write_all(b"\n")
                .context("failed to write MCP response newline")?;
            stdout.flush().context("failed to flush MCP response")?;
        }
    }

    Ok(())
}

fn handle_message(client: &dyn EngineClient, line: &str) -> Option<Value> {
    let message = match serde_json::from_str::<Value>(line) {
        Ok(message) => message,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("Parse error: {error}"),
            ))
        }
    };

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(error_response(
            message.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "Invalid Request: missing method",
        ));
    };

    let Some(id) = message.get("id").cloned() else {
        return handle_notification(method);
    };

    match method {
        "initialize" => Some(success_response(id, initialize_result(&message))),
        "ping" => Some(success_response(id, json!({}))),
        "tools/list" => Some(success_response(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => Some(handle_tool_call(client, id, message.get("params"))),
        _ => Some(error_response(
            id,
            -32601,
            format!("Method not found: {method}"),
        )),
    }
}

fn handle_notification(method: &str) -> Option<Value> {
    match method {
        "notifications/initialized" | "notifications/cancelled" => None,
        _ => None,
    }
}

fn initialize_result(message: &Value) -> Value {
    let requested_protocol = message
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);

    json!({
        "protocolVersion": requested_protocol,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "hyprduck",
            "title": "HyprDuck Local Brain",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "HyprDuck exposes the local brain through read tools and policy-controlled proposal tools. Use search_brain and get_context_pack first; proposed writes create auditable brain events and never overwrite source truth."
    })
}

fn handle_tool_call(client: &dyn EngineClient, id: Value, params: Option<&Value>) -> Value {
    let params = params.unwrap_or(&Value::Null);
    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => return error_response(id, -32602, "Invalid params: missing tool name"),
    };
    let arguments = match params.get("arguments") {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return error_response(id, -32602, "Invalid params: arguments must be an object")
        }
        None => return error_response(id, -32602, "Invalid params: missing arguments"),
    };

    let result = match call_tool(client, name, arguments) {
        Ok(value) => json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
                }
            ],
            "isError": false
        }),
        Err(error) => json!({
            "content": [
                {
                    "type": "text",
                    "text": error.to_string()
                }
            ],
            "isError": true
        }),
    };

    success_response(id, result)
}

fn call_tool(
    client: &dyn EngineClient,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<Value> {
    let scope = read_scope(arguments)?;

    match name {
        "search_brain" => {
            let query = required_string(arguments, "query")?;
            let limit = optional_usize(arguments, "limit")?;
            Ok(serde_json::to_value(client.search_brain(
                SearchBrainRequest {
                    scope,
                    query,
                    limit,
                },
            )?)?)
        }
        "get_context_pack" => {
            let query = required_string(arguments, "query")?;
            let budget = optional_usize(arguments, "budget")?;
            Ok(serde_json::to_value(client.get_context_pack(
                GetContextPackRequest {
                    scope,
                    query,
                    budget,
                },
            )?)?)
        }
        "read_source" => {
            let source_id = required_string(arguments, "sourceId")?;
            Ok(serde_json::to_value(
                client.read_source(ReadSourceRequest { scope, source_id })?,
            )?)
        }
        "read_wiki_page" => {
            let path = required_string(arguments, "path")?;
            Ok(serde_json::to_value(
                client.read_wiki_page(ReadWikiPageRequest { scope, path })?,
            )?)
        }
        "read_node" => {
            let node_id = required_string(arguments, "nodeId")?;
            Ok(serde_json::to_value(
                client.read_node(ReadNodeRequest { scope, node_id })?,
            )?)
        }
        "read_recent_events" => {
            let limit = optional_usize(arguments, "limit")?;
            Ok(serde_json::to_value(client.read_recent_events(
                ReadRecentEventsRequest { scope, limit },
            )?)?)
        }
        "read_health" => Ok(serde_json::to_value(
            client.get_brain_health(GetBrainHealthRequest { scope })?,
        )?),
        "propose_memory" => propose_update(client, scope, BrainProposalKind::Memory, arguments),
        "propose_claim" => propose_update(client, scope, BrainProposalKind::Claim, arguments),
        "propose_link" => propose_update(client, scope, BrainProposalKind::Link, arguments),
        "append_observation" => {
            propose_update(client, scope, BrainProposalKind::Observation, arguments)
        }
        "add_source_note" => {
            propose_update(client, scope, BrainProposalKind::SourceNote, arguments)
        }
        "request_consolidation" => {
            propose_update(client, scope, BrainProposalKind::Observation, arguments)
        }
        _ => Err(anyhow!("Unknown HyprDuck MCP tool: {name}")),
    }
}

fn propose_update(
    client: &dyn EngineClient,
    scope: BrainReadScope,
    kind: BrainProposalKind,
    arguments: &Map<String, Value>,
) -> Result<Value> {
    let mut target_source_id = optional_string(arguments, "targetSourceId")?;
    if target_source_id.is_none() {
        target_source_id = optional_string(arguments, "sourceId")?;
    }
    let request = ProposeBrainUpdateRequest {
        scope,
        kind,
        title: required_string(arguments, "title")?,
        body: required_string(arguments, "body")?,
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: optional_string(arguments, "actorId")?
                .unwrap_or_else(|| "hyprduck-mcp".into()),
        },
        target_node_id: optional_string(arguments, "targetNodeId")?,
        target_source_id,
        relation_kind: optional_relation_kind(arguments, "relationKind")?,
        source_description: optional_string(arguments, "sourceDescription")?,
        source_user_context: optional_string(arguments, "sourceUserContext")?,
        source_ingest_instruction: optional_string(arguments, "sourceIngestInstruction")?,
        source_refs: optional_string_array(arguments, "sourceRefs")?,
        node_refs: optional_string_array(arguments, "nodeRefs")?,
        evidence_refs: optional_string_array(arguments, "evidenceRefs")?,
    };
    Ok(serde_json::to_value(client.propose_brain_update(request)?)?)
}

fn read_scope(arguments: &Map<String, Value>) -> Result<BrainReadScope> {
    Ok(BrainReadScope {
        workspace_id: optional_string(arguments, "workspaceId")?
            .unwrap_or_else(|| "default".into()),
        root_dir: optional_string(arguments, "rootDir")?,
    })
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String> {
    optional_string(arguments, name)?.ok_or_else(|| anyhow!("missing required argument: {name}"))
}

fn optional_string(arguments: &Map<String, Value>, name: &str) -> Result<Option<String>> {
    match arguments.get(name) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(Value::String(_)) => Err(anyhow!("argument {name} cannot be empty")),
        Some(_) => Err(anyhow!("argument {name} must be a string")),
        None => Ok(None),
    }
}

fn optional_usize(arguments: &Map<String, Value>, name: &str) -> Result<Option<usize>> {
    match arguments.get(name) {
        Some(Value::Number(value)) => value
            .as_u64()
            .map(|value| Some(value as usize))
            .ok_or_else(|| anyhow!("argument {name} must be a positive integer")),
        Some(Value::String(value)) => value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| anyhow!("argument {name} must be a positive integer")),
        Some(_) => Err(anyhow!("argument {name} must be a positive integer")),
        None => Ok(None),
    }
}

fn optional_string_array(arguments: &Map<String, Value>, name: &str) -> Result<Vec<String>> {
    match arguments.get(name) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(ToString::to_string)
                    .ok_or_else(|| anyhow!("argument {name} must be an array of non-empty strings"))
            })
            .collect(),
        Some(_) => Err(anyhow!("argument {name} must be an array of strings")),
        None => Ok(Vec::new()),
    }
}

fn optional_relation_kind(
    arguments: &Map<String, Value>,
    name: &str,
) -> Result<Option<BrainRelationKind>> {
    optional_string(arguments, name)?
        .map(|value| parse_relation_kind(&value))
        .transpose()
}

fn parse_relation_kind(raw: &str) -> Result<BrainRelationKind> {
    match raw {
        "mentions" => Ok(BrainRelationKind::Mentions),
        "supports" => Ok(BrainRelationKind::Supports),
        "contradicts" => Ok(BrainRelationKind::Contradicts),
        "supersedes" => Ok(BrainRelationKind::Supersedes),
        "same_as" => Ok(BrainRelationKind::SameAs),
        "works_at" => Ok(BrainRelationKind::WorksAt),
        "founded" => Ok(BrainRelationKind::Founded),
        "invested_in" => Ok(BrainRelationKind::InvestedIn),
        "advises" => Ok(BrainRelationKind::Advises),
        "attended" => Ok(BrainRelationKind::Attended),
        "owns" => Ok(BrainRelationKind::Owns),
        "responsible_for" => Ok(BrainRelationKind::ResponsibleFor),
        "decided" => Ok(BrainRelationKind::Decided),
        "blocks" => Ok(BrainRelationKind::Blocks),
        "depends_on" => Ok(BrainRelationKind::DependsOn),
        "source_of" => Ok(BrainRelationKind::SourceOf),
        "derived_from" => Ok(BrainRelationKind::DerivedFrom),
        "related_to" => Ok(BrainRelationKind::RelatedTo),
        _ => Err(anyhow!("unknown brain relation kind: {raw}")),
    }
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_definition(
            "search_brain",
            "Search local HyprDuck brain artifacts and return ranked evidence-backed IDs.",
            json!({
                "query": { "type": "string", "description": "Search query." },
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum result count." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "get_context_pack",
            "Build an agent-ready context pack with relevant memories, claims, entities, relations, sources, evidence, and recent events.",
            json!({
                "query": { "type": "string", "description": "Task or question to build context for." },
                "budget": { "type": "integer", "minimum": 1, "description": "Approximate token budget." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "read_source",
            "Read an immutable source record with adjacent wiki and evidence refs.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_brain or get_context_pack." },
            }),
            vec!["sourceId"],
            true,
        ),
        tool_definition(
            "read_wiki_page",
            "Read a generated or saved-back wiki page by repo-relative path.",
            json!({
                "path": { "type": "string", "description": "Wiki page path returned by search_brain or get_context_pack." },
            }),
            vec!["path"],
            true,
        ),
        tool_definition(
            "read_node",
            "Read a graph node with its evidence and adjacent relations.",
            json!({
                "nodeId": { "type": "string", "description": "Graph node ID returned by search_brain or get_context_pack." },
            }),
            vec!["nodeId"],
            true,
        ),
        tool_definition(
            "read_recent_events",
            "Read recent append-only brain events.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum event count." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_health",
            "Read brain health and pending review summary without mutating artifacts.",
            json!({}),
            Vec::new(),
            true,
        ),
        tool_definition(
            "propose_memory",
            "Propose a durable memory. Low-risk project memories are policy auto-applied and still create audit events.",
            proposal_properties(json!({})),
            vec!["title", "body"],
            false,
        ),
        tool_definition(
            "propose_claim",
            "Propose a source-backed claim. Claims require review before becoming trusted graph state.",
            proposal_properties(json!({})),
            vec!["title", "body"],
            false,
        ),
        tool_definition(
            "propose_link",
            "Propose a typed graph relation. Link proposals require review before becoming trusted graph state.",
            proposal_properties(json!({
                "targetNodeId": { "type": "string", "description": "Primary graph node ID." },
                "nodeRefs": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Related graph node IDs."
                },
                "relationKind": { "type": "string", "description": "Relation kind such as supports, contradicts, same_as, or related_to." }
            })),
            vec!["title", "body", "targetNodeId", "nodeRefs", "relationKind"],
            false,
        ),
        tool_definition(
            "append_observation",
            "Append an agent observation as safe project memory with source/evidence refs when available.",
            proposal_properties(json!({})),
            vec!["title", "body"],
            false,
        ),
        tool_definition(
            "add_source_note",
            "Add source metadata notes through the policy path.",
            proposal_properties(json!({
                "sourceId": { "type": "string", "description": "Source ID to annotate." },
                "sourceDescription": { "type": "string", "description": "Optional source description override." },
                "sourceUserContext": { "type": "string", "description": "Optional user context for future parsing." },
                "sourceIngestInstruction": { "type": "string", "description": "Optional ingest instruction." }
            })),
            vec!["title", "body", "sourceId"],
            false,
        ),
        tool_definition(
            "request_consolidation",
            "Request a future maintenance/consolidation pass without directly changing source truth.",
            proposal_properties(json!({})),
            vec!["title", "body"],
            false,
        ),
    ]
}

fn proposal_properties(properties: Value) -> Value {
    let mut merged_properties = properties.as_object().cloned().unwrap_or_default();
    for (name, schema) in [
        (
            "title",
            json!({ "type": "string", "description": "Short proposal title." }),
        ),
        (
            "body",
            json!({ "type": "string", "description": "Proposal body or memory text." }),
        ),
        (
            "actorId",
            json!({ "type": "string", "description": "External agent ID. Defaults to hyprduck-mcp." }),
        ),
        (
            "targetSourceId",
            json!({ "type": "string", "description": "Optional target source ID." }),
        ),
        (
            "targetNodeId",
            json!({ "type": "string", "description": "Optional target graph node ID." }),
        ),
        (
            "relationKind",
            json!({ "type": "string", "description": "Optional typed relation kind." }),
        ),
        (
            "sourceRefs",
            json!({ "type": "array", "items": { "type": "string" }, "description": "Source IDs supporting this proposal." }),
        ),
        (
            "nodeRefs",
            json!({ "type": "array", "items": { "type": "string" }, "description": "Node IDs related to this proposal." }),
        ),
        (
            "evidenceRefs",
            json!({ "type": "array", "items": { "type": "string" }, "description": "Evidence IDs supporting this proposal." }),
        ),
    ] {
        merged_properties.entry(name).or_insert(schema);
    }
    Value::Object(merged_properties)
}

fn tool_definition(
    name: &str,
    description: &str,
    properties: Value,
    required: Vec<&str>,
    read_only: bool,
) -> Value {
    let mut merged_properties = properties.as_object().cloned().unwrap_or_default();
    merged_properties.insert(
        "workspaceId".into(),
        json!({
            "type": "string",
            "description": "HyprDuck workspace ID. Defaults to default."
        }),
    );
    merged_properties.insert(
        "rootDir".into(),
        json!({
            "type": "string",
            "description": "Optional materialized brain repo root."
        }),
    );

    json!({
        "name": name,
        "title": title_case_tool_name(name),
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": merged_properties,
            "required": required,
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "idempotentHint": read_only,
            "openWorldHint": false
        }
    })
}

fn title_case_tool_name(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut title = first.to_ascii_uppercase().to_string();
                    title.push_str(chars.as_str());
                    title
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
