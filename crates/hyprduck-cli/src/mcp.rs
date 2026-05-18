use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Context, Result};
use hyprduck_engine_client::{EngineClient, SubprocessEngineClient};
use hyprduck_engine_types::{
    BrainReadScope, GetBrainHealthRequest, GetContextPackRequest, ReadGraphHistoryRequest,
    ReadGraphSnapshotRequest, ReadNodeRequest, ReadRecentEventsRequest, ReadSourceRequest,
    ReadWikiPageRequest, SearchBrainRequest,
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
        "resources/list" => Some(success_response(
            id,
            json!({ "resources": resource_definitions() }),
        )),
        "resources/read" => Some(handle_resource_read(client, id, message.get("params"))),
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
            },
            "resources": {
                "subscribe": false,
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "hyprduck",
            "title": "HyprDuck Local Context",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "HyprDuck exposes local document context artifacts through read, search, context-pack, snapshot, and health tools. Use get_context_pack first, then search_documents or open cited sources, page evidence, wiki pages, nodes, or event history as needed."
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
        Ok(tool_result) => {
            let mut result = json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string_pretty(&tool_result.value)
                            .unwrap_or_else(|_| "{}".into())
                    }
                ],
                "isError": false
            });
            if let Some(cache_state) = tool_result.cache_state {
                result["_meta"] = json!({
                    "hyprduckGraphWikiCache": cache_state
                });
            }
            result
        }
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

fn handle_resource_read(client: &dyn EngineClient, id: Value, params: Option<&Value>) -> Value {
    match read_resource(client, params) {
        Ok(value) => success_response(id, value),
        Err(error) => error_response(id, -32602, error.to_string()),
    }
}

fn read_resource(client: &dyn EngineClient, params: Option<&Value>) -> Result<Value> {
    let params = params.unwrap_or(&Value::Null);
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| !uri.trim().is_empty())
        .ok_or_else(|| anyhow!("Invalid params: missing resource uri"))?;
    let resource = parse_resource_uri(uri)?;

    match resource.kind {
        BrainResourceKind::GraphSnapshot => {
            let snapshot = client.read_graph_snapshot(ReadGraphSnapshotRequest {
                scope: resource.scope,
            })?;
            Ok(json!({
                "contents": [
                    {
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&snapshot)?
                    }
                ]
            }))
        }
        BrainResourceKind::WikiPage { path } => {
            let page = client.read_wiki_page(ReadWikiPageRequest {
                scope: resource.scope,
                path,
            })?;
            Ok(json!({
                "contents": [
                    {
                        "uri": uri,
                        "mimeType": "text/markdown",
                        "text": page.page.body
                    }
                ]
            }))
        }
    }
}

#[derive(Debug)]
struct BrainResource {
    scope: BrainReadScope,
    kind: BrainResourceKind,
}

#[derive(Debug)]
enum BrainResourceKind {
    GraphSnapshot,
    WikiPage { path: String },
}

fn parse_resource_uri(uri: &str) -> Result<BrainResource> {
    let Some(rest) = uri.strip_prefix("hyprduck://brain/") else {
        return Err(anyhow!("unsupported HyprDuck resource uri: {uri}"));
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (workspace_id, resource_path) = path
        .split_once('/')
        .ok_or_else(|| anyhow!("HyprDuck resource uri must include workspace and resource path"))?;
    if workspace_id.trim().is_empty() {
        return Err(anyhow!("HyprDuck resource uri workspace cannot be empty"));
    }
    let query = parse_resource_query(query)?;
    let root_dir = query
        .get("rootDir")
        .and_then(Value::as_str)
        .map(str::to_string);
    if root_dir.is_some() && !root_dir_argument_allowed() {
        return Err(anyhow!(
            "rootDir is disabled by default; set HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 for development roots"
        ));
    }
    let scope = BrainReadScope {
        workspace_id: percent_decode(workspace_id)?,
        root_dir,
    };
    let kind = if resource_path == "graph/snapshot" {
        BrainResourceKind::GraphSnapshot
    } else if let Some(path) = resource_path.strip_prefix("wiki/") {
        BrainResourceKind::WikiPage {
            path: format!("wiki/{}", percent_decode(path)?),
        }
    } else {
        return Err(anyhow!(
            "unsupported HyprDuck resource path: {resource_path}"
        ));
    };
    Ok(BrainResource { scope, kind })
}

fn parse_resource_query(query: &str) -> Result<Map<String, Value>> {
    let mut values = Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key)?, Value::String(percent_decode(value)?));
    }
    Ok(values)
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .context("resource uri contains invalid percent encoding")?;
                let byte = u8::from_str_radix(hex, 16)
                    .context("resource uri contains invalid percent encoding")?;
                decoded.push(byte);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).context("resource uri contains invalid utf-8")
}

