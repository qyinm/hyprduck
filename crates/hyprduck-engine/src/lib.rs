use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use hyprduck_engine_types::{
    graph_status_is_ready, AnswerProjectRequest, AnswerProjectResponseData, AnswerResponse,
    AnswerStatus, ApplyCorrectionRequest, ApplyCorrectionResponseData, BrainActor, BrainActorType,
    BrainContextPack, BrainEvent, BrainEventCausality, BrainEventKind, BrainGovernanceReport,
    BrainHealthSourceReport, BrainHealthStatus, BrainKnowledgeStoreReport, BrainNodeKind,
    BrainNodeRecord, BrainReadScope, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot,
    BrainScope, BrainSearchResult, BrainSearchResultKind, ClaimRecord, CompileProjectRequest,
    CompileProjectResponseData, CorrectionAction, CorrectionKind, DocumentFormat, EngineCommand,
    EngineFailure, EntityRecord, EvidenceRef, GetBrainHealthRequest, GetBrainHealthResponseData,
    GetContextPackRequest, GetContextPackResponseData, GraphNodeDetail, GraphNodeKind,
    GraphNodePosition, GraphNodeSummary, ImportLifecycleState, ImportLifecycleStatus, IngestStatus,
    KnowledgeProject, LoadProjectRequest, LoadProjectResponseData, MemoryRecord, PageArtifact,
    PageEvidenceV0, ProjectOverview, ProjectStatus, ReadContextPackRequest,
    ReadContextPackResponseData, ReadImportJobRequest, ReadImportJobResponseData, ReadNodeRequest,
    ReadNodeResponseData, ReadPageEvidenceRequest, ReadPageEvidenceResponseData,
    ReadRecentEventsRequest, ReadRecentEventsResponseData, ReadSourceRequest,
    ReadSourceResponseData, ReadWikiPageRequest, ReadWikiPageResponseData, ReconstructBrainRequest,
    ReconstructBrainResponseData, RelationEdgeDetail, RelationEdgeSummary, RelationKind,
    SearchBrainRequest, SearchBrainResponseData, SourceArtifactManifest, SourceBacking, SourceId,
    SourceRecord, SourceStatus, SourceSummary, StructuredExtractionArtifact,
    StructuredExtractionClaim, StructuredExtractionEntity, StructuredExtractionMemoryCandidate,
    StructuredExtractionPageRef, StructuredExtractionRelation, StructuredExtractionTopic,
    SuggestedAction, SuggestedActionKind, UpdateImportJobGraphStatusRequest,
    UpdateImportJobGraphStatusResponseData, WikiPage, WorkspaceCorrection, WorkspaceId,
    BRAIN_EVENT_SCHEMA_VERSION,
};
#[cfg(test)]
use hyprduck_engine_types::{
    ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackSourceMetadataV0,
    EvidenceIndexV0, OutputAsset, ParseInput, ParseMetadata, ParseOptions, ParseRequest,
    ParseResult, ParsedPage, RetryFailedPagesRequest, RetryPageArtifactUpdate, SourcePackV0,
    WriteCommitAllRequest, WriteCommitRequest, WriteListRequest, WriteProposeRequest,
    WriteRejectRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(test)]
use std::io::ErrorKind;
#[cfg(test)]
use tempfile::tempdir;
use uuid::Uuid;

mod adapters;
mod application;
mod domains;
mod graph_commit;
mod graph_history;
mod graph_patch_policy;
mod infra;
mod policy;
mod ports;
pub mod runtime;
mod search_context;

#[cfg(test)]
pub(crate) use application::commands::brain_write::{
    handle_write_commit, handle_write_commit_all, handle_write_list, handle_write_propose,
    handle_write_reject,
};
#[cfg(test)]
pub(crate) use application::commands::graph::handle_apply_graph_patch;
pub(crate) use application::services::brain_health_service::{
    brain_node_record_content_matches, brain_relation_record_content_matches, is_wiki_markdown_ref,
    lint_brain_snapshot, materialized_graph_event_payload_json, missing_refs,
    refresh_materialized_wiki_pages,
};
#[cfg(test)]
pub(crate) use application::services::brain_health_service::{
    handle_get_brain_health, lint_missing_materialized_wiki_refs, run_brain_maintenance,
    BrainMaintenanceReport,
};
#[cfg(test)]
pub(crate) use application::services::brain_read_service::{
    handle_read_node, handle_read_page_evidence, handle_read_recent_events, handle_read_source,
    handle_search_brain,
};
#[cfg(test)]
pub(crate) use application::services::context_pack_service::{
    handle_get_context_pack, handle_read_context_pack, persist_context_pack_v1,
};
#[cfg(test)]
pub(crate) use application::services::project_service::{
    handle_answer_project, handle_apply_correction, handle_load_project, load_answerable_project,
};

