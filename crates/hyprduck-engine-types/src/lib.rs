use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub use hyprduck_knowledge::{
    AnswerResponse, AnswerStatus, BrainActor, BrainActorType, BrainEvent, BrainEventCausality,
    BrainEventKind, BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord,
    BrainRepoSnapshot, BrainScope, ClaimRecord, CorrectionAction, CorrectionKind, EntityRecord,
    EvidenceRef, GraphNodeDetail, GraphNodeKind, GraphNodePosition, GraphNodeSummary,
    KnowledgeProject, MemoryRecord, PolicyResult, ProjectOverview, ProjectStatus,
    RelationEdgeDetail, RelationEdgeSummary, RelationKind, SourceBacking, SourceFormat,
    SourceRecord, SourceStatus, StructuredExtractionArtifact, StructuredExtractionClaim,
    StructuredExtractionEntity, StructuredExtractionMemoryCandidate, StructuredExtractionPageRef,
    StructuredExtractionRelation, StructuredExtractionTopic, SuggestedAction, SuggestedActionKind,
    WikiPage, WorkspaceCorrection, BRAIN_EVENT_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCommand {
    Parse,
    CompileProject,
    LoadProject,
    ApplyCorrection,
    AnswerProject,
    SearchBrain,
    ReadSource,
    ReadWikiPage,
    ReadNode,
    ReadRecentEvents,
    ReadGraphHistory,
    ReadGraphSnapshot,
    ReconstructBrain,
    GetContextPack,
    GetBrainHealth,
    LoadConfig,
    SaveConfig,
    ValidateProvider,
    ListProviderModels,
    CheckReadiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    Pdf,
    Docx,
    Doc,
    Image,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseInput {
    pub path: String,
    pub format: DocumentFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParseOptions {
    pub preserve_images: bool,
    pub emit_structured_json: bool,
    pub emit_svg: bool,
    pub language_hints: Vec<String>,
    #[serde(default)]
    pub debug_request_path: Option<String>,
    #[serde(default)]
    pub debug_result_path: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            preserve_images: true,
            emit_structured_json: false,
            emit_svg: false,
            language_hints: Vec::new(),
            debug_request_path: None,
            debug_result_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ParseOutputTarget {
    pub root_dir: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub source_id: Option<SourceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseRequest {
    pub version: String,
    pub input: ParseInput,
    pub template: String,
    pub options: ParseOptions,
    pub output: Option<ParseOutputTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedPage {
    pub index: usize,
    pub markdown: Option<String>,
    pub plain_text: Option<String>,
    pub svg: Option<String>,
    #[serde(default)]
    pub image_asset_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputAsset {
    pub relative_path: String,
    pub mime_type: String,
    pub base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseMetadata {
    pub engine_id: String,
    pub duration_ms: u64,
    pub page_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResult {
    pub version: String,
    pub markdown: String,
    pub pages: Vec<ParsedPage>,
    pub assets: Vec<OutputAsset>,
    pub metadata: ParseMetadata,
    #[serde(default)]
    pub success_count: usize,
    #[serde(default)]
    pub failed_count: usize,
}

pub type WorkspaceId = String;
pub type SourceId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    Added,
    Rendering,
    Ingesting,
    Ingested,
    Partial,
    Failed,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageArtifact {
    pub index: usize,
    pub label: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub plain_text_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceArtifactManifest {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub artifact_root: String,
    pub manifest_path: String,
    pub format: DocumentFormat,
    pub output_name: String,
    pub status: IngestStatus,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
    pub pages: Vec<PageArtifact>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: DocumentFormat,
    pub status: IngestStatus,
    pub page_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestRun {
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub status: IngestStatus,
    pub started_at: u64,
    #[serde(default)]
    pub completed_at: Option<u64>,
    pub source_manifest_path: String,
    pub page_count: usize,
    pub success_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResponseData {
    pub result: ParseResult,
    #[serde(default)]
    pub saved_output_path: Option<String>,
    #[serde(default)]
    pub source_manifest: Option<SourceArtifactManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectRequest {
    pub source_markdown_path: String,
    #[serde(default)]
    pub source_document_path: Option<String>,
    #[serde(default)]
    pub source_manifest_path: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub source_id: Option<SourceId>,
    #[serde(default)]
    pub skip_graph_generation: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileProjectResponseData {
    pub project_id: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    #[serde(default)]
    pub graph_generation_status: Option<String>,
    #[serde(default)]
    pub graph_generation_skipped_reason: Option<String>,
    #[serde(default)]
    pub graph_generation_error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadProjectRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadProjectResponseData {
    #[serde(default)]
    pub project: Option<KnowledgeProject>,
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub sources: Vec<SourceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCorrectionRequest {
    pub project_id: String,
    pub node_id: String,
    pub kind: CorrectionKind,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyCorrectionResponseData {
    pub project: KnowledgeProject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerProjectRequest {
    pub project_id: String,
    #[serde(default)]
    pub node_id: Option<String>,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerProjectResponseData {
    pub answer: AnswerResponse,
}

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
pub struct ReadGraphHistoryRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphHistoryEntry {
    pub snapshot_id: String,
    pub materialized_at: u64,
    pub event_id: String,
    pub rollback_target: GraphRollbackTarget,
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
pub struct GraphRollbackTarget {
    pub snapshot_id: String,
    pub event_id: String,
    pub materialized_version: u64,
    pub replay_selector: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphHistoryResponseData {
    pub states: Vec<GraphHistoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadGraphSnapshotRequest {
    pub scope: BrainReadScope,
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

pub const CONTEXT_PACK_V0_SCHEMA_VERSION: &str = "hyprduck.context_pack.v0";
pub const SOURCE_PACK_V0_SCHEMA_VERSION: &str = "hyprduck.source_pack.v0";
pub const EVIDENCE_INDEX_V0_SCHEMA_VERSION: &str = "hyprduck.evidence_index.v0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackStaleness {
    Current,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackParseConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackFindingStatus {
    DerivedSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackWarningSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSourceV0 {
    pub source_id: SourceId,
    pub original_filename: String,
    pub content_hash: String,
    pub page_count: usize,
    pub ingestion_status: String,
    pub staleness: ContextPackStaleness,
    pub provider_route: String,
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackEvidenceV0 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub selection_reason: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackFindingV0 {
    pub finding_id: String,
    pub statement: String,
    pub status: ContextPackFindingStatus,
    pub statement_confidence: ContextPackParseConfidence,
    pub derived_from: Vec<String>,
    pub relevance_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackPageRefV0 {
    pub source_id: SourceId,
    pub page: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackWarningV0 {
    #[serde(rename = "type")]
    pub warning_type: String,
    pub severity: ContextPackWarningSeverity,
    pub message: String,
    #[serde(default)]
    pub page_refs: Vec<ContextPackPageRefV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackRetrievalTraceV0 {
    pub strategy: String,
    pub chunks_considered: usize,
    pub chunks_selected: usize,
    pub budget_requested: usize,
    pub budget_used: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSuggestedNextReadV0 {
    pub source_id: SourceId,
    pub page: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackPageV0 {
    pub page: usize,
    pub label: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub plain_text_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackWarningV0 {
    #[serde(rename = "type")]
    pub warning_type: String,
    pub severity: ContextPackWarningSeverity,
    pub message: String,
    #[serde(default)]
    pub page: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackV0 {
    pub schema_version: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_filename: String,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub artifact_root: String,
    pub content_hash: String,
    pub format: DocumentFormat,
    pub page_count: usize,
    pub ingestion_status: IngestStatus,
    pub provider_route: String,
    pub local_only: bool,
    pub pages: Vec<SourcePackPageV0>,
    pub warnings: Vec<SourcePackWarningV0>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndexItemV0 {
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
pub struct EvidenceIndexV0 {
    pub schema_version: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub content_hash: String,
    pub provider_route: String,
    pub local_only: bool,
    pub evidence: Vec<EvidenceIndexItemV0>,
    pub warnings: Vec<SourcePackWarningV0>,
    pub generated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackV0 {
    pub schema_version: String,
    pub pack_id: String,
    pub workspace_id: WorkspaceId,
    pub query: String,
    pub generated_at: String,
    pub source_set: Vec<ContextPackSourceV0>,
    pub selected_evidence: Vec<ContextPackEvidenceV0>,
    pub findings: Vec<ContextPackFindingV0>,
    pub warnings: Vec<ContextPackWarningV0>,
    pub retrieval_trace: ContextPackRetrievalTraceV0,
    pub suggested_next_reads: Vec<ContextPackSuggestedNextReadV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSourceMetadataV0 {
    pub content_hash: String,
    pub provider_route: String,
    pub local_only: bool,
}

impl ContextPackV0 {
    pub fn from_brain_context_pack(
        pack: &BrainContextPack,
        pack_id: impl Into<String>,
        generated_at: impl Into<String>,
        source_metadata: &BTreeMap<SourceId, ContextPackSourceMetadataV0>,
    ) -> Self {
        let source_set = pack
            .sources
            .iter()
            .filter(|source| source_metadata.contains_key(&source.source_id))
            .map(|source| ContextPackSourceV0::from_source_record(source, source_metadata))
            .collect::<Vec<_>>();
        let source_set_ids = source_set
            .iter()
            .map(|source| source.source_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let selected_evidence = pack
            .evidence
            .iter()
            .filter_map(|evidence| {
                ContextPackEvidenceV0::from_evidence_ref(evidence, source_metadata, &source_set_ids)
            })
            .collect::<Vec<_>>();
        let selected_evidence_ids = selected_evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let findings = pack
            .claims
            .iter()
            .filter_map(|claim| {
                let derived_from = claim
                    .evidence_refs
                    .iter()
                    .filter(|evidence_ref| selected_evidence_ids.contains(*evidence_ref))
                    .cloned()
                    .collect::<Vec<_>>();
                if derived_from.is_empty() {
                    return None;
                }
                Some(ContextPackFindingV0 {
                    finding_id: claim.claim_id.clone(),
                    statement: claim.statement.clone(),
                    status: ContextPackFindingStatus::DerivedSummary,
                    statement_confidence: ContextPackParseConfidence::Unknown,
                    derived_from,
                    relevance_reason: "Selected from the internal context pack for this query."
                        .into(),
                })
            })
            .collect();

        Self {
            schema_version: CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
            pack_id: pack_id.into(),
            workspace_id: pack.workspace_id.clone(),
            query: pack.query.clone(),
            generated_at: generated_at.into(),
            source_set,
            selected_evidence,
            findings,
            warnings: pack
                .warnings
                .iter()
                .map(|warning| ContextPackWarningV0 {
                    warning_type: "internal_context_warning".into(),
                    severity: ContextPackWarningSeverity::Medium,
                    message: warning.clone(),
                    page_refs: Vec::new(),
                })
                .chain(
                    pack.evidence
                        .iter()
                        .filter(|evidence| match evidence.source_id.as_ref() {
                            Some(source_id) => {
                                !source_metadata.contains_key(source_id)
                                    || !source_set_ids.contains(source_id)
                            }
                            None => true,
                        })
                        .map(|evidence| ContextPackWarningV0 {
                            warning_type: "evidence_missing_content_hash".into(),
                            severity: ContextPackWarningSeverity::High,
                            message: format!(
                                "Evidence {} was omitted from Context Pack v0 because its source content hash or provider route is unavailable.",
                                evidence.id
                            ),
                            page_refs: evidence.source_id.clone().map_or_else(Vec::new, |source_id| {
                                vec![ContextPackPageRefV0 {
                                    source_id,
                                    page: evidence.page_index.unwrap_or(0) + 1,
                                }]
                            }),
                        }),
                )
                .collect(),
            retrieval_trace: ContextPackRetrievalTraceV0 {
                strategy: "internal-brain-context-pack".into(),
                chunks_considered: pack.evidence.len(),
                chunks_selected: selected_evidence_ids.len(),
                budget_requested: pack.token_budget,
                budget_used: pack.summary.len(),
            },
            suggested_next_reads: Vec::new(),
        }
    }
}

impl ContextPackSourceV0 {
    fn from_source_record(
        source: &SourceRecord,
        metadata: &BTreeMap<SourceId, ContextPackSourceMetadataV0>,
    ) -> Self {
        let metadata = metadata.get(&source.source_id).unwrap_or_else(|| {
            panic!(
                "source metadata missing for Context Pack v0 source {}",
                source.source_id
            )
        });
        Self {
            source_id: source.source_id.clone(),
            original_filename: std::path::Path::new(&source.original_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(source.original_path.as_str())
                .into(),
            content_hash: metadata.content_hash.clone(),
            page_count: source.page_count,
            ingestion_status: source.status.to_string(),
            staleness: if source.status == SourceStatus::stale() {
                ContextPackStaleness::Stale
            } else {
                ContextPackStaleness::Current
            },
            provider_route: metadata.provider_route.clone(),
            local_only: metadata.local_only,
        }
    }
}

impl ContextPackEvidenceV0 {
    fn from_evidence_ref(
        evidence: &EvidenceRef,
        source_metadata: &BTreeMap<SourceId, ContextPackSourceMetadataV0>,
        source_set_ids: &std::collections::BTreeSet<SourceId>,
    ) -> Option<Self> {
        let source_id = evidence.source_id.clone()?;
        if !source_set_ids.contains(&source_id) {
            return None;
        }
        let source = source_metadata.get(&source_id)?;
        let page = evidence.page_index.unwrap_or(0) + 1;
        Some(Self {
            evidence_ref: evidence.id.clone(),
            source_id,
            page,
            region: Some(format!("page:{}", evidence.page_label)),
            span: None,
            quoted_text: evidence.snippet.clone(),
            parse_confidence: ContextPackParseConfidence::Unknown,
            selection_reason: "Selected from the internal context pack for this query.".into(),
            content_hash: source.content_hash.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetContextPackResponseData {
    pub context_pack: BrainContextPack,
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
    pub recent_events: Vec<BrainEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub id: String,
    pub label: String,
    pub requires_api_key: bool,
    pub supports_base_url: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfigPayload {
    pub provider: String,
    pub model_id: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub prompt_template: String,
    #[serde(default)]
    pub provider_options: Vec<ProviderOption>,
    #[serde(default)]
    pub model_options: Vec<String>,
    #[serde(default)]
    pub prompt_template_options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigResponseData {
    pub config: EngineConfigPayload,
    pub persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderResponseData {
    pub ready: bool,
    #[serde(default)]
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListProviderModelsRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelCatalogResponseData {
    pub provider_models: BTreeMap<String, Vec<String>>,
    pub ollama_vision_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheckReadinessRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessCheck {
    pub id: String,
    pub label: String,
    pub ready: bool,
    #[serde(default = "default_readiness_required")]
    pub required: bool,
    pub message: String,
}

fn default_readiness_required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReadinessResponseData {
    pub ready: bool,
    pub provider: String,
    pub model_id: String,
    #[serde(default)]
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadConfigRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConfigRequest {
    pub config: EngineConfigPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidateProviderRequest {
    #[serde(default)]
    pub config: Option<EngineConfigPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeRequest {
    pub id: Uuid,
    #[serde(flatten)]
    pub request: EngineRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineRuntimeMessageType {
    Response,
    Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeResponse<T> {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    #[serde(flatten)]
    pub response: EngineSuccess<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeFailure {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    #[serde(flatten)]
    pub failure: EngineFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRuntimeEvent {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub message_type: EngineRuntimeMessageType,
    pub event: ParseEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum EngineRequest {
    Parse(ParseRequest),
    CompileProject(CompileProjectRequest),
    LoadProject(LoadProjectRequest),
    ApplyCorrection(ApplyCorrectionRequest),
    AnswerProject(AnswerProjectRequest),
    SearchBrain(SearchBrainRequest),
    ReadSource(ReadSourceRequest),
    ReadWikiPage(ReadWikiPageRequest),
    ReadNode(ReadNodeRequest),
    ReadRecentEvents(ReadRecentEventsRequest),
    ReadGraphHistory(ReadGraphHistoryRequest),
    ReadGraphSnapshot(ReadGraphSnapshotRequest),
    ReconstructBrain(ReconstructBrainRequest),
    GetContextPack(GetContextPackRequest),
    GetBrainHealth(GetBrainHealthRequest),
    LoadConfig(LoadConfigRequest),
    SaveConfig(SaveConfigRequest),
    ValidateProvider(ValidateProviderRequest),
    ListProviderModels(ListProviderModelsRequest),
    CheckReadiness(CheckReadinessRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineSuccess<T> {
    pub ok: bool,
    pub command: EngineCommand,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineFailure {
    pub ok: bool,
    pub command: EngineCommand,
    pub error: EngineError,
}

impl<T> EngineSuccess<T> {
    pub fn new(command: EngineCommand, data: T) -> Self {
        Self {
            ok: true,
            command,
            data,
        }
    }
}

impl<T> EngineRuntimeResponse<T> {
    pub fn new(id: Uuid, response: EngineSuccess<T>) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Response,
            response,
        }
    }
}

impl EngineRuntimeFailure {
    pub fn new(id: Uuid, failure: EngineFailure) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Response,
            failure,
        }
    }
}

impl EngineRuntimeEvent {
    pub fn new(id: Uuid, event: ParseEvent) -> Self {
        Self {
            id,
            message_type: EngineRuntimeMessageType::Event,
            event,
        }
    }
}

impl EngineFailure {
    pub fn new(
        command: EngineCommand,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            ok: false,
            command,
            error: EngineError {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParseEvent {
    Queued,
    DocumentOpened { format: DocumentFormat },
    ConvertingPages { current: u32, total: u32 },
    Parsing { current: u32, total: u32 },
    Packaging,
    Completed,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseProgress {
    Queued,
    ConvertingPages { current: u32, total: u32 },
    Parsing { current: u32, total: u32 },
    Packaging,
    Completed,
    Failed { message: String },
}

impl From<ParseEvent> for ParseProgress {
    fn from(value: ParseEvent) -> Self {
        match value {
            ParseEvent::Queued => Self::Queued,
            ParseEvent::DocumentOpened { .. } => Self::Queued,
            ParseEvent::ConvertingPages { current, total } => {
                Self::ConvertingPages { current, total }
            }
            ParseEvent::Parsing { current, total } => Self::Parsing { current, total },
            ParseEvent::Packaging => Self::Packaging,
            ParseEvent::Completed => Self::Completed,
            ParseEvent::Failed { message } => Self::Failed { message },
        }
    }
}

/// Returns the list of supported model IDs for a given provider slug.
/// Single source of truth — used by both the engine and the desktop UI.
pub fn model_options_for(provider_slug: &str) -> Vec<&'static str> {
    match provider_slug {
        "open_router" => vec![
            "google/gemma-4-31b-it",
            "z-ai/glm-5v-turbo",
            "anthropic/claude-sonnet-4.6",
            "anthropic/claude-opus-4.6",
            "google/gemini-3-flash-preview",
            "qwen/qwen3.6-plus:free",
            "x-ai/grok-4.1-fast",
            "google/gemini-2.5-flash-lite",
            "google/gemini-2.5-flash",
            "moonshotai/kimi-k2.5",
        ],
        "open_ai" => vec![
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-nano",
            "gpt-4o",
            "gpt-4o-mini",
        ],
        "anthropic" => vec![
            "claude-3-7-sonnet-20250219",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
        ],
        "ollama" => vec![
            "gemma4:latest",
            "qwen3.5:latest",
            "qwen3-vl:8b",
            "qwen3-vl:72b",
            "kimi-k2.5:latest",
            "glm-ocr:latest",
            "deepseek-ocr:latest",
        ],
        _ => Vec::new(),
    }
}

/// Prefixes used to identify local Ollama models that can process page images.
pub fn ollama_vision_prefixes() -> Vec<&'static str> {
    vec![
        "gemma4",
        "qwen3.5",
        "qwen3-vl",
        "kimi-k2.5",
        "glm-ocr",
        "deepseek-ocr",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_snapshot_read_contract_schema_requires_materialized_state_fields() {
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/graph-snapshot-read.schema.json");
        let schema: Value = serde_json::from_str(
            &std::fs::read_to_string(&schema_path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display())),
        )
        .unwrap();

        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema must define top-level required fields");
        for field in [
            "snapshotId",
            "sourceIngestId",
            "sourceOfTruthPath",
            "latestReadableSnapshotPath",
            "createdAt",
            "materializedAt",
            "materializedPaths",
            "nodes",
            "edges",
            "claims",
            "memoryRefs",
        ] {
            assert!(
                required.iter().any(|value| value.as_str() == Some(field)),
                "schema must require {field}"
            );
        }

        assert_eq!(
            schema
                .pointer("/properties/snapshotId/type")
                .and_then(Value::as_str),
            Some("string")
        );
        assert_eq!(
            schema
                .pointer("/properties/sourceIngestId/type")
                .and_then(Value::as_str),
            Some("string")
        );
        assert_eq!(
            schema
                .pointer("/properties/sourceOfTruthPath/const")
                .and_then(Value::as_str),
            Some("events/brain_events.jsonl")
        );
        assert_eq!(
            schema
                .pointer("/properties/latestReadableSnapshotPath/const")
                .and_then(Value::as_str),
            Some("state/latest-readable-snapshot.json")
        );
        assert_eq!(
            schema
                .pointer("/properties/materializedPaths/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/stringArray")
        );
        assert_eq!(
            schema
                .pointer("/properties/nodes/items/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/node")
        );
        assert_eq!(
            schema
                .pointer("/properties/edges/items/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/edge")
        );
        assert_eq!(
            schema
                .pointer("/properties/claims/items/$ref")
                .and_then(Value::as_str),
            Some("#/$defs/claim")
        );
        assert_eq!(
            schema
                .pointer("/properties/memoryRefs/items/type")
                .and_then(Value::as_str),
            Some("string")
        );
    }

    #[test]
    fn parse_request_round_trip() {
        let request = EngineRequest::Parse(ParseRequest {
            version: "1".into(),
            input: ParseInput {
                path: "/tmp/sample.pdf".into(),
                format: DocumentFormat::Pdf,
            },
            template: "General".into(),
            options: ParseOptions::default(),
            output: Some(ParseOutputTarget {
                root_dir: Some("/tmp/out".into()),
                name: Some("sample".into()),
                workspace_id: Some("default".into()),
                source_id: None,
            }),
        });

        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn runtime_request_envelope_round_trip() {
        let request = EngineRuntimeRequest {
            id: Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap(),
            request: EngineRequest::LoadConfig(LoadConfigRequest {}),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"command\":\"load_config\""));
        let decoded: EngineRuntimeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn runtime_response_envelope_round_trip() {
        let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
        let response = EngineRuntimeResponse::new(
            id,
            EngineSuccess::new(
                EngineCommand::LoadConfig,
                serde_json::json!({"ready": true}),
            ),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"type\":\"response\""));
        assert!(json.contains("\"command\":\"load_config\""));

        let decoded: EngineRuntimeResponse<serde_json::Value> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn runtime_event_envelope_round_trip() {
        let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
        let event = EngineRuntimeEvent::new(id, ParseEvent::Queued);

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"event\""));
        assert!(json.contains("\"event\":{\"type\":\"queued\"}"));

        let decoded: EngineRuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn parse_success_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::Parse,
            ParseResponseData {
                result: ParseResult {
                    version: "1".into(),
                    markdown: "# sample".into(),
                    pages: vec![ParsedPage {
                        index: 0,
                        markdown: Some("# page".into()),
                        plain_text: Some("page".into()),
                        svg: None,
                        image_asset_path: Some("images/page_1.png".into()),
                        error_message: None,
                    }],
                    assets: vec![OutputAsset {
                        relative_path: "images/page_1.png".into(),
                        mime_type: "image/png".into(),
                        base64: "cG5n".into(),
                    }],
                    metadata: ParseMetadata {
                        engine_id: "stub".into(),
                        duration_ms: 5,
                        page_count: 1,
                    },
                    success_count: 1,
                    failed_count: 0,
                },
                saved_output_path: Some("/tmp/out/sample.md".into()),
                source_manifest: None,
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<ParseResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn config_success_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::LoadConfig,
            EngineConfigPayload {
                provider: "open_router".into(),
                model_id: "openai/gpt-4.1-mini".into(),
                api_key: "key".into(),
                base_url: None,
                prompt_template: "General".into(),
                provider_options: vec![ProviderOption {
                    id: "open_router".into(),
                    label: "OpenRouter".into(),
                    requires_api_key: true,
                    supports_base_url: true,
                }],
                model_options: vec!["openai/gpt-4.1-mini".into()],
                prompt_template_options: vec!["General".into()],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<EngineConfigPayload> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn load_project_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::LoadProject,
            LoadProjectResponseData {
                project: None,
                workspace_id: Some("default".into()),
                sources: Vec::new(),
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<LoadProjectResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::LoadProject);
        assert!(decoded.data.project.is_none());
        assert_eq!(decoded.data.workspace_id.as_deref(), Some("default"));
    }

    #[test]
    fn source_artifact_contract_round_trip() {
        let manifest = SourceArtifactManifest {
            workspace_id: "default".into(),
            source_id: "source-123".into(),
            original_path: "/tmp/input.pdf".into(),
            source_path: "/tmp/HyprDuck/default/sources/source-123/input.pdf".into(),
            markdown_path: "/tmp/HyprDuck/default/artifacts/source-123/input.md".into(),
            artifact_root: "/tmp/HyprDuck/default/artifacts/source-123".into(),
            manifest_path: "/tmp/HyprDuck/default/artifacts/source-123/source-manifest.json".into(),
            format: DocumentFormat::Pdf,
            output_name: "input".into(),
            status: IngestStatus::Ingested,
            description: "Project brief".into(),
            user_context: "Used for planning".into(),
            ingest_instruction: "Extract decisions".into(),
            pages: vec![PageArtifact {
                index: 0,
                label: "Page 1".into(),
                image_path: Some(
                    "/tmp/HyprDuck/default/artifacts/source-123/images/page_1.png".into(),
                ),
                markdown_path: Some(
                    "/tmp/HyprDuck/default/artifacts/source-123/pages/page_1.md".into(),
                ),
                plain_text_path: None,
                error_message: None,
            }],
            created_at: 1,
            updated_at: 2,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"status\":\"ingested\""));
        assert!(json.contains("\"format\":\"pdf\""));
        assert!(json.contains("\"description\":\"Project brief\""));
        let decoded: SourceArtifactManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn answer_project_round_trip() {
        let request = EngineRequest::AnswerProject(AnswerProjectRequest {
            project_id: "project-123".into(),
            node_id: Some("concept-a".into()),
            question: "What does this concept cover?".into(),
        });
        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);

        let response = EngineSuccess::new(
            EngineCommand::AnswerProject,
            AnswerProjectResponseData {
                answer: AnswerResponse {
                    status: AnswerStatus::Grounded,
                    text: Some("Grounded answer".into()),
                    explanation: "Based on visible evidence.".into(),
                    citations: vec![],
                    related_node_ids: vec!["concept-b".into()],
                    suggested_actions: vec![],
                },
            },
        );
        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<AnswerProjectResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::AnswerProject);
        assert_eq!(decoded.data.answer.status, AnswerStatus::Grounded);
    }

    #[test]
    fn brain_api_requests_round_trip() {
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: Some("/tmp/HyprDuck".into()),
        };
        let requests = vec![
            EngineRequest::SearchBrain(SearchBrainRequest {
                scope: scope.clone(),
                query: "agent context".into(),
                limit: Some(5),
            }),
            EngineRequest::ReadSource(ReadSourceRequest {
                scope: scope.clone(),
                source_id: "source-123".into(),
            }),
            EngineRequest::ReadWikiPage(ReadWikiPageRequest {
                scope: scope.clone(),
                path: "wiki/index.md".into(),
            }),
            EngineRequest::ReadNode(ReadNodeRequest {
                scope: scope.clone(),
                node_id: "concept-agent-context".into(),
            }),
            EngineRequest::ReadRecentEvents(ReadRecentEventsRequest {
                scope: scope.clone(),
                limit: Some(3),
                run_id: None,
                source_ref: None,
                node_id: None,
                edge_id: None,
                claim_id: None,
                memory_id: None,
                change_type: None,
            }),
            EngineRequest::ReadGraphHistory(ReadGraphHistoryRequest {
                scope: scope.clone(),
                limit: Some(3),
            }),
            EngineRequest::ReadGraphSnapshot(ReadGraphSnapshotRequest {
                scope: scope.clone(),
            }),
            EngineRequest::GetContextPack(GetContextPackRequest {
                scope: scope.clone(),
                query: "agent context".into(),
                budget: Some(8000),
                persist: false,
            }),
            EngineRequest::GetBrainHealth(GetBrainHealthRequest { scope }),
        ];
        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, request);
        }

        let response = EngineSuccess::new(
            EngineCommand::SearchBrain,
            SearchBrainResponseData {
                results: vec![BrainSearchResult {
                    kind: BrainSearchResultKind::WikiPage,
                    id: "wiki-index".into(),
                    title: "Brain Index".into(),
                    path: Some("wiki/index.md".into()),
                    score: 2,
                    snippet: "Agent context".into(),
                }],
            },
        );
        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<SearchBrainResponseData> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::SearchBrain);
        assert_eq!(
            decoded.data.results[0].kind,
            BrainSearchResultKind::WikiPage
        );
    }

    #[test]
    fn context_pack_v0_schema_requires_agent_facing_fields() {
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/context-pack.schema.json");
        let schema: Value = serde_json::from_str(
            &std::fs::read_to_string(&schema_path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display())),
        )
        .unwrap();

        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            CONTEXT_PACK_V0_SCHEMA_VERSION
        );
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema must define top-level required fields");
        for field in [
            "schemaVersion",
            "packId",
            "workspaceId",
            "query",
            "generatedAt",
            "sourceSet",
            "selectedEvidence",
            "findings",
            "warnings",
            "retrievalTrace",
            "suggestedNextReads",
        ] {
            assert!(
                required.iter().any(|value| value.as_str() == Some(field)),
                "schema must require {field}"
            );
        }

        let finding_required = schema
            .pointer("/$defs/finding/required")
            .and_then(Value::as_array)
            .expect("finding must define required fields");
        assert!(finding_required
            .iter()
            .any(|value| value.as_str() == Some("derivedFrom")));
        assert_eq!(
            schema.pointer("/$defs/finding/properties/status/const"),
            Some(&Value::String("derived_summary".into()))
        );
    }

    #[test]
    fn source_pack_and_evidence_index_schemas_require_import_artifact_fields() {
        let source_pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/source-pack.schema.json");
        let source_pack_schema: Value =
            serde_json::from_str(&std::fs::read_to_string(&source_pack_path).unwrap_or_else(
                |err| panic!("failed to read {}: {err}", source_pack_path.display()),
            ))
            .unwrap();
        assert_eq!(
            source_pack_schema["properties"]["schemaVersion"]["const"],
            SOURCE_PACK_V0_SCHEMA_VERSION
        );
        for field in ["sourceId", "contentHash", "pages", "warnings"] {
            assert!(source_pack_schema["required"]
                .as_array()
                .expect("source pack required")
                .iter()
                .any(|value| value.as_str() == Some(field)));
        }

        let evidence_index_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/evidence-index.schema.json");
        let evidence_index_schema: Value = serde_json::from_str(
            &std::fs::read_to_string(&evidence_index_path).unwrap_or_else(|err| {
                panic!("failed to read {}: {err}", evidence_index_path.display())
            }),
        )
        .unwrap();
        assert_eq!(
            evidence_index_schema["properties"]["schemaVersion"]["const"],
            EVIDENCE_INDEX_V0_SCHEMA_VERSION
        );
        let evidence_required = evidence_index_schema
            .pointer("/$defs/evidence/required")
            .and_then(Value::as_array)
            .expect("evidence required");
        for field in [
            "evidenceRef",
            "sourceId",
            "page",
            "region",
            "quotedText",
            "contentHash",
        ] {
            assert!(evidence_required
                .iter()
                .any(|value| value.as_str() == Some(field)));
        }
    }

    #[test]
    fn source_pack_and_evidence_index_round_trip() {
        let warning = SourcePackWarningV0 {
            warning_type: "page_parse_failed".into(),
            severity: ContextPackWarningSeverity::High,
            message: "Page failed".into(),
            page: Some(2),
        };
        let source_pack = SourcePackV0 {
            schema_version: SOURCE_PACK_V0_SCHEMA_VERSION.into(),
            workspace_id: "default".into(),
            source_id: "source-alpha".into(),
            original_filename: "sample.pdf".into(),
            original_path: "/tmp/sample.pdf".into(),
            source_path: "/tmp/HyprDuck/default/sources/source-alpha/sample.pdf".into(),
            markdown_path: "/tmp/HyprDuck/default/artifacts/source-alpha/sample.md".into(),
            artifact_root: "/tmp/HyprDuck/default/artifacts/source-alpha".into(),
            content_hash: "fnv64:abc123".into(),
            format: DocumentFormat::Pdf,
            page_count: 2,
            ingestion_status: IngestStatus::Partial,
            provider_route: "unknown".into(),
            local_only: false,
            pages: vec![SourcePackPageV0 {
                page: 1,
                label: "Page 1".into(),
                image_path: Some("/tmp/page_1.png".into()),
                markdown_path: Some("/tmp/page_1.md".into()),
                plain_text_path: None,
                error_message: None,
            }],
            warnings: vec![warning.clone()],
            created_at: 1,
            updated_at: 2,
        };
        let decoded: SourcePackV0 =
            serde_json::from_str(&serde_json::to_string(&source_pack).unwrap()).unwrap();
        assert_eq!(decoded, source_pack);

        let evidence_index = EvidenceIndexV0 {
            schema_version: EVIDENCE_INDEX_V0_SCHEMA_VERSION.into(),
            workspace_id: "default".into(),
            source_id: "source-alpha".into(),
            content_hash: "fnv64:abc123".into(),
            provider_route: "unknown".into(),
            local_only: false,
            evidence: vec![EvidenceIndexItemV0 {
                evidence_ref: "ev-source-alpha-source-1".into(),
                source_id: "source-alpha".into(),
                page: 1,
                region: "page:Page 1".into(),
                span: Some("page".into()),
                quoted_text: "Evidence text.".into(),
                parse_confidence: ContextPackParseConfidence::Unknown,
                content_hash: "fnv64:abc123".into(),
                markdown_path: Some("/tmp/page_1.md".into()),
                image_path: Some("/tmp/page_1.png".into()),
            }],
            warnings: vec![warning],
            generated_at: 3,
        };
        let decoded: EvidenceIndexV0 =
            serde_json::from_str(&serde_json::to_string(&evidence_index).unwrap()).unwrap();
        assert_eq!(decoded, evidence_index);
    }

    #[test]
    fn context_pack_v0_round_trip_preserves_evidence_backed_findings() {
        let pack = ContextPackV0 {
            schema_version: CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
            pack_id: "ctx_20260518_0001".into(),
            workspace_id: "default".into(),
            query: "What does the document say about agent reuse?".into(),
            generated_at: "2026-05-18T09:00:00Z".into(),
            source_set: vec![ContextPackSourceV0 {
                source_id: "src_agent_context".into(),
                original_filename: "agent-context.pdf".into(),
                content_hash: "sha256:abc123".into(),
                page_count: 2,
                ingestion_status: "ingested".into(),
                staleness: ContextPackStaleness::Current,
                provider_route: "ollama".into(),
                local_only: true,
            }],
            selected_evidence: vec![ContextPackEvidenceV0 {
                evidence_ref: "ev_src_agent_context_p1_b1".into(),
                source_id: "src_agent_context".into(),
                page: 1,
                region: Some("p1-block1".into()),
                span: Some("char:0-42".into()),
                quoted_text: "Context packs are reusable by coding agents.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                selection_reason: "Directly answers the reuse question.".into(),
                content_hash: "sha256:abc123".into(),
            }],
            findings: vec![ContextPackFindingV0 {
                finding_id: "f_agent_reuse".into(),
                statement: "The document says context packs can be reused by coding agents.".into(),
                status: ContextPackFindingStatus::DerivedSummary,
                statement_confidence: ContextPackParseConfidence::High,
                derived_from: vec!["ev_src_agent_context_p1_b1".into()],
                relevance_reason: "Directly answers the query.".into(),
            }],
            warnings: vec![ContextPackWarningV0 {
                warning_type: "visual_content_not_fully_parsed".into(),
                severity: ContextPackWarningSeverity::Medium,
                message: "A diagram may need visual inspection.".into(),
                page_refs: vec![ContextPackPageRefV0 {
                    source_id: "src_agent_context".into(),
                    page: 2,
                }],
            }],
            retrieval_trace: ContextPackRetrievalTraceV0 {
                strategy: "local-text-search+evidence-expansion".into(),
                chunks_considered: 4,
                chunks_selected: 1,
                budget_requested: 4000,
                budget_used: 1200,
            },
            suggested_next_reads: vec![ContextPackSuggestedNextReadV0 {
                source_id: "src_agent_context".into(),
                page: 2,
                reason: "Related diagram.".into(),
            }],
        };

        let json = serde_json::to_string(&pack).unwrap();
        assert!(json.contains("\"schemaVersion\":\"hyprduck.context_pack.v0\""));
        assert!(json.contains("\"selectedEvidence\""));
        assert!(json.contains("\"derivedFrom\""));
        let decoded: ContextPackV0 = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, pack);

        let selected = decoded
            .selected_evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(decoded.findings.iter().all(|finding| finding
            .derived_from
            .iter()
            .all(|evidence_ref| selected.contains(evidence_ref.as_str()))));
    }

    #[test]
    fn context_pack_v0_can_project_internal_brain_context_pack() {
        let internal = BrainContextPack {
            workspace_id: "default".into(),
            query: "agent reuse".into(),
            token_budget: 4000,
            summary: "Agent context reuse summary.".into(),
            wiki_pages: vec![],
            nodes: vec![],
            sources: vec![SourceRecord {
                source_id: "src_agent_context".into(),
                workspace_id: "default".into(),
                original_path: "/tmp/agent-context.pdf".into(),
                source_path: "/tmp/HyprDuck/default/sources/src_agent_context.pdf".into(),
                markdown_path: "/tmp/HyprDuck/default/sources/src_agent_context.md".into(),
                format: SourceFormat::pdf(),
                status: SourceStatus::ingested(),
                page_count: 2,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            }],
            memories: vec![],
            entities: vec![],
            claims: vec![ClaimRecord {
                claim_id: "claim_agent_reuse".into(),
                workspace_id: "default".into(),
                statement: "Context packs can be reused by agents.".into(),
                topic_refs: vec![],
                source_refs: vec!["src_agent_context".into()],
                evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
                status: "active".into(),
                updated_at: 2,
            }],
            relations: vec![],
            evidence: vec![EvidenceRef {
                id: "ev_src_agent_context_p1_b1".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Context packs are reusable by coding agents.".into(),
                source_path: None,
                source_id: Some("src_agent_context".into()),
                markdown_path: Some("/tmp/HyprDuck/default/sources/src_agent_context.md".into()),
                image_path: Some("/tmp/HyprDuck/default/artifacts/page_1.png".into()),
                provenance: Some("markdown_extract".into()),
            }],
            recent_events: vec![],
            warnings: vec!["budget truncated context pack".into()],
        };

        let source_metadata = BTreeMap::from([(
            "src_agent_context".into(),
            ContextPackSourceMetadataV0 {
                content_hash: "sha256:abc123".into(),
                provider_route: "ollama".into(),
                local_only: true,
            },
        )]);
        let external = ContextPackV0::from_brain_context_pack(
            &internal,
            "ctx_test",
            "2026-05-18T09:00:00Z",
            &source_metadata,
        );
        assert_eq!(external.schema_version, CONTEXT_PACK_V0_SCHEMA_VERSION);
        assert_eq!(
            external.source_set[0].original_filename,
            "agent-context.pdf"
        );
        assert_eq!(external.source_set[0].content_hash, "sha256:abc123");
        assert_eq!(external.source_set[0].provider_route, "ollama");
        assert_eq!(external.selected_evidence[0].page, 1);
        assert_eq!(external.selected_evidence[0].content_hash, "sha256:abc123");
        assert_eq!(
            external.findings[0].derived_from,
            vec!["ev_src_agent_context_p1_b1"]
        );
        assert_eq!(
            external.warnings[0].warning_type,
            "internal_context_warning"
        );
    }

    #[test]
    fn context_pack_v0_projection_warns_instead_of_emitting_unhashed_evidence() {
        let internal = BrainContextPack {
            workspace_id: "default".into(),
            query: "agent reuse".into(),
            token_budget: 4000,
            summary: "Agent context reuse summary.".into(),
            wiki_pages: vec![],
            nodes: vec![],
            sources: vec![],
            memories: vec![],
            entities: vec![],
            claims: vec![ClaimRecord {
                claim_id: "claim_agent_reuse".into(),
                workspace_id: "default".into(),
                statement: "Context packs can be reused by agents.".into(),
                topic_refs: vec![],
                source_refs: vec!["src_agent_context".into()],
                evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
                status: "active".into(),
                updated_at: 2,
            }],
            relations: vec![],
            evidence: vec![EvidenceRef {
                id: "ev_src_agent_context_p1_b1".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Context packs are reusable by coding agents.".into(),
                source_path: None,
                source_id: Some("src_agent_context".into()),
                markdown_path: None,
                image_path: None,
                provenance: Some("markdown_extract".into()),
            }],
            recent_events: vec![],
            warnings: vec![],
        };

        let external = ContextPackV0::from_brain_context_pack(
            &internal,
            "ctx_test",
            "2026-05-18T09:00:00Z",
            &BTreeMap::new(),
        );
        assert!(external.selected_evidence.is_empty());
        assert!(external.findings.is_empty());
        assert_eq!(
            external.warnings[0].warning_type,
            "evidence_missing_content_hash"
        );
    }

    #[test]
    fn provider_model_catalog_round_trip() {
        let mut provider_models = BTreeMap::new();
        provider_models.insert("open_router".into(), vec!["openai/gpt-4.1-mini".into()]);
        provider_models.insert("ollama".into(), vec!["qwen3-vl:8b".into()]);

        let response = EngineSuccess::new(
            EngineCommand::ListProviderModels,
            ProviderModelCatalogResponseData {
                provider_models,
                ollama_vision_prefixes: vec!["qwen3-vl".into()],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        let decoded: EngineSuccess<ProviderModelCatalogResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::ListProviderModels);
        assert!(decoded.data.provider_models.contains_key("open_router"));
    }

    #[test]
    fn readiness_response_round_trip() {
        let response = EngineSuccess::new(
            EngineCommand::CheckReadiness,
            RuntimeReadinessResponseData {
                ready: true,
                provider: "ollama".into(),
                model_id: "qwen3-vl:8b".into(),
                checks: vec![ReadinessCheck {
                    id: "runtime_process".into(),
                    label: "Runtime process".into(),
                    ready: true,
                    required: true,
                    message: "Runtime process is accepting commands.".into(),
                }],
            },
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"command\":\"check_readiness\""));
        let decoded: EngineSuccess<RuntimeReadinessResponseData> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, EngineCommand::CheckReadiness);
        assert!(decoded.data.ready);
    }

    #[test]
    fn failure_round_trip() {
        let failure = EngineFailure::new(
            EngineCommand::ValidateProvider,
            "invalid_api_key",
            "missing key",
        );
        let json = serde_json::to_string(&failure).unwrap();
        let decoded: EngineFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, failure);
    }

    #[test]
    fn event_round_trip() {
        let event = ParseEvent::Parsing {
            current: 1,
            total: 3,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: ParseEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn options_decode_with_missing_fields() {
        let decoded: ParseOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded, ParseOptions::default());
    }
}
