//! Shared engine command, response, graph, and artifact contracts for HyprDuck.
//!
//! This crate is the wire/DTO surface used by the desktop shell, CLI/MCP, and engine.
//! Modules are split for maintainability; the public API is re-exported from the crate root.

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

mod commands;
mod ingest;
mod import_lifecycle;
mod agent_chat;
mod graph_patch;
mod agent_write;
mod brain;
mod context_pack;
mod knowledge_project;
mod graph_history;
mod config;

pub use commands::*;
pub use ingest::*;
pub use import_lifecycle::*;
pub use agent_chat::*;
pub use graph_patch::*;
pub use agent_write::*;
pub use brain::*;
pub use context_pack::*;
pub use knowledge_project::*;
pub use graph_history::*;
pub use config::*;

#[cfg(test)]
mod tests;
