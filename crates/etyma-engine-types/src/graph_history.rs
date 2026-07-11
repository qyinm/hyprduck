use serde::{Deserialize, Serialize};

use etyma_knowledge::{
    BrainNodeRecord, BrainRelationRecord, ClaimRecord, SourceFormat, SourceStatus, WikiPage,
};

use crate::{BrainReadScope, SourceId, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphHistoryRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub record_kind: Option<GraphHistoryRecordKind>,
    #[serde(default)]
    pub record_id: Option<String>,
    #[serde(default)]
    pub wiki_path: Option<String>,
    #[serde(default)]
    pub include_diff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphHistoryEntry {
    pub snapshot_id: String,
    pub materialized_at: u64,
    pub event_id: String,
    #[serde(default)]
    pub operation_type: Option<String>,
    #[serde(default)]
    pub source_run_ids: Vec<String>,
    #[serde(default)]
    pub source_markdown_refs: Vec<String>,
    #[serde(default)]
    pub storage_locations: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
    pub claim_count: usize,
    pub memory_count: usize,
    pub wiki_page_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphHistoryResponseData {
    pub states: Vec<GraphHistoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_history: Option<GraphRecordHistoryResponse>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphHistoryRecordKind {
    Node,
    Relation,
    WikiPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRecordHistoryResponse {
    pub query: GraphRecordHistoryQuery,
    pub versions: Vec<GraphRecordHistoryVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRecordHistoryQuery {
    pub record_kind: GraphHistoryRecordKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wiki_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRecordHistoryVersion {
    pub record_kind: GraphHistoryRecordKind,
    pub logical_id: String,
    pub version_id: String,
    pub created_by_event_id: String,
    pub valid_from: u64,
    pub valid_to: Option<u64>,
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub node_refs: Vec<String>,
    #[serde(default)]
    pub relation_refs: Vec<String>,
    #[serde(default)]
    pub storage_locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphSnapshotRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub include_local_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSnapshotSourceRecord {
    pub source_id: SourceId,
    pub workspace_id: WorkspaceId,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: SourceFormat,
    pub status: SourceStatus,
    pub page_count: usize,
    #[serde(default)]
    pub success_count: usize,
    #[serde(default)]
    pub failed_count: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
    #[serde(default)]
    pub citation_ready: bool,
    #[serde(default)]
    pub graph_ready: bool,
    #[serde(default)]
    pub graph_status: String,
    #[serde(default)]
    pub manual_retry_available: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphSnapshotResponseData {
    pub snapshot_id: String,
    pub source_ingest_id: String,
    pub workspace_id: WorkspaceId,
    pub source_of_truth_path: String,
    pub latest_readable_snapshot_path: String,
    pub created_at: u64,
    pub materialized_at: u64,
    #[serde(default)]
    pub materialized_paths: Vec<String>,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub sources: Vec<GraphSnapshotSourceRecord>,
    #[serde(default)]
    pub graph_materialization_reports: Vec<GraphMaterializationReportSummary>,
    #[serde(default)]
    pub nodes: Vec<BrainNodeRecord>,
    #[serde(default, rename = "edges")]
    pub edges: Vec<BrainRelationRecord>,
    #[serde(default)]
    pub claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub memory_refs: Vec<String>,
    #[serde(default)]
    pub wiki_pages: Vec<WikiPage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMaterializationReportSummary {
    pub source_id: String,
    pub status: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub source_graph_materialized: bool,
    #[serde(default)]
    pub workspace_linking_materialized: bool,
    #[serde(default)]
    pub raw_source_graph_node_count: usize,
    #[serde(default)]
    pub raw_source_graph_relation_count: usize,
    #[serde(default)]
    pub canonical_source_graph_node_count: usize,
    #[serde(default)]
    pub canonical_source_graph_relation_count: usize,
    #[serde(default)]
    pub pruned_source_graph_node_count: usize,
    #[serde(default)]
    pub pruned_source_graph_relation_count: usize,
    #[serde(default)]
    pub compaction_status: Option<String>,
}
