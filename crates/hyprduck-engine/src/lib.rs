use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    PageEvidenceV0, ParseEvent, ParseMetadata, ParseRequest, ParseResponseData, ParseResult,
    ParsedPage, ProjectOverview, ProjectStatus, ReadContextPackRequest,
    ReadContextPackResponseData, ReadImportJobRequest, ReadImportJobResponseData, ReadNodeRequest,
    ReadNodeResponseData, ReadPageEvidenceRequest, ReadPageEvidenceResponseData,
    ReadRecentEventsRequest, ReadRecentEventsResponseData, ReadSourceRequest,
    ReadSourceResponseData, ReadWikiPageRequest, ReadWikiPageResponseData, ReconstructBrainRequest,
    ReconstructBrainResponseData, RelationEdgeDetail, RelationEdgeSummary, RelationKind,
    RetryFailedPagesRequest, RetryFailedPagesResponseData, SearchBrainRequest,
    SearchBrainResponseData, SourceArtifactManifest, SourceBacking, SourceId, SourceRecord,
    SourceStatus, SourceSummary, StructuredExtractionArtifact, StructuredExtractionClaim,
    StructuredExtractionEntity, StructuredExtractionMemoryCandidate, StructuredExtractionPageRef,
    StructuredExtractionRelation, StructuredExtractionTopic, SuggestedAction, SuggestedActionKind,
    UpdateImportJobGraphStatusRequest, UpdateImportJobGraphStatusResponseData, WikiPage,
    WorkspaceCorrection, WorkspaceId, BRAIN_EVENT_SCHEMA_VERSION,
};
#[cfg(test)]
use hyprduck_engine_types::{
    ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackSourceMetadataV0,
    EvidenceIndexV0, OutputAsset, ParseInput, ParseOptions, RetryPageArtifactUpdate, SourcePackV0,
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
    pub(crate) use crate::domains::provider::openai_compatible::*;
}

mod import_context {
    pub(crate) use crate::domains::retrieval::import_context::*;
}

mod knowledge {
    pub(crate) use crate::domains::knowledge::*;
}

mod parse {
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
use domains::ingest::output_package::retry_failed_page_artifacts;
#[cfg(test)]
use domains::ingest::output_package::write_output_package_with_fallback;
use domains::ingest::output_package::{
    build_markdown, export_output_package, load_source_manifest, resolved_source_ids,
    source_summary_from_manifest,
};
#[cfg(test)]
use domains::ingest::output_package::{build_source_id, write_source_manifest};
use domains::knowledge_store::KnowledgeStore;
#[allow(unused_imports)]
pub(crate) use graph_history::{
    event_matches_recent_events_request, graph_snapshot_source_ingest_id,
    handle_read_graph_history, handle_read_graph_snapshot, latest_graph_materialized_event,
};
use import_context::{
    build_import_evidence_context, import_evidence_context_allowed_refs, ImportEvidenceContext,
};
use infra::process::resolve_binary;
use knowledge::*;
use parse::{parse_document, EventSink, ProcessLocator};
use provider::{EngineConfig, EngineConfigStore};
pub(crate) use runtime::emit_event;
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

fn maybe_write_debug(path: &Option<String>, contents: &str) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating debug directory {}", parent.display()))?;
        }
        fs::write(path, contents)
            .with_context(|| format!("failed writing debug artifact {}", path))?;
    }
    Ok(())
}

fn handle_parse(
    request: ParseRequest,
    config_store: &EngineConfigStore,
) -> Result<ParseResponseData> {
    let started = Instant::now();
    let config = config_store.load()?;

    let mut event_sink = RuntimeParseEventSink;

    event_sink.emit(ParseEvent::Queued)?;
    event_sink.emit(ParseEvent::DocumentOpened {
        format: request.input.format.clone(),
    })?;

    let process_locator = RuntimeProcessLocator;
    let parse = parse_document(
        &request.input,
        &request.template,
        &request.options,
        &config,
        &mut event_sink,
        &process_locator,
    )?;
    let markdown = build_markdown(
        request
            .output
            .as_ref()
            .and_then(|target| target.name.clone())
            .unwrap_or_else(|| {
                Path::new(&request.input.path)
                    .file_stem()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "document".to_string())
            }),
        &parse.pages,
    );

    event_sink.emit(ParseEvent::Packaging)?;
    let result = ParseResult {
        version: request.version.clone(),
        markdown,
        pages: parse.pages,
        assets: parse.assets,
        metadata: ParseMetadata {
            engine_id: format!("{}/{}", config.provider.id_slug(), config.model_id),
            duration_ms: started.elapsed().as_millis() as u64,
            page_count: parse.page_count,
        },
        success_count: parse.success_count,
        failed_count: parse.failed_count,
    };

    let source_manifest = export_output_package(&request, &result, &config)?;
    let saved_output_path = source_manifest
        .as_ref()
        .map(|manifest| manifest.markdown_path.clone());
    event_sink.emit(ParseEvent::Completed)?;
    Ok(ParseResponseData {
        result,
        saved_output_path,
        source_manifest,
    })
}

pub(crate) fn handle_retry_failed_pages(
    request: RetryFailedPagesRequest,
    config: &EngineConfig,
) -> Result<RetryFailedPagesResponseData> {
    retry_failed_page_artifacts(&request, config)
}

struct RuntimeParseEventSink;

impl EventSink for RuntimeParseEventSink {
    fn emit(&mut self, event: ParseEvent) -> Result<()> {
        emit_event(&event)
    }
}

struct RuntimeProcessLocator;

impl ProcessLocator for RuntimeProcessLocator {
    fn resolve_binary(&self, name: &str, common_paths: &[&str]) -> PathBuf {
        resolve_binary(name, common_paths)
    }
}

fn handle_compile_project(request: CompileProjectRequest) -> Result<CompileProjectResponseData> {
    let markdown = fs::read_to_string(&request.source_markdown_path).with_context(|| {
        format!(
            "failed reading markdown package {}",
            request.source_markdown_path
        )
    })?;
    let source_manifest = load_source_manifest(&request)?;
    let (workspace_id, source_id) = resolved_source_ids(&request, source_manifest.as_ref())?;
    let project = compile_knowledge_project(&request, &markdown, source_manifest.as_ref());
    let store = KnowledgeProjectStore::default()?;
    store.save_project(&project, &request, source_manifest.as_ref())?;
    let mut graph_generation_status = None;
    let mut graph_generation_skipped_reason = None;
    let mut graph_generation_error_message = None;
    let mut graph_generation_retryable = None;
    let mut graph_generation_failed_reason = None;
    let mut graph_generation_stage = None;
    if let Some(manifest) = &source_manifest {
        let workspace_root = compile_workspace_root(manifest, &workspace_id)?;
        let chunks = chunk_source_markdown(manifest, &markdown);
        upsert_source_chunks(&workspace_root, manifest, &chunks)?;
    }
    if let Some(manifest) = source_manifest
        .as_ref()
        .filter(|_| !request.skip_graph_generation.unwrap_or(false))
    {
        let workspace_root = compile_workspace_root(manifest, &workspace_id)?;
        let chunks = chunk_source_markdown(manifest, &markdown);
        let snapshot = read_materialized_brain_snapshot(&workspace_root, &workspace_id)
            .unwrap_or_else(|_| empty_replayed_brain_snapshot(&workspace_id));
        let context = build_import_evidence_context(
            &workspace_root,
            manifest,
            &markdown,
            &snapshot,
            &chunks,
        )?;
        let report = maybe_generate_provider_graph_materialization(
            &workspace_root,
            &workspace_id,
            manifest,
            &markdown,
            &PathBuf::from(&manifest.artifact_root),
            &context,
        )?;
        graph_generation_status = Some(report.status);
        graph_generation_skipped_reason = report.skipped_reason;
        graph_generation_error_message = report.error_message;
        graph_generation_retryable = Some(report.retryable);
        graph_generation_failed_reason = report.failed_reason;
        graph_generation_stage = Some(report.stage);
        store.update_import_job_graph_status(
            &workspace_id,
            &manifest.source_id,
            graph_generation_status.as_deref().unwrap_or("unknown"),
            graph_generation_failed_reason.as_deref(),
            graph_generation_error_message.as_deref(),
            graph_generation_retryable.unwrap_or(false),
        )?;
    }
    Ok(CompileProjectResponseData {
        project_id: project.summary.project_id,
        workspace_id,
        source_id,
        graph_generation_status,
        graph_generation_skipped_reason,
        graph_generation_error_message,
        graph_generation_retryable,
        graph_generation_failed_reason,
        graph_generation_stage,
    })
}

fn compile_workspace_root(
    manifest: &SourceArtifactManifest,
    workspace_id: &str,
) -> Result<PathBuf> {
    if let Some(root) =
        workspace_root_from_path_segments(&manifest.source_path, "sources", &manifest.source_id)
            .or_else(|| {
                workspace_root_from_path_segments(
                    &manifest.markdown_path,
                    "artifacts",
                    &manifest.source_id,
                )
            })
    {
        return Ok(root);
    }
    resolve_brain_workspace_root(&BrainReadScope {
        workspace_id: workspace_id.into(),
        root_dir: None,
    })
}

