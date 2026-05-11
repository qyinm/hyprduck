use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Context, Result};
use duckdocs_engine_client::{EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    BrainReadScope, GetBrainHealthRequest, GetContextPackRequest, ReadNodeRequest,
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
        "instructions": "HyprDuck exposes the local brain as read-only MCP tools. Use search_brain and get_context_pack first, then read source, wiki, node, event, or health details by ID."
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

    let result = match call_read_only_tool(client, name, arguments) {
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

fn call_read_only_tool(
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
        _ => Err(anyhow!("Unknown read-only HyprDuck MCP tool: {name}")),
    }
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
        ),
        tool_definition(
            "get_context_pack",
            "Build an agent-ready context pack with relevant memories, claims, entities, relations, sources, evidence, and recent events.",
            json!({
                "query": { "type": "string", "description": "Task or question to build context for." },
                "budget": { "type": "integer", "minimum": 1, "description": "Approximate token budget." },
            }),
            vec!["query"],
        ),
        tool_definition(
            "read_source",
            "Read an immutable source record with adjacent wiki and evidence refs.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_brain or get_context_pack." },
            }),
            vec!["sourceId"],
        ),
        tool_definition(
            "read_wiki_page",
            "Read a generated or saved-back wiki page by repo-relative path.",
            json!({
                "path": { "type": "string", "description": "Wiki page path returned by search_brain or get_context_pack." },
            }),
            vec!["path"],
        ),
        tool_definition(
            "read_node",
            "Read a graph node with its evidence and adjacent relations.",
            json!({
                "nodeId": { "type": "string", "description": "Graph node ID returned by search_brain or get_context_pack." },
            }),
            vec!["nodeId"],
        ),
        tool_definition(
            "read_recent_events",
            "Read recent append-only brain events.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum event count." },
            }),
            Vec::new(),
        ),
        tool_definition(
            "read_health",
            "Read brain health and pending review summary without mutating artifacts.",
            json!({}),
            Vec::new(),
        ),
    ]
}

fn tool_definition(name: &str, description: &str, properties: Value, required: Vec<&str>) -> Value {
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
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
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