mod agent_workflow {
    pub(crate) use crate::domains::agent_workflow::*;
}

mod brain_repo {
    pub(crate) use crate::domains::brain::materialize::*;
    pub(crate) use crate::domains::brain::projector::*;
    pub(crate) use crate::domains::brain::reader::*;
    pub(crate) use crate::domains::brain::replay::*;
    pub(crate) use crate::domains::brain::repository::*;
    pub(crate) use crate::domains::brain::writer::*;
}

mod chat_openai_compatible_client {
    pub(crate) use crate::adapters::providers::openai_compatible::*;
}

mod import_context {
    pub(crate) use crate::domains::retrieval::import_context::*;
}

mod knowledge {
    pub(crate) use crate::domains::knowledge::*;
}

mod parse {
    #[allow(unused_imports)]
    pub(crate) use crate::domains::ingest::parse::*;
}

mod provider {
    pub(crate) use crate::domains::provider::*;
}

mod retrieval {
    pub(crate) use crate::domains::retrieval::search::*;
}

mod source_index {
    pub(crate) use crate::domains::retrieval::source_index::*;
}

pub(crate) use adapters::persistence::project_store::*;
use adapters::process::binary_locator::resolve_binary;
use agent_workflow::maybe_generate_provider_graph_materialization;
#[cfg(test)]
use agent_workflow::{
    build_workspace_linking_prompt, normalize_provider_source_local_graph_snapshot,
    normalize_provider_workspace_linking_snapshot, normalize_provider_workspace_rebuild_snapshot,
    parse_provider_workspace_rebuild_snapshot, validate_provider_source_local_graph_snapshot,
    validate_provider_workspace_linking_snapshot, validate_provider_workspace_rebuild_snapshot,
};
use brain_repo::*;
use chat_openai_compatible_client::{
    parse_openai_compatible_json_schema_with_timeout, provider_unavailable,
};
use domains::context_pack::artifact_metadata::{
    build_context_pack_artifact_metadata, read_evidence_index_artifact, read_source_pack_v0,
};
#[cfg(test)]
use domains::context_pack::artifact_metadata::{build_context_pack_source_metadata, fnv1a64};
#[cfg(test)]
use domains::ingest::markdown_queue::*;
#[cfg(test)]
use domains::ingest::output_package::retry_failed_page_artifacts;
#[cfg(test)]
use domains::ingest::output_package::write_output_package_with_fallback;
#[cfg(test)]
use domains::ingest::output_package::{build_source_id, write_source_manifest};
use domains::ingest::output_package::{
    load_source_manifest, resolved_source_ids, source_summary_from_manifest,
};
use domains::knowledge_store::KnowledgeStore;
#[allow(unused_imports)]
pub(crate) use graph_history::{
    event_matches_recent_events_request, graph_snapshot_source_ingest_id,
    handle_read_graph_history, handle_read_graph_snapshot, latest_graph_materialized_event,
};
use import_context::{
    build_import_evidence_context, import_evidence_context_allowed_refs, ImportEvidenceContext,
};
use knowledge::*;
#[cfg(test)]
use provider::EngineConfig;
#[allow(unused_imports)]
pub(crate) use search_context::{
    best_snippet, context_pack_warnings, evidence_snippet, match_score, normalize_search_token,
    search_terms, search_token_frequencies, trim_context_pack_to_budget,
};
use source_index::{chunk_source_markdown, read_workspace_source_chunks, upsert_source_chunks};