fn handle_read_import_job(request: ReadImportJobRequest) -> Result<ReadImportJobResponseData> {
    if request.job_id.is_none() && request.source_id.is_none() {
        bail!("read_import_job requires jobId or sourceId");
    }
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let job = store.read_import_job(
        &request.scope.workspace_id,
        request.job_id.as_deref(),
        request.source_id.as_deref(),
    )?;
    if job.is_some() {
        return Ok(ReadImportJobResponseData { job });
    }
    let project_store = KnowledgeProjectStore::default()?;
    if project_store.path == KnowledgeStore::default_path_for_root(&root) {
        return Ok(ReadImportJobResponseData { job: None });
    }
    let project_knowledge_store = KnowledgeStore::open(project_store.path)?;
    Ok(ReadImportJobResponseData {
        job: project_knowledge_store.read_import_job(
            &request.scope.workspace_id,
            request.job_id.as_deref(),
            request.source_id.as_deref(),
        )?,
    })
}

fn handle_update_import_job_graph_status(
    request: UpdateImportJobGraphStatusRequest,
) -> Result<UpdateImportJobGraphStatusResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let root_path = KnowledgeStore::default_path_for_root(&root);
    let root_store = KnowledgeStore::open(root_path.clone())?;
    let mut updated = root_store.update_import_job_graph_status_from_mcp(
        &request.scope.workspace_id,
        &request.source_id,
        &request.status,
        &request.graph_status,
        request.graph_error_category.as_deref(),
        request.graph_error_message_redacted.as_deref(),
        request.graph_retryable,
        request.graph_retry_attempt,
        request.graph_max_retry_attempts,
        request.graph_next_retry_at,
        request.manual_retry_available,
    )?;
    if !updated {
        let project_store = KnowledgeProjectStore::default()?;
        if project_store.path != root_path {
            let project_knowledge_store = KnowledgeStore::open(project_store.path)?;
            updated = project_knowledge_store.update_import_job_graph_status_from_mcp(
                &request.scope.workspace_id,
                &request.source_id,
                &request.status,
                &request.graph_status,
                request.graph_error_category.as_deref(),
                request.graph_error_message_redacted.as_deref(),
                request.graph_retryable,
                request.graph_retry_attempt,
                request.graph_max_retry_attempts,
                request.graph_next_retry_at,
                request.manual_retry_available,
            )?;
        }
    }
    Ok(UpdateImportJobGraphStatusResponseData { updated })
}

fn handle_load_project(request: LoadProjectRequest) -> Result<LoadProjectResponseData> {
    let store = KnowledgeProjectStore::default()?;
    if let Some(project_id) = request.project_id.as_deref() {
        if let Some(workspace_id) = project_id.strip_prefix("workspace:") {
            let project = store.load_workspace_project(workspace_id)?;
            let sources = store.load_sources(workspace_id)?;
            return Ok(LoadProjectResponseData {
                project,
                workspace_id: Some(workspace_id.to_string()),
                sources,
            });
        }

        let project = store
            .load_project(Some(project_id))?
            .map(source_ui_graph_projection);
        let stored_workspace_id = store.load_workspace_id_for_project(project_id)?;
        if let (Some(request_workspace_id), Some(actual_workspace_id)) = (
            request.workspace_id.as_deref(),
            stored_workspace_id.as_deref(),
        ) {
            if request_workspace_id != actual_workspace_id {
                bail!(
                    "project {project_id} belongs to workspace {actual_workspace_id}, not {request_workspace_id}"
                );
            }
        }
        let workspace_id = stored_workspace_id.or(request.workspace_id.clone());
        let sources = workspace_id
            .as_deref()
            .map(|workspace_id| store.load_sources(workspace_id))
            .transpose()?
            .unwrap_or_default();
        return Ok(LoadProjectResponseData {
            project,
            workspace_id,
            sources,
        });
    }

    let workspace_id = match request.workspace_id.clone() {
        Some(workspace_id) => Some(workspace_id),
        None => store.load_latest_workspace_id()?,
    };
    let mut project = workspace_id
        .as_deref()
        .map(|workspace_id| store.load_workspace_project(workspace_id))
        .transpose()?
        .flatten();
    if project.is_none() && request.workspace_id.is_none() {
        project = store.load_project(None)?.map(source_ui_graph_projection);
    }
    let sources = workspace_id
        .as_deref()
        .map(|workspace_id| store.load_sources(workspace_id))
        .transpose()?
        .unwrap_or_default();
    Ok(LoadProjectResponseData {
        project,
        workspace_id,
        sources,
    })
}

fn handle_apply_correction(request: ApplyCorrectionRequest) -> Result<ApplyCorrectionResponseData> {
    let store = KnowledgeProjectStore::default()?;
    if let Some(workspace_id) = workspace_id_from_project_id(&request.project_id) {
        return handle_apply_workspace_correction(&store, workspace_id, &request);
    }

    let mut project = store
        .load_project(Some(&request.project_id))?
        .ok_or_else(|| anyhow!("project {} was not found", request.project_id))?;
    apply_correction(&mut project, &request)?;
    store.update_project(&project)?;
    if let Some(workspace_id) = store.load_workspace_id_for_project(&project.summary.project_id)? {
        store.materialize_workspace_brain_repo(&workspace_id)?;
    }
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_apply_workspace_correction(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let rows = store.load_projects_for_workspace(workspace_id)?;
    if rows.is_empty() {
        if request.kind == CorrectionKind::Delete {
            return handle_delete_materialized_workspace_node(store, workspace_id, &rows, request);
        }
        bail!("workspace {workspace_id} was not found");
    }

    let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
    let selected_detail = match aggregate.details_by_node_id.get(&request.node_id) {
        Some(detail) => detail,
        None if request.kind == CorrectionKind::Delete => {
            return handle_delete_materialized_workspace_node(store, workspace_id, &rows, request);
        }
        None => bail!("workspace node {} was not found", request.node_id),
    };
    if request.kind == CorrectionKind::Delete && is_source_like_node_kind(selected_detail.node.kind)
    {
        return handle_delete_workspace_source_node(store, workspace_id, selected_detail, request);
    }
    if selected_detail.node.kind != GraphNodeKind::Concept {
        bail!("workspace corrections only support concept nodes");
    }
    let target_detail = match request.kind {
        CorrectionKind::Merge => {
            let target_node_id = request
                .target_node_id
                .as_deref()
                .ok_or_else(|| anyhow!("merge needs a target concept"))?;
            let detail = aggregate
                .details_by_node_id
                .get(target_node_id)
                .ok_or_else(|| anyhow!("workspace target node {target_node_id} was not found"))?;
            if detail.node.kind != GraphNodeKind::Concept {
                bail!("merge only supports concept nodes");
            }
            Some(detail)
        }
        CorrectionKind::KeepSeparate
        | CorrectionKind::Rename
        | CorrectionKind::Split
        | CorrectionKind::Delete => None,
    };

    let mut replayed_source_node_ids = BTreeSet::new();
    let mut changed_projects = Vec::new();
    for (row, project) in rows {
        let Some(mut project) = project else {
            continue;
        };
        let selected_source_node_ids = matching_source_concept_node_ids(&project, selected_detail);
        if selected_source_node_ids.is_empty() {
            continue;
        }

        let target_source_node_id = target_detail.and_then(|detail| {
            matching_source_concept_node_ids(&project, detail)
                .into_iter()
                .find(|node_id| !selected_source_node_ids.contains(node_id))
        });
        if request.kind == CorrectionKind::Merge && target_source_node_id.is_none() {
            continue;
        }

        let mut changed = false;
        for source_node_id in selected_source_node_ids {
            let source_request = ApplyCorrectionRequest {
                project_id: row.project_id.clone(),
                node_id: source_node_id.clone(),
                kind: request.kind.clone(),
                target_node_id: target_source_node_id.clone(),
                value: request.value.clone(),
            };
            apply_correction(&mut project, &source_request)?;
            replayed_source_node_ids.insert(format!("{}:{source_node_id}", row.project_id));
            changed = true;
            if request.kind == CorrectionKind::Merge {
                break;
            }
        }

        if changed {
            store.update_project(&project)?;
            changed_projects.push(row.project_id);
        }
    }

    if changed_projects.is_empty() {
        bail!(
            "workspace correction could not resolve node {} to any source snapshots",
            request.node_id
        );
    }

    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: request.target_node_id.clone(),
        value: request.value.clone(),
        evidence_ids: selected_detail
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_node_ids: replayed_source_node_ids.into_iter().collect(),
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_delete_materialized_workspace_node(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let workspace_root = workspace_root_for_rows(rows)
        .unwrap_or_else(|| fallback_workspace_root(&store.path, workspace_id));
    let snapshot = read_materialized_brain_snapshot(&workspace_root, workspace_id)?;
    let Some(node) = snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == request.node_id)
    else {
        store.materialize_workspace_brain_repo(workspace_id)?;
        let project = store
            .load_workspace_project(workspace_id)?
            .unwrap_or_else(|| empty_workspace_project(workspace_id));
        return Ok(ApplyCorrectionResponseData { project });
    };
    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: None,
        value: request.value.clone(),
        evidence_ids: node.evidence_ids.clone(),
        source_node_ids: vec![format!("materialized:{}", node.node_id)],
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

fn handle_delete_workspace_source_node(
    store: &KnowledgeProjectStore,
    workspace_id: &str,
    selected_detail: &GraphNodeDetail,
    request: &ApplyCorrectionRequest,
) -> Result<ApplyCorrectionResponseData> {
    let source_id = selected_detail
        .source
        .as_ref()
        .map(|source| source.source_id.clone())
        .or_else(|| {
            request
                .node_id
                .strip_prefix("source:")
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| anyhow!("source node {} has no source backing", request.node_id))?;
    let deleted_row = store
        .delete_workspace_source(workspace_id, &source_id)?
        .ok_or_else(|| anyhow!("source {source_id} was not found in workspace {workspace_id}"))?;
    store.append_workspace_correction(&WorkspaceCorrection {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        aggregate_node_id: request.node_id.clone(),
        kind: request.kind.clone(),
        target_node_id: None,
        value: request.value.clone(),
        evidence_ids: selected_detail
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_node_ids: vec![format!("{}:{}", deleted_row.project_id, request.node_id)],
        created_at: unix_timestamp_seconds(),
    })?;
    store.materialize_workspace_brain_repo(workspace_id)?;

    let project = store
        .load_workspace_project(workspace_id)?
        .unwrap_or_else(|| empty_workspace_project(workspace_id));
    Ok(ApplyCorrectionResponseData { project })
}

fn empty_workspace_project(workspace_id: &str) -> KnowledgeProject {
    finalize_workspace_project(
        workspace_id,
        Vec::new(),
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        0,
    )
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

fn handle_search_brain(request: SearchBrainRequest) -> Result<SearchBrainResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let db_results = store.search_brain_from_db(
        &request.scope.workspace_id,
        &request.query,
        request.limit.unwrap_or(10),
    )?;
    if !db_results.is_empty() {
        return Ok(SearchBrainResponseData {
            results: db_results,
        });
    }

    let reader = BrainReader::open(&request.scope)?;
    Ok(SearchBrainResponseData {
        results: reader.search(&request.query, request.limit.unwrap_or(10)),
    })
}

fn handle_read_source(request: ReadSourceRequest) -> Result<ReadSourceResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(mut response) = store.read_source_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            if let Ok(reader) = BrainReader::open(&request.scope) {
                enrich_read_source_with_local_paths(&mut response, &reader, &request.source_id);
            }
            expand_read_source_local_paths(&mut response, &root);
        }
        return Ok(response);
    }
    let reader = BrainReader::open(&request.scope)?;
    if let Some(mut response) = store.read_source_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            enrich_read_source_with_local_paths(&mut response, &reader, &request.source_id);
            expand_read_source_local_paths(&mut response, &root);
        }
        return Ok(response);
    }
    let source = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == request.source_id)
        .cloned()
        .ok_or_else(|| anyhow!("source {} was not found", request.source_id))?;
    let wiki_page = reader
        .snapshot
        .wiki_pages
        .iter()
        .find(|page| {
            page.source_refs
                .iter()
                .any(|source_ref| source_ref == &source.source_id)
        })
        .cloned()
        .map(|page| reader.read_wiki_page_body(page))
        .transpose()?;
    let evidence = reader
        .snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source.source_id.as_str()))
        .cloned()
        .collect();
    let mut response = ReadSourceResponseData {
        source,
        wiki_page,
        evidence,
    };
    if !request.include_local_paths {
        redact_read_source_agent_paths(&mut response);
    }
    Ok(response)
}

