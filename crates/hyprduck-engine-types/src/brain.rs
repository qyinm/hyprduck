use serde::{Deserialize, Serialize};

use hyprduck_knowledge::{
    BrainEvent, BrainNodeRecord, BrainRelationRecord, BrainRepoSnapshot, ClaimRecord, EntityRecord,
    EvidenceRef, MemoryRecord, SourceRecord, SourceStatus, WikiPage,
};

use crate::{
    ContextPackParseConfidence, ContextPackV0, ContextPackV1, ContextPackWarningV0, SourceId,
    WorkspaceId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainReadScope {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub root_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBrainRequest {
    pub scope: BrainReadScope,
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainSearchResultKind {
    Source,
    Memory,
    WikiPage,
    Node,
    Entity,
    Claim,
    Relation,
    Evidence,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainSearchResult {
    pub kind: BrainSearchResultKind,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub path: Option<String>,
    pub score: usize,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBrainResponseData {
    pub results: Vec<BrainSearchResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadSourceRequest {
    pub scope: BrainReadScope,
    pub source_id: SourceId,
    #[serde(default)]
    pub include_local_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadSourceResponseData {
    pub source: SourceRecord,
    #[serde(default)]
    pub wiki_page: Option<WikiPage>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPageEvidenceRequest {
    pub scope: BrainReadScope,
    pub source_id: SourceId,
    #[serde(default)]
    pub page: Option<usize>,
    #[serde(default)]
    pub include_local_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageEvidenceV0 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    pub region: String,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub content_hash: String,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPageEvidenceResponseData {
    pub source: SourceRecord,
    pub evidence: Vec<PageEvidenceV0>,
    pub warnings: Vec<ContextPackWarningV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadContextPackRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub pack_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadContextPackResponseData {
    pub context_pack: ContextPackV0,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadWikiPageRequest {
    pub scope: BrainReadScope,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadWikiPageResponseData {
    pub page: WikiPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadNodeRequest {
    pub scope: BrainReadScope,
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadNodeResponseData {
    pub node: BrainNodeRecord,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub relations: Vec<BrainRelationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadRecentEventsRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub edge_id: Option<String>,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub change_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadRecentEventsResponseData {
    pub events: Vec<BrainEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconstructBrainRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub up_to_timestamp: Option<u64>,
    #[serde(default)]
    pub up_to_materialized_version: Option<u64>,
    #[serde(default)]
    pub up_to_event_id: Option<String>,
    #[serde(default)]
    pub output_root: Option<String>,
    #[serde(default)]
    pub write_materialized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconstructBrainResponseData {
    pub snapshot: BrainRepoSnapshot,
    pub replayed_event_count: usize,
    pub selected_event_id: Option<String>,
    pub snapshot_id: String,
    pub output_root: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContextPackRequest {
    pub scope: BrainReadScope,
    pub query: String,
    #[serde(default)]
    pub selected_node_id: Option<String>,
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub persist: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainContextPack {
    pub workspace_id: WorkspaceId,
    pub query: String,
    pub token_budget: usize,
    pub summary: String,
    #[serde(default)]
    pub wiki_pages: Vec<WikiPage>,
    #[serde(default)]
    pub nodes: Vec<BrainNodeRecord>,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
    #[serde(default)]
    pub memories: Vec<MemoryRecord>,
    #[serde(default)]
    pub entities: Vec<EntityRecord>,
    #[serde(default)]
    pub claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub relations: Vec<BrainRelationRecord>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub recent_events: Vec<BrainEvent>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContextPackResponseData {
    pub context_pack: BrainContextPack,
    pub context_pack_v1: ContextPackV1,
    pub context_pack_v0: ContextPackV0,
    #[serde(default)]
    pub persisted_context_pack_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainHealthStatus {
    Clean,
    AttentionNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBrainHealthRequest {
    pub scope: BrainReadScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBrainHealthResponseData {
    pub status: BrainHealthStatus,
    pub attention_count: usize,
    #[serde(default)]
    pub governance: Option<BrainGovernanceReport>,
    #[serde(default)]
    pub knowledge_store: Option<BrainKnowledgeStoreReport>,
    #[serde(default)]
    pub source_reports: Vec<BrainHealthSourceReport>,
    #[serde(default)]
    pub recent_events: Vec<BrainEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainGovernanceReport {
    pub storage_locality: String,
    pub interaction_surface: String,
    pub evidence_governed: bool,
    pub mutating_tools_require_evidence: bool,
    pub local_path_disclosure_default: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainKnowledgeStoreReport {
    pub canonical_storage: String,
    pub primary_graph_store: String,
    pub pure_sqlite_relational_graph_rejected: bool,
    pub optional_graphqlite_acceleration_rejected: bool,
    pub graph_store_mode: String,
    pub graph_native_query_surface: String,
    pub migration_mode: String,
    pub long_dual_write_transition_rejected: bool,
    pub db_schema_version: i64,
    pub graph_schema_version: i64,
    pub graphqlite_loaded: bool,
    pub graphqlite_transactional: bool,
    pub graphqlite_release_gate: String,
    pub release_blocked_without_graphqlite: bool,
    pub migration_blast_radius: String,
    pub broad_verification_required: bool,
    pub json_artifacts_canonical: bool,
    pub json_artifact_role: String,
    pub vector_search_enabled: bool,
    pub vector_search_policy: String,
    pub checkpoint_rollback_api_enabled: bool,
    pub checkpoint_rollback_policy: String,
    pub graph_algorithms_enabled: bool,
    pub graph_algorithm_policy: String,
    pub evidence_item_count: usize,
    pub wiki_page_count: usize,
    pub graph_node_count: usize,
    pub graph_relation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainHealthSourceReport {
    pub source_id: SourceId,
    pub status: SourceStatus,
    pub page_count: usize,
    pub failed_page_count: usize,
    pub provider_route: String,
    #[serde(default)]
    pub local_only: Option<bool>,
    #[serde(default)]
    pub content_hash: Option<String>,
    pub content_hash_status: String,
    #[serde(default)]
    pub citation_ready: bool,
    #[serde(default)]
    pub graph_ready: bool,
    #[serde(default)]
    pub graph_status: String,
    #[serde(default)]
    pub manual_retry_available: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}