const DEFAULT_WORKSPACE_ID: &str = "default";
const PROJECT_SNAPSHOT_BATCH_SIZE: usize = 200;
#[cfg(test)]
const MARKDOWN_INGEST_QUEUE_PATH: &str = "state/markdown-ingest-queue.json";
#[cfg(test)]
const MARKDOWN_SOURCE_STATE_PATH: &str = "state/markdown-sources.json";
const LATEST_READABLE_SNAPSHOT_PATH: &str = "state/latest-readable-snapshot.json";
const MATERIALIZED_ARTIFACT_ROLE_MIGRATION_INPUT: &str = "migration_input";
const CANONICAL_STATE_STORE_SQLITE_GRAPHQLITE: &str = "hyprduck.sqlite+graphqlite";
const PROVIDER_GRAPH_AGENT_ID: &str = "hyprduck-provider-graph-agent";
const MCP_WRITE_AGENT_ID: &str = "hyprduck-mcp-write-agent";
const BRAIN_LOCK_DIRECTORY_NAME: &str = ".brain.lock";
const PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS: u64 = 300;

fn encode_failure_response(command: EngineCommand, error: &anyhow::Error) -> String {
    serde_json::to_string(&engine_failure(command, error)).unwrap_or_else(|_| {
        "{\"ok\":false,\"command\":\"validate_provider\",\"error\":{\"code\":\"runtime_error\",\"message\":\"failed to encode engine failure\",\"details\":null}}".to_string()
    })
}

fn engine_failure(command: EngineCommand, error: &anyhow::Error) -> EngineFailure {
    let code = if let Some(provider_code) = provider_failure_code(error) {
        provider_code
    } else if format!("{error:?}").contains("decode") {
        "invalid_request"
    } else if format!("{error:?}").contains("config") {
        "config_error"
    } else {
        "runtime_error"
    };
    EngineFailure::new(command, code, error.to_string())
}

fn provider_failure_code(error: &anyhow::Error) -> Option<&'static str> {
    let message = error.to_string();
    [
        "provider_config",
        "provider_timeout",
        "provider_response_invalid",
        "unsupported_provider",
    ]
    .into_iter()
    .find(|code| message.starts_with(&format!("{code}:")))
}

#[cfg(test)]
mod provider_failure_tests {
    use super::*;

    #[test]
    fn engine_failure_uses_provider_taxonomy_as_error_code() {
        let error = anyhow!("provider_timeout: provider request timed out after 1s");
        let failure = engine_failure(EngineCommand::Parse, &error);

        assert_eq!(failure.error.code, "provider_timeout");
        assert!(failure.error.message.contains("provider_timeout:"));
    }

    #[test]
    fn engine_failure_keeps_non_provider_config_errors_as_config_error() {
        let error = std::io::Error::new(ErrorKind::NotFound, "config file missing");
        let error = anyhow!(error).context("failed reading config");
        let failure = engine_failure(EngineCommand::LoadConfig, &error);

        assert_eq!(failure.error.code, "config_error");
    }
}

pub(crate) fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

pub(crate) fn resolve_brain_workspace_root(scope: &BrainReadScope) -> Result<PathBuf> {
    validate_workspace_id(&scope.workspace_id)?;
    let root = if let Some(root_dir) = &scope.root_dir {
        resolve_workspace_root_from_base(&PathBuf::from(root_dir), &scope.workspace_id)?
    } else if let Some(output_root) = std::env::var_os("HYPRDUCK_OUTPUT_DIR") {
        resolve_workspace_root_from_base(&PathBuf::from(output_root), &scope.workspace_id)?
    } else if let Some(application_support_root) = dirs::data_local_dir() {
        resolve_workspace_root_from_base(
            &application_support_root.join("HyprDuck"),
            &scope.workspace_id,
        )?
    } else {
        resolve_workspace_root_from_base(
            &std::env::temp_dir().join("HyprDuck"),
            &scope.workspace_id,
        )?
    };
    ensure_workspace_knowledge_store(&root)?;
    Ok(root)
}

fn ensure_workspace_knowledge_store(root: &Path) -> Result<()> {
    KnowledgeStore::open(KnowledgeStore::default_path_for_root(root))?.health()?;
    Ok(())
}

fn validate_workspace_id(workspace_id: &str) -> Result<()> {
    if workspace_id.trim().is_empty()
        || workspace_id == "."
        || workspace_id == ".."
        || workspace_id.contains('/')
        || workspace_id.contains('\\')
    {
        bail!("invalid workspaceId: workspace IDs must be single path segments");
    }
    Ok(())
}