fn handle_read_page_evidence(
    request: ReadPageEvidenceRequest,
) -> Result<ReadPageEvidenceResponseData> {
    if request.page == Some(0) {
        bail!("argument page must be a positive 1-based integer");
    }

    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(mut response) = store.read_page_evidence_from_db(
        &request.scope.workspace_id,
        &request.source_id,
        request.page,
        request.include_local_paths,
    )? {
        if request.include_local_paths {
            if let Ok(reader) = BrainReader::open(&request.scope) {
                enrich_page_evidence_with_local_paths(&mut response, &reader, &request.source_id);
            }
            expand_page_evidence_local_paths(&mut response, &root);
        }
        return Ok(response);
    }

    let reader = BrainReader::open(&request.scope)?;
    let source = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == request.source_id)
        .cloned()
        .ok_or_else(|| anyhow!("source {} was not found", request.source_id))?;

    let artifact_metadata =
        build_context_pack_artifact_metadata(reader.root(), std::slice::from_ref(&source));
    let mut evidence = artifact_metadata
        .evidence
        .get(&source.source_id)
        .into_iter()
        .flat_map(|source_evidence| source_evidence.iter())
        .filter(|(_, metadata)| request.page.map_or(true, |page| metadata.page == page))
        .map(|(evidence_ref, metadata)| PageEvidenceV0 {
            evidence_ref: evidence_ref.clone(),
            source_id: metadata.source_id.clone(),
            page: metadata.page,
            region: metadata
                .region
                .clone()
                .unwrap_or_else(|| format!("page:{}", metadata.page)),
            span: metadata.span.clone(),
            quoted_text: metadata.quoted_text.clone(),
            parse_confidence: metadata.parse_confidence.clone(),
            content_hash: metadata.content_hash.clone(),
            markdown_path: metadata.markdown_path.clone(),
            image_path: metadata.image_path.clone(),
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| {
        left.page
            .cmp(&right.page)
            .then_with(|| left.evidence_ref.cmp(&right.evidence_ref))
    });

    let mut response = ReadPageEvidenceResponseData {
        source,
        evidence,
        warnings: artifact_metadata.warnings,
    };
    if !request.include_local_paths {
        redact_page_evidence_agent_paths(&mut response);
    }
    Ok(response)
}

fn redact_read_source_agent_paths(response: &mut ReadSourceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.source_path);
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}

fn redact_page_evidence_agent_paths(response: &mut ReadPageEvidenceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}

fn redact_source_record_agent_paths(source: &mut SourceRecord) {
    source.original_path = redact_agent_path(&source.original_path);
    source.source_path = redact_agent_path(&source.source_path);
    source.markdown_path = redact_agent_path(&source.markdown_path);
}

fn redact_optional_agent_path(value: &mut Option<String>) {
    if let Some(path) = value {
        *path = redact_agent_path(path);
    }
}

fn redact_agent_path(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<redacted>".into())
}

fn enrich_read_source_with_local_paths(
    response: &mut ReadSourceResponseData,
    reader: &BrainReader,
    source_id: &str,
) {
    if let Some(source) = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.id.as_str()) {
            evidence.source_path = raw.source_path.clone();
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

fn expand_read_source_local_paths(response: &mut ReadSourceResponseData, workspace_root: &Path) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    for evidence in &mut response.evidence {
        let source_id = evidence
            .source_id
            .as_deref()
            .unwrap_or(response.source.source_id.as_str());
        expand_optional_path(
            &mut evidence.source_path,
            workspace_root,
            &["sources", source_id],
        );
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

fn enrich_page_evidence_with_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    reader: &BrainReader,
    source_id: &str,
) {
    if let Some(source) = reader
        .snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = reader
        .snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.evidence_ref.as_str()) {
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

fn expand_page_evidence_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    workspace_root: &Path,
) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    let source_id = response.source.source_id.as_str();
    for evidence in &mut response.evidence {
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

fn expand_source_record_local_paths(source: &mut SourceRecord, workspace_root: &Path) {
    expand_string_path(&mut source.original_path, workspace_root, &[]);
    expand_string_path(
        &mut source.source_path,
        workspace_root,
        &["sources", source.source_id.as_str()],
    );
    expand_string_path(
        &mut source.markdown_path,
        workspace_root,
        &["artifacts", source.source_id.as_str()],
    );
}

fn expand_optional_path(value: &mut Option<String>, workspace_root: &Path, segments: &[&str]) {
    if let Some(path) = value {
        expand_string_path(path, workspace_root, segments);
    }
}

fn expand_string_path(value: &mut String, workspace_root: &Path, segments: &[&str]) {
    if value.is_empty() || value == "[redacted-local-path]" || Path::new(value).is_absolute() {
        return;
    }
    let mut path = workspace_root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    path.push(value.as_str());
    *value = path.to_string_lossy().into_owned();
}

fn handle_read_wiki_page(request: ReadWikiPageRequest) -> Result<ReadWikiPageResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(page) = store.read_wiki_page_from_db(&request.scope.workspace_id, &request.path)? {
        return Ok(ReadWikiPageResponseData { page });
    }
    let reader = BrainReader::open(&request.scope)?;
    let page = reader.read_wiki_page(&request.path)?;
    Ok(ReadWikiPageResponseData { page })
}

fn handle_read_node(request: ReadNodeRequest) -> Result<ReadNodeResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    if let Some(response) =
        store.read_node_from_db(&request.scope.workspace_id, &request.node_id)?
    {
        return Ok(response);
    }
    let reader = BrainReader::open(&request.scope)?;
    let node = reader
        .snapshot
        .nodes
        .iter()
        .find(|node| node.node_id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    let evidence_ids = node.evidence_ids.iter().collect::<BTreeSet<_>>();
    let evidence = reader
        .snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence_ids.contains(&evidence.id))
        .cloned()
        .collect();
    let relations = reader
        .snapshot
        .relations
        .iter()
        .filter(|relation| {
            relation.source_node_id == node.node_id || relation.target_node_id == node.node_id
        })
        .cloned()
        .collect();
    Ok(ReadNodeResponseData {
        node,
        evidence,
        relations,
    })
}

