use anyhow::{anyhow, Context, Result};
use etyma_engine_client::EngineClient;
use etyma_engine_types::{BrainReadScope, ReadGraphSnapshotRequest, ReadWikiPageRequest};
use serde_json::{json, Map, Value};

use super::policy::{redact_local_path_text, redact_local_paths, validate_root_dir_argument};
use super::responses::{classify_mcp_error, error_response_with_data, success_response};

pub(super) fn handle_resource_read(
    client: &dyn EngineClient,
    id: Value,
    params: Option<&Value>,
) -> Value {
    match read_resource(client, params) {
        Ok(value) => success_response(id, value),
        Err(error) => error_response_with_data(
            id,
            -32602,
            redact_local_path_text(&error.to_string()),
            json!({
                "etymaErrorCategory": classify_mcp_error(&error.to_string())
            }),
        ),
    }
}

pub(super) fn read_resource(client: &dyn EngineClient, params: Option<&Value>) -> Result<Value> {
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
                include_local_paths: false,
            })?;
            let snapshot = redact_local_paths(serde_json::to_value(snapshot)?);
            Ok(json!({
                "contents": [
                    {
                        "uri": public_resource_uri(uri),
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
            let body = redact_local_path_text(&page.page.body);
            Ok(json!({
                "contents": [
                    {
                        "uri": public_resource_uri(uri),
                        "mimeType": "text/markdown",
                        "text": body
                    }
                ]
            }))
        }
    }
}

fn public_resource_uri(uri: &str) -> &str {
    uri.split_once('?').map_or(uri, |(path, _)| path)
}

#[derive(Debug)]
pub(super) struct BrainResource {
    pub(super) scope: BrainReadScope,
    kind: BrainResourceKind,
}

#[derive(Debug)]
enum BrainResourceKind {
    GraphSnapshot,
    WikiPage { path: String },
}

pub(super) fn parse_resource_uri(uri: &str) -> Result<BrainResource> {
    let Some(rest) = uri.strip_prefix("etyma://brain/") else {
        return Err(anyhow!("unsupported Etyma resource uri: {uri}"));
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (workspace_id, resource_path) = path
        .split_once('/')
        .ok_or_else(|| anyhow!("Etyma resource uri must include workspace and resource path"))?;
    if workspace_id.trim().is_empty() {
        return Err(anyhow!("Etyma resource uri workspace cannot be empty"));
    }
    let query = parse_resource_query(query)?;
    let root_dir = query
        .get("rootDir")
        .and_then(Value::as_str)
        .map(validate_root_dir_argument)
        .transpose()?;
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
            "unsupported Etyma resource path: {resource_path}"
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

pub(super) fn resource_definitions() -> Vec<Value> {
    vec![
        json!({
            "uri": "etyma://brain/default/graph/snapshot",
            "name": "Latest graph/wiki snapshot",
            "description": "Resolved latest completed materialized graph/wiki snapshot for the default workspace.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "etyma://brain/default/wiki/index.md",
            "name": "Wiki index",
            "description": "Current materialized wiki index for the default workspace.",
            "mimeType": "text/markdown"
        }),
    ]
}
