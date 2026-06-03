use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use hyprduck_engine_client::{EngineClient, SubprocessEngineClient};
use serde_json::{json, Value};

use super::policy::{redact_local_path_text, redact_local_paths};
use super::resources::{handle_resource_read, resource_definitions};
use super::responses::{classify_mcp_error, error_response, success_response};
use super::{call_tool, local_path_disclosure_for_tool, tool_definitions, ImportJobRegistry};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let client = SubprocessEngineClient::default();
    let state = McpServerState::default();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&client, &state, &line) {
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

#[derive(Clone, Default)]
pub(super) struct McpServerState {
    pub(super) import_jobs: ImportJobRegistry,
}

fn handle_message(client: &dyn EngineClient, state: &McpServerState, line: &str) -> Option<Value> {
    let message = match serde_json::from_str::<Value>(line) {
        Ok(message) => message,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("Parse error: {error}"),
            ));
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
        "tools/call" => Some(handle_tool_call(client, state, id, message.get("params"))),
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
        "instructions": "HyprDuck exposes local, desktop-first, evidence-governed document context through read, search, context-pack, snapshot, and health tools. Use get_context_pack first, then search_documents or open cited sources, page evidence, wiki pages, nodes, or event history as needed."
    })
}

fn handle_tool_call(
    client: &dyn EngineClient,
    state: &McpServerState,
    id: Value,
    params: Option<&Value>,
) -> Value {
    let params = params.unwrap_or(&Value::Null);
    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => return error_response(id, -32602, "Invalid params: missing tool name"),
    };
    let arguments = match params.get("arguments") {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return error_response(id, -32602, "Invalid params: arguments must be an object");
        }
        None => return error_response(id, -32602, "Invalid params: missing arguments"),
    };
    let include_local_paths = match local_path_disclosure_for_tool(name, arguments) {
        Ok(value) => value,
        Err(error) => {
            return success_response(
                id,
                json!({
                    "content": [
                        {
                            "type": "text",
                            "text": error.to_string()
                        }
                    ],
                    "isError": true,
                    "_meta": {
                        "hyprduckErrorCategory": classify_mcp_error(&error.to_string())
                    }
                }),
            );
        }
    };

    let result = match call_tool(client, state, name, arguments, include_local_paths) {
        Ok(tool_result) => {
            let value = if include_local_paths {
                tool_result.value
            } else {
                redact_local_paths(tool_result.value)
            };
            let mut result = json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string_pretty(&value)
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
                    "text": redact_local_path_text(&error.to_string())
                }
            ],
            "isError": true,
            "_meta": {
                "hyprduckErrorCategory": classify_mcp_error(&error.to_string())
            }
        }),
    };

    success_response(id, result)
}