fn handle_read_recent_events(
    request: ReadRecentEventsRequest,
) -> Result<ReadRecentEventsResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    Ok(ReadRecentEventsResponseData {
        events: reader.recent_events(&request),
    })
}

fn handle_get_context_pack(request: GetContextPackRequest) -> Result<GetContextPackResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let budget = request.budget.unwrap_or(8000);
    let context_pack = if request.selected_node_id.is_some() {
        reader.context_pack_with_selection(
            &request.query,
            budget,
            request.selected_node_id.as_deref(),
        )?
    } else {
        reader.context_pack(&request.query, budget)?
    };
    let artifact_metadata =
        build_context_pack_artifact_metadata(reader.root(), &context_pack.sources);
    let pack_id = format!("ctx_{}", uuid::Uuid::now_v7().simple());
    let generated_at = current_iso_timestamp_utc();
    let context_pack_v0 = hyprduck_engine_types::ContextPackV0::from_brain_context_pack(
        &context_pack,
        pack_id.clone(),
        generated_at.clone(),
        &artifact_metadata,
    );
    let context_pack_v1 = hyprduck_engine_types::ContextPackV1::from_brain_context_pack(
        &context_pack,
        pack_id,
        generated_at,
        &artifact_metadata,
    );
    let context_pack_v1 = if request.selected_node_id.is_none() {
        let root = resolve_brain_workspace_root(&request.scope)?;
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
        store
            .assemble_context_pack_v1_from_db(
                &request.scope.workspace_id,
                &request.query,
                budget,
                context_pack_v1.pack_id.clone(),
                context_pack_v1.generated_at.clone(),
            )
            .unwrap_or(context_pack_v1)
    } else {
        context_pack_v1
    };
    let persisted_context_pack_path = if request.persist {
        Some(persist_context_pack_v1(&request.scope, &context_pack_v1)?)
    } else {
        None
    };
    Ok(GetContextPackResponseData {
        context_pack,
        context_pack_v1,
        context_pack_v0,
        persisted_context_pack_path,
    })
}

fn persist_context_pack_v1(
    scope: &BrainReadScope,
    context_pack: &hyprduck_engine_types::ContextPackV1,
) -> Result<String> {
    let workspace_root = resolve_brain_workspace_root(scope)?;
    let history_dir = workspace_root.join("context_packs");
    fs::create_dir_all(&history_dir)
        .with_context(|| format!("failed creating {}", history_dir.display()))?;
    let json =
        serde_json::to_string_pretty(context_pack).context("failed encoding context pack v1")?;
    let history_path = history_dir.join(format!("{}.json", context_pack.pack_id));
    fs::write(&history_path, &json)
        .with_context(|| format!("failed writing {}", history_path.display()))?;
    let latest_path = workspace_root.join("context_pack.json");
    fs::write(&latest_path, json)
        .with_context(|| format!("failed writing {}", latest_path.display()))?;
    Ok(latest_path.display().to_string())
}

fn handle_read_context_pack(
    request: ReadContextPackRequest,
) -> Result<ReadContextPackResponseData> {
    let workspace_root = resolve_brain_workspace_root(&request.scope)?;
    let repo = BrainArtifactRepository::new(workspace_root);
    let path = match request.pack_id.as_deref() {
        Some(pack_id) => {
            validate_context_pack_id(pack_id)?;
            format!("context_packs/{pack_id}.json")
        }
        None => "context_pack.json".into(),
    };
    let value: Value = repo
        .read_json_artifact(&path)
        .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?;
    let schema_version = value
        .get("schemaVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let context_pack = match schema_version {
        hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION => serde_json::from_value(value)
            .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?,
        hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION => {
            let context_pack_v1: hyprduck_engine_types::ContextPackV1 =
                serde_json::from_value(value)
                    .map_err(|_| anyhow!("persisted context pack could not be read or decoded"))?;
            context_pack_v0_from_v1(context_pack_v1)
        }
        _ => bail!(
            "persisted context pack schemaVersion {} is unsupported",
            schema_version
        ),
    };
    if let Some(pack_id) = request.pack_id.as_deref() {
        if context_pack.pack_id != pack_id {
            bail!(
                "persisted context pack packId {} does not match requested packId {}",
                context_pack.pack_id,
                pack_id
            );
        }
    }
    if context_pack.workspace_id != request.scope.workspace_id {
        bail!(
            "context pack workspace {} does not match requested workspace {}",
            context_pack.workspace_id,
            request.scope.workspace_id
        );
    }
    Ok(ReadContextPackResponseData { context_pack })
}

fn context_pack_v0_from_v1(
    context_pack: hyprduck_engine_types::ContextPackV1,
) -> hyprduck_engine_types::ContextPackV0 {
    hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: context_pack.pack_id,
        workspace_id: context_pack.workspace_id,
        query: context_pack.query,
        generated_at: context_pack.generated_at,
        source_set: context_pack.source_set,
        selected_evidence: context_pack
            .selected_evidence
            .into_iter()
            .map(|evidence| hyprduck_engine_types::ContextPackEvidenceV0 {
                evidence_ref: evidence.evidence_ref,
                source_id: evidence.source_id,
                page: evidence.page,
                region: evidence.region,
                span: evidence.span,
                quoted_text: evidence.quoted_text,
                parse_confidence: evidence.parse_confidence,
                selection_reason: evidence.selection_reason,
                content_hash: evidence.content_hash,
            })
            .collect(),
        findings: context_pack.findings,
        warnings: context_pack.warnings,
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: context_pack.retrieval_trace.strategy,
            chunks_considered: context_pack.retrieval_trace.chunks_considered,
            chunks_selected: context_pack.retrieval_trace.chunks_selected,
            budget_requested: context_pack.retrieval_trace.budget_requested,
            budget_used: context_pack.retrieval_trace.budget_used,
        },
        suggested_next_reads: context_pack.suggested_next_reads,
    }
}

fn validate_context_pack_id(pack_id: &str) -> Result<()> {
    if pack_id.trim().is_empty()
        || pack_id == "."
        || pack_id == ".."
        || pack_id.contains('/')
        || pack_id.contains('\\')
    {
        bail!("invalid packId: context pack IDs must be single path segments");
    }
    Ok(())
}

fn current_iso_timestamp_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    unix_seconds_to_iso_utc(seconds)
}

fn unix_seconds_to_iso_utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

fn handle_get_brain_health(request: GetBrainHealthRequest) -> Result<GetBrainHealthResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let knowledge_store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let knowledge_store_report =
        brain_knowledge_store_report(&knowledge_store, &request.scope.workspace_id)?;
    let repo = BrainArtifactRepository::new(root.clone());
    if !repo.brain_manifest_path().exists() {
        return Ok(GetBrainHealthResponseData {
            status: BrainHealthStatus::Clean,
            attention_count: 0,
            governance: Some(brain_governance_report()),
            knowledge_store: Some(knowledge_store_report),
            source_reports: Vec::new(),
            recent_events: Vec::new(),
        });
    }
    let snapshot = read_materialized_brain_snapshot(&root, &request.scope.workspace_id)?;
    let mut report = lint_brain_snapshot(&snapshot);
    report
        .issues
        .extend(lint_missing_materialized_wiki_refs(&root, &snapshot));
    let mut source_reports = brain_health_source_reports(&repo, &snapshot);
    for report in &mut source_reports {
        if let Some(import_job) = knowledge_store.read_import_job(
            &request.scope.workspace_id,
            None,
            Some(report.source_id.as_str()),
        )? {
            let lifecycle = ImportLifecycleState::from_persisted(
                &import_job.status,
                &import_job.graph_status,
                import_job.citation_ready,
                import_job.graph_ready,
                import_job.graph_retryable,
                import_job.manual_retry_available,
            );
            report.citation_ready = lifecycle.citation_ready;
            report.graph_ready = lifecycle.graph_ready;
            report.graph_status = import_job.graph_status;
            report.manual_retry_available = lifecycle.manual_retry_available;
            if report.citation_ready && !report.graph_ready {
                let warning = match lifecycle.status {
                    ImportLifecycleStatus::CitationReadyGraphSkipped => {
                        "citation_ready_graph_skipped"
                    }
                    ImportLifecycleStatus::GraphRetryWaiting => "graph_retry_waiting",
                    _ => "citation_ready_graph_pending",
                };
                push_health_warning(&mut report.warnings, warning);
            }
        }
    }
    let source_attention_count: usize = source_reports
        .iter()
        .map(|report| report.warnings.len())
        .sum();
    let attention_count = report.issues.len() + source_attention_count;
    let mut recent_events = repo.read_brain_events()?;
    recent_events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    recent_events.truncate(10);
    Ok(GetBrainHealthResponseData {
        status: if attention_count == 0 {
            BrainHealthStatus::Clean
        } else {
            BrainHealthStatus::AttentionNeeded
        },
        attention_count,
        governance: Some(brain_governance_report()),
        knowledge_store: Some(knowledge_store_report),
        source_reports,
        recent_events,
    })
}

fn brain_governance_report() -> BrainGovernanceReport {
    BrainGovernanceReport {
        storage_locality: "local_workspace".into(),
        interaction_surface: "desktop_mcp".into(),
        evidence_governed: true,
        mutating_tools_require_evidence: true,
        local_path_disclosure_default: "redacted".into(),
    }
}

