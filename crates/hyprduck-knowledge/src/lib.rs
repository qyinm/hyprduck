use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Preview,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Source,
    Document,
    Page,
    Concept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Grounded,
    LowConfidence,
    Blocked,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    pub project_id: String,
    pub title: String,
    pub status: ProjectStatus,
    #[serde(default)]
    pub stale: bool,
    pub summary: String,
    pub document_count: usize,
    pub node_count: usize,
    pub relationship_count: usize,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeSummary {
    pub id: String,
    pub label: String,
    pub kind: GraphNodeKind,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub related_count: usize,
    pub evidence_count: usize,
    pub position: GraphNodePosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub id: String,
    pub page_label: String,
    #[serde(default)]
    pub page_index: Option<usize>,
    pub snippet: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBacking {
    pub workspace_id: String,
    pub source_id: String,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: String,
    pub status: String,
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
    #[serde(default)]
    pub manifest_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    Merge,
    KeepSeparate,
    Rename,
    Split,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionAction {
    pub kind: CorrectionKind,
    pub label: String,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeDetail {
    pub node: GraphNodeSummary,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub actions: Vec<CorrectionAction>,
    #[serde(default)]
    pub source: Option<SourceBacking>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    SourceDocument,
    RelatedTo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdgeSummary {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub kind: RelationKind,
    pub label: String,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdgeDetail {
    pub edge: RelationEdgeSummary,
    pub explanation: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedActionKind {
    InspectEvidence,
    ApplyCorrection,
    ReimportProject,
    AskDifferentQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedAction {
    pub kind: SuggestedActionKind,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResponse {
    pub status: AnswerStatus,
    #[serde(default)]
    pub text: Option<String>,
    pub explanation: String,
    #[serde(default)]
    pub citations: Vec<EvidenceRef>,
    #[serde(default)]
    pub related_node_ids: Vec<String>,
    #[serde(default)]
    pub suggested_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeProject {
    pub summary: ProjectOverview,
    pub nodes: Vec<GraphNodeSummary>,
    #[serde(default)]
    pub edges: Vec<RelationEdgeSummary>,
    #[serde(default)]
    pub details_by_node_id: BTreeMap<String, GraphNodeDetail>,
    #[serde(default)]
    pub edge_details_by_id: BTreeMap<String, RelationEdgeDetail>,
    #[serde(default)]
    pub answer_by_node_id: BTreeMap<String, AnswerResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCorrection {
    pub id: String,
    pub workspace_id: String,
    pub aggregate_node_id: String,
    pub kind: CorrectionKind,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub source_node_ids: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainScope {
    Personal,
    Project,
    Team,
    Company,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainNodeKind {
    Source,
    Memory,
    WikiPage,
    Person,
    Company,
    Project,
    Product,
    Team,
    Event,
    Decision,
    Task,
    Claim,
    Topic,
    Concept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainRelationKind {
    Mentions,
    Supports,
    Contradicts,
    Supersedes,
    SameAs,
    WorksAt,
    Founded,
    InvestedIn,
    Advises,
    Attended,
    Owns,
    ResponsibleFor,
    Decided,
    Blocks,
    DependsOn,
    SourceOf,
    DerivedFrom,
    RelatedTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainActorType {
    System,
    User,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainActor {
    pub actor_type: BrainActorType,
    pub actor_id: String,
}

pub const BRAIN_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainEventKind {
    SourceImported,
    SourceIngestQueued,
    SourceCompiled,
    GraphMaterialized,
    WikiMaterialized,
    CorrectionApplied,
    NodeProposed,
    MemoryProposed,
    ClaimProposed,
    LinkProposed,
    ObservationAppended,
    SourceNoteProposed,
    WikiPageProposed,
    MemoryAccepted,
    ReviewCreated,
    ReviewResolved,
    BrainMaintenanceRun,
}

fn default_brain_event_schema_version() -> u32 {
    BRAIN_EVENT_SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainEventCausality {
    #[serde(default)]
    pub caused_by_event_ids: Vec<String>,
    #[serde(default)]
    pub caused_by_proposal_id: Option<String>,
    #[serde(default)]
    pub caused_by_source_ids: Vec<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub previous_snapshot_id: Option<String>,
    #[serde(default = "default_brain_event_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub materialized_version: Option<u64>,
}

impl Default for BrainEventCausality {
    fn default() -> Self {
        Self {
            caused_by_event_ids: Vec::new(),
            caused_by_proposal_id: None,
            caused_by_source_ids: Vec::new(),
            snapshot_id: None,
            previous_snapshot_id: None,
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            materialized_version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainEvent {
    pub event_id: String,
    #[serde(default = "default_brain_event_schema_version")]
    pub schema_version: u32,
    pub workspace_id: String,
    pub scope: BrainScope,
    pub event_type: BrainEventKind,
    #[serde(default)]
    pub operation_type: Option<String>,
    pub actor: BrainActor,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub source_markdown_refs: Vec<String>,
    #[serde(default)]
    pub node_refs: Vec<String>,
    #[serde(default)]
    pub relation_refs: Vec<String>,
    #[serde(default)]
    pub claim_refs: Vec<String>,
    #[serde(default)]
    pub memory_refs: Vec<String>,
    #[serde(default)]
    pub target_node_ids: Vec<String>,
    #[serde(default)]
    pub target_edge_ids: Vec<String>,
    #[serde(default)]
    pub target_claim_ids: Vec<String>,
    #[serde(default)]
    pub target_memory_ids: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub payload_json: String,
    #[serde(default)]
    pub causality: BrainEventCausality,
    #[serde(default)]
    pub confidence: Option<String>,
    pub policy_result: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub source_id: String,
    pub workspace_id: String,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub format: String,
    pub status: String,
    pub page_count: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_context: String,
    #[serde(default)]
    pub ingest_instruction: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainNodeRecord {
    pub node_id: String,
    pub kind: BrainNodeKind,
    pub label: String,
    pub scope: BrainScope,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainRelationRecord {
    pub relation_id: String,
    pub kind: BrainRelationKind,
    pub source_node_id: String,
    pub target_node_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    pub memory_id: String,
    pub workspace_id: String,
    pub scope: BrainScope,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiPage {
    pub page_id: String,
    pub workspace_id: String,
    pub path: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub node_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRecord {
    pub entity_id: String,
    pub workspace_id: String,
    pub kind: BrainNodeKind,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRecord {
    pub claim_id: String,
    pub workspace_id: String,
    pub statement: String,
    #[serde(default)]
    pub topic_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub status: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionPageRef {
    pub page_label: String,
    #[serde(default)]
    pub page_index: Option<usize>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionEntity {
    pub entity_id: String,
    pub kind: BrainNodeKind,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionTopic {
    pub topic_id: String,
    pub title: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionClaim {
    pub claim_id: String,
    pub statement: String,
    #[serde(default)]
    pub subject_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub status: String,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionRelation {
    pub relation_id: String,
    pub kind: BrainRelationKind,
    pub source_node_id: String,
    pub target_node_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionMemoryCandidate {
    pub memory_id: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub status: String,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExtractionArtifact {
    pub artifact_id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub extractor: String,
    #[serde(default)]
    pub extractor_model: Option<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub page_refs: Vec<StructuredExtractionPageRef>,
    #[serde(default)]
    pub entities: Vec<StructuredExtractionEntity>,
    #[serde(default)]
    pub topics: Vec<StructuredExtractionTopic>,
    #[serde(default)]
    pub claims: Vec<StructuredExtractionClaim>,
    #[serde(default)]
    pub relations: Vec<StructuredExtractionRelation>,
    #[serde(default)]
    pub memories: Vec<StructuredExtractionMemoryCandidate>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub provenance: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainRepoSnapshot {
    pub workspace_id: String,
    pub generated_at: u64,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
    #[serde(default)]
    pub nodes: Vec<BrainNodeRecord>,
    #[serde(default)]
    pub relations: Vec<BrainRelationRecord>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub memories: Vec<MemoryRecord>,
    #[serde(default)]
    pub wiki_pages: Vec<WikiPage>,
    #[serde(default)]
    pub entities: Vec<EntityRecord>,
    #[serde(default)]
    pub claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub extractions: Vec<StructuredExtractionArtifact>,
    #[serde(default)]
    pub events: Vec<BrainEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainProposalKind {
    Node,
    Memory,
    Claim,
    Link,
    Observation,
    SourceNote,
    WikiPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainProposalStatus {
    PendingReview,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainUpdateProposal {
    pub proposal_id: String,
    pub workspace_id: String,
    pub kind: BrainProposalKind,
    pub status: BrainProposalStatus,
    pub actor: BrainActor,
    pub scope: BrainScope,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub target_source_id: Option<String>,
    #[serde(default)]
    pub relation_kind: Option<BrainRelationKind>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub node_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub proposal_payload: Option<AgentGraphProposalPayload>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "changeType", rename_all = "snake_case")]
pub enum AgentGraphProposalPayload {
    NewNode { node: AgentNewNodePayload },
    NewEdge { edge: AgentNewEdgePayload },
    NewClaim { claim: AgentNewClaimPayload },
    NewMemory { memory: AgentNewMemoryPayload },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNewNodePayload {
    pub label: String,
    pub kind: BrainNodeKind,
    pub source_path: String,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNewEdgePayload {
    pub source_node_id: String,
    pub target_node_id: String,
    pub kind: BrainRelationKind,
    pub label: String,
    pub source_path: String,
    #[serde(default)]
    pub edge_id: Option<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNewClaimPayload {
    pub statement: String,
    pub source_path: String,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub topic_refs: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNewMemoryPayload {
    pub title: String,
    pub body: String,
    pub source_path: String,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentGraphProposalValidationCode {
    MissingRequiredField,
    MissingSourceRefs,
    MissingEvidenceRefs,
    MissingNodeRefs,
    MissingTopicRefs,
    MissingTargetNode,
    MissingRelationKind,
    KindPayloadMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentGraphProposalValidationIssue {
    pub code: AgentGraphProposalValidationCode,
    pub field: String,
    pub message: String,
}

impl AgentGraphProposalValidationIssue {
    pub fn new(
        code: AgentGraphProposalValidationCode,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field: field.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentGraphProposalValidationError {
    pub error: String,
    pub issues: Vec<AgentGraphProposalValidationIssue>,
}

impl AgentGraphProposalValidationError {
    pub fn new(issues: Vec<AgentGraphProposalValidationIssue>) -> Self {
        Self {
            error: "invalid_agent_graph_proposal".into(),
            issues,
        }
    }
}

impl std::fmt::Display for AgentGraphProposalValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:", self.error)?;
        for issue in &self.issues {
            write!(formatter, " [{}] {}", issue.field, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for AgentGraphProposalValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_response_serializes_expected_status_shape() {
        let response = AnswerResponse {
            status: AnswerStatus::LowConfidence,
            text: Some("Need more evidence before answering confidently.".into()),
            explanation: "Preview answer is grounded by visible snippets only.".into(),
            citations: vec![EvidenceRef {
                id: "ev-1".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Visible evidence snippet".into(),
                source_path: Some("sample.pdf".into()),
                source_id: Some("source-1".into()),
                markdown_path: Some("pages/page_1.md".into()),
                image_path: Some("images/page_1.png".into()),
                provenance: Some("Test evidence".into()),
            }],
            related_node_ids: vec!["page-1".into()],
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::InspectEvidence,
                label: "Inspect evidence".into(),
                description: "Review the cited snippets before trusting the draft answer.".into(),
            }],
        };

        let encoded = serde_json::to_string(&response).expect("serialize answer response");
        assert!(encoded.contains("\"status\":\"low_confidence\""));
        assert!(encoded.contains("\"relatedNodeIds\":[\"page-1\"]"));
    }

    #[test]
    fn relation_edge_uses_camel_case_wire_shape() {
        let edge = RelationEdgeSummary {
            id: "edge-a-b".into(),
            source_node_id: "concept-a".into(),
            target_node_id: "concept-b".into(),
            kind: RelationKind::RelatedTo,
            label: "Related in source".into(),
            confidence: Some(0.74),
            evidence_count: 2,
        };

        let encoded = serde_json::to_string(&edge).expect("serialize relation edge");
        assert!(encoded.contains("\"sourceNodeId\":\"concept-a\""));
        assert!(encoded.contains("\"targetNodeId\":\"concept-b\""));
        assert!(encoded.contains("\"kind\":\"related_to\""));
    }
}
