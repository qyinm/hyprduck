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
use application::services::project_service::empty_workspace_project;
#[cfg(test)]
pub(crate) use application::services::project_service::{
    handle_apply_correction, handle_load_project,
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

fn handle_answer_project(request: AnswerProjectRequest) -> Result<AnswerProjectResponseData> {
    let store = KnowledgeProjectStore::default()?;
    if let Some(workspace_id) = workspace_id_from_project_id(&request.project_id) {
        let workspace_root = store.workspace_root(workspace_id)?;
        if !workspace_root.join("brain-manifest.json").exists() {
            store.materialize_workspace_brain_repo(workspace_id)?;
        }
        let reader = BrainReader::open_workspace_root(workspace_root, workspace_id)?;
        let answer = answer_materialized_workspace_project(&reader, &request)?;
        return Ok(AnswerProjectResponseData { answer });
    }
    let project = load_answerable_project(&store, &request.project_id)?;
    let answer = answer_project(&project, &request)?;
    Ok(AnswerProjectResponseData { answer })
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

fn load_answerable_project(
    store: &KnowledgeProjectStore,
    project_id: &str,
) -> Result<KnowledgeProject> {
    if let Some(workspace_id) = project_id.strip_prefix("workspace:") {
        return Ok(store
            .load_workspace_project(workspace_id)?
            .unwrap_or_else(|| empty_workspace_project(workspace_id)));
    }

    store
        .load_project(Some(project_id))?
        .ok_or_else(|| anyhow!("project {project_id} was not found"))
}

fn build_project_id(request: &CompileProjectRequest) -> String {
    let stable_source = request
        .source_document_path
        .as_deref()
        .unwrap_or(&request.source_markdown_path);
    format!("project-{:016x}", fnv1a_hash(stable_source.as_bytes()))
}

fn build_source_backed_project_id(workspace_id: &str, source_id: &str) -> String {
    format!(
        "project-{:016x}",
        fnv1a_hash(format!("{workspace_id}/{source_id}").as_bytes())
    )
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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

fn source_summary_from_sqlite_row(line: &str) -> Result<SourceSummary> {
    let columns: Vec<&str> = line.split('|').collect();
    if !matches!(columns.len(), 11 | 12 | 13 | 14) {
        bail!(
            "expected 11, 12, 13, or 14 source summary columns from sqlite, got {}",
            columns.len()
        );
    }
    let readiness_offset = (columns.len() >= 13).then_some(11);
    let manifest_index = if columns.len() >= 13 { 13 } else { 11 };
    let manifest = columns
        .get(manifest_index)
        .map(|encoded| decode_source_manifest_snapshot(encoded))
        .transpose()?;
    Ok(SourceSummary {
        workspace_id: decode_sqlite_hex_text(columns[0])?,
        source_id: decode_sqlite_hex_text(columns[1])?,
        original_path: decode_sqlite_hex_text(columns[2])?,
        source_path: decode_sqlite_hex_text(columns[3])?,
        markdown_path: decode_sqlite_hex_text(columns[4])?,
        format: document_format_from_slug(&decode_sqlite_hex_text(columns[5])?)?,
        status: ingest_status_from_slug(&decode_sqlite_hex_text(columns[6])?)?,
        page_count: columns[7]
            .parse()
            .context("failed to parse source page_count")?,
        success_count: columns[8]
            .parse()
            .context("failed to parse source success_count")?,
        failed_count: columns[9]
            .parse()
            .context("failed to parse source failed_count")?,
        citation_ready: readiness_offset
            .map(|offset| sqlite_bool(columns[offset]))
            .transpose()?
            .unwrap_or_else(|| columns[8].parse::<usize>().unwrap_or_default() > 0),
        graph_ready: readiness_offset
            .map(|offset| sqlite_bool(columns[offset + 1]))
            .transpose()?
            .unwrap_or(false),
        description: manifest
            .as_ref()
            .map(|manifest| manifest.description.clone())
            .unwrap_or_default(),
        user_context: manifest
            .as_ref()
            .map(|manifest| manifest.user_context.clone())
            .unwrap_or_default(),
        ingest_instruction: manifest
            .as_ref()
            .map(|manifest| manifest.ingest_instruction.clone())
            .unwrap_or_default(),
        updated_at: columns[10]
            .parse()
            .context("failed to parse source updated_at")?,
    })
}

fn stored_source_row_from_sqlite_row(line: &str) -> Result<StoredSourceRow> {
    let columns: Vec<&str> = line.split('|').collect();
    if !matches!(columns.len(), 13 | 14 | 15 | 16) {
        bail!(
            "expected 13, 14, 15, or 16 stored source columns from sqlite, got {}",
            columns.len()
        );
    }
    let readiness_offset = (columns.len() >= 15).then_some(13);
    let manifest_index = if columns.len() >= 15 { 15 } else { 13 };
    let manifest = columns
        .get(manifest_index)
        .map(|encoded| decode_source_manifest_snapshot(encoded))
        .transpose()?;
    Ok(StoredSourceRow {
        summary: SourceSummary {
            workspace_id: decode_sqlite_hex_text(columns[0])?,
            source_id: decode_sqlite_hex_text(columns[1])?,
            original_path: decode_sqlite_hex_text(columns[2])?,
            source_path: decode_sqlite_hex_text(columns[3])?,
            markdown_path: decode_sqlite_hex_text(columns[4])?,
            format: document_format_from_slug(&decode_sqlite_hex_text(columns[5])?)?,
            status: ingest_status_from_slug(&decode_sqlite_hex_text(columns[6])?)?,
            page_count: columns[7]
                .parse()
                .context("failed to parse source page_count")?,
            success_count: columns[8]
                .parse()
                .context("failed to parse source success_count")?,
            failed_count: columns[9]
                .parse()
                .context("failed to parse source failed_count")?,
            citation_ready: readiness_offset
                .map(|offset| sqlite_bool(columns[offset]))
                .transpose()?
                .unwrap_or_else(|| columns[8].parse::<usize>().unwrap_or_default() > 0),
            graph_ready: readiness_offset
                .map(|offset| sqlite_bool(columns[offset + 1]))
                .transpose()?
                .unwrap_or(false),
            description: manifest
                .as_ref()
                .map(|manifest| manifest.description.clone())
                .unwrap_or_default(),
            user_context: manifest
                .as_ref()
                .map(|manifest| manifest.user_context.clone())
                .unwrap_or_default(),
            ingest_instruction: manifest
                .as_ref()
                .map(|manifest| manifest.ingest_instruction.clone())
                .unwrap_or_default(),
            updated_at: columns[10]
                .parse()
                .context("failed to parse source updated_at")?,
        },
        project_id: decode_sqlite_hex_text(columns[11])?,
        manifest_path: decode_sqlite_hex_text(columns[12])?,
    })
}

fn workspace_correction_from_sqlite_row(line: &str) -> Result<WorkspaceCorrection> {
    let columns: Vec<&str> = line.split('|').collect();
    if columns.len() != 9 {
        bail!(
            "expected 9 workspace correction columns from sqlite, got {}",
            columns.len()
        );
    }
    let target_node_id = decode_sqlite_hex_text(columns[4])?;
    let value = decode_sqlite_hex_text(columns[5])?;
    let evidence_ids_json = decode_sqlite_hex_text(columns[6])?;
    let source_node_ids_json = decode_sqlite_hex_text(columns[7])?;
    Ok(WorkspaceCorrection {
        id: decode_sqlite_hex_text(columns[0])?,
        workspace_id: decode_sqlite_hex_text(columns[1])?,
        aggregate_node_id: decode_sqlite_hex_text(columns[2])?,
        kind: correction_kind_from_slug(columns[3])?,
        target_node_id: (!target_node_id.is_empty()).then_some(target_node_id),
        value: (!value.is_empty()).then_some(value),
        evidence_ids: serde_json::from_str(&evidence_ids_json)
            .context("failed to decode workspace correction evidence ids")?,
        source_node_ids: serde_json::from_str(&source_node_ids_json)
            .context("failed to decode workspace correction source node ids")?,
        created_at: columns[8]
            .parse()
            .context("failed to parse workspace correction created_at")?,
    })
}

fn correction_kind_slug(kind: &CorrectionKind) -> &'static str {
    match kind {
        CorrectionKind::Merge => "merge",
        CorrectionKind::KeepSeparate => "keep_separate",
        CorrectionKind::Rename => "rename",
        CorrectionKind::Split => "split",
        CorrectionKind::Delete => "delete",
    }
}

fn correction_kind_from_slug(value: &str) -> Result<CorrectionKind> {
    match value {
        "merge" => Ok(CorrectionKind::Merge),
        "keep_separate" => Ok(CorrectionKind::KeepSeparate),
        "rename" => Ok(CorrectionKind::Rename),
        "split" => Ok(CorrectionKind::Split),
        "delete" => Ok(CorrectionKind::Delete),
        _ => bail!("unknown correction kind {value}"),
    }
}

fn decode_project_snapshot(encoded: &str) -> Result<KnowledgeProject> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("failed to decode stored project snapshot")?;
    serde_json::from_slice(&bytes).context("failed to decode stored project")
}

fn decode_source_manifest_snapshot(encoded: &str) -> Result<SourceArtifactManifest> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("failed to decode stored source manifest snapshot")?;
    serde_json::from_slice(&bytes).context("failed to decode stored source manifest")
}