fn brain_knowledge_store_report(
    knowledge_store: &KnowledgeStore,
    workspace_id: &str,
) -> Result<BrainKnowledgeStoreReport> {
    let health = knowledge_store.health()?;
    let summary = knowledge_store.state_summary(workspace_id)?;
    Ok(BrainKnowledgeStoreReport {
        canonical_storage: "sqlite+graphqlite".into(),
        primary_graph_store: "graphqlite".into(),
        pure_sqlite_relational_graph_rejected: true,
        optional_graphqlite_acceleration_rejected: true,
        graph_store_mode: "required_primary".into(),
        graph_native_query_surface: "graphqlite_cypher".into(),
        migration_mode: "single_db_first_release".into(),
        long_dual_write_transition_rejected: true,
        db_schema_version: health.db_schema_version,
        graph_schema_version: health.graph_schema_version,
        graphqlite_loaded: health.graphqlite_loaded,
        graphqlite_transactional: health.graphqlite_transactional,
        graphqlite_release_gate: if health.graphqlite_loaded && health.graphqlite_transactional {
            "passed".into()
        } else {
            "blocked".into()
        },
        release_blocked_without_graphqlite: true,
        migration_blast_radius: "high".into(),
        broad_verification_required: true,
        json_artifacts_canonical: false,
        json_artifact_role: "migration_export_debug_compat".into(),
        vector_search_enabled: false,
        vector_search_policy: "defer_until_db_graphqlite_read_paths_stabilize".into(),
        checkpoint_rollback_api_enabled: false,
        checkpoint_rollback_policy: "defer_until_checkpoints_reliably_stored".into(),
        graph_algorithms_enabled: false,
        graph_algorithm_policy: "revisit_after_primary_graph_data_stabilizes".into(),
        evidence_item_count: summary.evidence_item_count,
        wiki_page_count: summary.wiki_page_count,
        graph_node_count: summary.graph_node_count,
        graph_relation_count: summary.graph_relation_count,
    })
}

fn brain_health_source_reports(
    repo: &BrainArtifactRepository,
    snapshot: &BrainRepoSnapshot,
) -> Vec<BrainHealthSourceReport> {
    snapshot
        .sources
        .iter()
        .map(|source| brain_health_source_report(repo, source))
        .collect()
}

fn brain_health_source_report(
    repo: &BrainArtifactRepository,
    source: &SourceRecord,
) -> BrainHealthSourceReport {
    let mut warnings = Vec::new();
    let mut provider_route = "unknown".to_string();
    let mut local_only = None;
    let mut content_hash = None;
    let mut content_hash_status = "unknown".to_string();
    let mut failed_page_count = 0usize;

    let mut source_pack_read_failed = false;
    let source_pack = match read_source_pack_v0(repo, &source.source_id) {
        Ok(source_pack) => source_pack,
        Err(_error) => {
            source_pack_read_failed = true;
            push_health_warning(&mut warnings, "source_pack_unreadable");
            None
        }
    };
    let valid_source_pack = source_pack.as_ref().filter(|pack| {
        pack.schema_version == hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
            && pack.source_id == source.source_id
            && pack.workspace_id == source.workspace_id
    });
    match source_pack.as_ref() {
        Some(pack)
            if pack.schema_version != hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION =>
        {
            push_health_warning(&mut warnings, "source_pack_schema_mismatch");
        }
        Some(pack) if pack.source_id != source.source_id => {
            push_health_warning(&mut warnings, "source_pack_source_mismatch");
        }
        Some(pack) if pack.workspace_id != source.workspace_id => {
            push_health_warning(&mut warnings, "source_pack_workspace_mismatch");
        }
        None if !source_pack_read_failed => {
            push_health_warning(&mut warnings, "source_pack_missing");
        }
        _ => {}
    }

    if let Some(pack) = valid_source_pack {
        provider_route = pack.provider_route.clone();
        local_only = Some(pack.local_only);
        content_hash = Some(pack.content_hash.clone());
        content_hash_status = "source_pack_only".into();
        failed_page_count = pack
            .pages
            .iter()
            .filter(|page| page.error_message.is_some())
            .count();
        for warning in &pack.warnings {
            push_health_warning(&mut warnings, source_pack_health_warning_summary(warning));
        }
    }

    let mut evidence_index_read_failed = false;
    let evidence_index = match read_evidence_index_artifact(repo, &source.source_id) {
        Ok(evidence_index) => evidence_index,
        Err(_error) => {
            evidence_index_read_failed = true;
            push_health_warning(&mut warnings, "evidence_index_unreadable");
            None
        }
    };
    let valid_evidence_index = evidence_index.as_ref().filter(|index| {
        (index.schema_version() == hyprduck_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
            || index.schema_version() == hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION)
            && index.source_id() == Some(source.source_id.as_str())
            && index.workspace_id() == Some(source.workspace_id.as_str())
    });
    match evidence_index.as_ref() {
        Some(index)
            if index.schema_version()
                != hyprduck_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
                && index.schema_version()
                    != hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION =>
        {
            push_health_warning(&mut warnings, "evidence_index_schema_mismatch");
        }
        Some(index) if index.source_id() != Some(source.source_id.as_str()) => {
            push_health_warning(&mut warnings, "evidence_index_source_mismatch");
        }
        Some(index) if index.workspace_id() != Some(source.workspace_id.as_str()) => {
            push_health_warning(&mut warnings, "evidence_index_workspace_mismatch");
        }
        None if !evidence_index_read_failed => {
            push_health_warning(&mut warnings, "evidence_index_missing");
        }
        _ => {}
    }

    if let Some(index) = valid_evidence_index {
        if provider_route == "unknown" {
            if let Some(route) = index.provider_route() {
                provider_route = route.to_string();
            }
            local_only = index.local_only();
        }
        if let Some(pack_hash) = content_hash.as_deref() {
            if Some(pack_hash) == index.content_hash() {
                content_hash_status = "current".into();
            } else {
                content_hash_status = "mismatch".into();
                push_health_warning(&mut warnings, "content_hash_mismatch");
            }
        } else if let Some(index_hash) = index.content_hash() {
            content_hash = Some(index_hash.to_string());
            content_hash_status = "evidence_index_only".into();
        }
    }

    if source.status == SourceStatus::partial() {
        push_health_warning(&mut warnings, "partial_import");
    } else if source.status == SourceStatus::failed() {
        push_health_warning(&mut warnings, "import_failed");
    } else if source.status == SourceStatus::stale() {
        content_hash_status = "stale".into();
        push_health_warning(&mut warnings, "stale_source");
    }
    if failed_page_count > 0 {
        push_health_warning(
            &mut warnings,
            format!("{failed_page_count} page(s) failed during import"),
        );
    }

    BrainHealthSourceReport {
        source_id: source.source_id.clone(),
        status: source.status.clone(),
        page_count: source.page_count,
        failed_page_count,
        provider_route,
        local_only,
        content_hash,
        content_hash_status,
        citation_ready: success_count_for_health_source(source),
        graph_ready: false,
        graph_status: String::new(),
        manual_retry_available: false,
        warnings,
    }
}

fn success_count_for_health_source(source: &SourceRecord) -> bool {
    source.status != SourceStatus::failed() && source.page_count > 0
}

fn source_pack_health_warning_summary(
    warning: &hyprduck_engine_types::SourcePackWarningV0,
) -> String {
    match warning.page {
        Some(page) => format!(
            "source_pack_warning:{}:severity:{}:page:{page}",
            warning.warning_type,
            warning_severity_slug(&warning.severity)
        ),
        None => format!(
            "source_pack_warning:{}:severity:{}",
            warning.warning_type,
            warning_severity_slug(&warning.severity)
        ),
    }
}

fn warning_severity_slug(
    severity: &hyprduck_engine_types::ContextPackWarningSeverity,
) -> &'static str {
    match severity {
        hyprduck_engine_types::ContextPackWarningSeverity::Low => "low",
        hyprduck_engine_types::ContextPackWarningSeverity::Medium => "medium",
        hyprduck_engine_types::ContextPackWarningSeverity::High => "high",
    }
}

