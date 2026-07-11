use crate::auth::{resolve_bearer_workspace, AppState};
use crate::compose::compose_pack;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/mcp", post(mcp_handler))
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

async fn mcp_handler(
    State(state): State<AppState>,
    // tools/call requires workspace token; initialize/list/ping stay open for MCP handshake.
    headers: axum::http::HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = req.id.clone().unwrap_or(Value::Null);
    match req.method.as_str() {
        "initialize" => Ok(Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "etyma", "version": "0.1.0" }
            }
        }))),
        "tools/list" => Ok(Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
                    "name": "get_context_pack",
                    "description": "Compose a cited multi-source context pack (V1) for the token workspace.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        },
                        "required": ["query"]
                    }
                }]
            }
        }))),
        "tools/call" => {
            let workspace_id = resolve_bearer_workspace(&state, &headers)?;
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if name != "get_context_pack" {
                return Ok(Json(rpc_error(id, -32601, format!("unknown tool: {name}"))));
            }
            let query = req
                .params
                .get("arguments")
                .and_then(|a| a.get("query"))
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .to_string();
            if query.trim().is_empty() {
                return Ok(Json(rpc_error(id, -32602, "query is required".into())));
            }
            let pack = compose_pack(&state.store, &workspace_id, &query)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let text = serde_json::to_string_pretty(&pack)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": false
                }
            })))
        }
        "ping" => Ok(Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} }))),
        other => Ok(Json(rpc_error(
            id,
            -32601,
            format!("method not found: {other}"),
        ))),
    }
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}