fn decode_sqlite_hex_text(value: &str) -> Result<String> {
    if !value.len().is_multiple_of(2) {
        bail!("sqlite hex text had an odd byte count");
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .context("failed to decode sqlite hex text byte")
        })
        .collect::<Result<Vec<_>>>()?;
    String::from_utf8(bytes).context("sqlite hex text was not valid UTF-8")
}

fn sqlite_bool(value: &str) -> Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        other => bail!("expected sqlite boolean 0 or 1, got {other}"),
    }
}

fn ingest_status_slug(status: &IngestStatus) -> &'static str {
    match status {
        IngestStatus::Added => "added",
        IngestStatus::Rendering => "rendering",
        IngestStatus::Ingesting => "ingesting",
        IngestStatus::Ingested => "ingested",
        IngestStatus::Partial => "partial",
        IngestStatus::Failed => "failed",
        IngestStatus::Stale => "stale",
    }
}

fn ingest_status_from_slug(value: &str) -> Result<IngestStatus> {
    match value {
        "added" => Ok(IngestStatus::Added),
        "rendering" => Ok(IngestStatus::Rendering),
        "ingesting" => Ok(IngestStatus::Ingesting),
        "ingested" => Ok(IngestStatus::Ingested),
        "partial" => Ok(IngestStatus::Partial),
        "failed" => Ok(IngestStatus::Failed),
        "stale" => Ok(IngestStatus::Stale),
        _ => bail!("unknown ingest status {value}"),
    }
}