fn call_tool(
    client: &dyn EngineClient,
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<McpToolResult> {
    let scope = read_scope(arguments)?;
    let cache_scope = scope.clone();
    let cache_before = cache_sensitive_tool(name)
        .then(|| read_graph_wiki_cache_state(client, &cache_scope))
        .transpose()?
        .flatten();

    let value = match name {
        "search_documents" | "search_brain" => {
            let query = required_string(arguments, "query")?;
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(client.search_brain(SearchBrainRequest {
                scope,
                query,
                limit,
            })?)?
        }
        "get_context_pack" => {
            let query = required_string(arguments, "query")?;
            let budget = optional_usize(arguments, "budget")?;
            serde_json::to_value(client.get_context_pack(GetContextPackRequest {
                scope,
                query,
                budget,
                persist: false,
            })?)?
        }
        "read_source" => {
            let source_id = required_string(arguments, "sourceId")?;
            serde_json::to_value(client.read_source(ReadSourceRequest { scope, source_id })?)?
        }
        "read_page_evidence" => {
            let source_id = required_string(arguments, "sourceId")?;
            let page = optional_usize(arguments, "page")?;
            if page == Some(0) {
                return Err(anyhow!("argument page must be a positive 1-based integer"));
            }
            let source = client.read_source(ReadSourceRequest { scope, source_id })?;
            let evidence = source
                .evidence
                .into_iter()
                .filter(|evidence| {
                    page.is_none_or(|page| evidence.page_index == Some(page.saturating_sub(1)))
                })
                .collect::<Vec<_>>();
            serde_json::to_value(json!({
                "source": source.source,
                "evidence": evidence
            }))?
        }
        "read_wiki_page" => {
            let path = required_string(arguments, "path")?;
            serde_json::to_value(client.read_wiki_page(ReadWikiPageRequest { scope, path })?)?
        }
        "read_node" => {
            let node_id = required_string(arguments, "nodeId")?;
            serde_json::to_value(client.read_node(ReadNodeRequest { scope, node_id })?)?
        }
        "read_recent_events" => {
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(client.read_recent_events(ReadRecentEventsRequest {
                scope,
                limit,
                run_id: optional_string(arguments, "runId")?,
                source_ref: optional_string(arguments, "sourceRef")?,
                node_id: optional_string(arguments, "nodeId")?,
                edge_id: optional_string(arguments, "edgeId")?,
                claim_id: optional_string(arguments, "claimId")?,
                memory_id: optional_string(arguments, "memoryId")?,
                change_type: optional_string(arguments, "changeType")?,
            })?)?
        }
        "read_graph_history" => {
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(
                client.read_graph_history(ReadGraphHistoryRequest { scope, limit })?,
            )?
        }
        "read_graph_snapshot" => {
            serde_json::to_value(client.read_graph_snapshot(ReadGraphSnapshotRequest { scope })?)?
        }
        "read_health" => {
            serde_json::to_value(client.get_brain_health(GetBrainHealthRequest { scope })?)?
        }
        _ => return Err(anyhow!("Unknown HyprDuck MCP tool: {name}")),
    };

    let cache_after = cache_sensitive_tool(name)
        .then(|| read_graph_wiki_cache_state(client, &cache_scope))
        .transpose()?
        .flatten();
    Ok(McpToolResult {
        value,
        cache_state: cache_after.map(|after| McpGraphWikiCacheState {
            invalidated: cache_before.as_ref() != Some(&after),
            current: after,
        }),
    })
}

struct McpToolResult {
    value: Value,
    cache_state: Option<McpGraphWikiCacheState>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpGraphWikiCacheState {
    invalidated: bool,
    current: McpGraphWikiCacheToken,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpGraphWikiCacheToken {
    workspace_id: String,
    snapshot_id: String,
    source_ingest_id: String,
    materialized_at: u64,
    latest_readable_snapshot_path: String,
    materialized_paths: Vec<String>,
}

fn cache_sensitive_tool(name: &str) -> bool {
    matches!(name, "read_health")
}

fn read_graph_wiki_cache_state(
    client: &dyn EngineClient,
    scope: &BrainReadScope,
) -> Result<Option<McpGraphWikiCacheToken>> {
    match client.read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: scope.clone(),
    }) {
        Ok(snapshot) => Ok(Some(McpGraphWikiCacheToken {
            workspace_id: snapshot.workspace_id,
            snapshot_id: snapshot.snapshot_id,
            source_ingest_id: snapshot.source_ingest_id,
            materialized_at: snapshot.materialized_at,
            latest_readable_snapshot_path: snapshot.latest_readable_snapshot_path,
            materialized_paths: snapshot.materialized_paths,
        })),
        Err(error)
            if error.to_string().contains("No such file")
                || error.to_string().contains("not found") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_scope(arguments: &Map<String, Value>) -> Result<BrainReadScope> {
    let root_dir = optional_string(arguments, "rootDir")?;
    if root_dir.is_some() && !root_dir_argument_allowed() {
        return Err(anyhow!(
            "rootDir is disabled by default; set HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 for development roots"
        ));
    }
    Ok(BrainReadScope {
        workspace_id: optional_string(arguments, "workspaceId")?
            .unwrap_or_else(|| "default".into()),
        root_dir,
    })
}

fn root_dir_argument_allowed() -> bool {
    std::env::var("HYPRDUCK_MCP_ALLOW_ROOT_DIR").is_ok_and(|value| value == "1")
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

fn resource_definitions() -> Vec<Value> {
    vec![
        json!({
            "uri": "hyprduck://brain/default/graph/snapshot",
            "name": "Latest graph/wiki snapshot",
            "description": "Resolved latest completed materialized graph/wiki snapshot for the default workspace.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "hyprduck://brain/default/wiki/index.md",
            "name": "Wiki index",
            "description": "Current materialized wiki index for the default workspace.",
            "mimeType": "text/markdown"
        }),
    ]
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_definition(
            "get_context_pack",
            "Build an agent-ready document context pack with selected sources, evidence, findings, warnings, and retrieval trace.",
            json!({
                "query": { "type": "string", "description": "Task or question to build context for." },
                "budget": { "type": "integer", "minimum": 1, "description": "Approximate token budget." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "search_documents",
            "Search local HyprDuck document context artifacts and return ranked evidence-backed IDs.",
            json!({
                "query": { "type": "string", "description": "Search query." },
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum result count." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "search_brain",
            "Compatibility alias for search_documents.",
            json!({
                "query": { "type": "string", "description": "Search query." },
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum result count." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "read_source",
            "Read an immutable source record with adjacent wiki and evidence refs.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_documents or get_context_pack." },
            }),
            vec!["sourceId"],
            true,
        ),
        tool_definition(
            "read_page_evidence",
            "Read source evidence refs for a source, optionally narrowed to one 1-based page.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_documents or get_context_pack." },
                "page": { "type": "integer", "minimum": 1, "description": "Optional 1-based page number." },
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
            "Read append-only graph loop events, optionally filtered by run, source, node, edge, claim, memory, or change type.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum event count." },
                "runId": { "type": "string", "description": "Filter by ingest run, source run, caused-by event, or payload runId." },
                "sourceRef": { "type": "string", "description": "Filter by source ID or markdown/source path ref." },
                "nodeId": { "type": "string", "description": "Filter by node ref or target node ID." },
                "edgeId": { "type": "string", "description": "Filter by edge/relation ref or target edge ID." },
                "claimId": { "type": "string", "description": "Filter by claim ref or target claim ID." },
                "memoryId": { "type": "string", "description": "Filter by memory ref or target memory ID." },
                "changeType": { "type": "string", "description": "Filter by event type, operation type, or payload changeType." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_graph_history",
            "List prior materialized graph states with timestamps, source run IDs, and storage locations.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum graph state count." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_graph_snapshot",
            "Read the latest completed materialized graph/wiki snapshot and its loading paths for UI, MCP, and agent consumers.",
            json!({}),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_health",
            "Read brain health without mutating artifacts.",
            json!({}),
            Vec::new(),
            true,
        ),
    ]
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
            "description": "Optional development-only materialized workspace root. Disabled unless HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 is set."
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

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_DIR_ENV: &str = "HYPRDUCK_MCP_ALLOW_ROOT_DIR";

    #[test]
    fn read_scope_rejects_root_dir_without_dev_env() {
        std::env::remove_var(ROOT_DIR_ENV);
        let mut arguments = Map::new();
        arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

        let error = read_scope(&arguments).expect_err("rootDir should be disabled by default");
        assert!(error.to_string().contains("rootDir is disabled"));
    }

    #[test]
    fn read_scope_rejects_root_dir_when_dev_env_is_not_one() {
        let mut arguments = Map::new();
        arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

        std::env::set_var(ROOT_DIR_ENV, "0");
        let zero_error = read_scope(&arguments).expect_err("rootDir=0 should stay disabled");
        assert!(zero_error.to_string().contains("rootDir is disabled"));

        std::env::set_var(ROOT_DIR_ENV, "");
        let empty_error =
            read_scope(&arguments).expect_err("empty rootDir env should stay disabled");
        assert!(empty_error.to_string().contains("rootDir is disabled"));

        std::env::remove_var(ROOT_DIR_ENV);
    }

    #[test]
    fn resource_uri_rejects_root_dir_without_dev_env() {
        std::env::remove_var(ROOT_DIR_ENV);

        let error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("resource rootDir should be disabled by default");
        assert!(error.to_string().contains("rootDir is disabled"));
    }

    #[test]
    fn resource_uri_rejects_root_dir_when_dev_env_is_not_one() {
        std::env::set_var(ROOT_DIR_ENV, "0");
        let zero_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("rootDir=0 should stay disabled for resources");
        assert!(zero_error.to_string().contains("rootDir is disabled"));

        std::env::set_var(ROOT_DIR_ENV, "");
        let empty_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("empty rootDir env should stay disabled for resources");
        assert!(empty_error.to_string().contains("rootDir is disabled"));

        std::env::remove_var(ROOT_DIR_ENV);
    }
}