fn push_health_warning(warnings: &mut Vec<String>, warning: impl Into<String>) {
    let warning = warning.into();
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrainLintIssue {
    issue_id: String,
    kind: String,
    severity: String,
    title: String,
    body: String,
    #[serde(default)]
    source_refs: Vec<String>,
    #[serde(default)]
    node_refs: Vec<String>,
    #[serde(default)]
    relation_refs: Vec<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrainMaintenanceReport {
    workspace_id: String,
    generated_at: u64,
    issue_count: usize,
    repair_count: usize,
    new_markdown_source_count: usize,
    enqueued_markdown_source_count: usize,
    ingest_worker_started: bool,
    ingested_markdown_source_count: usize,
    failed_markdown_source_count: usize,
    #[serde(default)]
    repairs: Vec<String>,
    #[serde(default)]
    new_markdown_sources: Vec<String>,
    #[serde(default)]
    enqueued_markdown_sources: Vec<String>,
    #[serde(default)]
    ingested_markdown_sources: Vec<String>,
    #[serde(default)]
    failed_markdown_sources: Vec<String>,
    #[serde(default)]
    issues: Vec<BrainLintIssue>,
}

#[cfg(test)]
fn run_brain_maintenance(scope: &BrainReadScope) -> Result<BrainMaintenanceReport> {
    let root = resolve_brain_workspace_root(scope)?;
    let ingest_paths = resolve_markdown_ingest_paths(scope)?;
    let initial_snapshot = read_materialized_brain_snapshot(&root, &scope.workspace_id)?;
    let source_state = read_markdown_source_state(&ingest_paths)?;
    let ingest_queue = read_markdown_ingest_queue(&ingest_paths)?;
    let markdown_scan = scan_new_markdown_sources(
        &ingest_paths,
        &initial_snapshot,
        &source_state,
        &ingest_queue,
    )?;
    let enqueue_result = {
        let writer = BrainWorkspaceWriter::open(root.clone())?;
        enqueue_markdown_sources(&writer, &ingest_paths, &ingest_queue, &markdown_scan)?
    };
    write_markdown_source_state(&ingest_paths, &markdown_scan.current_state)?;
    let queued_after_enqueue = read_markdown_ingest_queue(&ingest_paths)?;
    let store = KnowledgeProjectStore::default()?;
    let worker_result = run_markdown_ingest_worker(&ingest_paths, &queued_after_enqueue, &store)?;
    let mut snapshot = if worker_result.processed > 0 || worker_result.failed > 0 {
        read_materialized_brain_snapshot(&root, &scope.workspace_id)?
    } else {
        initial_snapshot
    };
    let mut report = lint_brain_snapshot(&snapshot);
    report.new_markdown_source_count = markdown_scan.new_sources.len();
    report.new_markdown_sources = markdown_scan
        .new_sources
        .iter()
        .map(|source| source.relative_path.display().to_string())
        .collect();
    report.enqueued_markdown_source_count = enqueue_result.enqueued.len();
    report.enqueued_markdown_sources = enqueue_result
        .enqueued
        .iter()
        .map(|source| source.relative_path.clone())
        .collect();
    report.ingest_worker_started = worker_result.started;
    report.ingested_markdown_source_count = worker_result.processed;
    report.failed_markdown_source_count = worker_result.failed;
    report.ingested_markdown_sources = worker_result.processed_sources;
    report.failed_markdown_sources = worker_result.failed_sources;
    report.repair_count +=
        repair_missing_materialized_wiki_stubs(&root, &mut snapshot, &mut report.repairs)?;
    report
        .issues
        .extend(lint_missing_materialized_wiki_refs(&root, &snapshot));
    report.repair_count += repair_generated_brain_artifacts(&root, &snapshot, &mut report.repairs)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    report.issue_count = report.issues.len();
    write_json_pretty(&root.join("state/maintenance-latest.json"), &report)?;
    if report.repair_count > 0
        || report.new_markdown_source_count > 0
        || report.enqueued_markdown_source_count > 0
        || report.ingest_worker_started
    {
        writer.append_event(&brain_maintenance_event(&snapshot, &report)?)?;
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn materialized_graph_event_payload_json(
    generated_at: u64,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    relations: &[BrainRelationRecord],
    evidence: &[EvidenceRef],
    memories: &[MemoryRecord],
    wiki_pages: &[WikiPage],
    entities: &[EntityRecord],
    claims: &[ClaimRecord],
    extractions: &[StructuredExtractionArtifact],
) -> Result<String> {
    serde_json::to_string(&json!({
        "nodeCount": nodes.len(),
        "relationCount": relations.len(),
        "sourceCount": sources.len(),
        "materializedGraph": {
            "generatedAt": generated_at,
            "sources": sources,
            "nodes": nodes,
            "edges": relations,
            "evidence": evidence,
            "memories": memories,
            "wikiPages": wiki_pages,
            "entities": entities,
            "claims": claims,
            "extractions": extractions,
        }
    }))
    .context("failed to encode materialized graph event payload")
}

fn lint_brain_snapshot(snapshot: &BrainRepoSnapshot) -> BrainMaintenanceReport {
    let mut issues = Vec::new();
    let generated_at = unix_timestamp_seconds();
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let connected_node_ids = snapshot
        .relations
        .iter()
        .flat_map(|relation| {
            [
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ]
        })
        .collect::<BTreeSet<_>>();

    for claim in &snapshot.claims {
        let missing_evidence = missing_refs(&claim.evidence_refs, &evidence_ids);
        if claim.evidence_refs.is_empty() || !missing_evidence.is_empty() {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id(
                    "missing-evidence",
                    &claim.claim_id,
                    &missing_evidence,
                ),
                kind: "missing_evidence".into(),
                severity: "risky".into(),
                title: format!("Claim needs evidence: {}", claim.statement),
                body: if claim.evidence_refs.is_empty() {
                    "This claim has no evidence refs. Review it before agents use it as durable brain context.".into()
                } else {
                    format!(
                        "This claim references missing evidence ids: {}.",
                        missing_evidence.join(", ")
                    )
                },
                source_refs: claim.source_refs.clone(),
                node_refs: claim.topic_refs.clone(),
                relation_refs: Vec::new(),
                evidence_refs: claim.evidence_refs.clone(),
            });
        }
    }

    for relation in &snapshot.relations {
        let mut missing_nodes = missing_refs(
            &[
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ],
            &node_ids,
        );
        let missing_evidence = missing_refs(&relation.evidence_ids, &evidence_ids);
        if !missing_nodes.is_empty() || !missing_evidence.is_empty() {
            missing_nodes.extend(missing_evidence);
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("orphan-relation", &relation.relation_id, &missing_nodes),
                kind: "orphan".into(),
                severity: "risky".into(),
                title: format!("Typed relation needs review: {}", relation.label),
                body: "This relation points at a missing node or evidence ref. Review it before keeping it in the durable graph.".into(),
                source_refs: Vec::new(),
                node_refs: vec![relation.source_node_id.clone(), relation.target_node_id.clone()],
                relation_refs: vec![relation.relation_id.clone()],
                evidence_refs: relation.evidence_ids.clone(),
            });
        }
    }

    for node in &snapshot.nodes {
        if matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic)
            && node.evidence_ids.is_empty()
            && node.source_ids.is_empty()
            && !connected_node_ids.contains(&node.node_id)
        {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("orphan-node", &node.node_id, &[]),
                kind: "orphan".into(),
                severity: "risky".into(),
                title: format!("Orphan node needs review: {}", node.label),
                body: "This node is not connected to a source, evidence ref, or typed relation."
                    .into(),
                source_refs: Vec::new(),
                node_refs: vec![node.node_id.clone()],
                relation_refs: Vec::new(),
                evidence_refs: Vec::new(),
            });
        }
        let missing_sources = missing_refs(&node.source_ids, &source_ids);
        if !missing_sources.is_empty() {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("missing-source", &node.node_id, &missing_sources),
                kind: "missing_evidence".into(),
                severity: "risky".into(),
                title: format!("Node references missing source: {}", node.label),
                body: format!("Missing source refs: {}.", missing_sources.join(", ")),
                source_refs: node.source_ids.clone(),
                node_refs: vec![node.node_id.clone()],
                relation_refs: Vec::new(),
                evidence_refs: node.evidence_ids.clone(),
            });
        }
    }

    for source in &snapshot.sources {
        if source.status == "stale" || source.updated_at > snapshot.generated_at {
            issues.push(BrainLintIssue {
                issue_id: stable_lint_issue_id("stale-source", &source.source_id, &[]),
                kind: "stale".into(),
                severity: "risky".into(),
                title: format!("Source may need recompilation: {}", source.source_id),
                body: "This source is stale or newer than the materialized brain snapshot.".into(),
                source_refs: vec![source.source_id.clone()],
                node_refs: Vec::new(),
                relation_refs: Vec::new(),
                evidence_refs: Vec::new(),
            });
        }
    }

    for (left_index, left) in snapshot.claims.iter().enumerate() {
        for right in snapshot.claims.iter().skip(left_index + 1) {
            if claims_may_conflict(left, right) {
                issues.push(BrainLintIssue {
                    issue_id: stable_lint_issue_id(
                        "conflict",
                        &left.claim_id,
                        std::slice::from_ref(&right.claim_id),
                    ),
                    kind: "conflict".into(),
                    severity: "risky".into(),
                    title: "Claims may conflict".into(),
                    body: format!(
                        "Review potentially conflicting claims: `{}` vs `{}`.",
                        left.statement, right.statement
                    ),
                    source_refs: left
                        .source_refs
                        .iter()
                        .chain(right.source_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    node_refs: left
                        .topic_refs
                        .iter()
                        .chain(right.topic_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    relation_refs: Vec::new(),
                    evidence_refs: left
                        .evidence_refs
                        .iter()
                        .chain(right.evidence_refs.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                });
            }
        }
    }

    BrainMaintenanceReport {
        workspace_id: snapshot.workspace_id.clone(),
        generated_at,
        issue_count: issues.len(),
        repair_count: 0,
        new_markdown_source_count: 0,
        enqueued_markdown_source_count: 0,
        ingest_worker_started: false,
        ingested_markdown_source_count: 0,
        failed_markdown_source_count: 0,
        repairs: Vec::new(),
        new_markdown_sources: Vec::new(),
        enqueued_markdown_sources: Vec::new(),
        ingested_markdown_sources: Vec::new(),
        failed_markdown_sources: Vec::new(),
        issues,
    }
}

fn missing_refs(refs: &[String], existing: &BTreeSet<String>) -> Vec<String> {
    refs.iter()
        .filter(|value| !existing.contains(*value))
        .cloned()
        .collect()
}

fn lint_missing_materialized_wiki_refs(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
) -> Vec<BrainLintIssue> {
    let wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let mut missing = BTreeMap::<String, BrainLintIssue>::new();

    for page in &snapshot.wiki_pages {
        if !root.join(&page.path).exists() {
            upsert_missing_wiki_issue(
                &mut missing,
                &page.path,
                &format!("wiki-page:{}", page.page_id),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
    }

    for node in &snapshot.nodes {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &node.source_ids);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("node:{}", node.node_id),
                vec![node.node_id.clone()],
                node.source_ids.clone(),
                node.evidence_ids.clone(),
            );
        }
    }

    for claim in &snapshot.claims {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &claim.source_refs);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("claim:{}", claim.claim_id),
                claim.topic_refs.clone(),
                claim.source_refs.clone(),
                claim.evidence_refs.clone(),
            );
        }
    }

    for memory in &snapshot.memories {
        let missing_refs = missing_wiki_refs(root, &wiki_paths, &memory.source_refs);
        for path in missing_refs {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("memory:{}", memory.memory_id),
                Vec::new(),
                memory.source_refs.clone(),
                memory.evidence_refs.clone(),
            );
        }
    }

    for event in &snapshot.events {
        if !is_graph_or_memory_change_event(event.event_type) {
            continue;
        }
        let event_refs = event
            .source_refs
            .iter()
            .chain(event.source_markdown_refs.iter())
            .cloned()
            .collect::<Vec<_>>();
        for path in missing_wiki_refs(root, &wiki_paths, &event_refs) {
            upsert_missing_wiki_issue(
                &mut missing,
                &path,
                &format!("event:{}", event.event_id),
                event.node_refs.clone(),
                event_refs.clone(),
                event.evidence_refs.clone(),
            );
        }
    }

    missing.into_values().collect()
}