fn document_format_slug(format: &DocumentFormat) -> &'static str {
    match format {
        DocumentFormat::Pdf => "pdf",
        DocumentFormat::Docx => "docx",
        DocumentFormat::Doc => "doc",
        DocumentFormat::Image => "image",
        DocumentFormat::Markdown => "markdown",
    }
}

fn document_format_from_slug(value: &str) -> Result<DocumentFormat> {
    match value {
        "pdf" => Ok(DocumentFormat::Pdf),
        "docx" => Ok(DocumentFormat::Docx),
        "doc" => Ok(DocumentFormat::Doc),
        "image" => Ok(DocumentFormat::Image),
        "markdown" | "md" => Ok(DocumentFormat::Markdown),
        _ => bail!("unknown document format {value}"),
    }
}

fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .replace(['/', '\\', ':'], "-")
        .replace("..", "-")
        .trim()
        .chars()
        .take(100)
        .collect::<String>();
    if sanitized.is_empty() {
        "output".into()
    } else {
        sanitized
    }
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.to_string()
}

struct KnowledgeProjectStore {
    path: PathBuf,
}

impl KnowledgeProjectStore {
    fn default() -> Result<Self> {
        if let Some(explicit_path) = std::env::var_os("HYPRDUCK_PROJECT_STORE") {
            return Ok(Self {
                path: PathBuf::from(explicit_path),
            });
        }

        let root = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("failed to resolve local data directory"))?;
        Self::from_data_root(&root)
    }

    fn from_data_root(root: &Path) -> Result<Self> {
        let store_dir = root.join("HyprDuck");
        let new_path = KnowledgeStore::default_path_for_root(&store_dir);
        let legacy_path = store_dir.join("knowledge.sqlite3");
        migrate_legacy_project_store(&legacy_path, &new_path)?;
        Ok(Self {
            path: if new_path.exists() {
                new_path
            } else if legacy_path.exists() {
                legacy_path
            } else {
                new_path
            },
        })
    }

    #[cfg(test)]
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn save_project(
        &self,
        project: &KnowledgeProject,
        request: &CompileProjectRequest,
        source_manifest: Option<&SourceArtifactManifest>,
    ) -> Result<()> {
        self.ensure_schema()?;
        if let Some(source_manifest) = source_manifest {
            self.save_source(project, source_manifest)?;
        }
        let snapshot_json =
            serde_json::to_string(project).context("failed to encode knowledge project")?;
        let snapshot_base64 = base64::engine::general_purpose::STANDARD.encode(snapshot_json);
        let source_document_path = request
            .source_document_path
            .as_ref()
            .map(|path| format!("'{}'", escape_sqlite(path)))
            .unwrap_or_else(|| "NULL".into());
        let status = match project.summary.status {
            ProjectStatus::Preview => "preview",
            ProjectStatus::Ready => "ready",
            ProjectStatus::Degraded => "degraded",
        };
        let sql = format!(
            "INSERT INTO projects (project_id, title, source_markdown_path, source_document_path, status, updated_at, snapshot_base64) \
             VALUES ('{project_id}', '{title}', '{markdown_path}', {source_document_path}, '{status}', {updated_at}, '{snapshot_base64}') \
             ON CONFLICT(project_id) DO UPDATE SET \
               title=excluded.title, \
               source_markdown_path=excluded.source_markdown_path, \
               source_document_path=excluded.source_document_path, \
               status=excluded.status, \
               updated_at=excluded.updated_at, \
               snapshot_base64=excluded.snapshot_base64;",
            project_id = escape_sqlite(&project.summary.project_id),
            title = escape_sqlite(&project.summary.title),
            markdown_path = escape_sqlite(&request.source_markdown_path),
            source_document_path = source_document_path,
            status = status,
            updated_at = unix_timestamp_seconds(),
            snapshot_base64 = snapshot_base64,
        );
        self.run_sql(&sql)?;
        if let Some(source_manifest) = source_manifest {
            self.materialize_workspace_brain_repo(&source_manifest.workspace_id)?;
        }
        Ok(())
    }

    fn update_project(&self, project: &KnowledgeProject) -> Result<()> {
        self.ensure_schema()?;
        let snapshot_json =
            serde_json::to_string(project).context("failed to encode knowledge project")?;
        let snapshot_base64 = base64::engine::general_purpose::STANDARD.encode(snapshot_json);
        let status = match project.summary.status {
            ProjectStatus::Preview => "preview",
            ProjectStatus::Ready => "ready",
            ProjectStatus::Degraded => "degraded",
        };
        let sql = format!(
            "UPDATE projects SET title = '{title}', status = '{status}', updated_at = {updated_at}, snapshot_base64 = '{snapshot_base64}' \
             WHERE project_id = '{project_id}';",
            title = escape_sqlite(&project.summary.title),
            status = status,
            updated_at = unix_timestamp_seconds(),
            snapshot_base64 = snapshot_base64,
            project_id = escape_sqlite(&project.summary.project_id),
        );
        self.run_sql(&sql).map(|_| ())
    }

    fn load_project(&self, project_id: Option<&str>) -> Result<Option<KnowledgeProject>> {
        self.ensure_schema()?;
        let sql = match project_id {
            Some(project_id) => format!(
                "SELECT snapshot_base64 FROM projects WHERE project_id = '{}' LIMIT 1;",
                escape_sqlite(project_id)
            ),
            None => "SELECT snapshot_base64 FROM projects ORDER BY updated_at DESC LIMIT 1;".into(),
        };
        let output = self.run_sql(&sql)?;
        let encoded = output.trim();
        if encoded.is_empty() {
            return Ok(None);
        }
        decode_project_snapshot(encoded).map(Some)
    }

    fn load_projects_by_ids(
        &self,
        project_ids: &[String],
    ) -> Result<BTreeMap<String, KnowledgeProject>> {
        self.ensure_schema()?;
        let unique_project_ids = project_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if unique_project_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let mut projects = BTreeMap::new();
        for chunk in unique_project_ids.chunks(PROJECT_SNAPSHOT_BATCH_SIZE) {
            let quoted_ids = chunk
                .iter()
                .map(|project_id| format!("'{}'", escape_sqlite(project_id)))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT hex(project_id), snapshot_base64 FROM projects WHERE project_id IN ({quoted_ids});"
            );
            let output = self.run_sql(&sql)?;
            for line in output.lines().filter(|line| !line.trim().is_empty()) {
                let columns = line.split('|').collect::<Vec<_>>();
                if columns.len() != 2 {
                    bail!(
                        "expected 2 project snapshot columns from sqlite, got {}",
                        columns.len()
                    );
                }
                projects.insert(
                    decode_sqlite_hex_text(columns[0])?,
                    decode_project_snapshot(columns[1])?,
                );
            }
        }
        Ok(projects)
    }

    fn load_latest_workspace_id(&self) -> Result<Option<WorkspaceId>> {
        self.ensure_schema()?;
        let output =
            self.run_sql("SELECT workspace_id FROM sources ORDER BY updated_at DESC LIMIT 1;")?;
        let workspace_id = output.trim();
        Ok((!workspace_id.is_empty()).then(|| workspace_id.to_string()))
    }

    fn update_import_job_graph_status(
        &self,
        workspace_id: &str,
        source_id: &str,
        graph_status: &str,
        failed_reason: Option<&str>,
        error_message: Option<&str>,
        retryable: bool,
    ) -> Result<()> {
        self.ensure_schema()?;
        KnowledgeStore::open(self.path.clone())?;
        let graph_ready = if graph_status_is_ready(Some(graph_status)) {
            1
        } else {
            0
        };
        let manual_retry_available = if graph_ready == 0 && graph_status != "skipped" {
            1
        } else {
            0
        };
        let status = if graph_ready == 1 {
            "context_ready"
        } else if graph_status == "skipped" {
            "citation_ready_graph_skipped"
        } else {
            "citation_ready_graph_pending"
        };
        let sql = format!(
            "UPDATE import_jobs
             SET status = '{status}',
                 graph_ready = {graph_ready},
                 graph_status = '{graph_status}',
                 graph_error_category = '{failed_reason}',
                 graph_error_message_redacted = '{error_message}',
                 graph_retryable = {retryable},
                 graph_max_retry_attempts = CASE WHEN graph_max_retry_attempts = 0 THEN 2 ELSE graph_max_retry_attempts END,
                 manual_retry_available = {manual_retry_available},
                 updated_at = {updated_at}
             WHERE workspace_id = '{workspace_id}' AND source_id = '{source_id}' AND citation_ready = 1;",
            status = escape_sqlite(status),
            graph_ready = graph_ready,
            graph_status = escape_sqlite(graph_status),
            failed_reason = escape_sqlite(failed_reason.unwrap_or_default()),
            error_message = escape_sqlite(error_message.unwrap_or_default()),
            retryable = if retryable { 1 } else { 0 },
            manual_retry_available = manual_retry_available,
            updated_at = unix_timestamp_seconds(),
            workspace_id = escape_sqlite(workspace_id),
            source_id = escape_sqlite(source_id),
        );
        self.run_sql(&sql).map(|_| ())
    }

    fn load_workspace_id_for_project(&self, project_id: &str) -> Result<Option<WorkspaceId>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT workspace_id FROM sources WHERE project_id = '{}' ORDER BY updated_at DESC LIMIT 1;",
            escape_sqlite(project_id)
        );
        let output = self.run_sql(&sql)?;
        let workspace_id = output.trim();
        Ok((!workspace_id.is_empty()).then(|| workspace_id.to_string()))
    }

    fn load_sources(&self, workspace_id: &str) -> Result<Vec<SourceSummary>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(sources.workspace_id), hex(sources.source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(sources.status), page_count, success_count, failed_count, sources.updated_at, COALESCE(import_jobs.citation_ready, CASE WHEN success_count > 0 THEN 1 ELSE 0 END), COALESCE(import_jobs.graph_ready, 0), manifest_base64 \
             FROM sources LEFT JOIN import_jobs ON import_jobs.workspace_id = sources.workspace_id AND import_jobs.source_id = sources.source_id WHERE sources.workspace_id = '{}' ORDER BY sources.updated_at DESC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(source_summary_from_sqlite_row)
            .collect()
    }

    fn load_source_rows(&self, workspace_id: &str) -> Result<Vec<StoredSourceRow>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(sources.workspace_id), hex(sources.source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(sources.status), page_count, success_count, failed_count, sources.updated_at, hex(project_id), hex(manifest_path), COALESCE(import_jobs.citation_ready, CASE WHEN success_count > 0 THEN 1 ELSE 0 END), COALESCE(import_jobs.graph_ready, 0), manifest_base64 \
             FROM sources LEFT JOIN import_jobs ON import_jobs.workspace_id = sources.workspace_id AND import_jobs.source_id = sources.source_id WHERE sources.workspace_id = '{}' ORDER BY sources.updated_at DESC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(stored_source_row_from_sqlite_row)
            .collect()
    }

    fn delete_workspace_source(
        &self,
        workspace_id: &str,
        source_id: &str,
    ) -> Result<Option<StoredSourceRow>> {
        self.ensure_schema()?;
        let row = self
            .load_source_rows(workspace_id)?
            .into_iter()
            .find(|row| row.summary.source_id == source_id);
        let Some(row) = row else {
            return Ok(None);
        };
        let sql = format!(
            "DELETE FROM sources WHERE workspace_id = '{workspace_id}' AND source_id = '{source_id}'; \
             DELETE FROM projects WHERE project_id = '{project_id}' AND NOT EXISTS (SELECT 1 FROM sources WHERE project_id = '{project_id}');",
            workspace_id = escape_sqlite(workspace_id),
            source_id = escape_sqlite(source_id),
            project_id = escape_sqlite(&row.project_id),
        );
        self.run_sql(&sql)?;
        Ok(Some(row))
    }

    fn load_projects_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<(StoredSourceRow, Option<KnowledgeProject>)>> {
        let rows = self.load_source_rows(workspace_id)?;
        let project_ids = rows
            .iter()
            .map(|row| row.project_id.clone())
            .collect::<Vec<_>>();
        let projects_by_id = self.load_projects_by_ids(&project_ids)?;
        rows.into_iter()
            .map(|row| {
                let project = projects_by_id.get(&row.project_id).cloned();
                Ok((row, project))
            })
            .collect()
    }

    fn load_workspace_project(&self, workspace_id: &str) -> Result<Option<KnowledgeProject>> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(workspace_ui_graph_projection(
            aggregate_workspace_project(workspace_id, rows),
        )))
    }

    fn workspace_root(&self, workspace_id: &str) -> Result<PathBuf> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        Ok(workspace_root_for_rows(&rows)
            .unwrap_or_else(|| fallback_workspace_root(&self.path, workspace_id)))
    }

    fn save_source(
        &self,
        project: &KnowledgeProject,
        manifest: &SourceArtifactManifest,
    ) -> Result<()> {
        KnowledgeStore::open(self.path.clone())?.persist_source_manifest(project, manifest)?;
        let manifest_json =
            serde_json::to_string(manifest).context("failed to encode source manifest snapshot")?;
        let manifest_base64 = base64::engine::general_purpose::STANDARD.encode(manifest_json);
        let summary = source_summary_from_manifest(manifest);
        let status = ingest_status_slug(&summary.status);
        let format = document_format_slug(&summary.format);
        let sql = format!(
            "INSERT INTO sources (source_id, workspace_id, project_id, original_path, source_path, markdown_path, format, status, page_count, success_count, failed_count, updated_at, manifest_path, manifest_base64) \
             VALUES ('{source_id}', '{workspace_id}', '{project_id}', '{original_path}', '{source_path}', '{markdown_path}', '{format}', '{status}', {page_count}, {success_count}, {failed_count}, {updated_at}, '{manifest_path}', '{manifest_base64}') \
             ON CONFLICT(source_id) DO UPDATE SET \
               workspace_id=excluded.workspace_id, \
               project_id=excluded.project_id, \
               original_path=excluded.original_path, \
               source_path=excluded.source_path, \
               markdown_path=excluded.markdown_path, \
               format=excluded.format, \
               status=excluded.status, \
               page_count=excluded.page_count, \
               success_count=excluded.success_count, \
               failed_count=excluded.failed_count, \
               updated_at=excluded.updated_at, \
               manifest_path=excluded.manifest_path, \
               manifest_base64=excluded.manifest_base64;",
            source_id = escape_sqlite(&summary.source_id),
            workspace_id = escape_sqlite(&summary.workspace_id),
            project_id = escape_sqlite(&project.summary.project_id),
            original_path = escape_sqlite(&summary.original_path),
            source_path = escape_sqlite(&summary.source_path),
            markdown_path = escape_sqlite(&summary.markdown_path),
            format = format,
            status = status,
            page_count = summary.page_count,
            success_count = summary.success_count,
            failed_count = summary.failed_count,
            updated_at = summary.updated_at,
            manifest_path = escape_sqlite(&manifest.manifest_path),
            manifest_base64 = manifest_base64,
        );
        self.run_sql(&sql).map(|_| ())
    }

    fn append_workspace_correction(&self, correction: &WorkspaceCorrection) -> Result<()> {
        self.ensure_schema()?;
        let evidence_ids_json = serde_json::to_string(&correction.evidence_ids)
            .context("failed to encode workspace correction evidence ids")?;
        let source_node_ids_json = serde_json::to_string(&correction.source_node_ids)
            .context("failed to encode workspace correction source node ids")?;
        let target_node_id = correction
            .target_node_id
            .as_ref()
            .map(|value| format!("'{}'", escape_sqlite(value)))
            .unwrap_or_else(|| "NULL".into());
        let value = correction
            .value
            .as_ref()
            .map(|value| format!("'{}'", escape_sqlite(value)))
            .unwrap_or_else(|| "NULL".into());
        let sql = format!(
            "INSERT INTO workspace_corrections (id, workspace_id, aggregate_node_id, kind, target_node_id, value, evidence_ids_json, source_node_ids_json, created_at) \
             VALUES ('{id}', '{workspace_id}', '{aggregate_node_id}', '{kind}', {target_node_id}, {value}, '{evidence_ids_json}', '{source_node_ids_json}', {created_at});",
            id = escape_sqlite(&correction.id),
            workspace_id = escape_sqlite(&correction.workspace_id),
            aggregate_node_id = escape_sqlite(&correction.aggregate_node_id),
            kind = correction_kind_slug(&correction.kind),
            target_node_id = target_node_id,
            value = value,
            evidence_ids_json = escape_sqlite(&evidence_ids_json),
            source_node_ids_json = escape_sqlite(&source_node_ids_json),
            created_at = correction.created_at,
        );
        self.run_sql(&sql).map(|_| ())
    }

    fn load_workspace_corrections(&self, workspace_id: &str) -> Result<Vec<WorkspaceCorrection>> {
        self.ensure_schema()?;
        let sql = format!(
            "SELECT hex(id), hex(workspace_id), hex(aggregate_node_id), kind, hex(COALESCE(target_node_id, '')), hex(COALESCE(value, '')), hex(evidence_ids_json), hex(source_node_ids_json), created_at \
             FROM workspace_corrections WHERE workspace_id = '{}' ORDER BY created_at ASC, id ASC;",
            escape_sqlite(workspace_id)
        );
        let output = self.run_sql(&sql)?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(workspace_correction_from_sqlite_row)
            .collect()
    }

    fn materialize_workspace_brain_repo(&self, workspace_id: &str) -> Result<()> {
        let rows = self.load_projects_for_workspace(workspace_id)?;
        if rows.is_empty() {
            let workspace_root = fallback_workspace_root(&self.path, workspace_id);
            let mut snapshot = empty_replayed_brain_snapshot(workspace_id);
            snapshot.generated_at = unix_timestamp_seconds();
            KnowledgeStore::open(self.path.clone())?.persist_graph_snapshot(&snapshot)?;
            return write_materialized_brain_repo(&workspace_root, &snapshot);
        }
        let workspace_root = workspace_root_for_rows(&rows)
            .unwrap_or_else(|| fallback_workspace_root(&self.path, workspace_id));
        let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
        let corrections = self.load_workspace_corrections(workspace_id)?;
        let existing_memories = read_memory_records(&workspace_root)?;
        let existing_nodes = read_existing_graph_nodes(&workspace_root)?;
        let existing_relations = read_existing_graph_relations(&workspace_root)?;
        let snapshot = build_brain_repo_snapshot(
            workspace_id,
            &rows,
            &aggregate,
            &corrections,
            &existing_memories,
            &existing_nodes,
            &existing_relations,
        );
        KnowledgeStore::open(self.path.clone())?.persist_graph_snapshot(&snapshot)?;
        write_materialized_brain_repo(&workspace_root, &snapshot)
    }

    fn ensure_schema(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        KnowledgeStore::open(self.path.clone())?;
        self.run_sql(
            "CREATE TABLE IF NOT EXISTS projects (
                project_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_markdown_path TEXT NOT NULL,
                source_document_path TEXT,
                status TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                snapshot_base64 TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_projects_updated_at ON projects(updated_at DESC);
            CREATE TABLE IF NOT EXISTS sources (
                source_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                original_path TEXT NOT NULL,
                source_path TEXT NOT NULL,
                markdown_path TEXT NOT NULL,
                format TEXT NOT NULL,
                status TEXT NOT NULL,
                page_count INTEGER NOT NULL,
                success_count INTEGER NOT NULL,
                failed_count INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                manifest_path TEXT NOT NULL,
                manifest_base64 TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sources_workspace_updated_at ON sources(workspace_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS workspace_corrections (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                aggregate_node_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_node_id TEXT,
                value TEXT,
                evidence_ids_json TEXT NOT NULL,
                source_node_ids_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workspace_corrections_workspace_created_at ON workspace_corrections(workspace_id, created_at ASC);",
        )
        .map(|_| ())
    }

    fn run_sql(&self, sql: &str) -> Result<String> {
        let output = Command::new(resolve_binary("sqlite3", &["/usr/bin/sqlite3"]))
            .arg(&self.path)
            .arg(sql)
            .output()
            .with_context(|| format!("failed to launch sqlite3 for {}", self.path.display()))?;

        if !output.status.success() {
            bail!(
                "sqlite3 failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        String::from_utf8(output.stdout).context("sqlite3 output was not valid UTF-8")
    }
}

fn migrate_legacy_project_store(legacy_path: &Path, new_path: &Path) -> Result<()> {
    if new_path.exists() || !legacy_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    fs::copy(legacy_path, new_path).with_context(|| {
        format!(
            "failed migrating legacy project store from {} to {}",
            legacy_path.display(),
            new_path.display()
        )
    })?;
    Ok(())
}

fn escape_sqlite(value: &str) -> String {
    value.replace('\'', "''")
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