fn resolve_workspace_root_from_base(base_root: &Path, workspace_id: &str) -> Result<PathBuf> {
    let canonical_base = canonicalize_existing_or_parent(base_root)?;
    let workspace_root = canonical_base.join(workspace_id);
    if workspace_root.exists() {
        let canonical_workspace = workspace_root
            .canonicalize()
            .with_context(|| format!("failed canonicalizing {}", workspace_root.display()))?;
        if !canonical_workspace.starts_with(&canonical_base) {
            bail!(
                "workspace root {} escapes allowed root {}",
                canonical_workspace.display(),
                canonical_base.display()
            );
        }
        Ok(canonical_workspace)
    } else {
        Ok(workspace_root)
    }
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("failed canonicalizing {}", path.display()));
    }
    if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
        let canonical_parent = parent
            .canonicalize()
            .with_context(|| format!("failed canonicalizing {}", parent.display()))?;
        if let Some(name) = path.file_name() {
            return Ok(canonical_parent.join(name));
        }
    }
    Ok(path.to_path_buf())
}

#[derive(Debug, Clone, Default)]
struct MaterializedFileSnapshot {
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatestReadableGraphSnapshotMarker {
    schema_version: u32,
    workspace_id: String,
    snapshot_id: String,
    event_id: String,
    source_ingest_id: String,
    #[serde(default = "default_materialized_artifact_role")]
    artifact_role: String,
    #[serde(default = "default_canonical_state_store")]
    canonical_state_store: String,
    materialized_at: u64,
    published_at: u64,
    #[serde(default)]
    source_markdown_refs: Vec<String>,
    #[serde(default)]
    materialized_files: Vec<String>,
}

fn default_materialized_artifact_role() -> String {
    MATERIALIZED_ARTIFACT_ROLE_MIGRATION_INPUT.into()
}

fn default_canonical_state_store() -> String {
    CANONICAL_STATE_STORE_SQLITE_GRAPHQLITE.into()
}

fn capture_materialized_file_snapshot(root: &Path) -> Result<MaterializedFileSnapshot> {
    let mut snapshot = MaterializedFileSnapshot::default();
    collect_materialized_snapshot_files(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn collect_materialized_snapshot_files(
    root: &Path,
    dir: &Path,
    snapshot: &mut MaterializedFileSnapshot,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed reading entry in {}", dir.display()))?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .with_context(|| format!("failed relativizing {}", path.display()))?;
        if should_skip_materialized_snapshot_path(relative_path) {
            continue;
        }
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed reading metadata for {}", path.display()))?;
        if file_type.is_dir() {
            collect_materialized_snapshot_files(root, &path, snapshot)?;
        } else if file_type.is_file() && is_materialized_snapshot_path(relative_path) {
            snapshot.files.insert(
                relative_path.to_string_lossy().replace('\\', "/"),
                fs::read(&path).with_context(|| format!("failed reading {}", path.display()))?,
            );
        }
    }
    Ok(())
}

fn is_materialized_snapshot_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == "brain-manifest.json"
        || normalized.starts_with("graph/")
        || normalized.starts_with("memory/")
        || normalized == LATEST_READABLE_SNAPSHOT_PATH
        || normalized.starts_with("source-index/")
        || normalized.starts_with("wiki/")
        || normalized.starts_with("events/")
}

fn should_skip_materialized_snapshot_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == "brain.lock"
        || normalized == BRAIN_LOCK_DIRECTORY_NAME
        || normalized.starts_with("snapshots/")
        || normalized.starts_with("runs/")
        || normalized.contains("/.")
        || normalized.starts_with('.')
}

fn changed_materialized_files(
    before: &MaterializedFileSnapshot,
    after: &MaterializedFileSnapshot,
) -> Vec<String> {
    let keys = before
        .files
        .keys()
        .chain(after.files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .filter(|key| before.files.get(key) != after.files.get(key))
        .collect()
}

fn normalize_wiki_path(path: &str) -> Result<String> {
    let mut normalized = path.trim().trim_start_matches('/').to_string();
    if !normalized.starts_with("wiki/") {
        normalized = format!("wiki/{normalized}");
    }
    if normalized.split('/').any(|part| part == "..") {
        bail!("wiki page path cannot contain ..");
    }
    Ok(normalized)
}

pub(crate) fn unix_timestamp_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