#[derive(Debug, Clone, Default)]
#[cfg(test)]
struct MissingWikiPageStub {
    path: String,
    title: String,
    contexts: Vec<String>,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
}

#[cfg(test)]
fn repair_missing_materialized_wiki_stubs(
    root: &Path,
    snapshot: &mut BrainRepoSnapshot,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let mut stubs = BTreeMap::<String, MissingWikiPageStub>::new();
    let node_labels = snapshot
        .nodes
        .iter()
        .map(|node| (node.node_id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();

    for page in &snapshot.wiki_pages {
        if !root.join(&page.path).exists() {
            let path_node_ref = page
                .path
                .strip_prefix("wiki/topics/")
                .and_then(|path| path.strip_suffix(".md"))
                .map(ToString::to_string);
            let page_node_refs = merge_string_refs(
                &page.node_refs,
                &path_node_ref.clone().into_iter().collect::<Vec<_>>(),
            );
            let page_context = if page_node_refs.is_empty() {
                format!(
                    "Existing materialized page record `{}` was missing on disk.",
                    page.page_id
                )
            } else {
                let labels = page
                    .node_refs
                    .iter()
                    .chain(path_node_ref.iter())
                    .map(|node_id| {
                        node_labels
                            .get(node_id)
                            .map(String::as_str)
                            .unwrap_or(node_id.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "Markdown-derived node page record `{}` was missing on disk for: {}.",
                    page.page_id, labels
                )
            };
            upsert_missing_wiki_stub(
                &mut stubs,
                &page.path,
                &page.title,
                page_context,
                page_node_refs,
                page.source_refs.clone(),
                page.evidence_refs.clone(),
            );
        }
    }

    for node in &snapshot.nodes {
        for path in missing_wiki_refs(root, &wiki_paths, &node.source_ids) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                &node.label,
                format!(
                    "Markdown-derived node `{}` (`{}`) originated from this missing wiki page.",
                    node.node_id, node.label
                ),
                vec![node.node_id.clone()],
                node.source_ids.clone(),
                node.evidence_ids.clone(),
            );
        }
    }

    for claim in &snapshot.claims {
        for path in missing_wiki_refs(root, &wiki_paths, &claim.source_refs) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                "Recovered Claim Context",
                format!(
                    "Markdown-derived claim `{}`: {}",
                    claim.claim_id, claim.statement
                ),
                claim.topic_refs.clone(),
                claim.source_refs.clone(),
                claim.evidence_refs.clone(),
            );
        }
    }

    for memory in &snapshot.memories {
        for path in missing_wiki_refs(root, &wiki_paths, &memory.source_refs) {
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                &memory.title,
                format!(
                    "Markdown-derived memory `{}`: {}",
                    memory.memory_id, memory.body
                ),
                Vec::new(),
                memory.source_refs.clone(),
                memory.evidence_refs.clone(),
            );
        }
    }

    for event in &snapshot.events {
        if !is_graph_or_memory_change_event(event.event_type) {
            continue;
        }
        let event_refs = event
            .source_refs
            .iter()
            .chain(event.source_markdown_refs.iter())
            .cloned()
            .collect::<Vec<_>>();
        for path in missing_wiki_refs(root, &wiki_paths, &event_refs) {
            let mut context = format!(
                "Event `{}` applied `{}` from markdown-derived graph context.",
                event.event_id,
                event.operation_type.as_deref().unwrap_or("graph_change")
            );
            if !event.relation_refs.is_empty() {
                let relation_contexts = event
                    .relation_refs
                    .iter()
                    .filter_map(|relation_id| {
                        snapshot
                            .relations
                            .iter()
                            .find(|relation| &relation.relation_id == relation_id)
                    })
                    .map(|relation| {
                        let source = node_labels
                            .get(&relation.source_node_id)
                            .map(String::as_str)
                            .unwrap_or(&relation.source_node_id);
                        let target = node_labels
                            .get(&relation.target_node_id)
                            .map(String::as_str)
                            .unwrap_or(&relation.target_node_id);
                        format!(
                            "edge `{}` connects `{}` to `{}` as `{}`",
                            relation.relation_id, source, target, relation.label
                        )
                    })
                    .collect::<Vec<_>>();
                if !relation_contexts.is_empty() {
                    context.push_str(" Related edge context: ");
                    context.push_str(&relation_contexts.join("; "));
                    context.push('.');
                }
            }
            upsert_missing_wiki_stub(
                &mut stubs,
                &path,
                "Recovered Graph Context",
                context,
                event.node_refs.clone(),
                event_refs.clone(),
                event.evidence_refs.clone(),
            );
        }
    }

    if stubs.is_empty() {
        return Ok(0);
    }

    let existing_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    let updated_at = unix_timestamp_seconds();
    for stub in stubs.values() {
        if !existing_paths.contains(&stub.path) {
            let existing_body = fs::read_to_string(root.join(&stub.path)).ok();
            snapshot.wiki_pages.push(WikiPage {
                page_id: format!("wiki-stub-{}", sanitize_name(&stub.path)),
                workspace_id: snapshot.workspace_id.clone(),
                path: stub.path.clone(),
                title: stub.title.clone(),
                body: existing_body.unwrap_or_else(|| missing_wiki_stub_body(stub)),
                node_refs: merge_string_refs(&stub.node_refs, &[]),
                source_refs: merge_string_refs(&stub.source_refs, std::slice::from_ref(&stub.path)),
                evidence_refs: merge_string_refs(&stub.evidence_refs, &[]),
                updated_at,
            });
        } else if let Some(page) = snapshot
            .wiki_pages
            .iter_mut()
            .find(|page| page.path == stub.path)
        {
            page.body = missing_wiki_stub_body(stub);
            page.node_refs = merge_string_refs(&page.node_refs, &stub.node_refs);
            page.source_refs = merge_string_refs(&page.source_refs, &stub.source_refs);
            page.evidence_refs = merge_string_refs(&page.evidence_refs, &stub.evidence_refs);
            page.updated_at = page.updated_at.max(updated_at);
        }
        merge_unique_string(repairs, &stub.path);
    }
    snapshot.wiki_pages = dedupe_wiki_pages(std::mem::take(&mut snapshot.wiki_pages));
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    Ok(stubs.len())
}

#[cfg(test)]
fn upsert_missing_wiki_stub(
    stubs: &mut BTreeMap<String, MissingWikiPageStub>,
    path: &str,
    title: &str,
    context: String,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
) {
    if !is_wiki_markdown_ref(path) {
        return;
    }
    let stub = stubs
        .entry(path.to_string())
        .or_insert_with(|| MissingWikiPageStub {
            path: path.to_string(),
            title: if title.trim().is_empty() {
                title_from_wiki_path(path)
            } else {
                title.trim().to_string()
            },
            contexts: Vec::new(),
            node_refs: Vec::new(),
            source_refs: vec![path.to_string()],
            evidence_refs: Vec::new(),
        });
    merge_unique_string(&mut stub.contexts, &context);
    for node_ref in node_refs {
        merge_unique_string(&mut stub.node_refs, &node_ref);
    }
    for source_ref in source_refs {
        merge_unique_string(&mut stub.source_refs, &source_ref);
    }
    for evidence_ref in evidence_refs {
        merge_unique_string(&mut stub.evidence_refs, &evidence_ref);
    }
}

