use anyhow::Result;
use etyma_engine_client::EngineClient;
use etyma_engine_types::{BrainReadScope, ReadGraphSnapshotRequest};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::mcp) struct McpGraphWikiCacheState {
    pub(in crate::mcp) invalidated: bool,
    pub(in crate::mcp) current: McpGraphWikiCacheToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::mcp) struct McpGraphWikiCacheToken {
    workspace_id: String,
    snapshot_id: String,
    source_ingest_id: String,
    materialized_at: u64,
    latest_readable_snapshot_path: String,
    materialized_paths: Vec<String>,
}

pub(super) fn cache_sensitive_tool(name: &str) -> bool {
    matches!(name, "graph_patch_apply" | "read_health")
}

pub(super) fn read_graph_wiki_cache_state(
    client: &dyn EngineClient,
    scope: &BrainReadScope,
) -> Result<Option<McpGraphWikiCacheToken>> {
    match client.read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: scope.clone(),
        include_local_paths: false,
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
