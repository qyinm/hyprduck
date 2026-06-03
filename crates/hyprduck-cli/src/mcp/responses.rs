use serde_json::{json, Value};

pub(super) fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

pub(super) fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

pub(super) fn error_response_with_data(
    id: Value,
    code: i64,
    message: impl Into<String>,
    data: Value,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
            "data": data
        }
    })
}

pub(super) fn classify_mcp_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("rootdir")
        || lower.contains("root dir")
        || lower.contains("allowed_root")
        || lower.contains("allowed roots")
        || lower.contains("allowed import")
        || lower.contains("sourcepath")
        || lower.contains("path")
    {
        "path_policy"
    } else if lower.contains("evidence") || lower.contains("sourceid") {
        "evidence_scope"
    } else if lower.contains("graphpatch")
        || lower.contains("schema")
        || lower.contains("proposalid")
        || lower.contains("contenttype")
        || lower.contains("argument")
        || lower.contains("invalid params")
    {
        "schema"
    } else if lower.contains("provider") || lower.contains("openrouter") || lower.contains("ollama")
    {
        "provider"
    } else if lower.contains("import job") || lower.contains("lifecycle") || lower.contains("retry")
    {
        "lifecycle"
    } else if lower.contains("database")
        || lower.contains("sqlite")
        || lower.contains("graphqlite")
        || lower.contains("failed reading")
        || lower.contains("failed committing")
    {
        "persistence"
    } else if lower.contains("graph")
        || lower.contains("materialization")
        || lower.contains("snapshot")
        || lower.contains("wiki")
    {
        "graph_materialization"
    } else if lower.contains("failed writing") {
        "persistence"
    } else {
        "unknown"
    }
}