#[cfg(test)]
fn missing_wiki_stub_body(stub: &MissingWikiPageStub) -> String {
    let contexts = if stub.contexts.is_empty() {
        "- Recovered from a missing materialized wiki reference.".into()
    } else {
        stub.contexts
            .iter()
            .map(|context| format!("- {context}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# {}\n\nThis page was automatically regenerated as a reviewable stub from markdown-derived graph context.\n\n## Origin Context\n\n{}\n\n## Refs\n\n- Nodes: {}\n- Sources: {}\n- Evidence: {}\n",
        stub.title,
        contexts,
        join_or_none(&stub.node_refs),
        join_or_none(&stub.source_refs),
        join_or_none(&stub.evidence_refs)
    )
}

#[cfg(test)]
fn title_from_wiki_path(path: &str) -> String {
    path.trim_start_matches("wiki/")
        .trim_end_matches(".md")
        .rsplit('/')
        .next()
        .unwrap_or("Recovered Wiki Page")
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn missing_wiki_refs(root: &Path, wiki_paths: &BTreeSet<String>, refs: &[String]) -> Vec<String> {
    refs.iter()
        .filter(|value| is_wiki_markdown_ref(value))
        .filter(|value| !wiki_paths.contains(*value) || !root.join(value).exists())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_wiki_markdown_ref(value: &str) -> bool {
    value.starts_with("wiki/") && value.ends_with(".md") && !value.contains("..")
}

fn is_graph_or_memory_change_event(event_type: BrainEventKind) -> bool {
    matches!(
        event_type,
        BrainEventKind::GraphMaterialized
            | BrainEventKind::WikiMaterialized
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::CorrectionApplied
            | BrainEventKind::BrainMaintenanceRun
    )
}

fn upsert_missing_wiki_issue(
    issues: &mut BTreeMap<String, BrainLintIssue>,
    path: &str,
    origin: &str,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
) {
    let issue = issues.entry(path.to_string()).or_insert_with(|| BrainLintIssue {
        issue_id: stable_lint_issue_id("missing-wiki-page", path, &[]),
        kind: "missing_wiki_page".into(),
        severity: "risky".into(),
        title: format!("Wiki page is not materialized: {path}"),
        body: "A graph or memory change references a wiki page that is absent from the materialized wiki. Re-run replay/materialization before agents use this path.".into(),
        source_refs: vec![path.to_string()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        evidence_refs: Vec::new(),
    });
    merge_unique_string(&mut issue.source_refs, path);
    merge_unique_string(&mut issue.source_refs, origin);
    for source_ref in source_refs {
        merge_unique_string(&mut issue.source_refs, &source_ref);
    }
    for node_ref in node_refs {
        merge_unique_string(&mut issue.node_refs, &node_ref);
    }
    for evidence_ref in evidence_refs {
        merge_unique_string(&mut issue.evidence_refs, &evidence_ref);
    }
}

fn stable_lint_issue_id(kind: &str, primary: &str, rest: &[String]) -> String {
    let mut parts = vec![kind.to_string(), primary.to_string()];
    parts.extend(rest.iter().cloned());
    format!("lint-{}", sanitize_name(&parts.join("-")))
}

fn claims_may_conflict(left: &ClaimRecord, right: &ClaimRecord) -> bool {
    if left.topic_refs.is_empty()
        || right.topic_refs.is_empty()
        || left
            .topic_refs
            .iter()
            .all(|topic| !right.topic_refs.contains(topic))
    {
        return false;
    }
    let left_negative = contains_negative_claim_marker(&left.statement);
    let right_negative = contains_negative_claim_marker(&right.statement);
    left_negative != right_negative && shared_claim_terms(&left.statement, &right.statement) >= 3
}

fn contains_negative_claim_marker(value: &str) -> bool {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "no" | "not" | "never" | "without"
            )
        })
}

fn shared_claim_terms(left: &str, right: &str) -> usize {
    let left_terms = claim_terms(left);
    let right_terms = claim_terms(right);
    left_terms.intersection(&right_terms).count()
}

fn claim_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| word.len() >= 4)
        .filter(|word| {
            !matches!(
                word.as_str(),
                "this" | "that" | "with" | "from" | "into" | "evidence" | "backed"
            )
        })
        .collect()
}

#[cfg(test)]
fn repair_generated_brain_artifacts(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let mut count = 0;
    for page in snapshot
        .wiki_pages
        .iter()
        .filter(|page| page.path == "wiki/index.md" || page.path == "wiki/log.md")
    {
        let path = root.join(&page.path);
        let next = materialized_wiki_page_body(page, snapshot);
        if fs::read_to_string(&path).unwrap_or_default() != next {
            write_file_atomic(&path, next.as_bytes())?;
            repairs.push(page.path.clone());
            count += 1;
        }
    }
    count += repair_json_artifact(
        &root.join("graph/nodes.json"),
        &snapshot.nodes,
        "graph/nodes.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/edges.json"),
        &snapshot.relations,
        "graph/edges.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/evidence.json"),
        &snapshot.evidence,
        "graph/evidence.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/entities.json"),
        &snapshot.entities,
        "graph/entities.json",
        repairs,
    )?;
    count += repair_json_artifact(
        &root.join("graph/claims.json"),
        &snapshot.claims,
        "graph/claims.json",
        repairs,
    )?;
    Ok(count)
}

#[cfg(test)]
fn repair_json_artifact<T: Serialize>(
    path: &Path,
    value: &T,
    label: &str,
    repairs: &mut Vec<String>,
) -> Result<usize> {
    let next = serde_json::to_string_pretty(value).context("failed to encode repair artifact")?;
    if fs::read_to_string(path).unwrap_or_default() == next {
        return Ok(0);
    }
    write_file_atomic(path, next.as_bytes())?;
    repairs.push(label.into());
    Ok(1)
}

#[cfg(test)]
fn brain_maintenance_event(
    snapshot: &BrainRepoSnapshot,
    report: &BrainMaintenanceReport,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::BrainMaintenanceRun,
        operation_type: Some("brain_maintenance_run".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "hyprduck-maintenance".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(report)
            .context("failed to encode maintenance event payload")?,
        causality: BrainEventCausality {
            snapshot_id: Some(format!(
                "snapshot-{}-{}",
                snapshot.workspace_id, snapshot.generated_at
            )),
            materialized_version: Some(snapshot.generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: if report.issue_count == 0 {
            "auto_repaired".into()
        } else {
            "attention_needed".into()
        },
        created_at: report.generated_at,
    })
}
fn brain_node_record_content_matches(left: &BrainNodeRecord, right: &BrainNodeRecord) -> bool {
    left.node_id == right.node_id
        && left.kind == right.kind
        && left.label == right.label
        && left.scope == right.scope
        && left.aliases == right.aliases
        && left.evidence_ids == right.evidence_ids
        && left.source_ids == right.source_ids
        && left.confidence == right.confidence
}

fn brain_relation_record_content_matches(
    left: &BrainRelationRecord,
    right: &BrainRelationRecord,
) -> bool {
    left.relation_id == right.relation_id
        && left.kind == right.kind
        && left.source_node_id == right.source_node_id
        && left.target_node_id == right.target_node_id
        && left.label == right.label
        && left.evidence_ids == right.evidence_ids
        && left.confidence == right.confidence
}

#[cfg(test)]
fn dedupe_wiki_pages(pages: Vec<WikiPage>) -> Vec<WikiPage> {
    let mut merged = BTreeMap::<String, WikiPage>::new();
    for mut page in pages {
        page.node_refs = merge_string_refs(&page.node_refs, &[]);
        page.source_refs = merge_string_refs(&page.source_refs, &[]);
        page.evidence_refs = merge_string_refs(&page.evidence_refs, &[]);
        match merged.get_mut(&page.path) {
            Some(existing) => merge_wiki_page_record(existing, page),
            None => {
                merged.insert(page.path.clone(), page);
            }
        }
    }
    let mut pages = merged.into_values().collect::<Vec<_>>();
    pages.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
    pages
}

#[cfg(test)]
fn merge_wiki_page_record(existing: &mut WikiPage, incoming: WikiPage) {
    existing.node_refs = merge_string_refs(&existing.node_refs, &incoming.node_refs);
    existing.source_refs = merge_string_refs(&existing.source_refs, &incoming.source_refs);
    existing.evidence_refs = merge_string_refs(&existing.evidence_refs, &incoming.evidence_refs);
    if incoming.updated_at >= existing.updated_at {
        existing.page_id = incoming.page_id;
        existing.workspace_id = incoming.workspace_id;
        existing.title = incoming.title;
        existing.body = incoming.body;
        existing.updated_at = incoming.updated_at;
    }
}

fn merge_unique_string(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

fn refresh_materialized_wiki_pages(snapshot: &mut BrainRepoSnapshot) {
    let generated = build_materialized_wiki_pages(
        &snapshot.workspace_id,
        &snapshot.sources,
        &snapshot.nodes,
        unix_timestamp_seconds(),
    );
    let generated_paths = generated
        .iter()
        .map(|page| page.path.clone())
        .collect::<BTreeSet<_>>();
    snapshot
        .wiki_pages
        .retain(|page| !generated_paths.contains(&page.path));
    snapshot.wiki_pages.extend(generated);
    snapshot.wiki_pages.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
}

fn join_or_none(values: &[String]) -> String {
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
