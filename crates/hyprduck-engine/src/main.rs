use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use hyprduck_engine_types::{
    AgentGraphProposalPayload, AgentGraphProposalValidationCode, AgentGraphProposalValidationError,
    AgentGraphProposalValidationIssue, AgentNewEdgePayload, AnswerProjectRequest,
    AnswerProjectResponseData, AnswerResponse, AnswerStatus, ApplyCorrectionRequest,
    ApplyCorrectionResponseData, BrainActor, BrainActorType, BrainContextPack, BrainEvent,
    BrainEventCausality, BrainEventKind, BrainHealthStatus, BrainNodeKind, BrainNodeRecord,
    BrainProposalKind, BrainProposalStatus, BrainReadScope, BrainRelationKind, BrainRelationRecord,
    BrainRepoSnapshot, BrainReviewDecision, BrainReviewItem, BrainScope, BrainSearchResult,
    BrainSearchResultKind, BrainUpdateProposal, ClaimRecord, CompileProjectRequest,
    CompileProjectResponseData, CorrectionAction, CorrectionKind, DocumentFormat, EngineCommand,
    EngineFailure, EngineRequest, EngineRuntimeEvent, EngineRuntimeFailure, EngineRuntimeRequest,
    EngineRuntimeResponse, EngineSuccess, EntityRecord, EvidenceRef, GetBrainHealthRequest,
    GetBrainHealthResponseData, GetContextPackRequest, GetContextPackResponseData,
    GraphHistoryEntry, GraphNodeDetail, GraphNodeKind, GraphNodePosition, GraphNodeSummary,
    GraphRollbackTarget, IngestStatus, KnowledgeProject, ListBrainReviewItemsRequest,
    ListBrainReviewItemsResponseData, LoadConfigRequest, LoadProjectRequest,
    LoadProjectResponseData, MemoryRecord, PageArtifact, ParseEvent, ParseMetadata, ParseRequest,
    ParseResponseData, ParseResult, ParsedPage, ProjectOverview, ProjectStatus,
    ProposeBrainUpdateRequest, ProposeBrainUpdateResponseData, ReadGraphHistoryRequest,
    ReadGraphHistoryResponseData, ReadGraphSnapshotRequest, ReadGraphSnapshotResponseData,
    ReadNodeRequest, ReadNodeResponseData, ReadRecentEventsRequest, ReadRecentEventsResponseData,
    ReadSourceRequest, ReadSourceResponseData, ReadWikiPageRequest, ReadWikiPageResponseData,
    ReconstructBrainRequest, ReconstructBrainResponseData, RelationEdgeDetail, RelationEdgeSummary,
    RelationKind, ResolveBrainReviewItemRequest, ResolveBrainReviewItemResponseData,
    SaveConfigRequest, SaveConfigResponseData, SearchBrainRequest, SearchBrainResponseData,
    SourceArtifactManifest, SourceBacking, SourceId, SourceRecord, SourceSummary,
    StructuredExtractionArtifact, StructuredExtractionClaim, StructuredExtractionEntity,
    StructuredExtractionMemoryCandidate, StructuredExtractionPageRef, StructuredExtractionRelation,
    StructuredExtractionTopic, SuggestedAction, SuggestedActionKind, ValidateProviderRequest,
    WikiPage, WorkspaceCorrection, WorkspaceId, BRAIN_EVENT_SCHEMA_VERSION,
};
#[cfg(test)]
use hyprduck_engine_types::{
    AgentNewClaimPayload, AgentNewMemoryPayload, AgentNewNodePayload, OutputAsset, ParseInput,
    ParseOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(test)]
use tempfile::tempdir;
use uuid::{Uuid, Version};

mod brain_repo;
mod chat_openai_compatible_client;
mod consolidation;
mod import_context;
mod knowledge;
mod parse;
mod provider;
mod provider_graph_prompt;
mod retrieval;
mod run_artifacts;
mod source_index;

use brain_repo::*;
use chat_openai_compatible_client::{parse_openai_compatible_with_timeout, provider_unavailable};
use consolidation::{
    build_post_import_consolidation_prompt, failed_post_import_consolidation_report_value,
    post_import_consolidation_report_value, should_run_post_import_consolidation,
};
use import_context::{
    build_import_evidence_context, import_evidence_context_allowed_refs, ImportEvidenceContext,
};
use knowledge::*;
use parse::parse_document;
use provider::{
    check_readiness, provider_model_catalog, validate_provider, EngineConfig, EngineConfigStore,
};
use provider_graph_prompt::build_provider_graph_proposal_prompt;
use run_artifacts::queued_proposal_provider_response_value;
use source_index::{chunk_source_markdown, upsert_source_chunks};

const DEFAULT_WORKSPACE_ID: &str = "default";
const PROJECT_SNAPSHOT_BATCH_SIZE: usize = 200;
const MARKDOWN_INGEST_QUEUE_PATH: &str = "state/markdown-ingest-queue.json";
const MARKDOWN_SOURCE_STATE_PATH: &str = "state/markdown-sources.json";
const LATEST_READABLE_SNAPSHOT_PATH: &str = "state/latest-readable-snapshot.json";
const PROVIDER_GRAPH_AGENT_ID: &str = "hyprduck-provider-graph-agent";
const BRAIN_LOCK_DIRECTORY_NAME: &str = ".brain.lock";
const PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS: u64 = 45;

thread_local! {
    static RUNTIME_EVENT_REQUEST_ID: RefCell<Option<Uuid>> = const { RefCell::new(None) };
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if std::env::args().skip(1).any(|arg| arg == "serve") {
        return run_runtime_server();
    }

    let mut payload = String::new();
    io::stdin()
        .read_to_string(&mut payload)
        .context("failed to read engine request")?;
    let request = decode_request(&payload)?;
    let config_store = EngineConfigStore::default()?;
    let response = match request {
        EngineRequest::Parse(request) => encode_parse_response(request, &payload, &config_store)?,
        request => encode_success_response(request, &config_store)?,
    };
    io::stdout()
        .write_all(response.as_bytes())
        .context("failed to write engine response")?;
    Ok(())
}

fn run_runtime_server() -> Result<()> {
    let stdin = io::stdin();
    let config_store = EngineConfigStore::default()?;

    for line in stdin.lock().lines() {
        let payload = line.context("failed to read runtime request")?;
        if payload.trim().is_empty() {
            continue;
        }

        let response = match decode_runtime_request(&payload) {
            Ok(envelope) if !is_uuid_v7(envelope.id) => {
                let command = request_command(&envelope.request);
                serde_json::to_string(&EngineRuntimeFailure::new(
                    envelope.id,
                    EngineFailure::new(
                        command,
                        "invalid_request_id",
                        "runtime request id must be a UUIDv7 string",
                    ),
                ))
                .context("failed to encode invalid runtime request id response")?
            }
            Ok(envelope) => {
                let id = envelope.id;
                match envelope.request {
                    EngineRequest::Parse(request) => {
                        encode_runtime_parse_response(id, request, &payload, &config_store)
                            .unwrap_or_else(|error| {
                                encode_runtime_failure_response(id, EngineCommand::Parse, &error)
                            })
                    }
                    request => {
                        let command = request_command(&request);
                        encode_success_response(request, &config_store)
                            .and_then(|response| wrap_runtime_response(id, &response))
                            .unwrap_or_else(|error| {
                                encode_runtime_failure_response(id, command, &error)
                            })
                    }
                }
            }
            Err(error) => serde_json::to_string(&json!({
                "id": null,
                "type": "response",
                "ok": false,
                "error": {
                    "code": "invalid_request",
                    "message": error.to_string()
                }
            }))
            .context("failed to encode invalid runtime request response")?,
        };
        io::stdout()
            .write_all(response.as_bytes())
            .context("failed to write runtime response")?;
        io::stdout()
            .write_all(b"\n")
            .context("failed to write runtime response newline")?;
        io::stdout()
            .flush()
            .context("failed to flush runtime response")?;
    }
    Ok(())
}

fn decode_request(payload: &str) -> Result<EngineRequest> {
    serde_json::from_str(payload)
        .or_else(|_| serde_json::from_str::<ParseRequest>(payload).map(EngineRequest::Parse))
        .context("failed to decode engine request JSON")
}

fn decode_runtime_request(payload: &str) -> Result<EngineRuntimeRequest> {
    serde_json::from_str(payload).context("failed to decode runtime request JSON")
}

fn is_uuid_v7(value: Uuid) -> bool {
    value.get_version() == Some(Version::SortRand)
}

fn encode_runtime_parse_response(
    request_id: Uuid,
    request: ParseRequest,
    raw_payload: &str,
    config_store: &EngineConfigStore,
) -> Result<String> {
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = Some(request_id);
    });
    let response = encode_parse_response(request, raw_payload, config_store)
        .and_then(|response| wrap_runtime_response(request_id, &response));
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = None;
    });
    response
}

fn encode_parse_response(
    request: ParseRequest,
    raw_payload: &str,
    config_store: &EngineConfigStore,
) -> Result<String> {
    maybe_write_debug(&request.options.debug_request_path, raw_payload)?;
    let debug_result_path = request.options.debug_result_path.clone();
    let response = handle_parse(request, config_store)
        .map(|data| serde_json::to_string(&EngineSuccess::new(EngineCommand::Parse, data)))
        .unwrap_or_else(|error| {
            let _ = emit_event(&ParseEvent::Failed {
                message: error.to_string(),
            });
            serde_json::to_string(&engine_failure(EngineCommand::Parse, &error))
        })
        .context("failed to encode parse response")?;
    maybe_write_debug(&debug_result_path, &response)?;
    Ok(response)
}

fn wrap_runtime_response(request_id: Uuid, response: &str) -> Result<String> {
    if let Ok(success) = serde_json::from_str::<EngineSuccess<Value>>(response) {
        return serde_json::to_string(&EngineRuntimeResponse::new(request_id, success))
            .context("failed to encode runtime response");
    }

    let failure = serde_json::from_str::<EngineFailure>(response)
        .context("failed to decode engine response for runtime envelope")?;
    serde_json::to_string(&EngineRuntimeFailure::new(request_id, failure))
        .context("failed to encode runtime failure response")
}

fn encode_success_response(
    request: EngineRequest,
    config_store: &EngineConfigStore,
) -> Result<String> {
    let response = match request {
        EngineRequest::Parse(_) => {
            unreachable!("parse requests are handled by encode_parse_response")
        }
        EngineRequest::CompileProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::CompileProject,
            handle_compile_project(request)?,
        )),
        EngineRequest::LoadProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::LoadProject,
            handle_load_project(request)?,
        )),
        EngineRequest::ApplyCorrection(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ApplyCorrection,
            handle_apply_correction(request)?,
        )),
        EngineRequest::AnswerProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::AnswerProject,
            handle_answer_project(request)?,
        )),
        EngineRequest::SearchBrain(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::SearchBrain,
            handle_search_brain(request)?,
        )),
        EngineRequest::ReadSource(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadSource,
            handle_read_source(request)?,
        )),
        EngineRequest::ReadWikiPage(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadWikiPage,
            handle_read_wiki_page(request)?,
        )),
        EngineRequest::ReadNode(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadNode,
            handle_read_node(request)?,
        )),
        EngineRequest::ReadRecentEvents(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadRecentEvents,
            handle_read_recent_events(request)?,
        )),
        EngineRequest::ReadGraphHistory(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadGraphHistory,
            handle_read_graph_history(request)?,
        )),
        EngineRequest::ReadGraphSnapshot(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadGraphSnapshot,
            handle_read_graph_snapshot(request)?,
        )),
        EngineRequest::ReconstructBrain(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReconstructBrain,
            handle_reconstruct_brain(request)?,
        )),
        EngineRequest::GetContextPack(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::GetContextPack,
            handle_get_context_pack(request)?,
        )),
        EngineRequest::ProposeBrainUpdate(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ProposeBrainUpdate,
            handle_propose_brain_update(request)?,
        )),
        EngineRequest::ListBrainReviewItems(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ListBrainReviewItems,
            handle_list_brain_review_items(request)?,
        )),
        EngineRequest::ResolveBrainReviewItem(request) => {
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::ResolveBrainReviewItem,
                handle_resolve_brain_review_item(request)?,
            ))
        }
        EngineRequest::GetBrainHealth(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::GetBrainHealth,
            handle_get_brain_health(request)?,
        )),
        EngineRequest::LoadConfig(LoadConfigRequest {}) => {
            let config = config_store.load()?;
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::LoadConfig,
                config.to_payload(),
            ))
        }
        EngineRequest::SaveConfig(SaveConfigRequest { config }) => {
            let config = EngineConfig::from_payload(config);
            config_store.save(&config)?;
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::SaveConfig,
                SaveConfigResponseData {
                    config: config.to_payload(),
                    persisted: true,
                },
            ))
        }
        EngineRequest::ValidateProvider(ValidateProviderRequest { config }) => {
            let config = config
                .map(EngineConfig::from_payload)
                .unwrap_or(config_store.load()?);
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::ValidateProvider,
                validate_provider(&config),
            ))
        }
        EngineRequest::ListProviderModels(_) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ListProviderModels,
            provider_model_catalog(),
        )),
        EngineRequest::CheckReadiness(_) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::CheckReadiness,
            check_readiness(config_store),
        )),
    }
    .context("failed to encode engine response")?;
    Ok(response)
}

fn encode_failure_response(command: EngineCommand, error: &anyhow::Error) -> String {
    serde_json::to_string(&engine_failure(command, error)).unwrap_or_else(|_| {
        "{\"ok\":false,\"command\":\"validate_provider\",\"error\":{\"code\":\"runtime_error\",\"message\":\"failed to encode engine failure\",\"details\":null}}".to_string()
    })
}

fn encode_runtime_failure_response(
    request_id: Uuid,
    command: EngineCommand,
    error: &anyhow::Error,
) -> String {
    serde_json::to_string(&EngineRuntimeFailure::new(
        request_id,
        engine_failure(command, error),
    ))
    .unwrap_or_else(|_| encode_failure_response(command, error))
}

fn request_command(request: &EngineRequest) -> EngineCommand {
    match request {
        EngineRequest::Parse(_) => EngineCommand::Parse,
        EngineRequest::CompileProject(_) => EngineCommand::CompileProject,
        EngineRequest::LoadProject(_) => EngineCommand::LoadProject,
        EngineRequest::ApplyCorrection(_) => EngineCommand::ApplyCorrection,
        EngineRequest::AnswerProject(_) => EngineCommand::AnswerProject,
        EngineRequest::SearchBrain(_) => EngineCommand::SearchBrain,
        EngineRequest::ReadSource(_) => EngineCommand::ReadSource,
        EngineRequest::ReadWikiPage(_) => EngineCommand::ReadWikiPage,
        EngineRequest::ReadNode(_) => EngineCommand::ReadNode,
        EngineRequest::ReadRecentEvents(_) => EngineCommand::ReadRecentEvents,
        EngineRequest::ReadGraphHistory(_) => EngineCommand::ReadGraphHistory,
        EngineRequest::ReadGraphSnapshot(_) => EngineCommand::ReadGraphSnapshot,
        EngineRequest::GetContextPack(_) => EngineCommand::GetContextPack,
        EngineRequest::ProposeBrainUpdate(_) => EngineCommand::ProposeBrainUpdate,
        EngineRequest::ListBrainReviewItems(_) => EngineCommand::ListBrainReviewItems,
        EngineRequest::ResolveBrainReviewItem(_) => EngineCommand::ResolveBrainReviewItem,
        EngineRequest::GetBrainHealth(_) => EngineCommand::GetBrainHealth,
        EngineRequest::ReconstructBrain(_) => EngineCommand::ReconstructBrain,
        EngineRequest::LoadConfig(_) => EngineCommand::LoadConfig,
        EngineRequest::SaveConfig(_) => EngineCommand::SaveConfig,
        EngineRequest::ValidateProvider(_) => EngineCommand::ValidateProvider,
        EngineRequest::ListProviderModels(_) => EngineCommand::ListProviderModels,
        EngineRequest::CheckReadiness(_) => EngineCommand::CheckReadiness,
    }
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

fn emit_event(event: &ParseEvent) -> Result<()> {
    if let Some(request_id) = RUNTIME_EVENT_REQUEST_ID.with(|current| current.borrow().clone()) {
        let line = serde_json::to_string(&EngineRuntimeEvent::new(request_id, event.clone()))
            .context("failed to encode runtime parse event")?;
        let mut stderr = io::stderr().lock();
        stderr
            .write_all(line.as_bytes())
            .context("failed to write runtime parse event")?;
        stderr
            .write_all(b"\n")
            .context("failed to write runtime parse event newline")?;
        stderr
            .flush()
            .context("failed to flush runtime parse event")?;
        return Ok(());
    }

    let line = serde_json::to_string(event).context("failed to encode parse event")?;
    eprintln!("{line}");
    Ok(())
}

fn handle_parse(
    request: ParseRequest,
    config_store: &EngineConfigStore,
) -> Result<ParseResponseData> {
    let started = Instant::now();
    let config = config_store.load()?;

    emit_event(&ParseEvent::Queued)?;
    emit_event(&ParseEvent::DocumentOpened {
        format: request.input.format.clone(),
    })?;

    let parse = parse_document(&request.input, &request.template, &request.options, &config)?;
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

    emit_event(&ParseEvent::Packaging)?;
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

    let source_manifest = export_output_package(&request, &result)?;
    let saved_output_path = source_manifest
        .as_ref()
        .map(|manifest| manifest.markdown_path.clone());
    emit_event(&ParseEvent::Completed)?;
    Ok(ParseResponseData {
        result,
        saved_output_path,
        source_manifest,
    })
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
    if let Some(manifest) = &source_manifest {
        let workspace_root = resolve_brain_workspace_root(&BrainReadScope {
            workspace_id: workspace_id.clone(),
            root_dir: None,
        })?;
        let chunks = chunk_source_markdown(manifest, &markdown);
        upsert_source_chunks(&workspace_root, manifest, &chunks)?;
        let snapshot = read_materialized_brain_snapshot(&workspace_root, &workspace_id)
            .unwrap_or_else(|_| empty_replayed_brain_snapshot(&workspace_id));
        let context = build_import_evidence_context(
            &workspace_root,
            manifest,
            &markdown,
            &snapshot,
            &chunks,
        )?;
        let report = maybe_generate_provider_graph_proposals(
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
    }
    Ok(CompileProjectResponseData {
        project_id: project.summary.project_id,
        workspace_id,
        source_id,
        graph_generation_status,
        graph_generation_skipped_reason,
        graph_generation_error_message,
    })
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

        let project = store.load_project(Some(project_id))?;
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
        project = store.load_project(None)?;
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
    let project = load_answerable_project(&store, &request.project_id)?;
    let answer = answer_project(&project, &request)?;
    Ok(AnswerProjectResponseData { answer })
}

fn handle_search_brain(request: SearchBrainRequest) -> Result<SearchBrainResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    Ok(SearchBrainResponseData {
        results: reader.search(&request.query, request.limit.unwrap_or(10)),
    })
}

fn handle_read_source(request: ReadSourceRequest) -> Result<ReadSourceResponseData> {
    let reader = BrainReader::open(&request.scope)?;
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
    Ok(ReadSourceResponseData {
        source,
        wiki_page,
        evidence,
    })
}

fn handle_read_wiki_page(request: ReadWikiPageRequest) -> Result<ReadWikiPageResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let page = reader.read_wiki_page(&request.path)?;
    Ok(ReadWikiPageResponseData { page })
}

fn handle_read_node(request: ReadNodeRequest) -> Result<ReadNodeResponseData> {
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

fn handle_read_graph_history(
    request: ReadGraphHistoryRequest,
) -> Result<ReadGraphHistoryResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let mut states = reader
        .events
        .iter()
        .filter(|event| {
            event.workspace_id == request.scope.workspace_id
                && is_completed_graph_materialized_event(event)
        })
        .cloned()
        .map(|event| graph_history_entry_from_event(reader.root(), event))
        .collect::<Result<Vec<_>>>()?;
    states.sort_by(|left, right| {
        right
            .materialized_at
            .cmp(&left.materialized_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    if let Some(limit) = request.limit {
        states.truncate(limit);
    }
    Ok(ReadGraphHistoryResponseData { states })
}

fn handle_read_graph_snapshot(
    request: ReadGraphSnapshotRequest,
) -> Result<ReadGraphSnapshotResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    let marker = read_latest_readable_graph_snapshot_marker(reader.root())?;
    let marker_event = marker.as_ref().and_then(|marker| {
        (marker.workspace_id == request.scope.workspace_id).then(|| {
            reader.events.iter().find(|event| {
                event.workspace_id == request.scope.workspace_id
                    && is_completed_graph_materialized_event(event)
                    && event.event_id == marker.event_id
            })
        })?
    });
    let latest = marker_event
        .or_else(|| latest_graph_materialized_event(&reader.events, &request.scope.workspace_id));
    let materialized_at = latest
        .and_then(|event| event.causality.materialized_version)
        .unwrap_or(reader.snapshot.generated_at);
    let created_at = latest
        .map(|event| event.created_at)
        .unwrap_or(reader.snapshot.generated_at);
    let snapshot_id = latest
        .and_then(|event| event.causality.snapshot_id.clone())
        .unwrap_or_else(|| {
            format!(
                "snapshot-{}-{}",
                reader.snapshot.workspace_id, materialized_at
            )
        });
    let source_ingest_id = latest
        .map(graph_snapshot_source_ingest_id)
        .unwrap_or_else(|| format!("materialized://{}", reader.snapshot.workspace_id));
    let materialized_paths = marker
        .as_ref()
        .filter(|_| marker_event.is_some())
        .map(|marker| marker.materialized_files.clone())
        .unwrap_or_else(|| latest_readable_materialized_file_refs(&reader.snapshot));

    Ok(ReadGraphSnapshotResponseData {
        snapshot_id,
        source_ingest_id,
        workspace_id: reader.snapshot.workspace_id.clone(),
        source_of_truth_path: "events/brain_events.jsonl".into(),
        latest_readable_snapshot_path: LATEST_READABLE_SNAPSHOT_PATH.into(),
        created_at,
        materialized_at,
        materialized_paths,
        source_paths: graph_snapshot_source_paths(&reader.snapshot),
        nodes: reader.snapshot.nodes.clone(),
        edges: reader.snapshot.relations.clone(),
        claims: reader.snapshot.claims.clone(),
        memory_refs: reader
            .snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        wiki_pages: reader.read_all_wiki_pages()?,
    })
}

fn graph_snapshot_source_ingest_id(event: &BrainEvent) -> String {
    event
        .source_refs
        .first()
        .cloned()
        .or_else(|| event.causality.caused_by_source_ids.first().cloned())
        .unwrap_or_else(|| event.event_id.clone())
}

fn latest_graph_materialized_event<'a>(
    events: &'a [BrainEvent],
    workspace_id: &str,
) -> Option<&'a BrainEvent> {
    events
        .iter()
        .filter(|event| {
            event.workspace_id == workspace_id && is_completed_graph_materialized_event(event)
        })
        .max_by(|left, right| {
            left.causality
                .materialized_version
                .unwrap_or(left.created_at)
                .cmp(
                    &right
                        .causality
                        .materialized_version
                        .unwrap_or(right.created_at),
                )
                .then_with(|| left.event_id.cmp(&right.event_id))
        })
}

fn is_completed_graph_materialized_event(event: &BrainEvent) -> bool {
    event.event_type == BrainEventKind::GraphMaterialized
        && event.causality.materialized_version.is_some()
        && !matches!(
            event.policy_result.as_str(),
            "failed" | "stale" | "in_progress" | "ingest_in_progress"
        )
}

fn graph_snapshot_source_paths(snapshot: &BrainRepoSnapshot) -> Vec<String> {
    snapshot
        .sources
        .iter()
        .flat_map(|source| [source.source_path.clone(), source.markdown_path.clone()])
        .filter(|path| !path.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn graph_history_entry_from_event(root: &Path, event: BrainEvent) -> Result<GraphHistoryEntry> {
    let snapshot_id = event
        .causality
        .snapshot_id
        .clone()
        .unwrap_or_else(|| format!("snapshot-{}-{}", event.workspace_id, event.created_at));
    let materialized_at = event
        .causality
        .materialized_version
        .unwrap_or(event.created_at);
    let payload = serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json).ok();
    let fallback_payload =
        serde_json::from_str::<Value>(&event.payload_json).unwrap_or(Value::Null);
    let graph = payload.and_then(|payload| payload.materialized_graph);

    Ok(GraphHistoryEntry {
        snapshot_id: snapshot_id.clone(),
        materialized_at,
        event_id: event.event_id.clone(),
        rollback_target: graph_rollback_target(&snapshot_id, &event.event_id, materialized_at),
        operation_type: event.operation_type.clone(),
        source_run_ids: graph_history_source_run_ids(&event),
        source_markdown_refs: event.source_markdown_refs.clone(),
        storage_locations: graph_history_storage_locations(
            root,
            &snapshot_id,
            &event.event_id,
            materialized_at,
        ),
        node_count: graph
            .as_ref()
            .map(|graph| graph.nodes.len())
            .or_else(|| json_usize(&fallback_payload, "nodeCount"))
            .unwrap_or(event.node_refs.len()),
        edge_count: graph
            .as_ref()
            .map(|graph| graph.relations.len())
            .or_else(|| json_usize(&fallback_payload, "relationCount"))
            .unwrap_or(event.relation_refs.len()),
        claim_count: graph
            .as_ref()
            .map(|graph| graph.claims.len())
            .or_else(|| json_usize(&fallback_payload, "claimCount"))
            .unwrap_or(event.claim_refs.len()),
        memory_count: graph
            .as_ref()
            .map(|graph| graph.memories.len())
            .or_else(|| json_usize(&fallback_payload, "memoryCount"))
            .unwrap_or(event.memory_refs.len()),
        wiki_page_count: graph
            .as_ref()
            .map(|graph| graph.wiki_pages.len())
            .or_else(|| json_usize(&fallback_payload, "wikiPageCount"))
            .unwrap_or(0),
    })
}

fn graph_rollback_target(
    snapshot_id: &str,
    event_id: &str,
    materialized_version: u64,
) -> GraphRollbackTarget {
    GraphRollbackTarget {
        snapshot_id: snapshot_id.to_string(),
        event_id: event_id.to_string(),
        materialized_version,
        replay_selector: format!("--event {event_id}"),
    }
}

fn graph_history_source_run_ids(event: &BrainEvent) -> Vec<String> {
    let mut ids = BTreeSet::new();
    ids.extend(event.source_refs.iter().cloned());
    ids.extend(event.causality.caused_by_source_ids.iter().cloned());
    ids.extend(event.causality.caused_by_event_ids.iter().cloned());
    ids.into_iter().collect()
}

fn graph_history_storage_locations(
    root: &Path,
    snapshot_id: &str,
    event_id: &str,
    materialized_at: u64,
) -> Vec<String> {
    let mut locations = vec![
        format!("events/brain_events.jsonl#{event_id}"),
        format!("replay://up_to_event_id={event_id}"),
        format!("replay://up_to_materialized_version={materialized_at}"),
    ];
    let snapshot_files = root.join("snapshots").join(snapshot_id).join("files");
    if snapshot_files.exists() {
        locations.push(format!("snapshots/{snapshot_id}/files"));
    }
    locations.extend([
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "memory/records.json".to_string(),
        "wiki/index.md".to_string(),
    ]);
    locations
}

fn json_usize(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn event_matches_recent_events_request(
    event: &BrainEvent,
    request: &ReadRecentEventsRequest,
) -> bool {
    if let Some(run_id) = request.run_id.as_deref() {
        if !event_matches_run_id(event, run_id) {
            return false;
        }
    }
    if let Some(source_ref) = request.source_ref.as_deref() {
        if !event.source_refs.iter().any(|value| value == source_ref)
            && !event
                .source_markdown_refs
                .iter()
                .any(|value| value == source_ref)
            && !event
                .causality
                .caused_by_source_ids
                .iter()
                .any(|value| value == source_ref)
        {
            return false;
        }
    }
    if let Some(node_id) = request.node_id.as_deref() {
        if !event.node_refs.iter().any(|value| value == node_id)
            && !event.target_node_ids.iter().any(|value| value == node_id)
        {
            return false;
        }
    }
    if let Some(edge_id) = request.edge_id.as_deref() {
        if !event.relation_refs.iter().any(|value| value == edge_id)
            && !event.target_edge_ids.iter().any(|value| value == edge_id)
        {
            return false;
        }
    }
    if let Some(claim_id) = request.claim_id.as_deref() {
        if !event.claim_refs.iter().any(|value| value == claim_id)
            && !event.target_claim_ids.iter().any(|value| value == claim_id)
        {
            return false;
        }
    }
    if let Some(memory_id) = request.memory_id.as_deref() {
        if !event.memory_refs.iter().any(|value| value == memory_id)
            && !event
                .target_memory_ids
                .iter()
                .any(|value| value == memory_id)
        {
            return false;
        }
    }
    if let Some(change_type) = request.change_type.as_deref() {
        if !event_matches_change_type(event, change_type) {
            return false;
        }
    }
    true
}

fn event_matches_run_id(event: &BrainEvent, run_id: &str) -> bool {
    graph_history_source_run_ids(event)
        .iter()
        .any(|value| value == run_id)
        || event_payload_string(event, "runId").as_deref() == Some(run_id)
}

fn event_matches_change_type(event: &BrainEvent, change_type: &str) -> bool {
    event.operation_type.as_deref() == Some(change_type)
        || serialized_event_type(event).as_deref() == Some(change_type)
        || event_payload_string(event, "changeType").as_deref() == Some(change_type)
        || event_payload_string(event, "operationType").as_deref() == Some(change_type)
}

fn serialized_event_type(event: &BrainEvent) -> Option<String> {
    serde_json::to_value(event.event_type)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn event_payload_string(event: &BrainEvent, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(&event.payload_json)
        .ok()
        .and_then(|value| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn handle_get_context_pack(request: GetContextPackRequest) -> Result<GetContextPackResponseData> {
    let reader = BrainReader::open(&request.scope)?;
    Ok(GetContextPackResponseData {
        context_pack: reader.context_pack(&request.query, request.budget.unwrap_or(8000))?,
    })
}

fn handle_propose_brain_update(
    request: ProposeBrainUpdateRequest,
) -> Result<ProposeBrainUpdateResponseData> {
    validate_brain_update_proposal(&request)?;
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root)?;

    let created_at = unix_timestamp_seconds();
    let proposal_id = format!("proposal-{}", Uuid::now_v7());
    let mut source_refs = request.source_refs.clone();
    if let Some(source_id) = &request.target_source_id {
        if !source_refs.iter().any(|value| value == source_id) {
            source_refs.push(source_id.clone());
        }
    }
    let mut node_refs = request.node_refs.clone();
    if let Some(node_id) = &request.target_node_id {
        if !node_refs.iter().any(|value| value == node_id) {
            node_refs.push(node_id.clone());
        }
    }
    let mut target_node_id = request.target_node_id.clone();
    let mut relation_kind = request.relation_kind;
    let mut evidence_refs = request.evidence_refs.clone();
    if let Some(payload) = &request.proposal_payload {
        match payload {
            AgentGraphProposalPayload::NewNode { node } => {
                for source_ref in &node.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &node.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
            AgentGraphProposalPayload::NewEdge { edge } => {
                let source_node_id = edge.source_node_id.trim();
                if !source_node_id.is_empty() {
                    node_refs.retain(|node_id| node_id != source_node_id);
                    node_refs.insert(0, source_node_id.to_string());
                }
                merge_unique_string(&mut node_refs, edge.target_node_id.trim());
                for source_ref in &edge.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &edge.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
                if target_node_id.is_none() {
                    target_node_id = Some(edge.target_node_id.trim().to_string());
                }
                if relation_kind.is_none() {
                    relation_kind = Some(edge.kind);
                }
            }
            AgentGraphProposalPayload::NewClaim { claim } => {
                for topic_ref in &claim.topic_refs {
                    merge_unique_string(&mut node_refs, topic_ref);
                }
                for source_ref in &claim.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &claim.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
            AgentGraphProposalPayload::NewMemory { memory } => {
                for source_ref in &memory.source_refs {
                    merge_unique_string(&mut source_refs, source_ref);
                }
                for evidence_ref in &memory.evidence_refs {
                    merge_unique_string(&mut evidence_refs, evidence_ref);
                }
            }
        }
    }

    let auto_apply = should_auto_apply_brain_proposal(&request);
    let proposal = BrainUpdateProposal {
        proposal_id: proposal_id.clone(),
        workspace_id: request.scope.workspace_id.clone(),
        kind: request.kind,
        status: if auto_apply {
            BrainProposalStatus::Accepted
        } else {
            BrainProposalStatus::PendingReview
        },
        actor: request.actor.clone(),
        scope: BrainScope::Project,
        title: request.title.trim().to_string(),
        body: request.body.trim().to_string(),
        target_node_id,
        target_source_id: request.target_source_id.clone(),
        relation_kind,
        source_refs,
        node_refs,
        evidence_refs,
        proposal_payload: request.proposal_payload.clone(),
        created_at,
    };
    let mut event = brain_event_for_proposal(&proposal)?;
    if auto_apply {
        event.policy_result = "auto_applied".into();
    }
    let proposal_path = writer.write_proposal(&proposal)?;
    writer.append_event(&event)?;
    if auto_apply {
        if proposal.kind == BrainProposalKind::SourceNote {
            writer.apply_source_note_metadata(&proposal, &request)?;
        } else if matches!(
            proposal.kind,
            BrainProposalKind::Node
                | BrainProposalKind::Claim
                | BrainProposalKind::Link
                | BrainProposalKind::Memory
        ) {
            if proposal.kind == BrainProposalKind::Memory {
                let memory = memory_record_for_proposal(&proposal);
                writer.upsert_memory_record(memory)?;
                let accepted_event = brain_memory_accepted_event(&proposal)?;
                writer.append_event(&accepted_event)?;
            } else {
                writer.apply_accepted_proposal(&proposal)?;
            }
            writer.append_event(&brain_graph_mutation_applied_event(&proposal)?)?;
        } else {
            let memory = memory_record_for_proposal(&proposal);
            writer.upsert_memory_record(memory)?;
            let accepted_event = brain_memory_accepted_event(&proposal)?;
            writer.append_event(&accepted_event)?;
        }
    }

    Ok(ProposeBrainUpdateResponseData {
        proposal,
        event,
        proposal_path: proposal_path.display().to_string(),
    })
}

fn handle_list_brain_review_items(
    request: ListBrainReviewItemsRequest,
) -> Result<ListBrainReviewItemsResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    Ok(ListBrainReviewItemsResponseData {
        items: list_pending_brain_review_items(&root, &request.scope.workspace_id)?,
    })
}

fn handle_get_brain_health(request: GetBrainHealthRequest) -> Result<GetBrainHealthResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    if !root.join("brain-manifest.json").exists() {
        return Ok(GetBrainHealthResponseData {
            status: BrainHealthStatus::Clean,
            attention_count: 0,
            review_items: Vec::new(),
            recent_events: Vec::new(),
        });
    }
    run_brain_maintenance(&request.scope)?;
    let review_items = list_pending_brain_review_items(&root, &request.scope.workspace_id)?;
    let attention_count = review_items.len();
    let mut recent_events = read_brain_events_jsonl(&root.join("events/brain_events.jsonl"))?;
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
        review_items,
        recent_events,
    })
}

fn handle_resolve_brain_review_item(
    request: ResolveBrainReviewItemRequest,
) -> Result<ResolveBrainReviewItemResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root)?;
    let proposal_path = writer.proposal_path(&request.proposal_id);
    let mut proposal: BrainUpdateProposal = read_json_artifact(&proposal_path)
        .with_context(|| format!("failed reading review proposal {}", request.proposal_id))?;
    if proposal.workspace_id != request.scope.workspace_id {
        bail!(
            "proposal workspace_id {} does not match requested workspace {}",
            proposal.workspace_id,
            request.scope.workspace_id
        );
    }
    if !is_reviewable_brain_proposal(&proposal) {
        bail!(
            "proposal {} is not a pending claim or link review item",
            proposal.proposal_id
        );
    }
    proposal.status = match request.decision {
        BrainReviewDecision::Accept => BrainProposalStatus::Accepted,
        BrainReviewDecision::Reject => BrainProposalStatus::Rejected,
    };
    if request.decision == BrainReviewDecision::Accept {
        writer.apply_accepted_proposal(&proposal)?;
    }
    let proposal_path = writer.write_proposal(&proposal)?;
    let event = brain_review_resolved_event(&proposal, &request)?;
    writer.append_event(&event)?;
    Ok(ResolveBrainReviewItemResponseData {
        proposal,
        event,
        proposal_path: proposal_path.display().to_string(),
    })
}

fn list_pending_brain_review_items(
    root: &Path,
    workspace_id: &str,
) -> Result<Vec<BrainReviewItem>> {
    let mut items = read_brain_update_proposals(root)?
        .into_iter()
        .filter(|(proposal, _)| proposal.workspace_id == workspace_id)
        .filter(|(proposal, _)| is_reviewable_brain_proposal(proposal))
        .map(|(proposal, path)| brain_review_item_for_proposal(&proposal, &path))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.proposal_id.cmp(&right.proposal_id))
    });
    Ok(items)
}

fn read_brain_update_proposals(root: &Path) -> Result<Vec<(BrainUpdateProposal, PathBuf)>> {
    let proposals_dir = root.join("reviews/proposed-updates");
    if !proposals_dir.exists() {
        return Ok(Vec::new());
    }
    let mut proposals = Vec::new();
    for entry in fs::read_dir(&proposals_dir)
        .with_context(|| format!("failed reading {}", proposals_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed reading {}", proposals_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        proposals.push((read_json_artifact(&path)?, path));
    }
    Ok(proposals)
}

fn is_reviewable_brain_proposal(proposal: &BrainUpdateProposal) -> bool {
    proposal.status == BrainProposalStatus::PendingReview
        && matches!(
            proposal.kind,
            BrainProposalKind::Node
                | BrainProposalKind::Claim
                | BrainProposalKind::Link
                | BrainProposalKind::WikiPage
        )
}

fn brain_review_item_for_proposal(proposal: &BrainUpdateProposal, path: &Path) -> BrainReviewItem {
    BrainReviewItem {
        review_id: proposal.proposal_id.clone(),
        proposal_id: proposal.proposal_id.clone(),
        workspace_id: proposal.workspace_id.clone(),
        kind: proposal.kind,
        status: proposal.status,
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        proposal_path: path.display().to_string(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        created_at: proposal.created_at,
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
    new_review_count: usize,
    new_markdown_source_count: usize,
    enqueued_markdown_source_count: usize,
    ingest_worker_started: bool,
    ingested_markdown_source_count: usize,
    failed_markdown_source_count: usize,
    applied_agent_proposal_count: usize,
    failed_agent_proposal_count: usize,
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
    applied_agent_proposals: Vec<String>,
    #[serde(default)]
    failed_agent_proposals: Vec<AgentProposalFailureReport>,
    #[serde(default)]
    issues: Vec<BrainLintIssue>,
}

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
    let proposal_apply_result = run_queued_agent_proposal_apply_worker(&root, &scope.workspace_id)?;
    let mut snapshot = if worker_result.processed > 0
        || worker_result.failed > 0
        || !proposal_apply_result.applied.is_empty()
    {
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
    report.applied_agent_proposal_count = proposal_apply_result.applied.len();
    report.failed_agent_proposal_count = proposal_apply_result.failed.len();
    report.applied_agent_proposals = proposal_apply_result.applied;
    report.failed_agent_proposals = proposal_apply_result.failed;
    report.repair_count +=
        repair_missing_materialized_wiki_stubs(&root, &mut snapshot, &mut report.repairs)?;
    report
        .issues
        .extend(lint_missing_materialized_wiki_refs(&root, &snapshot));
    report.repair_count += repair_generated_brain_artifacts(&root, &snapshot, &mut report.repairs)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    report.new_review_count += write_lint_review_items(&writer, &snapshot, &report.issues)?;
    report.issue_count = report.issues.len();
    write_json_pretty(&root.join("reviews/lint-reports/latest.json"), &report)?;
    if report.repair_count > 0
        || report.new_review_count > 0
        || report.new_markdown_source_count > 0
        || report.enqueued_markdown_source_count > 0
        || report.ingest_worker_started
        || report.applied_agent_proposal_count > 0
        || report.failed_agent_proposal_count > 0
    {
        writer.append_event(&brain_maintenance_event(&snapshot, &report)?)?;
    }
    Ok(report)
}

fn read_materialized_brain_snapshot(root: &Path, workspace_id: &str) -> Result<BrainRepoSnapshot> {
    let manifest_path = root.join("brain-manifest.json");
    if !manifest_path.exists() {
        return Ok(empty_replayed_brain_snapshot(workspace_id));
    }
    let mut snapshot: BrainRepoSnapshot = read_json_artifact(&manifest_path)?;
    if snapshot.workspace_id != workspace_id {
        bail!(
            "brain manifest workspace_id {} does not match requested workspace {}",
            snapshot.workspace_id,
            workspace_id
        );
    }
    if root.join("graph/nodes.json").exists() {
        snapshot.nodes = read_json_artifact(&root.join("graph/nodes.json"))?;
    }
    if root.join("graph/edges.json").exists() {
        snapshot.relations = read_json_artifact(&root.join("graph/edges.json"))?;
    }
    if root.join("graph/evidence.json").exists() {
        snapshot.evidence = read_json_artifact(&root.join("graph/evidence.json"))?;
    }
    if root.join("graph/entities.json").exists() {
        snapshot.entities = read_json_artifact(&root.join("graph/entities.json"))?;
    }
    if root.join("graph/claims.json").exists() {
        snapshot.claims = read_json_artifact(&root.join("graph/claims.json"))?;
    }
    if let Ok(extractions) = read_structured_extraction_artifacts(root, &snapshot.sources) {
        if !extractions.is_empty() {
            snapshot.extractions = extractions;
        }
    }
    snapshot.memories = read_memory_records(root)?;
    snapshot.events = read_brain_events_jsonl(&root.join("events/brain_events.jsonl"))?;
    Ok(snapshot)
}

fn handle_reconstruct_brain(
    request: ReconstructBrainRequest,
) -> Result<ReconstructBrainResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let events_path = root.join("events/brain_events.jsonl");
    let events = read_brain_events_jsonl(&events_path)
        .with_context(|| format!("failed reading replay events {}", events_path.display()))?;
    let replay = reconstruct_brain_snapshot_from_events(
        &request.scope.workspace_id,
        &events,
        request.up_to_timestamp,
        request.up_to_materialized_version,
        request.up_to_event_id.as_deref(),
    )?;
    let snapshot_id = format!(
        "snapshot-replay-{}-{}",
        request.scope.workspace_id,
        replay
            .selected_materialized_version
            .unwrap_or_else(unix_timestamp_seconds)
    );
    let output_root = request
        .output_root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("snapshots").join(&snapshot_id).join("files"));
    let before = capture_materialized_file_snapshot(&output_root).unwrap_or_default();
    persist_reconstructed_brain_snapshot(&output_root, &replay.snapshot)?;
    let mut changed_files = changed_materialized_files(
        &before,
        &capture_materialized_file_snapshot(&output_root).unwrap_or_default(),
    );

    if request.write_materialized {
        let writer = BrainWorkspaceWriter::open(root.clone())?;
        let before_current = capture_materialized_file_snapshot(writer.root())?;
        let rollback_at = unix_timestamp_seconds();
        let backup_snapshot_id = format!(
            "snapshot-pre-rollback-{}-{}",
            request.scope.workspace_id, rollback_at
        );
        let previous_snapshot =
            read_materialized_brain_snapshot(writer.root(), &request.scope.workspace_id).ok();
        persist_materialized_snapshot(writer.root(), &backup_snapshot_id, &before_current)?;
        let rollback_result = (|| -> Result<Vec<String>> {
            let mut restored_snapshot = replay.snapshot.clone();
            restored_snapshot.generated_at = rollback_at;
            let rollback_event = brain_graph_rollback_event(
                &restored_snapshot,
                &snapshot_id,
                &backup_snapshot_id,
                previous_snapshot.as_ref(),
                replay.selected_event_id.as_deref(),
                rollback_at,
            )?;
            restored_snapshot.events = events
                .iter()
                .cloned()
                .chain(std::iter::once(rollback_event))
                .collect();
            restore_selected_materialized_brain_snapshot(
                writer.root(),
                &restored_snapshot,
                previous_snapshot.as_ref(),
            )?;
            Ok(changed_materialized_files(
                &before_current,
                &capture_materialized_file_snapshot(writer.root())?,
            ))
        })();
        match rollback_result {
            Ok(current_changed_files) => changed_files = current_changed_files,
            Err(error) => {
                restore_materialized_file_snapshot(writer.root(), &before_current)?;
                return Err(error);
            }
        }
    }

    Ok(ReconstructBrainResponseData {
        snapshot: replay.snapshot,
        replayed_event_count: replay.replayed_event_count,
        selected_event_id: replay.selected_event_id,
        snapshot_id,
        output_root: output_root.display().to_string(),
        changed_files,
    })
}

#[derive(Debug, Clone)]
struct BrainReplayResult {
    snapshot: BrainRepoSnapshot,
    replayed_event_count: usize,
    selected_event_id: Option<String>,
    selected_materialized_version: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializedGraphEventPayload {
    #[serde(default)]
    materialized_graph: Option<MaterializedGraphPayload>,
    #[serde(default)]
    proposal: Option<BrainUpdateProposal>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializedGraphPayload {
    #[serde(default)]
    generated_at: Option<u64>,
    #[serde(default)]
    sources: Vec<SourceRecord>,
    #[serde(default)]
    nodes: Vec<BrainNodeRecord>,
    #[serde(default, rename = "edges")]
    relations: Vec<BrainRelationRecord>,
    #[serde(default)]
    evidence: Vec<EvidenceRef>,
    #[serde(default)]
    memories: Vec<MemoryRecord>,
    #[serde(default)]
    wiki_pages: Vec<WikiPage>,
    #[serde(default)]
    entities: Vec<EntityRecord>,
    #[serde(default)]
    claims: Vec<ClaimRecord>,
    #[serde(default)]
    extractions: Vec<StructuredExtractionArtifact>,
}

fn reconstruct_brain_snapshot_from_events(
    workspace_id: &str,
    events: &[BrainEvent],
    up_to_timestamp: Option<u64>,
    up_to_materialized_version: Option<u64>,
    up_to_event_id: Option<&str>,
) -> Result<BrainReplayResult> {
    let mut ordered = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.workspace_id == workspace_id)
        .map(|(index, event)| (index, event.clone()))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.1
            .causality
            .materialized_version
            .unwrap_or(left.1.created_at)
            .cmp(
                &right
                    .1
                    .causality
                    .materialized_version
                    .unwrap_or(right.1.created_at),
            )
            .then_with(|| left.1.created_at.cmp(&right.1.created_at))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut replay_state = BrainReplayState::new(workspace_id);
    let mut included = Vec::new();
    let mut selected_event_id = None;
    let mut selected_materialized_version = None;
    for (_, event) in ordered {
        if up_to_timestamp.is_some_and(|cutoff| event.created_at > cutoff) {
            break;
        }
        if up_to_materialized_version.is_some_and(|cutoff| {
            event
                .causality
                .materialized_version
                .unwrap_or(event.created_at)
                > cutoff
        }) {
            break;
        }
        selected_materialized_version = event
            .causality
            .materialized_version
            .or(Some(event.created_at));
        replay_state.apply_event(&event)?;
        selected_event_id = Some(event.event_id.clone());
        included.push(event.clone());
        if up_to_event_id.is_some_and(|target| target == event.event_id) {
            break;
        }
    }

    let mut snapshot = replay_state.into_snapshot();
    snapshot.events = included;
    if let Some(target_event_id) = up_to_event_id {
        if selected_event_id.as_deref() != Some(target_event_id) {
            bail!("replay target event `{target_event_id}` was not found in events/brain_events.jsonl");
        }
    }
    snapshot.generated_at = selected_materialized_version.unwrap_or(snapshot.generated_at);
    refresh_materialized_wiki_pages(&mut snapshot);
    Ok(BrainReplayResult {
        replayed_event_count: snapshot.events.len(),
        snapshot,
        selected_event_id,
        selected_materialized_version,
    })
}

#[derive(Debug, Clone)]
struct BrainReplayState {
    snapshot: BrainRepoSnapshot,
}

impl BrainReplayState {
    fn new(workspace_id: &str) -> Self {
        Self {
            snapshot: empty_replayed_brain_snapshot(workspace_id),
        }
    }

    fn apply_event(&mut self, event: &BrainEvent) -> Result<()> {
        apply_replayed_brain_event(event, &mut self.snapshot)
    }

    fn into_snapshot(self) -> BrainRepoSnapshot {
        self.snapshot
    }
}

fn empty_replayed_brain_snapshot(workspace_id: &str) -> BrainRepoSnapshot {
    BrainRepoSnapshot {
        workspace_id: workspace_id.to_string(),
        generated_at: 0,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    }
}

fn apply_replayed_brain_event(event: &BrainEvent, snapshot: &mut BrainRepoSnapshot) -> Result<()> {
    match event.event_type {
        BrainEventKind::GraphMaterialized => {
            let payload =
                serde_json::from_str::<MaterializedGraphEventPayload>(&event.payload_json)
                    .unwrap_or(MaterializedGraphEventPayload {
                        materialized_graph: None,
                        proposal: None,
                    });
            if let Some(materialized) = payload.materialized_graph {
                apply_materialized_graph_payload(snapshot, materialized, event);
            } else if let Some(mut proposal) = payload.proposal {
                proposal.status = BrainProposalStatus::Accepted;
                if proposal.kind == BrainProposalKind::Memory {
                    upsert_replayed_memory(snapshot, memory_record_for_proposal(&proposal));
                } else {
                    apply_accepted_proposal_to_snapshot(&proposal, snapshot)?;
                }
            }
        }
        BrainEventKind::MemoryAccepted => {
            if let Ok(proposal) = serde_json::from_str::<BrainUpdateProposal>(&event.payload_json) {
                upsert_replayed_memory(snapshot, memory_record_for_proposal(&proposal));
            } else if let Ok(memory) = serde_json::from_str::<MemoryRecord>(&event.payload_json) {
                upsert_replayed_memory(snapshot, memory);
            }
        }
        _ => {}
    }
    Ok(())
}

fn apply_materialized_graph_payload(
    snapshot: &mut BrainRepoSnapshot,
    payload: MaterializedGraphPayload,
    event: &BrainEvent,
) {
    snapshot.generated_at = payload
        .generated_at
        .or(event.causality.materialized_version)
        .unwrap_or(event.created_at);
    snapshot.sources = payload.sources;
    snapshot.nodes = payload.nodes;
    snapshot.relations = payload.relations;
    snapshot.evidence = payload.evidence;
    snapshot.memories = payload.memories;
    snapshot.wiki_pages = payload.wiki_pages;
    snapshot.entities = payload.entities;
    snapshot.claims = payload.claims;
    snapshot.extractions = payload.extractions;
}

fn upsert_replayed_memory(snapshot: &mut BrainRepoSnapshot, memory: MemoryRecord) {
    if let Some(existing) = snapshot
        .memories
        .iter_mut()
        .find(|existing| existing.memory_id == memory.memory_id)
    {
        merge_memory_record(existing, memory);
    } else {
        snapshot.memories.push(memory);
    }
    snapshot.memories.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
}

fn persist_reconstructed_brain_snapshot(root: &Path, snapshot: &BrainRepoSnapshot) -> Result<()> {
    ensure_materialized_brain_repo_dirs(root)?;
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    write_json_pretty(&root.join("memory/records.json"), &snapshot.memories)?;
    write_structured_extraction_artifacts(root, &snapshot.extractions)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_latest_readable_graph_snapshot_marker(root, snapshot)?;
    Ok(())
}

fn restore_selected_materialized_brain_snapshot(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    previous_snapshot: Option<&BrainRepoSnapshot>,
) -> Result<()> {
    ensure_materialized_brain_repo_dirs(root)?;
    if let Some(previous_snapshot) = previous_snapshot {
        let next_wiki_paths = snapshot
            .wiki_pages
            .iter()
            .map(|page| page.path.as_str())
            .collect::<BTreeSet<_>>();
        for page in &previous_snapshot.wiki_pages {
            if !next_wiki_paths.contains(page.path.as_str()) && is_wiki_markdown_ref(&page.path) {
                let path = root.join(&page.path);
                if path.exists() {
                    fs::remove_file(&path).with_context(|| {
                        format!("failed removing stale wiki page {}", path.display())
                    })?;
                }
            }
        }
    }
    persist_materialized_graph_and_wiki_state(root, snapshot)?;
    write_json_pretty(&root.join("memory/records.json"), &snapshot.memories)?;
    write_structured_extraction_artifacts(root, &snapshot.extractions)?;
    write_brain_events_jsonl(&root.join("events/brain_events.jsonl"), &snapshot.events)?;
    publish_latest_readable_graph_snapshot_marker(root, snapshot)?;
    Ok(())
}

fn brain_graph_rollback_event(
    snapshot: &BrainRepoSnapshot,
    snapshot_id: &str,
    pre_rollback_snapshot_id: &str,
    previous_snapshot: Option<&BrainRepoSnapshot>,
    selected_event_id: Option<&str>,
    rollback_at: u64,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_rollback".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-rollback".into(),
        },
        source_refs: snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: snapshot
            .sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
        node_refs: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        target_node_ids: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: snapshot
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        evidence_refs: snapshot
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: rollback_materialized_graph_event_payload_json(
            snapshot,
            snapshot_id,
            pre_rollback_snapshot_id,
            previous_snapshot,
            selected_event_id,
        )?,
        causality: BrainEventCausality {
            caused_by_event_ids: selected_event_id
                .map(|event_id| vec![event_id.to_string()])
                .unwrap_or_default(),
            caused_by_source_ids: snapshot
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(snapshot_id.to_string()),
            previous_snapshot_id: Some(pre_rollback_snapshot_id.to_string()),
            materialized_version: Some(rollback_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "rollback_applied".into(),
        created_at: rollback_at,
    })
}

fn rollback_materialized_graph_event_payload_json(
    snapshot: &BrainRepoSnapshot,
    restored_snapshot_id: &str,
    pre_rollback_snapshot_id: &str,
    previous_snapshot: Option<&BrainRepoSnapshot>,
    selected_event_id: Option<&str>,
) -> Result<String> {
    serde_json::to_string(&json!({
        "nodeCount": snapshot.nodes.len(),
        "relationCount": snapshot.relations.len(),
        "sourceCount": snapshot.sources.len(),
        "rollback": {
            "restoredSnapshotId": restored_snapshot_id,
            "preRollbackSnapshotId": pre_rollback_snapshot_id,
            "selectedEventId": selected_event_id,
            "replaySelector": selected_event_id
                .map(|event_id| format!("--event {event_id}"))
                .unwrap_or_else(|| "--latest".into()),
            "sourceEventCount": snapshot.events.len(),
            "sourceOfTruth": "events/brain_events.jsonl",
        },
        "diff": rollback_snapshot_diff(previous_snapshot, snapshot),
        "materializedGraph": {
            "generatedAt": snapshot.generated_at,
            "sources": snapshot.sources,
            "nodes": snapshot.nodes,
            "edges": snapshot.relations,
            "evidence": snapshot.evidence,
            "memories": snapshot.memories,
            "wikiPages": snapshot.wiki_pages,
            "entities": snapshot.entities,
            "claims": snapshot.claims,
            "extractions": snapshot.extractions,
        }
    }))
    .context("failed to encode rollback materialized graph event payload")
}

fn rollback_snapshot_diff(
    previous_snapshot: Option<&BrainRepoSnapshot>,
    snapshot: &BrainRepoSnapshot,
) -> Value {
    let previous_node_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .nodes
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_edge_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .relations
                .iter()
                .map(|relation| relation.relation_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_edge_ids = snapshot
        .relations
        .iter()
        .map(|relation| relation.relation_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_claim_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .claims
                .iter()
                .map(|claim| claim.claim_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_claim_ids = snapshot
        .claims
        .iter()
        .map(|claim| claim.claim_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_memory_ids = previous_snapshot
        .map(|snapshot| {
            snapshot
                .memories
                .iter()
                .map(|memory| memory.memory_id.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_memory_ids = snapshot
        .memories
        .iter()
        .map(|memory| memory.memory_id.as_str())
        .collect::<BTreeSet<_>>();
    let previous_wiki_paths = previous_snapshot
        .map(|snapshot| {
            snapshot
                .wiki_pages
                .iter()
                .map(|page| page.path.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let next_wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();

    json!({
        "nodeCountBefore": previous_node_ids.len(),
        "nodeCountAfter": next_node_ids.len(),
        "edgeCountBefore": previous_edge_ids.len(),
        "edgeCountAfter": next_edge_ids.len(),
        "claimCountBefore": previous_claim_ids.len(),
        "claimCountAfter": next_claim_ids.len(),
        "memoryCountBefore": previous_memory_ids.len(),
        "memoryCountAfter": next_memory_ids.len(),
        "wikiPageCountBefore": previous_wiki_paths.len(),
        "wikiPageCountAfter": next_wiki_paths.len(),
        "addedNodeIds": sorted_set_difference(&next_node_ids, &previous_node_ids),
        "removedNodeIds": sorted_set_difference(&previous_node_ids, &next_node_ids),
        "addedEdgeIds": sorted_set_difference(&next_edge_ids, &previous_edge_ids),
        "removedEdgeIds": sorted_set_difference(&previous_edge_ids, &next_edge_ids),
        "addedClaimIds": sorted_set_difference(&next_claim_ids, &previous_claim_ids),
        "removedClaimIds": sorted_set_difference(&previous_claim_ids, &next_claim_ids),
        "addedMemoryIds": sorted_set_difference(&next_memory_ids, &previous_memory_ids),
        "removedMemoryIds": sorted_set_difference(&previous_memory_ids, &next_memory_ids),
        "addedWikiPaths": sorted_set_difference(&next_wiki_paths, &previous_wiki_paths),
        "removedWikiPaths": sorted_set_difference(&previous_wiki_paths, &next_wiki_paths),
    })
}

fn sorted_set_difference(left: &BTreeSet<&str>, right: &BTreeSet<&str>) -> Vec<String> {
    left.difference(right)
        .map(|value| (*value).to_string())
        .collect()
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
        new_review_count: 0,
        new_markdown_source_count: 0,
        enqueued_markdown_source_count: 0,
        ingest_worker_started: false,
        ingested_markdown_source_count: 0,
        failed_markdown_source_count: 0,
        applied_agent_proposal_count: 0,
        failed_agent_proposal_count: 0,
        repairs: Vec::new(),
        new_markdown_sources: Vec::new(),
        enqueued_markdown_sources: Vec::new(),
        ingested_markdown_sources: Vec::new(),
        failed_markdown_sources: Vec::new(),
        applied_agent_proposals: Vec::new(),
        failed_agent_proposals: Vec::new(),
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
struct MissingWikiPageStub {
    path: String,
    title: String,
    contexts: Vec<String>,
    node_refs: Vec<String>,
    source_refs: Vec<String>,
    evidence_refs: Vec<String>,
}

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
                source_refs: merge_string_refs(&stub.source_refs, &[stub.path.clone()]),
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
            | BrainEventKind::NodeProposed
            | BrainEventKind::MemoryProposed
            | BrainEventKind::ClaimProposed
            | BrainEventKind::LinkProposed
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::ReviewResolved
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

fn write_lint_review_items(
    writer: &BrainWorkspaceWriter,
    snapshot: &BrainRepoSnapshot,
    issues: &[BrainLintIssue],
) -> Result<usize> {
    let mut created = 0;
    for issue in issues.iter().filter(|issue| issue.severity == "risky") {
        let proposal_id = format!("proposal-{}", issue.issue_id);
        let proposal_path = writer.proposal_path(&proposal_id);
        if proposal_path.exists() {
            continue;
        }
        let kind = if issue.kind == "orphan" {
            BrainProposalKind::Link
        } else {
            BrainProposalKind::Claim
        };
        let proposal = BrainUpdateProposal {
            proposal_id: proposal_id.clone(),
            workspace_id: snapshot.workspace_id.clone(),
            kind,
            status: BrainProposalStatus::PendingReview,
            actor: BrainActor {
                actor_type: BrainActorType::System,
                actor_id: "hyprduck-maintenance".into(),
            },
            scope: BrainScope::Project,
            title: issue.title.clone(),
            body: issue.body.clone(),
            target_node_id: issue.node_refs.first().cloned(),
            target_source_id: issue.source_refs.first().cloned(),
            relation_kind: if kind == BrainProposalKind::Link {
                Some(BrainRelationKind::RelatedTo)
            } else {
                None
            },
            source_refs: issue.source_refs.clone(),
            node_refs: issue.node_refs.clone(),
            evidence_refs: issue.evidence_refs.clone(),
            proposal_payload: None,
            created_at: unix_timestamp_seconds(),
        };
        writer.write_proposal(&proposal)?;
        writer.append_event(&brain_review_created_event(&proposal, issue)?)?;
        created += 1;
    }
    Ok(created)
}

fn brain_review_created_event(
    proposal: &BrainUpdateProposal,
    issue: &BrainLintIssue,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewCreated,
        operation_type: Some("review_created".into()),
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs: issue.relation_refs.clone(),
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(issue)
            .context("failed to encode lint review event payload")?,
        causality: proposal_event_causality(proposal),
        confidence: None,
        policy_result: "needs_review".into(),
        created_at: proposal.created_at,
    })
}

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

fn markdown_source_queued_event(
    workspace_id: &str,
    queued: &MarkdownIngestQueueRecord,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::SourceIngestQueued,
        operation_type: Some("source_ingest_queued".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs: vec![queued.queue_id.clone()],
        source_markdown_refs: vec![queued.relative_path.clone()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(queued)
            .context("failed to encode source ingest queued event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![queued.queue_id.clone()],
            materialized_version: Some(queued.enqueued_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "auto_enqueued".into(),
        created_at: queued.enqueued_at,
    })
}

fn markdown_source_compiled_event(
    record: &MarkdownIngestQueueRecord,
    manifest: Option<&SourceArtifactManifest>,
) -> Result<BrainEvent> {
    let source_refs = manifest
        .map(|manifest| vec![manifest.source_id.clone()])
        .unwrap_or_else(|| vec![record.queue_id.clone()]);
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: record.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::SourceCompiled,
        operation_type: Some("source_compiled".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs,
        source_markdown_refs: vec![record.relative_path.clone()],
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(record)
            .context("failed to encode source compiled event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: manifest
                .map(|manifest| vec![manifest.source_id.clone()])
                .unwrap_or_else(|| vec![record.queue_id.clone()]),
            materialized_version: record.completed_at,
            ..Default::default()
        },
        confidence: None,
        policy_result: if record.status == "ingested" {
            "auto_compiled".into()
        } else {
            "failed".into()
        },
        created_at: record.completed_at.unwrap_or_else(unix_timestamp_seconds),
    })
}

fn markdown_ingest_idempotent_noop_event(
    record: &MarkdownIngestQueueRecord,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    let snapshot_id = format!(
        "snapshot-{}-{}",
        snapshot.workspace_id, snapshot.generated_at
    );
    let evidence_refs = snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(manifest.source_id.as_str()))
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: record.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialize_noop".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-ingest".into(),
        },
        source_refs: vec![manifest.source_id.clone()],
        source_markdown_refs: vec![record.relative_path.clone(), manifest.markdown_path.clone()],
        node_refs: snapshot
            .nodes
            .iter()
            .filter(|node| node.source_ids.contains(&manifest.source_id))
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: Vec::new(),
        claim_refs: snapshot
            .claims
            .iter()
            .filter(|claim| claim.source_refs.contains(&manifest.source_id))
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: snapshot
            .memories
            .iter()
            .filter(|memory| memory.source_refs.contains(&manifest.source_id))
            .map(|memory| memory.memory_id.clone())
            .collect(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: evidence_refs.into_iter().collect(),
        payload_json: serde_json::to_string(&json!({
            "mutationType": "noop",
            "result": "idempotent",
            "reason": "markdown_source_already_materialized",
            "sourceId": manifest.source_id,
            "sourcePath": record.relative_path,
            "contentHash": record.content_hash,
            "snapshotId": snapshot_id,
            "diff": {
                "changedFiles": [],
                "nodeChanges": [],
                "edgeChanges": [],
                "claimChanges": [],
                "memoryChanges": [],
                "wikiChanges": []
            },
            "rollbackHint": "No materialized graph/wiki files changed; replay remains anchored by events/brain_events.jsonl."
        }))
        .context("failed to encode idempotent markdown ingest event payload")?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            snapshot_id: Some(snapshot_id),
            materialized_version: Some(snapshot.generated_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "idempotent_noop".into(),
        created_at: record.completed_at.unwrap_or_else(unix_timestamp_seconds),
    })
}

fn should_auto_apply_brain_proposal(request: &ProposeBrainUpdateRequest) -> bool {
    if matches!(
        request.kind,
        BrainProposalKind::Node
            | BrainProposalKind::Memory
            | BrainProposalKind::Observation
            | BrainProposalKind::SourceNote
    ) {
        return true;
    }
    matches!(
        (&request.kind, &request.proposal_payload),
        (
            BrainProposalKind::Claim,
            Some(AgentGraphProposalPayload::NewClaim { .. })
        ) | (
            BrainProposalKind::Link,
            Some(AgentGraphProposalPayload::NewEdge { .. })
        )
    )
}

fn validate_brain_update_proposal(request: &ProposeBrainUpdateRequest) -> Result<()> {
    let mut issues = Vec::new();
    if request.title.trim().is_empty() {
        push_agent_proposal_issue(
            &mut issues,
            AgentGraphProposalValidationCode::MissingRequiredField,
            "title",
            "brain update proposal title cannot be empty",
        );
    }
    if request.body.trim().is_empty() {
        push_agent_proposal_issue(
            &mut issues,
            AgentGraphProposalValidationCode::MissingRequiredField,
            "body",
            "brain update proposal body cannot be empty",
        );
    }
    match request.kind {
        BrainProposalKind::Node => {
            if !matches!(
                &request.proposal_payload,
                Some(AgentGraphProposalPayload::NewNode { .. })
            ) {
                push_agent_proposal_issue(
                    &mut issues,
                    AgentGraphProposalValidationCode::KindPayloadMismatch,
                    "proposalPayload.changeType",
                    "node proposal needs new_node proposalPayload",
                );
            }
        }
        BrainProposalKind::Link => {
            let payload_edge = match &request.proposal_payload {
                Some(AgentGraphProposalPayload::NewEdge { edge }) => Some(edge),
                _ => None,
            };
            if request
                .target_node_id
                .as_deref()
                .or_else(|| payload_edge.map(|edge| edge.target_node_id.as_str()))
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                push_agent_proposal_issue(
                    &mut issues,
                    AgentGraphProposalValidationCode::MissingTargetNode,
                    "targetNodeId",
                    "link proposal needs --target-node",
                );
            }
            if request.node_refs.is_empty()
                && !payload_edge.is_some_and(|edge| !edge.source_node_id.trim().is_empty())
            {
                push_agent_proposal_issue(
                    &mut issues,
                    AgentGraphProposalValidationCode::MissingNodeRefs,
                    "nodeRefs",
                    "link proposal needs at least one --node ref",
                );
            }
            if request.relation_kind.is_none() && payload_edge.is_none() {
                push_agent_proposal_issue(
                    &mut issues,
                    AgentGraphProposalValidationCode::MissingRelationKind,
                    "relationKind",
                    "link proposal needs --relation",
                );
            }
        }
        BrainProposalKind::SourceNote => {
            if request
                .target_source_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                push_agent_proposal_issue(
                    &mut issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "targetSourceId",
                    "source note proposal needs --source",
                );
            }
        }
        BrainProposalKind::Memory
        | BrainProposalKind::Claim
        | BrainProposalKind::Observation
        | BrainProposalKind::WikiPage => {}
    }
    if let Some(payload) = &request.proposal_payload {
        validate_agent_graph_proposal_payload(request, payload, &mut issues);
    }
    if !issues.is_empty() {
        return Err(anyhow!(AgentGraphProposalValidationError::new(issues)));
    }
    Ok(())
}

fn validate_agent_graph_proposal_payload(
    request: &ProposeBrainUpdateRequest,
    payload: &AgentGraphProposalPayload,
    issues: &mut Vec<AgentGraphProposalValidationIssue>,
) {
    match payload {
        AgentGraphProposalPayload::NewNode { node } => {
            if request.kind != BrainProposalKind::Node {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::KindPayloadMismatch,
                    "proposalPayload.changeType",
                    "new_node proposal payload requires kind=node",
                );
            }
            if node.label.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.node.label",
                    "new_node proposal payload needs node.label",
                );
            }
            if node.source_path.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.node.sourcePath",
                    "new_node proposal payload needs node.sourcePath",
                );
            }
            if node.source_refs.is_empty() && request.source_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingSourceRefs,
                    "proposalPayload.node.sourceRefs",
                    "new_node proposal payload needs sourceRefs",
                );
            }
            if node.evidence_refs.is_empty() && request.evidence_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingEvidenceRefs,
                    "proposalPayload.node.evidenceRefs",
                    "new_node proposal payload needs evidenceRefs",
                );
            }
        }
        AgentGraphProposalPayload::NewEdge { edge } => {
            if request.kind != BrainProposalKind::Link {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::KindPayloadMismatch,
                    "proposalPayload.changeType",
                    "new_edge proposal payload requires kind=link",
                );
            }
            if edge.source_node_id.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.edge.sourceNodeId",
                    "new_edge proposal payload needs edge.sourceNodeId",
                );
            }
            if edge.target_node_id.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.edge.targetNodeId",
                    "new_edge proposal payload needs edge.targetNodeId",
                );
            }
            if edge.label.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.edge.label",
                    "new_edge proposal payload needs edge.label",
                );
            }
            if edge.source_path.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.edge.sourcePath",
                    "new_edge proposal payload needs edge.sourcePath",
                );
            }
            if edge.source_refs.is_empty() && request.source_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingSourceRefs,
                    "proposalPayload.edge.sourceRefs",
                    "new_edge proposal payload needs sourceRefs",
                );
            }
            if edge.evidence_refs.is_empty() && request.evidence_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingEvidenceRefs,
                    "proposalPayload.edge.evidenceRefs",
                    "new_edge proposal payload needs evidenceRefs",
                );
            }
        }
        AgentGraphProposalPayload::NewClaim { claim } => {
            if request.kind != BrainProposalKind::Claim {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::KindPayloadMismatch,
                    "proposalPayload.changeType",
                    "new_claim proposal payload requires kind=claim",
                );
            }
            if claim.statement.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.claim.statement",
                    "new_claim proposal payload needs claim.statement",
                );
            }
            if claim.source_path.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.claim.sourcePath",
                    "new_claim proposal payload needs claim.sourcePath",
                );
            }
            if claim.topic_refs.is_empty() && request.node_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingTopicRefs,
                    "proposalPayload.claim.topicRefs",
                    "new_claim proposal payload needs topicRefs",
                );
            }
            if claim.source_refs.is_empty() && request.source_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingSourceRefs,
                    "proposalPayload.claim.sourceRefs",
                    "new_claim proposal payload needs sourceRefs",
                );
            }
            if claim.evidence_refs.is_empty() && request.evidence_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingEvidenceRefs,
                    "proposalPayload.claim.evidenceRefs",
                    "new_claim proposal payload needs evidenceRefs",
                );
            }
        }
        AgentGraphProposalPayload::NewMemory { memory } => {
            if request.kind != BrainProposalKind::Memory {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::KindPayloadMismatch,
                    "proposalPayload.changeType",
                    "new_memory proposal payload requires kind=memory",
                );
            }
            if memory.title.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.memory.title",
                    "new_memory proposal payload needs memory.title",
                );
            }
            if memory.body.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.memory.body",
                    "new_memory proposal payload needs memory.body",
                );
            }
            if memory.source_path.trim().is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingRequiredField,
                    "proposalPayload.memory.sourcePath",
                    "new_memory proposal payload needs memory.sourcePath",
                );
            }
            if memory.source_refs.is_empty() && request.source_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingSourceRefs,
                    "proposalPayload.memory.sourceRefs",
                    "new_memory proposal payload needs sourceRefs",
                );
            }
            if memory.evidence_refs.is_empty() && request.evidence_refs.is_empty() {
                push_agent_proposal_issue(
                    issues,
                    AgentGraphProposalValidationCode::MissingEvidenceRefs,
                    "proposalPayload.memory.evidenceRefs",
                    "new_memory proposal payload needs evidenceRefs",
                );
            }
        }
    }
}

fn push_agent_proposal_issue(
    issues: &mut Vec<AgentGraphProposalValidationIssue>,
    code: AgentGraphProposalValidationCode,
    field: &str,
    message: &str,
) {
    issues.push(AgentGraphProposalValidationIssue::new(code, field, message));
}

fn proposal_operation_type(proposal: &BrainUpdateProposal) -> Option<String> {
    Some(
        match proposal.kind {
            BrainProposalKind::Node => "new_node",
            BrainProposalKind::Memory => "new_memory",
            BrainProposalKind::Claim => "new_claim",
            BrainProposalKind::Link => "new_edge",
            BrainProposalKind::Observation => "new_observation",
            BrainProposalKind::SourceNote => "source_note",
            BrainProposalKind::WikiPage => "wiki_page",
        }
        .into(),
    )
}

fn proposal_source_markdown_refs(proposal: &BrainUpdateProposal) -> Vec<String> {
    let mut refs = Vec::new();
    match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewNode { node }) => {
            merge_unique_string(&mut refs, node.source_path.trim());
        }
        Some(AgentGraphProposalPayload::NewEdge { edge }) => {
            merge_unique_string(&mut refs, edge.source_path.trim());
        }
        Some(AgentGraphProposalPayload::NewClaim { claim }) => {
            merge_unique_string(&mut refs, claim.source_path.trim());
        }
        Some(AgentGraphProposalPayload::NewMemory { memory }) => {
            merge_unique_string(&mut refs, memory.source_path.trim());
        }
        None => {}
    }
    refs
}

fn proposal_target_node_ids(proposal: &BrainUpdateProposal) -> Result<Vec<String>> {
    let mut node_ids = proposal.node_refs.clone();
    if let Some(target_node_id) = proposal.target_node_id.as_deref() {
        merge_unique_string(&mut node_ids, target_node_id);
    }
    match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewNode { .. }) => {
            merge_unique_string(&mut node_ids, &agent_new_node_payload_node_id(proposal)?);
        }
        Some(AgentGraphProposalPayload::NewEdge { edge }) => {
            merge_unique_string(&mut node_ids, edge.source_node_id.trim());
            merge_unique_string(&mut node_ids, edge.target_node_id.trim());
        }
        Some(AgentGraphProposalPayload::NewClaim { claim }) => {
            for topic_ref in &claim.topic_refs {
                merge_unique_string(&mut node_ids, topic_ref);
            }
        }
        Some(AgentGraphProposalPayload::NewMemory { .. }) | None => {}
    }
    Ok(node_ids)
}

fn proposal_target_edge_ids(proposal: &BrainUpdateProposal) -> Result<Vec<String>> {
    if proposal.kind == BrainProposalKind::Link {
        Ok(vec![relation_record_for_proposal(proposal)?.relation_id])
    } else {
        Ok(Vec::new())
    }
}

fn proposal_target_claim_ids(proposal: &BrainUpdateProposal) -> Vec<String> {
    if proposal.kind == BrainProposalKind::Claim {
        vec![claim_record_for_proposal(proposal).claim_id]
    } else {
        Vec::new()
    }
}

fn proposal_target_memory_ids(proposal: &BrainUpdateProposal) -> Vec<String> {
    if proposal.kind == BrainProposalKind::Memory {
        vec![memory_record_for_proposal(proposal).memory_id]
    } else {
        Vec::new()
    }
}

fn proposal_event_causality(proposal: &BrainUpdateProposal) -> BrainEventCausality {
    BrainEventCausality {
        caused_by_proposal_id: Some(proposal.proposal_id.clone()),
        caused_by_source_ids: proposal.source_refs.clone(),
        materialized_version: Some(proposal.created_at),
        ..Default::default()
    }
}

fn brain_event_for_proposal(proposal: &BrainUpdateProposal) -> Result<BrainEvent> {
    let event_type = match proposal.kind {
        BrainProposalKind::Node => BrainEventKind::NodeProposed,
        BrainProposalKind::Memory => BrainEventKind::MemoryProposed,
        BrainProposalKind::Claim => BrainEventKind::ClaimProposed,
        BrainProposalKind::Link => BrainEventKind::LinkProposed,
        BrainProposalKind::Observation => BrainEventKind::ObservationAppended,
        BrainProposalKind::SourceNote => BrainEventKind::SourceNoteProposed,
        BrainProposalKind::WikiPage => BrainEventKind::WikiPageProposed,
    };
    let relation_refs = if proposal.kind == BrainProposalKind::Link {
        vec![relation_record_for_proposal(proposal)?.relation_id]
    } else {
        Vec::new()
    };
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type,
        operation_type: proposal_operation_type(proposal),
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs,
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(proposal)
            .context("failed to encode proposal event payload")?,
        causality: proposal_event_causality(proposal),
        confidence: None,
        policy_result: "needs_review".into(),
        created_at: proposal.created_at,
    })
}

fn brain_graph_mutation_applied_event(proposal: &BrainUpdateProposal) -> Result<BrainEvent> {
    let mutation_type = match proposal.kind {
        BrainProposalKind::Node => "new_node",
        BrainProposalKind::Claim => "new_claim",
        BrainProposalKind::Link => "new_edge",
        BrainProposalKind::Memory => "new_memory",
        _ => bail!(
            "proposal {} is not an auto-applied graph mutation",
            proposal.proposal_id
        ),
    };
    let mut node_refs = proposal.node_refs.clone();
    if proposal.kind == BrainProposalKind::Node {
        merge_unique_string(&mut node_refs, &agent_new_node_payload_node_id(proposal)?);
    }
    let relation_refs = if proposal.kind == BrainProposalKind::Link {
        vec![relation_record_for_proposal(proposal)?.relation_id]
    } else {
        Vec::new()
    };
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some(mutation_type.into()),
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs,
        relation_refs,
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "mutationType": mutation_type,
            "proposalId": proposal.proposal_id,
            "proposal": proposal,
        }))
        .context("failed to encode applied graph mutation event payload")?,
        causality: proposal_event_causality(proposal),
        confidence: None,
        policy_result: "auto_applied".into(),
        created_at: proposal.created_at,
    })
}

fn memory_record_for_proposal(proposal: &BrainUpdateProposal) -> MemoryRecord {
    let payload_memory = match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewMemory { memory }) => Some(memory),
        _ => None,
    };
    let mut source_refs = payload_memory
        .map(|memory| merge_unique_strings(&proposal.source_refs, &memory.source_refs))
        .unwrap_or_else(|| proposal.source_refs.clone());
    if source_refs.is_empty() {
        if let Some(source_path) = payload_memory
            .map(|memory| memory.source_path.trim())
            .filter(|source_path| !source_path.is_empty())
        {
            source_refs.push(source_path.to_string());
        }
    }
    let evidence_refs = payload_memory
        .map(|memory| merge_unique_strings(&proposal.evidence_refs, &memory.evidence_refs))
        .unwrap_or_else(|| proposal.evidence_refs.clone());
    MemoryRecord {
        memory_id: payload_memory
            .and_then(|memory| memory.memory_id.as_deref())
            .map(str::trim)
            .filter(|memory_id| !memory_id.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("memory-{}", brain_proposal_fingerprint(proposal))),
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        title: payload_memory
            .map(|memory| memory.title.trim())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| proposal.title.trim())
            .to_string(),
        body: payload_memory
            .map(|memory| memory.body.trim())
            .filter(|body| !body.is_empty())
            .unwrap_or_else(|| proposal.body.trim())
            .to_string(),
        source_refs,
        evidence_refs,
        created_at: proposal.created_at,
        updated_at: proposal.created_at,
    }
}

fn brain_memory_accepted_event(proposal: &BrainUpdateProposal) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::MemoryAccepted,
        operation_type: Some("new_memory".into()),
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(proposal)
            .context("failed to encode accepted memory event payload")?,
        causality: proposal_event_causality(proposal),
        confidence: None,
        policy_result: "auto_applied".into(),
        created_at: proposal.created_at,
    })
}

fn brain_review_resolved_event(
    proposal: &BrainUpdateProposal,
    request: &ResolveBrainReviewItemRequest,
) -> Result<BrainEvent> {
    let decision = match request.decision {
        BrainReviewDecision::Accept => "accept",
        BrainReviewDecision::Reject => "reject",
    };
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewResolved,
        operation_type: Some("review_resolved".into()),
        actor: request.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "proposalId": proposal.proposal_id,
            "decision": decision,
            "reason": request.reason,
            "status": proposal.status,
        }))
        .context("failed to encode review resolved event payload")?,
        causality: proposal_event_causality(proposal),
        confidence: None,
        policy_result: decision.into(),
        created_at: unix_timestamp_seconds(),
    })
}

fn brain_proposal_fingerprint(proposal: &BrainUpdateProposal) -> String {
    let mut parts = vec![
        proposal.workspace_id.clone(),
        format!("{:?}", proposal.kind).to_ascii_lowercase(),
        proposal.actor.actor_id.clone(),
        proposal.title.trim().to_ascii_lowercase(),
        proposal.body.trim().to_ascii_lowercase(),
    ];
    parts.extend(proposal.source_refs.iter().cloned());
    parts.extend(proposal.node_refs.iter().cloned());
    parts.extend(proposal.evidence_refs.iter().cloned());
    if let Some(target_node_id) = &proposal.target_node_id {
        parts.push(target_node_id.clone());
    }
    if let Some(target_source_id) = &proposal.target_source_id {
        parts.push(target_source_id.clone());
    }
    if let Some(relation_kind) = proposal.relation_kind {
        parts.push(format!("{relation_kind:?}").to_ascii_lowercase());
    }
    sanitize_name(&parts.join("-"))
}

struct BrainWorkspaceWriter {
    repo: BrainArtifactRepository,
    _lock: WorkspaceLock,
}

impl BrainWorkspaceWriter {
    fn open(root: PathBuf) -> Result<Self> {
        let (repo, lock) = BrainArtifactRepository::open(root)?;
        Ok(Self { repo, _lock: lock })
    }

    fn root(&self) -> &Path {
        self.repo.root()
    }

    fn write_proposal(&self, proposal: &BrainUpdateProposal) -> Result<PathBuf> {
        self.repo.write_proposal(proposal)
    }

    fn proposal_path(&self, proposal_id: &str) -> PathBuf {
        self.repo.proposal_path(proposal_id)
    }

    fn append_event(&self, event: &BrainEvent) -> Result<()> {
        self.repo.append_event(event)
    }

    fn upsert_memory_record(&self, memory: MemoryRecord) -> Result<()> {
        let mut memories = self.repo.read_memory_records()?;
        if let Some(existing) = memories
            .iter_mut()
            .find(|record| record.memory_id == memory.memory_id)
        {
            *existing = memory;
        } else {
            memories.push(memory);
        }
        memories.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        self.repo.write_memory_records(&memories)
    }

    fn apply_source_note_metadata(
        &self,
        proposal: &BrainUpdateProposal,
        request: &ProposeBrainUpdateRequest,
    ) -> Result<()> {
        let source_id = proposal
            .target_source_id
            .as_ref()
            .or_else(|| proposal.source_refs.first())
            .context("source note proposal needs a source id")?;
        let manifest_path = self
            .repo
            .root()
            .join("artifacts")
            .join(source_id)
            .join("source-manifest.json");
        let mut manifest: SourceArtifactManifest = read_json_artifact(&manifest_path)?;
        merge_source_metadata(
            &mut manifest.description,
            request
                .source_description
                .as_deref()
                .unwrap_or(&proposal.body),
        );
        merge_source_metadata(
            &mut manifest.user_context,
            request.source_user_context.as_deref().unwrap_or(""),
        );
        merge_source_metadata(
            &mut manifest.ingest_instruction,
            request.source_ingest_instruction.as_deref().unwrap_or(""),
        );
        manifest.updated_at = unix_timestamp_seconds();
        write_source_manifest(&manifest)?;
        if let Some(output_root) = self.repo.output_root() {
            let store = KnowledgeProjectStore::new(output_root.join("knowledge.sqlite3"));
            store.update_source_manifest_snapshot(&manifest)?;
        }
        self.update_brain_manifest_source(&manifest)?;
        Ok(())
    }

    fn apply_accepted_proposal(&self, proposal: &BrainUpdateProposal) -> Result<()> {
        if proposal.status != BrainProposalStatus::Accepted {
            return Ok(());
        }
        let manifest_path = self.repo.brain_manifest_path();
        if !manifest_path.exists() {
            return Ok(());
        }
        let mut snapshot = self.repo.read_brain_manifest()?;
        match proposal.kind {
            BrainProposalKind::Node => {
                apply_accepted_proposal_to_snapshot(proposal, &mut snapshot)?;
                persist_materialized_graph_and_wiki_state(self.repo.root(), &snapshot)?;
            }
            BrainProposalKind::Claim | BrainProposalKind::Link => {
                apply_accepted_proposal_to_snapshot(proposal, &mut snapshot)?;
                refresh_materialized_wiki_pages(&mut snapshot);
                persist_materialized_graph_and_wiki_state(self.repo.root(), &snapshot)?;
            }
            BrainProposalKind::WikiPage => {
                let page =
                    resolve_persisted_wiki_page_for_proposal(self.repo.root(), &snapshot, proposal);
                apply_wiki_page_to_snapshot(&mut snapshot, page.clone());
                persist_materialized_graph_and_wiki_state(self.repo.root(), &snapshot)?;
            }
            BrainProposalKind::Memory
            | BrainProposalKind::Observation
            | BrainProposalKind::SourceNote => {}
        }
        Ok(())
    }

    fn update_brain_manifest_source(&self, manifest: &SourceArtifactManifest) -> Result<()> {
        let path = self.repo.brain_manifest_path();
        if !path.exists() {
            return Ok(());
        }
        let mut snapshot = self.repo.read_brain_manifest()?;
        if let Some(source) = snapshot
            .sources
            .iter_mut()
            .find(|source| source.source_id == manifest.source_id)
        {
            source.description = manifest.description.clone();
            source.user_context = manifest.user_context.clone();
            source.ingest_instruction = manifest.ingest_instruction.clone();
            source.updated_at = manifest.updated_at;
            self.repo.write_brain_manifest(&snapshot)?;
        }
        Ok(())
    }
}

fn merge_source_metadata(target: &mut String, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        *target = value.to_string();
    }
}

fn apply_accepted_proposals_to_snapshot(
    root: &Path,
    snapshot: &mut BrainRepoSnapshot,
) -> Result<()> {
    let node_redirects = merge_correction_node_redirects(snapshot);
    let deleted_node_ids = delete_correction_node_ids(snapshot);
    let valid_source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    let valid_evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();
    let mut accepted = read_brain_update_proposals(root)?
        .into_iter()
        .map(|(proposal, _)| proposal)
        .filter(|proposal| proposal.workspace_id == snapshot.workspace_id)
        .filter(|proposal| proposal.status == BrainProposalStatus::Accepted)
        .filter_map(|mut proposal| {
            retain_proposal_refs_for_snapshot(&mut proposal, &valid_source_ids, &valid_evidence_ids)
                .then_some(proposal)
        })
        .filter(|proposal| !proposal_references_deleted_node(proposal, &deleted_node_ids))
        .collect::<Vec<_>>();
    accepted.sort_by(|left, right| {
        brain_proposal_replay_priority(left.kind)
            .cmp(&brain_proposal_replay_priority(right.kind))
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.proposal_id.cmp(&right.proposal_id))
    });
    for proposal in accepted {
        let proposal = remap_proposal_node_refs_for_merge(proposal, &node_redirects);
        if proposal_references_missing_snapshot_node(&proposal, snapshot) {
            continue;
        }
        apply_accepted_proposal_to_snapshot_with_root(root, &proposal, snapshot)?;
    }
    normalize_snapshot_after_merge_redirects(snapshot, &node_redirects);
    normalize_snapshot_after_deleted_nodes(snapshot, &deleted_node_ids);
    Ok(())
}

fn retain_proposal_refs_for_snapshot(
    proposal: &mut BrainUpdateProposal,
    valid_source_ids: &BTreeSet<String>,
    valid_evidence_ids: &BTreeSet<String>,
) -> bool {
    let source_refs = proposal_source_refs(proposal);
    if !source_refs.is_empty()
        && !source_refs
            .iter()
            .any(|source| valid_source_ids.contains(source))
    {
        return false;
    }
    let evidence_refs = proposal_evidence_refs(proposal);
    if !evidence_refs.is_empty()
        && !evidence_refs
            .iter()
            .any(|evidence| valid_evidence_ids.contains(evidence))
    {
        return false;
    }

    proposal
        .source_refs
        .retain(|source| valid_source_ids.contains(source));
    proposal
        .evidence_refs
        .retain(|evidence| valid_evidence_ids.contains(evidence));
    if let Some(payload) = &mut proposal.proposal_payload {
        match payload {
            AgentGraphProposalPayload::NewNode { node } => {
                node.source_refs
                    .retain(|source| valid_source_ids.contains(source));
                node.evidence_refs
                    .retain(|evidence| valid_evidence_ids.contains(evidence));
            }
            AgentGraphProposalPayload::NewEdge { edge } => {
                edge.source_refs
                    .retain(|source| valid_source_ids.contains(source));
                edge.evidence_refs
                    .retain(|evidence| valid_evidence_ids.contains(evidence));
            }
            AgentGraphProposalPayload::NewClaim { claim } => {
                claim
                    .source_refs
                    .retain(|source| valid_source_ids.contains(source));
                claim
                    .evidence_refs
                    .retain(|evidence| valid_evidence_ids.contains(evidence));
            }
            AgentGraphProposalPayload::NewMemory { memory } => {
                memory
                    .source_refs
                    .retain(|source| valid_source_ids.contains(source));
                memory
                    .evidence_refs
                    .retain(|evidence| valid_evidence_ids.contains(evidence));
            }
        }
    }
    true
}

fn proposal_source_refs(proposal: &BrainUpdateProposal) -> BTreeSet<String> {
    let mut refs = proposal
        .source_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewNode { node }) => {
            refs.extend(node.source_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewEdge { edge }) => {
            refs.extend(edge.source_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewClaim { claim }) => {
            refs.extend(claim.source_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewMemory { memory }) => {
            refs.extend(memory.source_refs.iter().cloned());
        }
        None => {}
    }
    refs
}

fn proposal_evidence_refs(proposal: &BrainUpdateProposal) -> BTreeSet<String> {
    let mut refs = proposal
        .evidence_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewNode { node }) => {
            refs.extend(node.evidence_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewEdge { edge }) => {
            refs.extend(edge.evidence_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewClaim { claim }) => {
            refs.extend(claim.evidence_refs.iter().cloned());
        }
        Some(AgentGraphProposalPayload::NewMemory { memory }) => {
            refs.extend(memory.evidence_refs.iter().cloned());
        }
        None => {}
    }
    refs
}

fn delete_correction_node_ids(snapshot: &BrainRepoSnapshot) -> BTreeSet<String> {
    snapshot
        .events
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::CorrectionApplied
                && (event.operation_type.as_deref() == Some("delete")
                    || event.payload_json.contains("\"kind\":\"delete\""))
        })
        .flat_map(|event| {
            event
                .target_node_ids
                .iter()
                .chain(event.node_refs.iter())
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect()
}

fn proposal_references_deleted_node(
    proposal: &BrainUpdateProposal,
    deleted_node_ids: &BTreeSet<String>,
) -> bool {
    if deleted_node_ids.is_empty() {
        return false;
    }
    if proposal
        .target_node_id
        .as_ref()
        .is_some_and(|node_id| deleted_node_ids.contains(node_id))
        || proposal
            .node_refs
            .iter()
            .any(|node_id| deleted_node_ids.contains(node_id))
    {
        return true;
    }
    match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewNode { node }) => node
            .node_id
            .as_ref()
            .is_some_and(|node_id| deleted_node_ids.contains(node_id)),
        Some(AgentGraphProposalPayload::NewEdge { edge }) => {
            deleted_node_ids.contains(&edge.source_node_id)
                || deleted_node_ids.contains(&edge.target_node_id)
        }
        Some(AgentGraphProposalPayload::NewClaim { claim }) => claim
            .topic_refs
            .iter()
            .any(|node_id| deleted_node_ids.contains(node_id)),
        Some(AgentGraphProposalPayload::NewMemory { .. }) | None => false,
    }
}

fn proposal_references_missing_snapshot_node(
    proposal: &BrainUpdateProposal,
    snapshot: &BrainRepoSnapshot,
) -> bool {
    let required_node_ids = proposal_required_snapshot_node_refs(proposal);
    if required_node_ids.is_empty() {
        return false;
    }
    let snapshot_node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    required_node_ids
        .iter()
        .any(|node_id| !snapshot_node_ids.contains(node_id.as_str()))
}

fn proposal_required_snapshot_node_refs(proposal: &BrainUpdateProposal) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    match proposal.kind {
        BrainProposalKind::Link => {
            insert_nonempty_node_ref(&mut refs, proposal.target_node_id.as_deref());
            for node_id in &proposal.node_refs {
                insert_nonempty_node_ref(&mut refs, Some(node_id));
            }
            if let Some(AgentGraphProposalPayload::NewEdge { edge }) = &proposal.proposal_payload {
                insert_nonempty_node_ref(&mut refs, Some(&edge.source_node_id));
                insert_nonempty_node_ref(&mut refs, Some(&edge.target_node_id));
            }
        }
        BrainProposalKind::Claim => {
            insert_nonempty_node_ref(&mut refs, proposal.target_node_id.as_deref());
            for node_id in &proposal.node_refs {
                insert_nonempty_node_ref(&mut refs, Some(node_id));
            }
            if let Some(AgentGraphProposalPayload::NewClaim { claim }) = &proposal.proposal_payload
            {
                for node_id in &claim.topic_refs {
                    insert_nonempty_node_ref(&mut refs, Some(node_id));
                }
            }
        }
        BrainProposalKind::WikiPage => {
            insert_nonempty_node_ref(&mut refs, proposal.target_node_id.as_deref());
            for node_id in &proposal.node_refs {
                insert_nonempty_node_ref(&mut refs, Some(node_id));
            }
        }
        BrainProposalKind::Node
        | BrainProposalKind::Memory
        | BrainProposalKind::Observation
        | BrainProposalKind::SourceNote => {}
    }
    refs
}

fn insert_nonempty_node_ref(refs: &mut BTreeSet<String>, value: Option<&str>) {
    if let Some(node_id) = value.map(str::trim).filter(|node_id| !node_id.is_empty()) {
        refs.insert(node_id.to_string());
    }
}

fn brain_proposal_replay_priority(kind: BrainProposalKind) -> u8 {
    match kind {
        BrainProposalKind::Node => 0,
        BrainProposalKind::Claim => 1,
        BrainProposalKind::Link => 2,
        BrainProposalKind::WikiPage => 3,
        BrainProposalKind::Memory
        | BrainProposalKind::Observation
        | BrainProposalKind::SourceNote => 4,
    }
}

fn apply_accepted_proposal_to_snapshot_with_root(
    root: &Path,
    proposal: &BrainUpdateProposal,
    snapshot: &mut BrainRepoSnapshot,
) -> Result<()> {
    if proposal.kind == BrainProposalKind::WikiPage {
        let page = resolve_persisted_wiki_page_for_proposal(root, snapshot, proposal);
        apply_wiki_page_to_snapshot(snapshot, page);
        return Ok(());
    }
    apply_accepted_proposal_to_snapshot(proposal, snapshot)
}

fn apply_accepted_proposal_to_snapshot(
    proposal: &BrainUpdateProposal,
    snapshot: &mut BrainRepoSnapshot,
) -> Result<()> {
    match proposal.kind {
        BrainProposalKind::Node => {
            let node = node_record_for_proposal(proposal)?;
            if let Some(existing) = snapshot
                .nodes
                .iter_mut()
                .find(|existing| existing.node_id == node.node_id)
            {
                merge_brain_node_record(existing, node.clone());
            } else {
                snapshot.nodes.push(node.clone());
            }
            snapshot.nodes.sort_by(|left, right| {
                left.node_id
                    .cmp(&right.node_id)
                    .then_with(|| left.label.cmp(&right.label))
            });
            if let Some(entity) = entity_record_for_node(proposal.workspace_id.as_str(), &node) {
                if let Some(existing) = snapshot
                    .entities
                    .iter_mut()
                    .find(|existing| existing.entity_id == entity.entity_id)
                {
                    *existing = entity;
                } else {
                    snapshot.entities.push(entity);
                }
                snapshot.entities.sort_by(|left, right| {
                    left.entity_id
                        .cmp(&right.entity_id)
                        .then_with(|| left.name.cmp(&right.name))
                });
            }
            refresh_materialized_wiki_pages(snapshot);
        }
        BrainProposalKind::Claim => {
            let claim = claim_record_for_proposal(proposal);
            if let Some(existing) = snapshot
                .claims
                .iter_mut()
                .find(|existing| claim_records_match_for_reuse(existing, &claim))
            {
                merge_claim_record(existing, claim);
            } else {
                snapshot.claims.push(claim);
            }
            snapshot.claims.sort_by(|left, right| {
                left.claim_id
                    .cmp(&right.claim_id)
                    .then_with(|| left.statement.cmp(&right.statement))
            });
        }
        BrainProposalKind::Link => {
            validate_relation_node_refs(proposal, snapshot)?;
            let relation = relation_record_for_proposal(proposal)?;
            if let Some(existing) = snapshot
                .relations
                .iter_mut()
                .find(|existing| existing.relation_id == relation.relation_id)
            {
                merge_brain_relation_record(existing, relation);
            } else {
                snapshot.relations.push(relation);
            }
            snapshot.relations.sort_by(|left, right| {
                left.relation_id
                    .cmp(&right.relation_id)
                    .then_with(|| left.label.cmp(&right.label))
            });
        }
        BrainProposalKind::WikiPage => {
            apply_wiki_page_to_snapshot(snapshot, wiki_page_for_proposal(proposal));
        }
        BrainProposalKind::Memory
        | BrainProposalKind::Observation
        | BrainProposalKind::SourceNote => {}
    }
    Ok(())
}

fn apply_wiki_page_to_snapshot(snapshot: &mut BrainRepoSnapshot, page: WikiPage) {
    if let Some(existing) = snapshot
        .wiki_pages
        .iter_mut()
        .find(|existing| existing.page_id == page.page_id)
    {
        *existing = page;
    } else {
        snapshot.wiki_pages.push(page);
    }
    snapshot.wiki_pages.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
}

fn merge_correction_node_redirects(snapshot: &BrainRepoSnapshot) -> BTreeMap<String, String> {
    let existing_nodes = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<BTreeSet<_>>();
    let mut redirects = BTreeMap::new();
    for event in &snapshot.events {
        if event.event_type != BrainEventKind::CorrectionApplied
            || !event.payload_json.contains("\"kind\":\"merge\"")
        {
            continue;
        }
        let Some(source_node_id) = event.node_refs.first() else {
            continue;
        };
        let Some(target_node_id) = event.node_refs.get(1) else {
            continue;
        };
        if source_node_id != target_node_id && existing_nodes.contains(target_node_id) {
            redirects.insert(source_node_id.clone(), target_node_id.clone());
        }
    }
    redirects
}

fn remap_proposal_node_refs_for_merge(
    mut proposal: BrainUpdateProposal,
    redirects: &BTreeMap<String, String>,
) -> BrainUpdateProposal {
    if redirects.is_empty() {
        return proposal;
    }
    proposal.target_node_id = proposal
        .target_node_id
        .map(|node_id| remap_merged_node_ref(&node_id, redirects));
    proposal.node_refs = remap_merged_node_refs(&proposal.node_refs, redirects);
    if let Some(payload) = proposal.proposal_payload {
        proposal.proposal_payload = Some(match payload {
            AgentGraphProposalPayload::NewNode { mut node } => {
                node.node_id = node
                    .node_id
                    .map(|node_id| remap_merged_node_ref(&node_id, redirects));
                AgentGraphProposalPayload::NewNode { node }
            }
            AgentGraphProposalPayload::NewEdge { mut edge } => {
                edge.source_node_id = remap_merged_node_ref(&edge.source_node_id, redirects);
                edge.target_node_id = remap_merged_node_ref(&edge.target_node_id, redirects);
                AgentGraphProposalPayload::NewEdge { edge }
            }
            AgentGraphProposalPayload::NewClaim { mut claim } => {
                claim.topic_refs = remap_merged_node_refs(&claim.topic_refs, redirects);
                AgentGraphProposalPayload::NewClaim { claim }
            }
            AgentGraphProposalPayload::NewMemory { memory } => {
                AgentGraphProposalPayload::NewMemory { memory }
            }
        });
    }
    proposal
}

fn normalize_snapshot_after_merge_redirects(
    snapshot: &mut BrainRepoSnapshot,
    redirects: &BTreeMap<String, String>,
) {
    if !redirects.is_empty() {
        for node in &mut snapshot.nodes {
            node.evidence_ids = merge_string_refs(&node.evidence_ids, &[]);
            node.source_ids = merge_string_refs(&node.source_ids, &[]);
            node.aliases = merge_string_refs(&node.aliases, &[]);
        }
        for entity in &mut snapshot.entities {
            entity.evidence_refs = merge_string_refs(&entity.evidence_refs, &[]);
            entity.source_refs = merge_string_refs(&entity.source_refs, &[]);
            entity.aliases = merge_string_refs(&entity.aliases, &[]);
        }
        for relation in &mut snapshot.relations {
            relation.source_node_id = remap_merged_node_ref(&relation.source_node_id, redirects);
            relation.target_node_id = remap_merged_node_ref(&relation.target_node_id, redirects);
            relation.evidence_ids = merge_string_refs(&relation.evidence_ids, &[]);
        }
        for claim in &mut snapshot.claims {
            claim.topic_refs = remap_merged_node_refs(&claim.topic_refs, redirects);
            claim.source_refs = merge_string_refs(&claim.source_refs, &[]);
            claim.evidence_refs = merge_string_refs(&claim.evidence_refs, &[]);
        }
        for memory in &mut snapshot.memories {
            memory.source_refs = merge_string_refs(&memory.source_refs, &[]);
            memory.evidence_refs = merge_string_refs(&memory.evidence_refs, &[]);
        }
        for page in &mut snapshot.wiki_pages {
            page.node_refs = remap_merged_node_refs(&page.node_refs, redirects);
            page.source_refs = merge_string_refs(&page.source_refs, &[]);
            page.evidence_refs = merge_string_refs(&page.evidence_refs, &[]);
        }
    }
    snapshot.relations = dedupe_brain_relations(std::mem::take(&mut snapshot.relations));
    snapshot.claims = dedupe_claim_records(std::mem::take(&mut snapshot.claims));
    snapshot.wiki_pages = dedupe_wiki_pages(std::mem::take(&mut snapshot.wiki_pages));
}

fn normalize_snapshot_after_deleted_nodes(
    snapshot: &mut BrainRepoSnapshot,
    deleted_node_ids: &BTreeSet<String>,
) {
    if deleted_node_ids.is_empty() {
        return;
    }

    let deleted_topic_paths = deleted_node_ids
        .iter()
        .map(|node_id| format!("wiki/topics/{}.md", sanitize_name(node_id)))
        .collect::<BTreeSet<_>>();
    snapshot
        .nodes
        .retain(|node| !deleted_node_ids.contains(&node.node_id));
    snapshot.relations.retain(|relation| {
        !deleted_node_ids.contains(&relation.source_node_id)
            && !deleted_node_ids.contains(&relation.target_node_id)
    });
    snapshot.claims.retain(|claim| {
        !claim
            .topic_refs
            .iter()
            .any(|node_id| deleted_node_ids.contains(node_id))
    });
    snapshot.entities.retain(|entity| {
        let node_id = entity
            .entity_id
            .strip_prefix("ent-")
            .unwrap_or(&entity.entity_id);
        !deleted_node_ids.contains(node_id)
    });
    for extraction in &mut snapshot.extractions {
        extraction
            .entities
            .retain(|entity| !deleted_node_ids.contains(&entity.entity_id));
        extraction
            .topics
            .retain(|topic| !deleted_node_ids.contains(&topic.topic_id));
        extraction.claims.retain(|claim| {
            !claim
                .subject_refs
                .iter()
                .any(|node_id| deleted_node_ids.contains(node_id))
        });
        extraction.relations.retain(|relation| {
            !deleted_node_ids.contains(&relation.source_node_id)
                && !deleted_node_ids.contains(&relation.target_node_id)
        });
    }
    snapshot
        .wiki_pages
        .retain(|page| !deleted_topic_paths.contains(&page.path));
    for page in &mut snapshot.wiki_pages {
        page.node_refs
            .retain(|node_id| !deleted_node_ids.contains(node_id));
    }
    refresh_materialized_wiki_pages(snapshot);
}

fn remap_merged_node_refs(
    node_refs: &[String],
    redirects: &BTreeMap<String, String>,
) -> Vec<String> {
    node_refs
        .iter()
        .map(|node_id| remap_merged_node_ref(node_id, redirects))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn remap_merged_node_ref(node_id: &str, redirects: &BTreeMap<String, String>) -> String {
    let mut current = node_id.to_string();
    let mut seen = BTreeSet::new();
    while let Some(next) = redirects.get(&current) {
        if !seen.insert(current.clone()) {
            break;
        }
        current = next.clone();
    }
    current
}

fn dedupe_brain_relations(relations: Vec<BrainRelationRecord>) -> Vec<BrainRelationRecord> {
    let mut merged = BTreeMap::<(String, String, String, String), BrainRelationRecord>::new();
    for relation in relations
        .into_iter()
        .filter(|relation| relation.source_node_id != relation.target_node_id)
    {
        let key = (
            format!("{:?}", relation.kind),
            relation.source_node_id.clone(),
            relation.target_node_id.clone(),
            relation.label.clone(),
        );
        match merged.get_mut(&key) {
            Some(existing) => merge_brain_relation_record(existing, relation),
            None => {
                merged.insert(key, relation);
            }
        }
    }
    let mut relations = merged.into_values().collect::<Vec<_>>();
    relations.sort_by(|left, right| {
        left.relation_id
            .cmp(&right.relation_id)
            .then_with(|| left.label.cmp(&right.label))
    });
    relations
}

fn merge_brain_relation_record(existing: &mut BrainRelationRecord, incoming: BrainRelationRecord) {
    existing.evidence_ids = merge_string_refs(&existing.evidence_ids, &incoming.evidence_ids);
    existing.confidence = match (existing.confidence, incoming.confidence) {
        (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    };
    existing.updated_at = existing.updated_at.max(incoming.updated_at);
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

fn dedupe_claim_records(claims: Vec<ClaimRecord>) -> Vec<ClaimRecord> {
    let mut merged = BTreeMap::<(String, Vec<String>), ClaimRecord>::new();
    for claim in claims {
        let key = claim_record_reuse_key(&claim);
        match merged.get_mut(&key) {
            Some(existing) => merge_claim_record(existing, claim),
            None => {
                merged.insert(key, claim);
            }
        }
    }
    let mut claims = merged.into_values().collect::<Vec<_>>();
    claims.sort_by(|left, right| {
        left.claim_id
            .cmp(&right.claim_id)
            .then_with(|| left.statement.cmp(&right.statement))
    });
    claims
}

fn claim_records_match_for_reuse(existing: &ClaimRecord, incoming: &ClaimRecord) -> bool {
    existing.claim_id == incoming.claim_id
        || claim_record_reuse_key(existing) == claim_record_reuse_key(incoming)
}

fn claim_record_reuse_key(claim: &ClaimRecord) -> (String, Vec<String>) {
    (
        normalize_claim_record_statement(&claim.statement),
        merge_string_refs(&claim.topic_refs, &[]),
    )
}

fn normalize_claim_record_statement(statement: &str) -> String {
    statement
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

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

fn validate_relation_node_refs(
    proposal: &BrainUpdateProposal,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    if proposal.kind != BrainProposalKind::Link {
        return Ok(());
    }

    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let source_node_id = proposal
        .node_refs
        .first()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .context("accepted link proposal needs a source node ref")?;
    let target_node_id = proposal
        .target_node_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("accepted link proposal needs a target node ref")?;
    let mut missing = proposal
        .node_refs
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|node_id| !node_ids.contains(node_id))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !node_ids.contains(source_node_id)
        && !missing.iter().any(|node_id| node_id == source_node_id)
    {
        missing.push(source_node_id.to_string());
    }
    if !node_ids.contains(target_node_id)
        && !missing.iter().any(|node_id| node_id == target_node_id)
    {
        missing.push(target_node_id.to_string());
    }

    if !missing.is_empty() {
        bail!(
            "link proposal {} references missing graph node id(s): {}",
            proposal.proposal_id,
            missing.join(", ")
        );
    }
    Ok(())
}

fn node_record_for_proposal(proposal: &BrainUpdateProposal) -> Result<BrainNodeRecord> {
    let Some(AgentGraphProposalPayload::NewNode { node }) = &proposal.proposal_payload else {
        bail!("accepted node proposal needs new_node proposalPayload");
    };
    let node_id = agent_new_node_payload_node_id(proposal)?;
    let mut source_ids = merge_unique_strings(&proposal.source_refs, &node.source_refs);
    if source_ids.is_empty() {
        source_ids.push(node.source_path.clone());
    }
    Ok(BrainNodeRecord {
        node_id,
        kind: node.kind,
        label: node.label.trim().to_string(),
        scope: proposal.scope,
        aliases: merge_unique_strings(&node.aliases, &[]),
        evidence_ids: merge_unique_strings(&proposal.evidence_refs, &node.evidence_refs),
        source_ids,
        confidence: None,
        updated_at: proposal.created_at,
    })
}

fn agent_new_node_payload_node_id(proposal: &BrainUpdateProposal) -> Result<String> {
    let Some(AgentGraphProposalPayload::NewNode { node }) = &proposal.proposal_payload else {
        bail!("node proposal needs new_node proposalPayload");
    };
    Ok(node
        .node_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("concept-{}", bounded_artifact_key(&node.label, 80))))
}

fn merge_brain_node_record(existing: &mut BrainNodeRecord, incoming: BrainNodeRecord) {
    existing.kind = incoming.kind;
    existing.label = incoming.label;
    existing.scope = incoming.scope;
    existing.aliases = merge_unique_strings(&existing.aliases, &incoming.aliases);
    existing.evidence_ids = merge_unique_strings(&existing.evidence_ids, &incoming.evidence_ids);
    existing.source_ids = merge_unique_strings(&existing.source_ids, &incoming.source_ids);
    existing.confidence = incoming.confidence.or(existing.confidence);
    existing.updated_at = incoming.updated_at;
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

fn entity_record_for_node(workspace_id: &str, node: &BrainNodeRecord) -> Option<EntityRecord> {
    matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic).then(|| EntityRecord {
        entity_id: format!("ent-{}", node.node_id),
        workspace_id: workspace_id.to_string(),
        kind: node.kind,
        name: node.label.clone(),
        aliases: node.aliases.clone(),
        source_refs: node.source_ids.clone(),
        evidence_refs: node.evidence_ids.clone(),
        updated_at: node.updated_at,
    })
}

fn merge_unique_strings(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right.iter())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn claim_record_for_proposal(proposal: &BrainUpdateProposal) -> ClaimRecord {
    let payload_claim = match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewClaim { claim }) => Some(claim),
        _ => None,
    };
    let statement = payload_claim
        .map(|claim| claim.statement.trim())
        .filter(|statement| !statement.is_empty())
        .unwrap_or_else(|| proposal.body.trim())
        .to_string();
    let topic_refs = payload_claim
        .map(|claim| merge_unique_strings(&proposal.node_refs, &claim.topic_refs))
        .unwrap_or_else(|| proposal.node_refs.clone());
    let mut source_refs = payload_claim
        .map(|claim| merge_unique_strings(&proposal.source_refs, &claim.source_refs))
        .unwrap_or_else(|| proposal.source_refs.clone());
    if source_refs.is_empty() {
        if let Some(source_path) = payload_claim
            .map(|claim| claim.source_path.trim())
            .filter(|source_path| !source_path.is_empty())
        {
            source_refs.push(source_path.to_string());
        }
    }
    let evidence_refs = payload_claim
        .map(|claim| merge_unique_strings(&proposal.evidence_refs, &claim.evidence_refs))
        .unwrap_or_else(|| proposal.evidence_refs.clone());
    let claim_id = payload_claim
        .and_then(|claim| claim.claim_id.as_deref())
        .map(str::trim)
        .filter(|claim_id| !claim_id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("claim-{}", brain_proposal_fingerprint(proposal)));
    ClaimRecord {
        claim_id,
        workspace_id: proposal.workspace_id.clone(),
        statement,
        topic_refs,
        source_refs,
        evidence_refs: evidence_refs.clone(),
        status: if evidence_refs.is_empty() {
            "accepted".into()
        } else {
            "supported".into()
        },
        updated_at: proposal.created_at,
    }
}

fn relation_record_for_proposal(proposal: &BrainUpdateProposal) -> Result<BrainRelationRecord> {
    let payload_edge = match &proposal.proposal_payload {
        Some(AgentGraphProposalPayload::NewEdge { edge }) => Some(edge),
        _ => None,
    };
    let source_node_id = proposal
        .node_refs
        .first()
        .map(|value| value.trim().to_string())
        .or_else(|| payload_edge.map(|edge| edge.source_node_id.trim().to_string()))
        .filter(|value| !value.is_empty())
        .context("accepted link proposal needs a source node ref")?;
    let target_node_id = proposal
        .target_node_id
        .clone()
        .or_else(|| payload_edge.map(|edge| edge.target_node_id.trim().to_string()))
        .filter(|value| !value.is_empty())
        .context("accepted link proposal needs a target node ref")?;
    let evidence_ids = payload_edge
        .map(|edge| merge_unique_strings(&proposal.evidence_refs, &edge.evidence_refs))
        .unwrap_or_else(|| proposal.evidence_refs.clone());
    Ok(BrainRelationRecord {
        relation_id: payload_edge
            .and_then(|edge| edge.edge_id.as_deref())
            .map(str::trim)
            .filter(|edge_id| !edge_id.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("relation-{}", brain_proposal_fingerprint(proposal))),
        kind: proposal
            .relation_kind
            .or_else(|| payload_edge.map(|edge| edge.kind))
            .unwrap_or(BrainRelationKind::RelatedTo),
        source_node_id,
        target_node_id,
        label: payload_edge
            .map(|edge| edge.label.trim())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| proposal.title.as_str())
            .to_string(),
        evidence_ids,
        confidence: None,
        updated_at: proposal.created_at,
    })
}

fn wiki_page_for_proposal(proposal: &BrainUpdateProposal) -> WikiPage {
    let page_id = format!("wiki-save-back-{}", brain_proposal_fingerprint(proposal));
    WikiPage {
        page_id,
        workspace_id: proposal.workspace_id.clone(),
        path: format!("wiki/save-back/{}.md", wiki_slug(&proposal.title)),
        title: proposal.title.clone(),
        body: format!(
            "# {}\n\n{}\n\n## Provenance\n\n- Sources: {}\n- Nodes: {}\n- Evidence: {}\n",
            proposal.title,
            proposal.body,
            join_or_none(&proposal.source_refs),
            join_or_none(&proposal.node_refs),
            join_or_none(&proposal.evidence_refs)
        ),
        node_refs: proposal.node_refs.clone(),
        source_refs: proposal.source_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        updated_at: proposal.created_at,
    }
}

fn resolve_persisted_wiki_page_for_proposal(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    proposal: &BrainUpdateProposal,
) -> WikiPage {
    let mut page = wiki_page_for_proposal(proposal);
    if let Some(existing) = snapshot
        .wiki_pages
        .iter()
        .find(|existing| existing.page_id == page.page_id)
    {
        page.path = existing.path.clone();
        return page;
    }
    if let Some(existing_path) = persisted_wiki_page_path_for_id(root, &page.page_id) {
        page.path = existing_path;
        return page;
    }
    page.path = non_overwriting_wiki_page_path(root, snapshot, &page);
    page
}

fn persisted_wiki_page_path_for_id(root: &Path, page_id: &str) -> Option<String> {
    read_json_artifact::<BrainRepoSnapshot>(&root.join("brain-manifest.json"))
        .ok()
        .and_then(|snapshot| {
            snapshot
                .wiki_pages
                .into_iter()
                .find(|page| page.page_id == page_id)
                .map(|page| page.path)
        })
}

fn non_overwriting_wiki_page_path(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    page: &WikiPage,
) -> String {
    if wiki_page_path_is_available(root, snapshot, &page.path, &page.page_id) {
        return page.path.clone();
    }
    let fingerprint = page
        .page_id
        .strip_prefix("wiki-save-back-")
        .unwrap_or(&page.page_id);
    let suffix = &fingerprint[..fingerprint.len().min(12)];
    let mut candidate = wiki_page_path_with_suffix(&page.path, suffix);
    let mut counter = 2;
    while !wiki_page_path_is_available(root, snapshot, &candidate, &page.page_id) {
        candidate = wiki_page_path_with_suffix(&page.path, &format!("{suffix}-{counter}"));
        counter += 1;
    }
    candidate
}

fn wiki_page_path_is_available(
    root: &Path,
    snapshot: &BrainRepoSnapshot,
    path: &str,
    page_id: &str,
) -> bool {
    match snapshot.wiki_pages.iter().find(|page| page.path == path) {
        Some(existing) => existing.page_id == page_id,
        None => !root.join(path).exists(),
    }
}

fn wiki_page_path_with_suffix(path: &str, suffix: &str) -> String {
    let path = Path::new(path);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("page");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    let file_name = format!("{stem}-{suffix}{extension}");
    path.parent()
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
        .to_string_lossy()
        .to_string()
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn wiki_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            slug.push(char.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "page".into()
    } else {
        slug
    }
}

struct BrainReader {
    repo: BrainArtifactRepository,
    snapshot: BrainRepoSnapshot,
    events: Vec<BrainEvent>,
}

impl BrainReader {
    fn open(scope: &BrainReadScope) -> Result<Self> {
        let root = resolve_brain_workspace_root(scope)?;
        let repo = BrainArtifactRepository::new(root);
        let manifest_path = repo.brain_manifest_path();
        if !manifest_path.exists() {
            return Ok(Self {
                repo,
                snapshot: empty_replayed_brain_snapshot(&scope.workspace_id),
                events: Vec::new(),
            });
        }
        let mut snapshot = repo.read_brain_manifest()?;
        if snapshot.workspace_id != scope.workspace_id {
            bail!(
                "brain manifest workspace_id {} does not match requested workspace {}",
                snapshot.workspace_id,
                scope.workspace_id
            );
        }
        snapshot.nodes = read_json_artifact(&repo.root().join("graph/nodes.json"))?;
        snapshot.relations = read_json_artifact(&repo.root().join("graph/edges.json"))?;
        snapshot.evidence = read_json_artifact(&repo.root().join("graph/evidence.json"))?;
        if let Ok(entities) = read_json_artifact(&repo.root().join("graph/entities.json")) {
            snapshot.entities = entities;
        }
        if let Ok(claims) = read_json_artifact(&repo.root().join("graph/claims.json")) {
            snapshot.claims = claims;
        }
        snapshot.memories = repo.read_memory_records()?;
        let events = repo.read_brain_events()?;
        snapshot.events = events.clone();
        Ok(Self {
            repo,
            snapshot,
            events,
        })
    }

    fn root(&self) -> &Path {
        self.repo.root()
    }

    fn search(&self, query: &str, limit: usize) -> Vec<BrainSearchResult> {
        let terms = search_terms(query);
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for source in &self.snapshot.sources {
            let haystack = format!(
                "{} {} {} {}",
                source.source_id, source.original_path, source.source_path, source.markdown_path
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Source,
                    id: source.source_id.clone(),
                    title: source.source_id.clone(),
                    path: Some(source.source_path.clone()),
                    score,
                    snippet: source.original_path.clone(),
                });
            }
        }
        for node in &self.snapshot.nodes {
            let haystack = format!("{} {} {}", node.node_id, node.label, node.aliases.join(" "));
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Node,
                    id: node.node_id.clone(),
                    title: node.label.clone(),
                    path: None,
                    score,
                    snippet: evidence_snippet(&node.evidence_ids),
                });
            }
        }
        for entity in &self.snapshot.entities {
            let haystack = format!(
                "{} {:?} {} {} {}",
                entity.entity_id,
                entity.kind,
                entity.name,
                entity.aliases.join(" "),
                entity.source_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Entity,
                    id: entity.entity_id.clone(),
                    title: entity.name.clone(),
                    path: None,
                    score,
                    snippet: evidence_snippet(&entity.evidence_refs),
                });
            }
        }
        for claim in &self.snapshot.claims {
            let haystack = format!(
                "{} {} {} {} {}",
                claim.claim_id,
                claim.statement,
                claim.topic_refs.join(" "),
                claim.source_refs.join(" "),
                claim.evidence_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Claim,
                    id: claim.claim_id.clone(),
                    title: claim.statement.clone(),
                    path: Some("graph/claims.json".into()),
                    score,
                    snippet: format!(
                        "{}; {}",
                        evidence_snippet(&claim.evidence_refs),
                        best_snippet(&claim.statement, &terms)
                    ),
                });
            }
        }
        for relation in &self.snapshot.relations {
            let haystack = format!(
                "{} {:?} {} {} {} {}",
                relation.relation_id,
                relation.kind,
                relation.source_node_id,
                relation.target_node_id,
                relation.label,
                relation.evidence_ids.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Relation,
                    id: relation.relation_id.clone(),
                    title: relation.label.clone(),
                    path: Some("graph/edges.json".into()),
                    score,
                    snippet: format!(
                        "{:?}: {} -> {}; {}",
                        relation.kind,
                        relation.source_node_id,
                        relation.target_node_id,
                        evidence_snippet(&relation.evidence_ids)
                    ),
                });
            }
        }
        for memory in &self.snapshot.memories {
            let haystack = format!(
                "{} {} {} {} {}",
                memory.memory_id,
                memory.title,
                memory.body,
                memory.source_refs.join(" "),
                memory.evidence_refs.join(" ")
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Memory,
                    id: memory.memory_id.clone(),
                    title: memory.title.clone(),
                    path: Some("memory/records.json".into()),
                    score,
                    snippet: format!(
                        "{}; {}",
                        evidence_snippet(&memory.evidence_refs),
                        best_snippet(&memory.body, &terms)
                    ),
                });
            }
        }
        for page in &self.snapshot.wiki_pages {
            let page = self
                .read_wiki_page_body(page.clone())
                .unwrap_or_else(|_| page.clone());
            let haystack = format!("{} {} {}", page.path, page.title, page.body);
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::WikiPage,
                    id: page.page_id,
                    title: page.title,
                    path: Some(page.path),
                    score,
                    snippet: best_snippet(&page.body, &terms),
                });
            }
        }
        for evidence in &self.snapshot.evidence {
            let haystack = format!(
                "{} {} {}",
                evidence.id, evidence.page_label, evidence.snippet
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Evidence,
                    id: evidence.id.clone(),
                    title: evidence.page_label.clone(),
                    path: evidence.markdown_path.clone(),
                    score,
                    snippet: best_snippet(&evidence.snippet, &terms),
                });
            }
        }
        for event in &self.events {
            let haystack = format!(
                "{} {:?} {} {}",
                event.event_id, event.event_type, event.policy_result, event.payload_json
            );
            if let Some(score) = match_score(&terms, &haystack) {
                results.push(BrainSearchResult {
                    kind: BrainSearchResultKind::Event,
                    id: event.event_id.clone(),
                    title: format!("{:?}", event.event_type),
                    path: Some("events/brain_events.jsonl".into()),
                    score,
                    snippet: event.payload_json.clone(),
                });
            }
        }
        results.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.title.cmp(&right.title))
        });
        results.truncate(limit);
        results
    }

    fn read_wiki_page(&self, path: &str) -> Result<WikiPage> {
        let normalized_path = normalize_wiki_path(path)?;
        let page = self
            .snapshot
            .wiki_pages
            .iter()
            .find(|page| page.path == normalized_path)
            .cloned()
            .ok_or_else(|| anyhow!("wiki page {normalized_path} was not found"))?;
        self.read_wiki_page_body(page)
    }

    fn read_wiki_page_body(&self, mut page: WikiPage) -> Result<WikiPage> {
        let path = self.repo.root().join(&page.path);
        page.body = fs::read_to_string(&path)
            .with_context(|| format!("failed reading {}", path.display()))?;
        Ok(page)
    }

    fn read_all_wiki_pages(&self) -> Result<Vec<WikiPage>> {
        self.snapshot
            .wiki_pages
            .iter()
            .cloned()
            .map(|page| self.read_wiki_page_body(page))
            .collect()
    }

    fn recent_events(&self, request: &ReadRecentEventsRequest) -> Vec<BrainEvent> {
        let mut events = self
            .events
            .iter()
            .filter(|event| event_matches_recent_events_request(event, request))
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.event_id.cmp(&left.event_id))
        });
        events.truncate(request.limit.unwrap_or(20));
        events
    }

    fn context_pack(&self, query: &str, budget: usize) -> Result<BrainContextPack> {
        let results = self.search(query, 24);
        let mut page_paths = BTreeSet::new();
        let mut node_ids = BTreeSet::new();
        let mut entity_ids = BTreeSet::new();
        let mut claim_ids = BTreeSet::new();
        let mut relation_ids = BTreeSet::new();
        let mut source_ids = BTreeSet::new();
        let mut evidence_ids = BTreeSet::new();
        let mut memory_ids = BTreeSet::new();
        for result in &results {
            match result.kind {
                BrainSearchResultKind::WikiPage => {
                    if let Some(path) = &result.path {
                        page_paths.insert(path.clone());
                    }
                }
                BrainSearchResultKind::Node => {
                    node_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Entity => {
                    entity_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Claim => {
                    claim_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Relation => {
                    relation_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Source => {
                    source_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Memory => {
                    memory_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Evidence => {
                    evidence_ids.insert(result.id.clone());
                }
                BrainSearchResultKind::Event => {
                    if let Some(event) =
                        self.events.iter().find(|event| event.event_id == result.id)
                    {
                        source_ids.extend(event.source_refs.iter().cloned());
                        node_ids.extend(event.node_refs.iter().cloned());
                        relation_ids.extend(event.relation_refs.iter().cloned());
                        evidence_ids.extend(event.evidence_refs.iter().cloned());
                    }
                }
            }
        }

        for evidence in &self.snapshot.evidence {
            if evidence_ids.contains(&evidence.id) {
                if let Some(source_id) = &evidence.source_id {
                    source_ids.insert(source_id.clone());
                }
            }
            if evidence
                .source_id
                .as_ref()
                .is_some_and(|source_id| source_ids.contains(source_id))
            {
                evidence_ids.insert(evidence.id.clone());
            }
        }
        for node in &self.snapshot.nodes {
            if node_ids.contains(&node.node_id)
                || node
                    .source_ids
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || node
                    .evidence_ids
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                node_ids.insert(node.node_id.clone());
                source_ids.extend(node.source_ids.iter().cloned());
                evidence_ids.extend(node.evidence_ids.iter().cloned());
            }
        }
        for entity in &self.snapshot.entities {
            if entity_ids.contains(&entity.entity_id)
                || entity
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || entity
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                entity_ids.insert(entity.entity_id.clone());
                node_ids.insert(entity.entity_id.clone());
                source_ids.extend(entity.source_refs.iter().cloned());
                evidence_ids.extend(entity.evidence_refs.iter().cloned());
            }
        }
        for claim in &self.snapshot.claims {
            if claim_ids.contains(&claim.claim_id)
                || claim
                    .topic_refs
                    .iter()
                    .any(|node_id| node_ids.contains(node_id))
                || claim
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || claim
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                claim_ids.insert(claim.claim_id.clone());
                node_ids.extend(claim.topic_refs.iter().cloned());
                source_ids.extend(claim.source_refs.iter().cloned());
                evidence_ids.extend(claim.evidence_refs.iter().cloned());
            }
        }
        for relation in &self.snapshot.relations {
            if relation_ids.contains(&relation.relation_id)
                || node_ids.contains(&relation.source_node_id)
                || node_ids.contains(&relation.target_node_id)
                || relation
                    .evidence_ids
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                relation_ids.insert(relation.relation_id.clone());
                node_ids.insert(relation.source_node_id.clone());
                node_ids.insert(relation.target_node_id.clone());
                evidence_ids.extend(relation.evidence_ids.iter().cloned());
            }
        }
        for node in &self.snapshot.nodes {
            if node_ids.contains(&node.node_id) {
                source_ids.extend(node.source_ids.iter().cloned());
                evidence_ids.extend(node.evidence_ids.iter().cloned());
            }
        }

        let entities = self
            .snapshot
            .entities
            .iter()
            .filter(|entity| entity_ids.contains(&entity.entity_id))
            .cloned()
            .collect::<Vec<_>>();
        let claims = self
            .snapshot
            .claims
            .iter()
            .filter(|claim| claim_ids.contains(&claim.claim_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut nodes = self
            .snapshot
            .nodes
            .iter()
            .filter(|node| node_ids.contains(&node.node_id))
            .cloned()
            .collect::<Vec<_>>();
        let selected_node_ids = nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<BTreeSet<_>>();
        let relations = self
            .snapshot
            .relations
            .iter()
            .filter(|relation| {
                relation_ids.contains(&relation.relation_id)
                    || selected_node_ids.contains(&relation.source_node_id)
                    || selected_node_ids.contains(&relation.target_node_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let memory_terms = search_terms(query);
        let memories = self
            .snapshot
            .memories
            .iter()
            .filter(|memory| {
                let haystack = format!("{} {}", memory.title, memory.body);
                memory_ids.contains(&memory.memory_id)
                    || (!memory_terms.is_empty() && match_score(&memory_terms, &haystack).is_some())
            })
            .cloned()
            .collect::<Vec<_>>();
        for memory in &memories {
            source_ids.extend(memory.source_refs.iter().cloned());
            evidence_ids.extend(memory.evidence_refs.iter().cloned());
        }
        let sources = self
            .snapshot
            .sources
            .iter()
            .filter(|source| source_ids.contains(&source.source_id))
            .cloned()
            .collect::<Vec<_>>();
        let evidence = self
            .snapshot
            .evidence
            .iter()
            .filter(|evidence| evidence_ids.contains(&evidence.id))
            .cloned()
            .collect::<Vec<_>>();

        let mut wiki_pages = Vec::new();
        for page in &self.snapshot.wiki_pages {
            if page
                .node_refs
                .iter()
                .any(|node_id| node_ids.contains(node_id))
                || page
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || page
                    .evidence_refs
                    .iter()
                    .any(|evidence_id| evidence_ids.contains(evidence_id))
            {
                page_paths.insert(page.path.clone());
            }
        }
        for path in page_paths {
            if let Ok(page) = self.read_wiki_page(&path) {
                wiki_pages.push(page);
            }
        }
        if wiki_pages.is_empty() {
            for path in ["wiki/index.md", "wiki/overview.md"] {
                if let Ok(page) = self.read_wiki_page(path) {
                    wiki_pages.push(page);
                }
            }
        }

        let warnings = context_pack_warnings(&nodes, &evidence, budget);
        trim_context_pack_to_budget(budget, &mut wiki_pages, &mut nodes);
        let recent_events = self.recent_events(&ReadRecentEventsRequest {
            scope: BrainReadScope {
                workspace_id: self.snapshot.workspace_id.clone(),
                root_dir: Some(self.repo.root().display().to_string()),
            },
            limit: Some(5),
            run_id: None,
            source_ref: None,
            node_id: None,
            edge_id: None,
            claim_id: None,
            memory_id: None,
            change_type: None,
        });
        Ok(BrainContextPack {
            workspace_id: self.snapshot.workspace_id.clone(),
            query: query.to_string(),
            token_budget: budget,
            summary: format!(
                "Context pack for \"{}\" with {} wiki pages, {} nodes, {} entities, {} claims, {} relations, {} sources, {} memories, {} evidence refs, and {} recent events.",
                query,
                wiki_pages.len(),
                nodes.len(),
                entities.len(),
                claims.len(),
                relations.len(),
                sources.len(),
                memories.len(),
                evidence.len(),
                recent_events.len()
            ),
            wiki_pages,
            nodes,
            sources,
            memories,
            entities,
            claims,
            relations,
            evidence,
            recent_events,
            warnings,
        })
    }
}

fn resolve_brain_workspace_root(scope: &BrainReadScope) -> Result<PathBuf> {
    if let Some(root_dir) = &scope.root_dir {
        return Ok(PathBuf::from(root_dir).join(&scope.workspace_id));
    }
    if let Some(output_root) = std::env::var_os("HYPRDUCK_OUTPUT_DIR") {
        return Ok(PathBuf::from(output_root).join(&scope.workspace_id));
    }
    if let Some(application_support_root) = dirs::data_local_dir() {
        return Ok(application_support_root
            .join("HyprDuck")
            .join(&scope.workspace_id));
    }
    Ok(std::env::temp_dir()
        .join("HyprDuck")
        .join(&scope.workspace_id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownIngestPaths {
    workspace_root: PathBuf,
    source_dir: PathBuf,
    wiki_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NewMarkdownSource {
    source_path: PathBuf,
    relative_path: PathBuf,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownSourceStateFile {
    generated_at: u64,
    #[serde(default)]
    sources: Vec<MarkdownSourceStateRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownSourceStateRecord {
    source_path: String,
    relative_path: String,
    normalized_path: String,
    size_bytes: u64,
    #[serde(default)]
    modified_at: Option<u64>,
    content_hash: String,
    first_seen_at: u64,
    last_seen_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownSourceScan {
    new_sources: Vec<NewMarkdownSource>,
    current_state: MarkdownSourceStateFile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownIngestQueueFile {
    generated_at: u64,
    #[serde(default)]
    records: Vec<MarkdownIngestQueueRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownIngestQueueRecord {
    queue_id: String,
    workspace_id: String,
    source_path: String,
    relative_path: String,
    normalized_path: String,
    size_bytes: u64,
    #[serde(default)]
    modified_at: Option<u64>,
    content_hash: String,
    status: String,
    #[serde(default = "default_markdown_trigger_status")]
    trigger_status: String,
    #[serde(default)]
    trigger_error_message: Option<String>,
    discovered_at: u64,
    enqueued_at: u64,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    completed_at: Option<u64>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MarkdownEnqueueResult {
    enqueued: Vec<MarkdownIngestQueueRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MarkdownIngestWorkerResult {
    started: bool,
    processed: usize,
    failed: usize,
    processed_sources: Vec<String>,
    failed_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedMarkdownSourceMetadata {
    workspace_id: String,
    queue_id: String,
    source_id: String,
    source_path: String,
    relative_path: String,
    normalized_path: String,
    markdown_path: String,
    manifest_path: String,
    artifact_root: String,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
    discovered_at: u64,
    enqueued_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedMarkdownIngest {
    record: MarkdownIngestQueueRecord,
    manifest: SourceArtifactManifest,
    source_metadata: CompletedMarkdownSourceMetadata,
}

impl CompletedMarkdownIngest {
    fn new(record: &MarkdownIngestQueueRecord, manifest: &SourceArtifactManifest) -> Self {
        Self {
            record: record.clone(),
            manifest: manifest.clone(),
            source_metadata: CompletedMarkdownSourceMetadata {
                workspace_id: record.workspace_id.clone(),
                queue_id: record.queue_id.clone(),
                source_id: manifest.source_id.clone(),
                source_path: record.source_path.clone(),
                relative_path: record.relative_path.clone(),
                normalized_path: record.normalized_path.clone(),
                markdown_path: manifest.markdown_path.clone(),
                manifest_path: manifest.manifest_path.clone(),
                artifact_root: manifest.artifact_root.clone(),
                size_bytes: record.size_bytes,
                modified_at: record.modified_at,
                content_hash: record.content_hash.clone(),
                discovered_at: record.discovered_at,
                enqueued_at: record.enqueued_at,
                started_at: record.started_at,
                completed_at: record.completed_at,
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct QueuedAgentProposalApplyResult {
    applied: Vec<String>,
    failed: Vec<AgentProposalFailureReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentProposalFailureReport {
    proposal_id: String,
    run_id: String,
    snapshot_id: String,
    error_code: String,
    error_message: String,
    #[serde(default)]
    validation_issues: Vec<AgentGraphProposalValidationIssue>,
    audit_path: String,
}

impl std::fmt::Display for AgentProposalFailureReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}: {}",
            self.proposal_id, self.error_code, self.error_message
        )
    }
}

impl std::error::Error for AgentProposalFailureReport {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentProposalApplyAudit {
    run_id: String,
    snapshot_id: String,
    workspace_id: String,
    proposal_id: String,
    status: String,
    started_at: u64,
    completed_at: u64,
    #[serde(default)]
    changed_files: Vec<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    validation_issues: Vec<AgentGraphProposalValidationIssue>,
    #[serde(default)]
    rollback_hint: String,
}

#[derive(Debug, Clone, Default)]
struct MaterializedFileSnapshot {
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunDiff {
    run_id: String,
    workspace_id: String,
    proposal_id: String,
    status: String,
    changed_files: Vec<String>,
    created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunValidationReport {
    run_id: String,
    workspace_id: String,
    proposal_id: String,
    status: String,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    validation_issues: Vec<AgentGraphProposalValidationIssue>,
    created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatestReadableGraphSnapshotMarker {
    schema_version: u32,
    workspace_id: String,
    snapshot_id: String,
    event_id: String,
    source_ingest_id: String,
    materialized_at: u64,
    published_at: u64,
    #[serde(default)]
    source_markdown_refs: Vec<String>,
    #[serde(default)]
    materialized_files: Vec<String>,
}

fn default_markdown_trigger_status() -> String {
    "accepted".into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownSourceFileState {
    source_path: PathBuf,
    relative_path: PathBuf,
    normalized_absolute_path: String,
    normalized_relative_path: String,
    size_bytes: u64,
    modified_at: Option<u64>,
    content_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrainWorkspaceConfig {
    #[serde(default, alias = "markdown_sources_dir")]
    markdown_sources_dir: Option<String>,
    #[serde(default, alias = "wiki_dir")]
    wiki_dir: Option<String>,
}

fn resolve_markdown_ingest_paths(scope: &BrainReadScope) -> Result<MarkdownIngestPaths> {
    let workspace_root = resolve_brain_workspace_root(scope)?;
    let config = read_brain_workspace_config(&workspace_root)?;
    let configured_source_dir = config.markdown_sources_dir.as_deref();
    let source_dir =
        resolve_workspace_config_path(&workspace_root, configured_source_dir, "sources")?;
    let wiki_dir =
        resolve_workspace_config_path(&workspace_root, config.wiki_dir.as_deref(), "wiki")?;

    if configured_source_dir.is_some() && !source_dir.is_dir() {
        bail!(
            "configured markdown source directory {} does not exist or is not a directory",
            source_dir.display()
        );
    }
    if configured_source_dir.is_none() {
        fs::create_dir_all(&source_dir).with_context(|| {
            format!(
                "failed creating markdown source directory {}",
                source_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&wiki_dir)
        .with_context(|| format!("failed creating wiki directory {}", wiki_dir.display()))?;

    Ok(MarkdownIngestPaths {
        workspace_root,
        source_dir,
        wiki_dir,
    })
}

fn scan_new_markdown_sources(
    paths: &MarkdownIngestPaths,
    snapshot: &BrainRepoSnapshot,
    source_state: &MarkdownSourceStateFile,
    ingest_queue: &MarkdownIngestQueueFile,
) -> Result<MarkdownSourceScan> {
    let mut known_paths = BTreeSet::new();
    for source in &snapshot.sources {
        for raw_path in [
            source.original_path.as_str(),
            source.source_path.as_str(),
            source.markdown_path.as_str(),
        ] {
            if !raw_path.trim().is_empty() {
                known_paths.insert(normalize_ingest_path_for_compare(raw_path));
            }
        }
    }
    let previous_state_by_path = source_state_by_path(source_state);
    for record in previous_state_by_path.values() {
        known_paths.insert(record.normalized_path.clone());
        known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
        known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
    }
    for record in &ingest_queue.records {
        known_paths.insert(record.normalized_path.clone());
        known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
        known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
    }
    for event in &snapshot.events {
        if event.event_type != BrainEventKind::SourceIngestQueued {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<MarkdownIngestQueueRecord>(&event.payload_json) {
            known_paths.insert(record.normalized_path);
            known_paths.insert(normalize_ingest_path_for_compare(&record.relative_path));
            known_paths.insert(normalize_ingest_path_for_compare(&record.source_path));
        }
    }

    let mut file_states = Vec::new();
    collect_markdown_source_file_states(&paths.source_dir, &paths.source_dir, &mut file_states)?;
    file_states.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let mut new_sources = file_states
        .iter()
        .filter(|file| {
            !known_paths.contains(&file.normalized_absolute_path)
                && !known_paths.contains(&file.normalized_relative_path)
        })
        .map(|file| NewMarkdownSource {
            source_path: file.source_path.clone(),
            relative_path: file.relative_path.clone(),
            size_bytes: file.size_bytes,
            modified_at: file.modified_at,
            content_hash: file.content_hash.clone(),
        })
        .collect::<Vec<_>>();
    new_sources.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let generated_at = unix_timestamp_seconds();
    let current_state = MarkdownSourceStateFile {
        generated_at,
        sources: file_states
            .into_iter()
            .map(|file| {
                let previous = previous_state_by_path
                    .get(&file.normalized_relative_path)
                    .or_else(|| previous_state_by_path.get(&file.normalized_absolute_path));
                MarkdownSourceStateRecord {
                    source_path: file.source_path.display().to_string(),
                    relative_path: file.relative_path.display().to_string(),
                    normalized_path: file.normalized_relative_path,
                    size_bytes: file.size_bytes,
                    modified_at: file.modified_at,
                    content_hash: file.content_hash,
                    first_seen_at: previous
                        .map(|record| record.first_seen_at)
                        .unwrap_or(generated_at),
                    last_seen_at: generated_at,
                }
            })
            .collect(),
    };

    Ok(MarkdownSourceScan {
        new_sources,
        current_state,
    })
}

fn enqueue_markdown_sources(
    writer: &BrainWorkspaceWriter,
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    scan: &MarkdownSourceScan,
) -> Result<MarkdownEnqueueResult> {
    if scan.new_sources.is_empty() {
        write_markdown_ingest_queue(paths, ingest_queue)?;
        return Ok(MarkdownEnqueueResult::default());
    }

    let mut records = ingest_queue.records.clone();
    let mut queued_keys = records
        .iter()
        .map(markdown_queue_dedupe_key)
        .collect::<BTreeSet<_>>();
    let mut enqueued = Vec::new();
    let enqueued_at = unix_timestamp_seconds();

    for source in &scan.new_sources {
        let normalized_path =
            normalize_ingest_path_for_compare(&source.relative_path.display().to_string());
        let key = format!("{}:{}", normalized_path, source.content_hash);
        if queued_keys.contains(&key) {
            continue;
        }
        let record = MarkdownIngestQueueRecord {
            queue_id: format!("markdown-source-{}", sanitize_name(&key)),
            workspace_id: paths
                .workspace_root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.into()),
            source_path: source.source_path.display().to_string(),
            relative_path: source.relative_path.display().to_string(),
            normalized_path,
            size_bytes: source.size_bytes,
            modified_at: source.modified_at,
            content_hash: source.content_hash.clone(),
            status: "queued".into(),
            trigger_status: "accepted".into(),
            trigger_error_message: None,
            discovered_at: enqueued_at,
            enqueued_at,
            started_at: None,
            completed_at: None,
            error_message: None,
        };
        writer.append_event(&markdown_source_queued_event(
            &record.workspace_id,
            &record,
        )?)?;
        queued_keys.insert(markdown_queue_dedupe_key(&record));
        records.push(record.clone());
        enqueued.push(record);
    }

    records.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then_with(|| left.queue_id.cmp(&right.queue_id))
    });
    write_markdown_ingest_queue(
        paths,
        &MarkdownIngestQueueFile {
            generated_at: enqueued_at,
            records,
        },
    )?;

    Ok(MarkdownEnqueueResult { enqueued })
}

fn run_markdown_ingest_worker(
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    store: &KnowledgeProjectStore,
) -> Result<MarkdownIngestWorkerResult> {
    run_markdown_ingest_worker_with_post_ingest_hook(paths, ingest_queue, store, &mut |_| Ok(()))
}

fn run_markdown_ingest_worker_with_post_ingest_hook(
    paths: &MarkdownIngestPaths,
    ingest_queue: &MarkdownIngestQueueFile,
    store: &KnowledgeProjectStore,
    post_ingest_hook: &mut dyn FnMut(&CompletedMarkdownIngest) -> Result<()>,
) -> Result<MarkdownIngestWorkerResult> {
    let queued_indexes = ingest_queue
        .records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| (record.status == "queued").then_some(index))
        .collect::<Vec<_>>();
    if queued_indexes.is_empty() {
        return Ok(MarkdownIngestWorkerResult::default());
    }

    let mut queue = ingest_queue.clone();
    let mut result = MarkdownIngestWorkerResult {
        started: true,
        ..MarkdownIngestWorkerResult::default()
    };

    for index in queued_indexes {
        let started_at = unix_timestamp_seconds();
        queue.records[index].status = "ingesting".into();
        queue.records[index].started_at = Some(started_at);
        queue.records[index].error_message = None;
        write_markdown_ingest_queue(paths, &queue)?;

        let compile_result = compile_queued_markdown_source(paths, &queue.records[index], store);
        let completed_at = unix_timestamp_seconds();
        queue.records[index].completed_at = Some(completed_at);
        match compile_result {
            Ok(manifest) => {
                queue.records[index].status = "ingested".into();
                queue.records[index].error_message = None;
                result.processed += 1;
                result
                    .processed_sources
                    .push(queue.records[index].relative_path.clone());
                let completed_ingest =
                    CompletedMarkdownIngest::new(&queue.records[index], &manifest);
                post_ingest_hook(&completed_ingest).with_context(|| {
                    format!(
                        "post-ingest hook failed for markdown source {}",
                        queue.records[index].relative_path
                    )
                })?;
                let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
                writer.append_event(&markdown_source_compiled_event(
                    &queue.records[index],
                    Some(&manifest),
                )?)?;
            }
            Err(error) => {
                queue.records[index].status = "failed".into();
                queue.records[index].error_message = Some(error.to_string());
                result.failed += 1;
                result
                    .failed_sources
                    .push(queue.records[index].relative_path.clone());
                let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
                writer.append_event(&markdown_source_compiled_event(
                    &queue.records[index],
                    None,
                )?)?;
            }
        }
        write_markdown_ingest_queue(paths, &queue)?;
    }

    Ok(result)
}

fn run_queued_agent_proposal_apply_worker(
    root: &Path,
    workspace_id: &str,
) -> Result<QueuedAgentProposalApplyResult> {
    let writer = BrainWorkspaceWriter::open(root.to_path_buf())?;
    let mut result = QueuedAgentProposalApplyResult::default();
    let mut proposals = read_brain_update_proposals(root)?
        .into_iter()
        .map(|(proposal, _)| proposal)
        .filter(|proposal| is_queued_agent_graph_proposal(proposal, workspace_id))
        .collect::<Vec<_>>();
    proposals.sort_by(|left, right| {
        brain_proposal_replay_priority(left.kind)
            .cmp(&brain_proposal_replay_priority(right.kind))
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.proposal_id.cmp(&right.proposal_id))
    });

    for proposal in proposals {
        match apply_queued_agent_proposal_transaction(&writer, proposal) {
            Ok(audit) => {
                result.applied.push(audit.proposal_id);
            }
            Err(error) => {
                if let Some(failure) = error.downcast_ref::<AgentProposalFailureReport>() {
                    result.failed.push(failure.clone());
                } else {
                    result.failed.push(AgentProposalFailureReport {
                        proposal_id: proposal_id_from_apply_error(&error)
                            .unwrap_or_else(|| "unknown-proposal".into()),
                        run_id: "unknown-run".into(),
                        snapshot_id: "unknown-snapshot".into(),
                        error_code: "apply_error".into(),
                        error_message: format!("{error:#}"),
                        validation_issues: Vec::new(),
                        audit_path: String::new(),
                    });
                }
            }
        }
    }

    Ok(result)
}

fn is_queued_agent_graph_proposal(proposal: &BrainUpdateProposal, workspace_id: &str) -> bool {
    proposal.workspace_id == workspace_id
        && proposal.status == BrainProposalStatus::PendingReview
        && proposal.actor.actor_type == BrainActorType::Agent
        && matches!(
            proposal.kind,
            BrainProposalKind::Node
                | BrainProposalKind::Claim
                | BrainProposalKind::Link
                | BrainProposalKind::Memory
        )
        && proposal.proposal_payload.is_some()
}

fn apply_queued_agent_proposal_transaction(
    writer: &BrainWorkspaceWriter,
    mut proposal: BrainUpdateProposal,
) -> Result<AgentProposalApplyAudit> {
    let run_id = format!("apply-{}", Uuid::now_v7());
    let snapshot_id = format!("snapshot-{}", Uuid::now_v7());
    let started_at = unix_timestamp_seconds();
    let before = capture_materialized_file_snapshot(writer.root())?;
    persist_materialized_snapshot(writer.root(), &snapshot_id, &before)?;
    persist_agent_run_snapshot(writer.root(), &run_id, "before", &before)?;
    write_json_pretty(
        &writer
            .root()
            .join("runs")
            .join(&run_id)
            .join("provider-response.json"),
        &queued_proposal_provider_response_value(
            &run_id,
            &proposal.workspace_id,
            &proposal.proposal_id,
            &proposal.actor,
            proposal.proposal_payload.as_ref(),
            started_at,
        ),
    )?;

    let apply_result = (|| -> Result<()> {
        enrich_agent_graph_proposal_refs(&mut proposal);
        validate_queued_agent_proposal(&proposal)?;
        proposal.status = BrainProposalStatus::Accepted;
        writer.write_proposal(&proposal)?;
        let mut proposed_event = brain_event_for_proposal(&proposal)?;
        proposed_event.policy_result = "auto_applied".into();
        writer.append_event(&proposed_event)?;
        match proposal.kind {
            BrainProposalKind::Memory => {
                let memory = memory_record_for_proposal(&proposal);
                writer.upsert_memory_record(memory)?;
                writer.append_event(&brain_memory_accepted_event(&proposal)?)?;
            }
            BrainProposalKind::Node | BrainProposalKind::Claim | BrainProposalKind::Link => {
                writer.apply_accepted_proposal(&proposal)?;
            }
            BrainProposalKind::Observation
            | BrainProposalKind::SourceNote
            | BrainProposalKind::WikiPage => {}
        }
        writer.append_event(&queued_agent_proposal_applied_event(
            &proposal,
            &run_id,
            &snapshot_id,
        )?)?;
        Ok(())
    })();

    match apply_result {
        Ok(()) => {
            let after = capture_materialized_file_snapshot(writer.root())?;
            let changed_files = changed_materialized_files(&before, &after);
            persist_agent_run_snapshot(writer.root(), &run_id, "after", &after)?;
            let completed_at = unix_timestamp_seconds();
            let audit_path = writer
                .root()
                .join("reviews/applied-runs")
                .join(format!("{run_id}.json"));
            let audit = AgentProposalApplyAudit {
                run_id: run_id.clone(),
                snapshot_id,
                workspace_id: proposal.workspace_id.clone(),
                proposal_id: proposal.proposal_id.clone(),
                status: "applied".into(),
                started_at,
                completed_at,
                changed_files: changed_files.clone(),
                error_message: None,
                error_code: None,
                validation_issues: Vec::new(),
                rollback_hint: "Restore files from snapshots/<snapshotId>/files or replay accepted events/proposals by rematerializing the workspace brain repo.".into(),
            };
            write_json_pretty(&audit_path, &audit)?;
            write_agent_run_diff(
                writer.root(),
                &run_id,
                &proposal.workspace_id,
                &proposal.proposal_id,
                "applied",
                &changed_files,
            )?;
            write_agent_run_validation_report(
                writer.root(),
                AgentRunValidationReport {
                    run_id: run_id.clone(),
                    workspace_id: proposal.workspace_id.clone(),
                    proposal_id: proposal.proposal_id.clone(),
                    status: "applied".into(),
                    error_code: None,
                    error_message: None,
                    validation_issues: Vec::new(),
                    created_at: completed_at,
                },
            )?;
            Ok(audit)
        }
        Err(error) => {
            restore_materialized_file_snapshot(writer.root(), &before)?;
            let completed_at = unix_timestamp_seconds();
            let validation_error = error.downcast_ref::<AgentGraphProposalValidationError>();
            let error_code = validation_error
                .map(|error| error.error.clone())
                .unwrap_or_else(|| "agent_proposal_apply_failed".into());
            let validation_issues = validation_error
                .map(|error| error.issues.clone())
                .unwrap_or_default();
            let audit_path = writer
                .root()
                .join("reviews/applied-runs")
                .join(format!("{run_id}.json"));
            let audit = AgentProposalApplyAudit {
                run_id: run_id.clone(),
                snapshot_id,
                workspace_id: proposal.workspace_id.clone(),
                proposal_id: proposal.proposal_id.clone(),
                status: "failed".into(),
                started_at,
                completed_at,
                changed_files: Vec::new(),
                error_message: Some(error.to_string()),
                error_code: Some(error_code.clone()),
                validation_issues: validation_issues.clone(),
                rollback_hint: "No graph mutation was kept; the pre-apply snapshot was restored."
                    .into(),
            };
            write_json_pretty(&audit_path, &audit)?;
            write_agent_run_diff(
                writer.root(),
                &run_id,
                &proposal.workspace_id,
                &proposal.proposal_id,
                "failed",
                &[],
            )?;
            write_agent_run_validation_report(
                writer.root(),
                AgentRunValidationReport {
                    run_id: run_id.clone(),
                    workspace_id: proposal.workspace_id.clone(),
                    proposal_id: proposal.proposal_id.clone(),
                    status: "failed".into(),
                    error_code: Some(error_code.clone()),
                    error_message: Some(error.to_string()),
                    validation_issues: validation_issues.clone(),
                    created_at: completed_at,
                },
            )?;
            proposal.status = BrainProposalStatus::Rejected;
            writer.write_proposal(&proposal)?;
            writer.append_event(&queued_agent_proposal_failed_event(
                &proposal,
                &audit,
                &error_code,
                &validation_issues,
            )?)?;
            Err(anyhow!(AgentProposalFailureReport {
                proposal_id: proposal.proposal_id.clone(),
                run_id: audit.run_id.clone(),
                snapshot_id: audit.snapshot_id.clone(),
                error_code,
                error_message: error.to_string(),
                validation_issues,
                audit_path: audit_path.display().to_string(),
            }))
        }
    }
}

fn enrich_agent_graph_proposal_refs(proposal: &mut BrainUpdateProposal) {
    let Some(payload) = &proposal.proposal_payload else {
        return;
    };
    match payload {
        AgentGraphProposalPayload::NewNode { node } => {
            for source_ref in &node.source_refs {
                merge_unique_string(&mut proposal.source_refs, source_ref);
            }
            for evidence_ref in &node.evidence_refs {
                merge_unique_string(&mut proposal.evidence_refs, evidence_ref);
            }
        }
        AgentGraphProposalPayload::NewEdge { edge } => {
            let source_node_id = edge.source_node_id.trim();
            if !source_node_id.is_empty() {
                proposal
                    .node_refs
                    .retain(|node_id| node_id != source_node_id);
                proposal.node_refs.insert(0, source_node_id.to_string());
            }
            merge_unique_string(&mut proposal.node_refs, edge.target_node_id.trim());
            for source_ref in &edge.source_refs {
                merge_unique_string(&mut proposal.source_refs, source_ref);
            }
            for evidence_ref in &edge.evidence_refs {
                merge_unique_string(&mut proposal.evidence_refs, evidence_ref);
            }
            if proposal.target_node_id.is_none() {
                proposal.target_node_id = Some(edge.target_node_id.trim().to_string());
            }
            if proposal.relation_kind.is_none() {
                proposal.relation_kind = Some(edge.kind);
            }
        }
        AgentGraphProposalPayload::NewClaim { claim } => {
            for topic_ref in &claim.topic_refs {
                merge_unique_string(&mut proposal.node_refs, topic_ref);
            }
            for source_ref in &claim.source_refs {
                merge_unique_string(&mut proposal.source_refs, source_ref);
            }
            for evidence_ref in &claim.evidence_refs {
                merge_unique_string(&mut proposal.evidence_refs, evidence_ref);
            }
        }
        AgentGraphProposalPayload::NewMemory { memory } => {
            for source_ref in &memory.source_refs {
                merge_unique_string(&mut proposal.source_refs, source_ref);
            }
            for evidence_ref in &memory.evidence_refs {
                merge_unique_string(&mut proposal.evidence_refs, evidence_ref);
            }
        }
    }
}

fn validate_queued_agent_proposal(proposal: &BrainUpdateProposal) -> Result<()> {
    validate_brain_update_proposal(&ProposeBrainUpdateRequest {
        scope: BrainReadScope {
            workspace_id: proposal.workspace_id.clone(),
            root_dir: None,
        },
        kind: proposal.kind,
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        actor: proposal.actor.clone(),
        target_node_id: proposal.target_node_id.clone(),
        target_source_id: proposal.target_source_id.clone(),
        relation_kind: proposal.relation_kind,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        proposal_payload: proposal.proposal_payload.clone(),
    })
}

fn queued_agent_proposal_applied_event(
    proposal: &BrainUpdateProposal,
    run_id: &str,
    snapshot_id: &str,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewResolved,
        operation_type: Some("queued_proposal_auto_accept".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-apply".into(),
        },
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs: if proposal.kind == BrainProposalKind::Link {
            vec![relation_record_for_proposal(proposal)?.relation_id]
        } else {
            Vec::new()
        },
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "proposalId": proposal.proposal_id,
            "decision": "auto_accept",
            "runId": run_id,
            "snapshotId": snapshot_id,
            "status": proposal.status,
        }))
        .context("failed to encode queued proposal apply event payload")?,
        causality: BrainEventCausality {
            caused_by_proposal_id: Some(proposal.proposal_id.clone()),
            caused_by_source_ids: proposal.source_refs.clone(),
            snapshot_id: Some(snapshot_id.to_string()),
            materialized_version: Some(proposal.created_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "auto_applied".into(),
        created_at: unix_timestamp_seconds(),
    })
}

fn queued_agent_proposal_failed_event(
    proposal: &BrainUpdateProposal,
    audit: &AgentProposalApplyAudit,
    error_code: &str,
    validation_issues: &[AgentGraphProposalValidationIssue],
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewResolved,
        operation_type: Some("queued_proposal_auto_reject".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-agent-apply".into(),
        },
        source_refs: proposal.source_refs.clone(),
        source_markdown_refs: proposal_source_markdown_refs(proposal),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        claim_refs: proposal_target_claim_ids(proposal),
        memory_refs: proposal_target_memory_ids(proposal),
        target_node_ids: proposal_target_node_ids(proposal)?,
        target_edge_ids: proposal_target_edge_ids(proposal)?,
        target_claim_ids: proposal_target_claim_ids(proposal),
        target_memory_ids: proposal_target_memory_ids(proposal),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "proposalId": proposal.proposal_id,
            "decision": "auto_reject",
            "runId": audit.run_id,
            "snapshotId": audit.snapshot_id,
            "status": proposal.status,
            "errorCode": error_code,
            "errorMessage": audit.error_message,
            "validationIssues": validation_issues,
            "auditPath": format!("reviews/applied-runs/{}.json", audit.run_id),
        }))
        .context("failed to encode queued proposal failed event payload")?,
        causality: BrainEventCausality {
            caused_by_proposal_id: Some(proposal.proposal_id.clone()),
            caused_by_source_ids: proposal.source_refs.clone(),
            snapshot_id: Some(audit.snapshot_id.clone()),
            materialized_version: Some(proposal.created_at),
            ..Default::default()
        },
        confidence: None,
        policy_result: "auto_rejected".into(),
        created_at: unix_timestamp_seconds(),
    })
}

fn proposal_id_from_apply_error(error: &anyhow::Error) -> Option<String> {
    let message = error.to_string();
    message
        .split_once(':')
        .map(|(proposal_id, _)| proposal_id.to_string())
        .filter(|proposal_id| proposal_id.starts_with("proposal-"))
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
        || normalized.starts_with("reviews/proposed-updates/")
}

fn should_skip_materialized_snapshot_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == "brain.lock"
        || normalized == BRAIN_LOCK_DIRECTORY_NAME
        || normalized.starts_with("snapshots/")
        || normalized.starts_with("runs/")
        || normalized.starts_with("reviews/applied-runs/")
        || normalized.contains("/.")
        || normalized.starts_with('.')
}

fn persist_materialized_snapshot(
    root: &Path,
    snapshot_id: &str,
    snapshot: &MaterializedFileSnapshot,
) -> Result<()> {
    let snapshot_root = root.join("snapshots").join(snapshot_id).join("files");
    for (relative_path, bytes) in &snapshot.files {
        write_file_atomic(&snapshot_root.join(relative_path), bytes)?;
    }
    write_json_pretty(
        &root
            .join("snapshots")
            .join(snapshot_id)
            .join("manifest.json"),
        &json!({
            "snapshotId": snapshot_id,
            "fileCount": snapshot.files.len(),
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

fn persist_agent_run_snapshot(
    root: &Path,
    run_id: &str,
    label: &str,
    snapshot: &MaterializedFileSnapshot,
) -> Result<()> {
    let snapshot_root = root.join("runs").join(run_id).join(label);
    for (relative_path, bytes) in &snapshot.files {
        write_file_atomic(&snapshot_root.join(relative_path), bytes)?;
    }
    write_json_pretty(
        &root.join("runs").join(run_id).join(format!("{label}.json")),
        &json!({
            "runId": run_id,
            "label": label,
            "fileCount": snapshot.files.len(),
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

fn write_agent_run_diff(
    root: &Path,
    run_id: &str,
    workspace_id: &str,
    proposal_id: &str,
    status: &str,
    changed_files: &[String],
) -> Result<()> {
    write_json_pretty(
        &root.join("runs").join(run_id).join("graph-diff.json"),
        &AgentRunDiff {
            run_id: run_id.to_string(),
            workspace_id: workspace_id.to_string(),
            proposal_id: proposal_id.to_string(),
            status: status.to_string(),
            changed_files: changed_files.to_vec(),
            created_at: unix_timestamp_seconds(),
        },
    )
}

fn write_agent_run_validation_report(root: &Path, report: AgentRunValidationReport) -> Result<()> {
    write_json_pretty(
        &root
            .join("runs")
            .join(&report.run_id)
            .join("validation-report.json"),
        &report,
    )
}

fn restore_materialized_file_snapshot(
    root: &Path,
    snapshot: &MaterializedFileSnapshot,
) -> Result<()> {
    let after = capture_materialized_file_snapshot(root)?;
    for relative_path in after.files.keys() {
        if !snapshot.files.contains_key(relative_path) {
            let path = root.join(relative_path);
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("failed removing {}", path.display()))?;
            }
        }
    }
    for (relative_path, bytes) in &snapshot.files {
        write_file_atomic(&root.join(relative_path), bytes)?;
    }
    Ok(())
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

fn compile_queued_markdown_source(
    paths: &MarkdownIngestPaths,
    record: &MarkdownIngestQueueRecord,
    store: &KnowledgeProjectStore,
) -> Result<SourceArtifactManifest> {
    let source_path = PathBuf::from(&record.source_path);
    let markdown = fs::read_to_string(&source_path).with_context(|| {
        format!(
            "failed reading queued markdown source {}",
            source_path.display()
        )
    })?;
    let source_id = build_source_id(
        &format!("{}:{}", record.normalized_path, record.content_hash),
        0,
    );
    if let Some(manifest) = read_idempotent_markdown_ingest_manifest(paths, record, &source_id)? {
        let snapshot = read_json_artifact::<BrainRepoSnapshot>(
            &paths.workspace_root.join("brain-manifest.json"),
        )?;
        let writer = BrainWorkspaceWriter::open(paths.workspace_root.clone())?;
        writer.append_event(&markdown_ingest_idempotent_noop_event(
            record, &manifest, &snapshot,
        )?)?;
        return Ok(manifest);
    }
    let safe_name = sanitize_name(
        source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("markdown-source"),
    );
    let artifact_root = paths.workspace_root.join("artifacts").join(&source_id);
    let pages_dir = artifact_root.join("pages");
    fs::create_dir_all(&pages_dir)
        .with_context(|| format!("failed creating {}", pages_dir.display()))?;
    let markdown_path = artifact_root.join(format!("{safe_name}.md"));
    let page_markdown_path = pages_dir.join("page_1.md");
    let node_candidates = extract_markdown_node_candidates_for_workspace(
        &markdown,
        &source_path.display().to_string(),
        &paths.workspace_root,
    )
    .unwrap_or_else(|_| {
        extract_markdown_node_candidates(&markdown, &source_path.display().to_string())
    });
    let mut markdown_signals = extract_markdown_signals(
        &markdown,
        &source_path.display().to_string(),
        Some(source_id.as_str()),
        &node_candidates,
    );
    markdown_signals.related_pages = rank_related_wiki_pages_for_signals(
        &paths.workspace_root,
        &record.workspace_id,
        &markdown_signals,
    )?;
    let edge_candidates = extract_markdown_relationship_evidence(
        &markdown,
        &source_path.display().to_string(),
        Some(source_id.as_str()),
        &node_candidates,
    );
    let claim_candidates = extract_markdown_claim_candidates(
        &markdown,
        &source_path.display().to_string(),
        Some(source_id.as_str()),
        &node_candidates,
    );
    write_file_atomic(&markdown_path, markdown.as_bytes())?;
    write_file_atomic(&page_markdown_path, markdown.as_bytes())?;
    write_json_pretty(
        &artifact_root.join("node-candidates.json"),
        &node_candidates,
    )?;
    write_json_pretty(
        &artifact_root.join("markdown-signals.json"),
        &markdown_signals,
    )?;
    write_json_pretty(
        &artifact_root.join("relationship-evidence.json"),
        &edge_candidates,
    )?;
    write_json_pretty(
        &artifact_root.join("edge-candidates.json"),
        &edge_candidates,
    )?;
    write_json_pretty(
        &artifact_root.join("claim-candidates.json"),
        &claim_candidates,
    )?;

    let now = unix_timestamp_seconds();
    let manifest_path = artifact_root.join("source-manifest.json");
    let manifest = SourceArtifactManifest {
        workspace_id: record.workspace_id.clone(),
        source_id: source_id.clone(),
        original_path: record.source_path.clone(),
        source_path: record.source_path.clone(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: artifact_root.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: DocumentFormat::Markdown,
        output_name: safe_name,
        status: IngestStatus::Ingested,
        description: format!("Markdown source ingested from {}", record.relative_path),
        user_context: String::new(),
        ingest_instruction: "Autonomous markdown source ingest loop".into(),
        pages: vec![PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: None,
            markdown_path: Some(page_markdown_path.display().to_string()),
            plain_text_path: None,
            error_message: None,
        }],
        created_at: record.discovered_at,
        updated_at: now,
    };
    write_source_manifest(&manifest)?;
    let chunks = chunk_source_markdown(&manifest, &markdown);
    upsert_source_chunks(&paths.workspace_root, &manifest, &chunks)?;

    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(record.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(record.workspace_id.clone()),
        source_id: Some(source_id.clone()),
    };
    let project = compile_knowledge_project(&request, &markdown, Some(&manifest));
    store.save_project(&project, &request, Some(&manifest))?;
    let snapshot = read_materialized_brain_snapshot(&paths.workspace_root, &record.workspace_id)
        .unwrap_or_else(|_| empty_replayed_brain_snapshot(&record.workspace_id));
    let context = build_import_evidence_context(
        &paths.workspace_root,
        &manifest,
        &markdown,
        &snapshot,
        &chunks,
    )?;
    persist_completed_ingest_related_wiki_pages(
        &paths.workspace_root,
        &record.workspace_id,
        &source_id,
        &markdown_signals.related_pages,
    )?;
    persist_completed_ingest_wiki_update_targets(
        &paths.workspace_root,
        &record.workspace_id,
        &source_id,
        &artifact_root,
        &markdown_signals.related_pages,
    )?;
    maybe_generate_provider_graph_proposals(
        &paths.workspace_root,
        &record.workspace_id,
        &manifest,
        &markdown,
        &artifact_root,
        &context,
    )?;
    Ok(manifest)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderGraphProposalGenerationReport {
    status: String,
    provider: String,
    model: String,
    source_id: String,
    proposal_count: usize,
    applied_count: usize,
    failed_count: usize,
    #[serde(default)]
    proposal_ids: Vec<String>,
    #[serde(default)]
    applied_proposal_ids: Vec<String>,
    #[serde(default)]
    failed_proposals: Vec<AgentProposalFailureReport>,
    #[serde(default)]
    skipped_reason: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    provider_run_id: Option<String>,
    updated_at: u64,
}

fn maybe_generate_provider_graph_proposals(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    artifact_root: &Path,
    context: &ImportEvidenceContext,
) -> Result<ProviderGraphProposalGenerationReport> {
    let report_path = artifact_root.join("provider-graph-proposals.json");
    if let Ok(existing) = read_json_artifact::<ProviderGraphProposalGenerationReport>(&report_path)
    {
        if !existing.proposal_ids.is_empty() && existing.source_id == manifest.source_id {
            return Ok(existing);
        }
    }

    let config = match EngineConfigStore::default().and_then(|store| store.load()) {
        Ok(config) => config,
        Err(error) => {
            let report = ProviderGraphProposalGenerationReport {
                status: "failed".into(),
                provider: "unknown".into(),
                model: "unknown".into(),
                source_id: manifest.source_id.clone(),
                proposal_count: 0,
                applied_count: 0,
                failed_count: 0,
                proposal_ids: Vec::new(),
                applied_proposal_ids: Vec::new(),
                failed_proposals: Vec::new(),
                skipped_reason: None,
                error_message: Some(format!("{error:#}")),
                provider_run_id: None,
                updated_at: unix_timestamp_seconds(),
            };
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    let mut report = ProviderGraphProposalGenerationReport {
        status: "skipped".into(),
        provider: config.provider.id_slug().into(),
        model: config.model_id.clone(),
        source_id: manifest.source_id.clone(),
        proposal_count: 0,
        applied_count: 0,
        failed_count: 0,
        proposal_ids: Vec::new(),
        applied_proposal_ids: Vec::new(),
        failed_proposals: Vec::new(),
        skipped_reason: None,
        error_message: None,
        provider_run_id: None,
        updated_at: unix_timestamp_seconds(),
    };

    if provider_graph_generation_disabled_for_process() {
        report.skipped_reason = Some("provider_graph_generation_disabled".into());
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    if provider_unavailable(&config) {
        report.skipped_reason = Some("provider_unavailable".into());
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)
        .unwrap_or_else(|_| empty_replayed_brain_snapshot(workspace_id));
    write_json_pretty(&artifact_root.join("provider-graph-context.json"), context)?;
    let default_evidence_refs =
        source_evidence_refs_for_provider_proposals(&snapshot, &manifest.source_id);
    let prompt = build_provider_graph_proposal_prompt(
        manifest,
        markdown,
        &snapshot,
        context,
        &default_evidence_refs,
    );
    let provider_run_id = format!("provider-graph-{}", Uuid::now_v7());
    report.provider_run_id = Some(provider_run_id.clone());
    let provider_response = match parse_openai_compatible_with_timeout(
        &config,
        &prompt,
        None,
        Some(Duration::from_secs(
            PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
        )),
    ) {
        Ok(response) => response,
        Err(error) => {
            report.status = "failed".into();
            report.error_message = Some(format!("{error:#}"));
            write_provider_graph_run_artifacts(
                workspace_root,
                &provider_run_id,
                workspace_id,
                manifest,
                "failed",
                Some(&prompt),
                None,
                Some(format!("{error:#}")),
            )?;
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    write_provider_graph_run_artifacts(
        workspace_root,
        &provider_run_id,
        workspace_id,
        manifest,
        "received",
        Some(&prompt),
        Some(&provider_response),
        None,
    )?;

    let mut payloads = match parse_provider_graph_proposal_payloads(&provider_response) {
        Ok(payloads) => payloads,
        Err(error) => {
            report.status = "failed".into();
            report.error_message = Some(format!("{error:#}"));
            write_provider_graph_run_validation_report(
                workspace_root,
                &provider_run_id,
                workspace_id,
                &manifest.source_id,
                "failed",
                0,
                Some(format!("{error:#}")),
            )?;
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    write_provider_graph_run_validation_report(
        workspace_root,
        &provider_run_id,
        workspace_id,
        &manifest.source_id,
        "parsed",
        payloads.len(),
        None,
    )?;

    if payloads.is_empty() {
        report.status = "empty".into();
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let mut allowed_context_evidence_refs = import_evidence_context_allowed_refs(context);
    for evidence_ref in &default_evidence_refs {
        merge_unique_string(&mut allowed_context_evidence_refs, evidence_ref);
    }
    for payload in &mut payloads {
        normalize_provider_graph_proposal_payload(payload, manifest, &default_evidence_refs);
    }
    ensure_provider_graph_edge_payloads(&mut payloads, manifest);

    let writer = BrainWorkspaceWriter::open(workspace_root.to_path_buf())?;
    for payload in payloads {
        let mut proposal = provider_graph_payload_to_proposal(
            workspace_id,
            &format!("{PROVIDER_GRAPH_AGENT_ID}:{}", config.provider.id_slug()),
            payload,
        );
        enrich_agent_graph_proposal_refs(&mut proposal);
        let request = ProposeBrainUpdateRequest {
            scope: BrainReadScope {
                workspace_id: workspace_id.to_string(),
                root_dir: Some(
                    workspace_root
                        .parent()
                        .unwrap_or(workspace_root)
                        .display()
                        .to_string(),
                ),
            },
            kind: proposal.kind,
            title: proposal.title.clone(),
            body: proposal.body.clone(),
            actor: proposal.actor.clone(),
            target_node_id: proposal.target_node_id.clone(),
            target_source_id: proposal.target_source_id.clone(),
            relation_kind: proposal.relation_kind,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: proposal.source_refs.clone(),
            node_refs: proposal.node_refs.clone(),
            evidence_refs: proposal.evidence_refs.clone(),
            proposal_payload: proposal.proposal_payload.clone(),
        };
        if let Err(error) =
            validate_provider_graph_proposal_with_context(&request, &allowed_context_evidence_refs)
                .and_then(|_| validate_brain_update_proposal(&request))
        {
            report.failed_count += 1;
            report.failed_proposals.push(AgentProposalFailureReport {
                proposal_id: proposal.proposal_id.clone(),
                run_id: "provider-graph-generation".into(),
                snapshot_id: "not_applied".into(),
                error_code: "provider_payload_validation_failed".into(),
                error_message: format!("{error:#}"),
                validation_issues: error
                    .downcast_ref::<AgentGraphProposalValidationError>()
                    .map(|error| error.issues.clone())
                    .unwrap_or_default(),
                audit_path: report_path.display().to_string(),
            });
            continue;
        }
        writer.write_proposal(&proposal)?;
        writer.append_event(&brain_event_for_proposal(&proposal)?)?;
        report.proposal_ids.push(proposal.proposal_id.clone());
    }
    drop(writer);

    report.proposal_count = report.proposal_ids.len();
    if report.proposal_count == 0 {
        report.status = if report.failed_count > 0 {
            "failed".into()
        } else {
            "empty".into()
        };
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let apply_result = run_queued_agent_proposal_apply_worker(workspace_root, workspace_id)?;
    report.applied_count = apply_result.applied.len();
    report.failed_count += apply_result.failed.len();
    report.applied_proposal_ids = apply_result.applied;
    report.failed_proposals.extend(apply_result.failed);
    if should_run_post_import_consolidation(report.applied_count, context) {
        maybe_run_post_import_consolidation_worker(
            workspace_root,
            workspace_id,
            manifest,
            context,
            &config,
            artifact_root,
            report.provider_run_id.as_deref(),
        )?;
    }
    report.status = if report.failed_count == 0 {
        "applied".into()
    } else if report.applied_count > 0 {
        "partially_applied".into()
    } else {
        "failed".into()
    };
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(&report_path, &report)?;
    Ok(report)
}

fn provider_graph_generation_disabled_for_process() -> bool {
    std::env::var_os("HYPRDUCK_DISABLE_PROVIDER_GRAPH").is_some()
        || (cfg!(test) && std::env::var_os("HYPRDUCK_TEST_ENABLE_PROVIDER_GRAPH").is_none())
}

fn maybe_run_post_import_consolidation_worker(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    context: &ImportEvidenceContext,
    config: &EngineConfig,
    artifact_root: &Path,
    parent_run_id: Option<&str>,
) -> Result<()> {
    let report_path = artifact_root.join("provider-graph-consolidation.json");
    let run_id = format!("provider-consolidation-{}", Uuid::now_v7());
    let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)
        .unwrap_or_else(|_| empty_replayed_brain_snapshot(workspace_id));
    let prompt =
        build_post_import_consolidation_prompt(manifest, &snapshot, context, parent_run_id);
    let provider_response = match parse_openai_compatible_with_timeout(
        config,
        &prompt,
        None,
        Some(Duration::from_secs(
            PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
        )),
    ) {
        Ok(response) => response,
        Err(error) => {
            write_json_pretty(
                &report_path,
                &failed_post_import_consolidation_report_value(
                    &run_id,
                    parent_run_id,
                    &manifest.source_id,
                    format!("{error:#}"),
                    unix_timestamp_seconds(),
                ),
            )?;
            return Ok(());
        }
    };
    write_provider_graph_run_artifacts(
        workspace_root,
        &run_id,
        workspace_id,
        manifest,
        "received",
        Some(&prompt),
        Some(&provider_response),
        None,
    )?;

    let mut payloads = match parse_provider_graph_proposal_payloads(&provider_response) {
        Ok(payloads) => payloads,
        Err(error) => {
            write_provider_graph_run_validation_report(
                workspace_root,
                &run_id,
                workspace_id,
                &manifest.source_id,
                "failed",
                0,
                Some(format!("{error:#}")),
            )?;
            write_json_pretty(
                &report_path,
                &failed_post_import_consolidation_report_value(
                    &run_id,
                    parent_run_id,
                    &manifest.source_id,
                    format!("{error:#}"),
                    unix_timestamp_seconds(),
                ),
            )?;
            return Ok(());
        }
    };
    write_provider_graph_run_validation_report(
        workspace_root,
        &run_id,
        workspace_id,
        &manifest.source_id,
        "parsed",
        payloads.len(),
        None,
    )?;

    let default_evidence_refs =
        source_evidence_refs_for_provider_proposals(&snapshot, &manifest.source_id);
    let mut allowed_context_evidence_refs = import_evidence_context_allowed_refs(context);
    for evidence_ref in &default_evidence_refs {
        merge_unique_string(&mut allowed_context_evidence_refs, evidence_ref);
    }
    for payload in &mut payloads {
        normalize_provider_graph_proposal_payload(payload, manifest, &default_evidence_refs);
    }
    ensure_provider_graph_edge_payloads(&mut payloads, manifest);

    let writer = BrainWorkspaceWriter::open(workspace_root.to_path_buf())?;
    let mut proposal_ids = Vec::new();
    let mut failed = Vec::new();
    for payload in payloads {
        let mut proposal = provider_graph_payload_to_proposal(
            workspace_id,
            &format!(
                "{PROVIDER_GRAPH_AGENT_ID}:{}:consolidation",
                config.provider.id_slug()
            ),
            payload,
        );
        enrich_agent_graph_proposal_refs(&mut proposal);
        let request = ProposeBrainUpdateRequest {
            scope: BrainReadScope {
                workspace_id: workspace_id.to_string(),
                root_dir: Some(
                    workspace_root
                        .parent()
                        .unwrap_or(workspace_root)
                        .display()
                        .to_string(),
                ),
            },
            kind: proposal.kind,
            title: proposal.title.clone(),
            body: proposal.body.clone(),
            actor: proposal.actor.clone(),
            target_node_id: proposal.target_node_id.clone(),
            target_source_id: proposal.target_source_id.clone(),
            relation_kind: proposal.relation_kind,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: proposal.source_refs.clone(),
            node_refs: proposal.node_refs.clone(),
            evidence_refs: proposal.evidence_refs.clone(),
            proposal_payload: proposal.proposal_payload.clone(),
        };
        if let Err(error) =
            validate_provider_graph_proposal_with_context(&request, &allowed_context_evidence_refs)
                .and_then(|_| validate_brain_update_proposal(&request))
        {
            failed.push(format!("{error:#}"));
            continue;
        }
        writer.write_proposal(&proposal)?;
        writer.append_event(&brain_event_for_proposal(&proposal)?)?;
        proposal_ids.push(proposal.proposal_id.clone());
    }
    drop(writer);

    let apply_result = run_queued_agent_proposal_apply_worker(workspace_root, workspace_id)?;
    let status = if failed.is_empty() && apply_result.failed.is_empty() {
        "applied"
    } else {
        "partially_applied"
    };
    let failed_proposals =
        serde_json::to_value(&apply_result.failed).context("failed to encode failed proposals")?;
    write_json_pretty(
        &report_path,
        &post_import_consolidation_report_value(
            status,
            &run_id,
            parent_run_id,
            &manifest.source_id,
            &proposal_ids,
            &apply_result.applied,
            &failed,
            failed_proposals,
            unix_timestamp_seconds(),
        ),
    )
}

fn write_provider_graph_run_artifacts(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    status: &str,
    prompt: Option<&str>,
    provider_response: Option<&str>,
    error_message: Option<String>,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("provider-response.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "status": status,
            "prompt": prompt,
            "providerResponse": provider_response,
            "errorMessage": error_message,
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

fn write_provider_graph_run_validation_report(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    source_id: &str,
    status: &str,
    parsed_count: usize,
    error_message: Option<String>,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("validation-report.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": source_id,
            "status": status,
            "parsedProposalCount": parsed_count,
            "errorMessage": error_message,
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

fn parse_provider_graph_proposal_payloads(raw: &str) -> Result<Vec<AgentGraphProposalPayload>> {
    let value = extract_provider_json_value(raw)?;
    if value.is_array() {
        return serde_json::from_value(value)
            .context("failed to decode provider graph proposal array");
    }
    let proposals = value
        .get("proposals")
        .cloned()
        .ok_or_else(|| anyhow!("provider graph response missing proposals array"))?;
    serde_json::from_value(proposals).context("failed to decode provider graph proposals")
}

fn extract_provider_json_value(raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }

    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return Ok(value);
    }

    let starts = raw
        .char_indices()
        .filter_map(|(index, ch)| matches!(ch, '{' | '[').then_some(index))
        .collect::<Vec<_>>();
    let ends = raw
        .char_indices()
        .filter_map(|(index, ch)| matches!(ch, '}' | ']').then_some(index + ch.len_utf8()))
        .rev()
        .collect::<Vec<_>>();
    for start in starts {
        for end in &ends {
            if *end <= start {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&raw[start..*end]) {
                return Ok(value);
            }
        }
    }
    bail!("provider graph response did not contain valid JSON")
}

fn normalize_provider_graph_proposal_payload(
    payload: &mut AgentGraphProposalPayload,
    manifest: &SourceArtifactManifest,
    default_evidence_refs: &[String],
) {
    match payload {
        AgentGraphProposalPayload::NewNode { node } => {
            normalize_provider_payload_refs(
                &mut node.source_path,
                &mut node.source_refs,
                &mut node.evidence_refs,
                manifest,
                default_evidence_refs,
            );
            if node.node_id.is_none() {
                node.node_id = Some(format!(
                    "node-provider-{}",
                    sanitize_id_fragment(&node.label)
                ));
            }
        }
        AgentGraphProposalPayload::NewEdge { edge } => {
            normalize_provider_payload_refs(
                &mut edge.source_path,
                &mut edge.source_refs,
                &mut edge.evidence_refs,
                manifest,
                default_evidence_refs,
            );
            if edge.edge_id.is_none() {
                edge.edge_id = Some(format!(
                    "edge-provider-{}-{}-{}",
                    sanitize_id_fragment(&edge.source_node_id),
                    relation_kind_slug(edge.kind),
                    sanitize_id_fragment(&edge.target_node_id)
                ));
            }
        }
        AgentGraphProposalPayload::NewClaim { claim } => {
            normalize_provider_payload_refs(
                &mut claim.source_path,
                &mut claim.source_refs,
                &mut claim.evidence_refs,
                manifest,
                default_evidence_refs,
            );
            if claim.claim_id.is_none() {
                claim.claim_id = Some(format!(
                    "claim-provider-{}",
                    sanitize_id_fragment(&claim.statement)
                ));
            }
        }
        AgentGraphProposalPayload::NewMemory { memory } => {
            normalize_provider_payload_refs(
                &mut memory.source_path,
                &mut memory.source_refs,
                &mut memory.evidence_refs,
                manifest,
                default_evidence_refs,
            );
            if memory.memory_id.is_none() {
                memory.memory_id = Some(format!(
                    "memory-provider-{}",
                    sanitize_id_fragment(&memory.title)
                ));
            }
        }
    }
}

fn ensure_provider_graph_edge_payloads(
    payloads: &mut Vec<AgentGraphProposalPayload>,
    manifest: &SourceArtifactManifest,
) {
    let mut existing_edges = payloads
        .iter()
        .filter_map(|payload| match payload {
            AgentGraphProposalPayload::NewEdge { edge } => Some((
                edge.source_node_id.trim().to_string(),
                edge.target_node_id.trim().to_string(),
                relation_kind_slug(edge.kind).to_string(),
            )),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut generated_edges = Vec::new();
    let source_node_id = format!("source:{}", manifest.source_id);

    for payload in payloads.iter() {
        match payload {
            AgentGraphProposalPayload::NewNode { node }
                if node.kind != BrainNodeKind::Source
                    && node
                        .node_id
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|node_id| !node_id.is_empty()) =>
            {
                let target_node_id = node.node_id.as_deref().unwrap().trim();
                push_generated_provider_edge(
                    &mut generated_edges,
                    &mut existing_edges,
                    ProviderGeneratedEdge {
                        source_node_id: &source_node_id,
                        target_node_id,
                        kind: BrainRelationKind::SourceOf,
                        label: "Source contains concept",
                        reason: "Provider created a source-backed node but did not emit the source-to-node edge.",
                        source_path: &node.source_path,
                        source_refs: &node.source_refs,
                        evidence_refs: &node.evidence_refs,
                    },
                );
            }
            AgentGraphProposalPayload::NewClaim { claim } => {
                let topic_refs = claim
                    .topic_refs
                    .iter()
                    .map(|topic_ref| topic_ref.trim())
                    .filter(|topic_ref| !topic_ref.is_empty())
                    .collect::<Vec<_>>();
                if topic_refs.len() < 2 {
                    continue;
                }
                let source_node_id = topic_refs[0];
                for target_node_id in topic_refs.iter().skip(1) {
                    push_generated_provider_edge(
                        &mut generated_edges,
                        &mut existing_edges,
                        ProviderGeneratedEdge {
                            source_node_id,
                            target_node_id,
                            kind: BrainRelationKind::RelatedTo,
                            label: "Related by provider claim",
                            reason: "Provider claim links multiple topicRefs, so HyprDuck materialized the relationship as an edge.",
                            source_path: &claim.source_path,
                            source_refs: &claim.source_refs,
                            evidence_refs: &claim.evidence_refs,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    payloads.extend(generated_edges);
}

struct ProviderGeneratedEdge<'a> {
    source_node_id: &'a str,
    target_node_id: &'a str,
    kind: BrainRelationKind,
    label: &'a str,
    reason: &'a str,
    source_path: &'a str,
    source_refs: &'a [String],
    evidence_refs: &'a [String],
}

fn push_generated_provider_edge(
    generated_edges: &mut Vec<AgentGraphProposalPayload>,
    existing_edges: &mut BTreeSet<(String, String, String)>,
    edge: ProviderGeneratedEdge<'_>,
) {
    if edge.source_node_id == edge.target_node_id {
        return;
    }
    let edge_key = (
        edge.source_node_id.to_string(),
        edge.target_node_id.to_string(),
        relation_kind_slug(edge.kind).to_string(),
    );
    if !existing_edges.insert(edge_key) {
        return;
    }
    generated_edges.push(AgentGraphProposalPayload::NewEdge {
        edge: AgentNewEdgePayload {
            source_node_id: edge.source_node_id.to_string(),
            target_node_id: edge.target_node_id.to_string(),
            kind: edge.kind,
            label: edge.label.to_string(),
            source_path: edge.source_path.to_string(),
            edge_id: Some(format!(
                "edge-provider-{}-{}-{}",
                sanitize_id_fragment(edge.source_node_id),
                relation_kind_slug(edge.kind),
                sanitize_id_fragment(edge.target_node_id)
            )),
            source_refs: edge.source_refs.to_vec(),
            evidence_refs: edge.evidence_refs.to_vec(),
            reason: Some(edge.reason.to_string()),
        },
    });
}

fn normalize_provider_payload_refs(
    source_path: &mut String,
    source_refs: &mut Vec<String>,
    evidence_refs: &mut Vec<String>,
    manifest: &SourceArtifactManifest,
    default_evidence_refs: &[String],
) {
    if source_path.trim().is_empty() {
        *source_path = manifest.markdown_path.clone();
    }
    merge_unique_string(source_refs, &manifest.source_id);
    if evidence_refs.is_empty() {
        for evidence_ref in default_evidence_refs.iter().take(3) {
            merge_unique_string(evidence_refs, evidence_ref);
        }
    }
}

fn validate_provider_graph_proposal_with_context(
    request: &ProposeBrainUpdateRequest,
    allowed_context_evidence_refs: &[String],
) -> Result<()> {
    if request.evidence_refs.is_empty() {
        bail!("provider graph proposal requires at least one evidence ref");
    }
    let allowed = allowed_context_evidence_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let unknown = request
        .evidence_refs
        .iter()
        .filter(|evidence_ref| !allowed.contains(*evidence_ref))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        bail!(
            "provider graph proposal cited evidence refs outside import context: {}",
            unknown.join(", ")
        );
    }
    Ok(())
}

fn provider_graph_payload_to_proposal(
    workspace_id: &str,
    actor_id: &str,
    payload: AgentGraphProposalPayload,
) -> BrainUpdateProposal {
    let kind = provider_graph_payload_kind(&payload);
    let (title, body, target_node_id, relation_kind, source_refs, node_refs, evidence_refs) =
        provider_graph_payload_proposal_fields(&payload);
    BrainUpdateProposal {
        proposal_id: format!("proposal-{}", Uuid::now_v7()),
        workspace_id: workspace_id.to_string(),
        kind,
        status: BrainProposalStatus::PendingReview,
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: actor_id.to_string(),
        },
        scope: BrainScope::Project,
        title,
        body,
        target_node_id,
        target_source_id: source_refs.first().cloned(),
        relation_kind,
        source_refs,
        node_refs,
        evidence_refs,
        proposal_payload: Some(payload),
        created_at: unix_timestamp_seconds(),
    }
}

fn provider_graph_payload_kind(payload: &AgentGraphProposalPayload) -> BrainProposalKind {
    match payload {
        AgentGraphProposalPayload::NewNode { .. } => BrainProposalKind::Node,
        AgentGraphProposalPayload::NewEdge { .. } => BrainProposalKind::Link,
        AgentGraphProposalPayload::NewClaim { .. } => BrainProposalKind::Claim,
        AgentGraphProposalPayload::NewMemory { .. } => BrainProposalKind::Memory,
    }
}

fn provider_graph_payload_proposal_fields(
    payload: &AgentGraphProposalPayload,
) -> (
    String,
    String,
    Option<String>,
    Option<BrainRelationKind>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
) {
    match payload {
        AgentGraphProposalPayload::NewNode { node } => (
            format!("Add graph node: {}", node.label),
            node.reason.clone().unwrap_or_else(|| {
                format!(
                    "Provider identified `{}` as a durable graph node.",
                    node.label
                )
            }),
            node.node_id.clone(),
            None,
            node.source_refs.clone(),
            node.node_id.iter().cloned().collect(),
            node.evidence_refs.clone(),
        ),
        AgentGraphProposalPayload::NewEdge { edge } => (
            format!("Connect graph nodes: {}", edge.label),
            edge.reason.clone().unwrap_or_else(|| {
                format!(
                    "Provider identified a {:?} relation from {} to {}.",
                    edge.kind, edge.source_node_id, edge.target_node_id
                )
            }),
            Some(edge.target_node_id.clone()),
            Some(edge.kind),
            edge.source_refs.clone(),
            vec![edge.source_node_id.clone(), edge.target_node_id.clone()],
            edge.evidence_refs.clone(),
        ),
        AgentGraphProposalPayload::NewClaim { claim } => (
            format!(
                "Add graph claim: {}",
                truncate_for_prompt(&claim.statement, 80)
            ),
            claim.reason.clone().unwrap_or_else(|| {
                "Provider identified a source-backed claim for the graph.".into()
            }),
            claim.topic_refs.first().cloned(),
            None,
            claim.source_refs.clone(),
            claim.topic_refs.clone(),
            claim.evidence_refs.clone(),
        ),
        AgentGraphProposalPayload::NewMemory { memory } => (
            format!("Add brain memory: {}", memory.title),
            memory.reason.clone().unwrap_or_else(|| memory.body.clone()),
            None,
            None,
            memory.source_refs.clone(),
            Vec::new(),
            memory.evidence_refs.clone(),
        ),
    }
}

fn source_evidence_refs_for_provider_proposals(
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
) -> Vec<String> {
    snapshot
        .evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .map(|evidence| evidence.id.clone())
        .take(8)
        .collect()
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}

fn sanitize_id_fragment(value: &str) -> String {
    let mut fragment = value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '/' | ':' | '.') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    while fragment.contains("--") {
        fragment = fragment.replace("--", "-");
    }
    let fragment = fragment.trim_matches('-');
    if fragment.is_empty() {
        Uuid::now_v7().to_string()
    } else {
        fragment.chars().take(64).collect()
    }
}

fn relation_kind_slug(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "mentions",
        BrainRelationKind::Supports => "supports",
        BrainRelationKind::Contradicts => "contradicts",
        BrainRelationKind::Supersedes => "supersedes",
        BrainRelationKind::SameAs => "same-as",
        BrainRelationKind::WorksAt => "works-at",
        BrainRelationKind::Founded => "founded",
        BrainRelationKind::InvestedIn => "invested-in",
        BrainRelationKind::Advises => "advises",
        BrainRelationKind::Attended => "attended",
        BrainRelationKind::Owns => "owns",
        BrainRelationKind::ResponsibleFor => "responsible-for",
        BrainRelationKind::Decided => "decided",
        BrainRelationKind::Blocks => "blocks",
        BrainRelationKind::DependsOn => "depends-on",
        BrainRelationKind::SourceOf => "source-of",
        BrainRelationKind::DerivedFrom => "derived-from",
        BrainRelationKind::RelatedTo => "related-to",
    }
}

fn read_idempotent_markdown_ingest_manifest(
    paths: &MarkdownIngestPaths,
    record: &MarkdownIngestQueueRecord,
    source_id: &str,
) -> Result<Option<SourceArtifactManifest>> {
    let manifest_path = paths.workspace_root.join("brain-manifest.json");
    if !manifest_path.exists() || !materialized_markdown_source_files_exist(paths, source_id) {
        return Ok(None);
    }

    let snapshot: BrainRepoSnapshot = read_json_artifact(&manifest_path)?;
    let Some(source) = snapshot.sources.iter().find(|source| {
        source.source_id == source_id
            && source.workspace_id == record.workspace_id
            && source.original_path == record.source_path
            && source.source_path == record.source_path
            && source.status == "ingested"
    }) else {
        return Ok(None);
    };
    if !snapshot
        .wiki_pages
        .iter()
        .any(|page| page.source_refs.contains(&source.source_id))
    {
        return Ok(None);
    }

    let source_manifest_path = paths
        .workspace_root
        .join("artifacts")
        .join(source_id)
        .join("source-manifest.json");
    let source_manifest: SourceArtifactManifest = read_json_artifact(&source_manifest_path)?;
    if source_manifest.source_id != source.source_id
        || source_manifest.workspace_id != record.workspace_id
        || source_manifest.source_path != record.source_path
    {
        return Ok(None);
    }
    Ok(Some(source_manifest))
}

fn materialized_markdown_source_files_exist(paths: &MarkdownIngestPaths, source_id: &str) -> bool {
    [
        paths.workspace_root.join("brain-manifest.json"),
        paths.workspace_root.join("graph/nodes.json"),
        paths.workspace_root.join("graph/edges.json"),
        paths.workspace_root.join("graph/claims.json"),
        paths.workspace_root.join("memory/records.json"),
        paths.workspace_root.join("wiki/index.md"),
        paths
            .workspace_root
            .join("artifacts")
            .join(source_id)
            .join("source-manifest.json"),
    ]
    .iter()
    .all(|path| path.exists())
}

fn persist_completed_ingest_related_wiki_pages(
    workspace_root: &Path,
    workspace_id: &str,
    source_id: &str,
    related_pages: &[MarkdownRelatedPageSignal],
) -> Result<()> {
    if related_pages.is_empty() {
        return Ok(());
    }

    let manifest_path = workspace_root.join("brain-manifest.json");
    if !manifest_path.exists() {
        return Ok(());
    }

    let mut snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)?;
    let Some(page_index) = snapshot.wiki_pages.iter().position(|page| {
        page.page_id == format!("wiki-source-{source_id}")
            || (page.path.starts_with("wiki/sources/")
                && page.source_refs.iter().any(|source| source == source_id))
    }) else {
        return Ok(());
    };

    {
        let page = &mut snapshot.wiki_pages[page_index];
        let base_body = strip_related_wiki_pages_section(&page.body);
        page.body = format!(
            "{}\n\n## Related Wiki Pages\n\n{}",
            base_body.trim_end(),
            related_wiki_pages_markdown(related_pages)
        );
        page.updated_at = unix_timestamp_seconds();
    }

    let page_path = snapshot.wiki_pages[page_index].path.clone();
    let page_body = materialized_wiki_page_body(&snapshot.wiki_pages[page_index], &snapshot);
    write_json_pretty(&manifest_path, &snapshot)?;
    write_file_atomic(&workspace_root.join(page_path), page_body.as_bytes())?;
    Ok(())
}

fn strip_related_wiki_pages_section(body: &str) -> String {
    body.split("\n\n## Related Wiki Pages\n\n")
        .next()
        .unwrap_or(body)
        .to_string()
}

fn related_wiki_pages_markdown(related_pages: &[MarkdownRelatedPageSignal]) -> String {
    let rows = related_pages
        .iter()
        .map(|page| {
            let link_target = wiki_source_page_relative_link(&page.path);
            format!(
                "- [{}]({}) (score: {}; reason: {}; matched: {})",
                page.title,
                link_target,
                page.score,
                page.reason,
                join_or_none(&page.matched_terms)
            )
        })
        .collect::<Vec<_>>();
    format!("{}\n", rows.join("\n"))
}

fn wiki_source_page_relative_link(path: &str) -> String {
    path.strip_prefix("wiki/")
        .map(|relative| format!("../{relative}"))
        .unwrap_or_else(|| path.to_string())
}

fn persist_completed_ingest_wiki_update_targets(
    workspace_root: &Path,
    workspace_id: &str,
    source_id: &str,
    artifact_root: &Path,
    related_pages: &[MarkdownRelatedPageSignal],
) -> Result<()> {
    let targets = if workspace_root.join("brain-manifest.json").exists() {
        let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)?;
        build_wiki_update_targets_for_source(&snapshot, source_id, related_pages)
    } else {
        Vec::new()
    };
    write_json_pretty(&artifact_root.join("wiki-update-targets.json"), &targets)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkdownWikiUpdateTarget {
    entity_type: String,
    entity_id: String,
    #[serde(default)]
    entity_label: String,
    change_kind: String,
    page_id: String,
    path: String,
    title: String,
    target_section: String,
    reason: String,
    #[serde(default)]
    source_refs: Vec<String>,
    #[serde(default)]
    node_refs: Vec<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
}

fn build_wiki_update_targets_for_source(
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
    related_pages: &[MarkdownRelatedPageSignal],
) -> Vec<MarkdownWikiUpdateTarget> {
    let evidence_by_id = snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    let related_paths = related_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut targets = BTreeMap::<(String, String, String, String), MarkdownWikiUpdateTarget>::new();

    for node in snapshot
        .nodes
        .iter()
        .filter(|node| node.source_ids.iter().any(|source| source == source_id))
    {
        upsert_wiki_update_targets(
            &mut targets,
            snapshot,
            &related_paths,
            "node",
            &node.node_id,
            &node.label,
            "changed",
            std::slice::from_ref(&node.node_id),
            &node.source_ids,
            &node.evidence_ids,
            "## Evidence",
            "node source or evidence refs overlap this ingest source",
        );
    }

    for relation in snapshot
        .relations
        .iter()
        .filter(|relation| refs_include_source(&relation.evidence_ids, source_id, &evidence_by_id))
    {
        upsert_wiki_update_targets(
            &mut targets,
            snapshot,
            &related_paths,
            "edge",
            &relation.relation_id,
            &relation.label,
            "changed",
            &[
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ],
            &[source_id.to_string()],
            &relation.evidence_ids,
            "## Relations",
            "edge evidence refs overlap this ingest source",
        );
    }

    for claim in snapshot.claims.iter().filter(|claim| {
        claim.source_refs.iter().any(|source| source == source_id)
            || refs_include_source(&claim.evidence_refs, source_id, &evidence_by_id)
    }) {
        upsert_wiki_update_targets(
            &mut targets,
            snapshot,
            &related_paths,
            "claim",
            &claim.claim_id,
            &claim.statement,
            "changed",
            &claim.topic_refs,
            &claim.source_refs,
            &claim.evidence_refs,
            "## Claims",
            "claim source or evidence refs overlap this ingest source",
        );
    }

    for memory in snapshot.memories.iter().filter(|memory| {
        memory.source_refs.iter().any(|source| source == source_id)
            || refs_include_source(&memory.evidence_refs, source_id, &evidence_by_id)
    }) {
        upsert_wiki_update_targets(
            &mut targets,
            snapshot,
            &related_paths,
            "memory",
            &memory.memory_id,
            &memory.title,
            "changed",
            &[],
            &memory.source_refs,
            &memory.evidence_refs,
            "## Memories",
            "memory source or evidence refs overlap this ingest source",
        );
    }

    targets.into_values().collect()
}

fn refs_include_source(
    refs: &[String],
    source_id: &str,
    evidence_by_id: &BTreeMap<&str, &EvidenceRef>,
) -> bool {
    refs.iter().any(|reference| {
        reference == source_id
            || evidence_by_id
                .get(reference.as_str())
                .and_then(|evidence| evidence.source_id.as_deref())
                .is_some_and(|evidence_source_id| evidence_source_id == source_id)
    })
}

#[allow(clippy::too_many_arguments)]
fn upsert_wiki_update_targets(
    targets: &mut BTreeMap<(String, String, String, String), MarkdownWikiUpdateTarget>,
    snapshot: &BrainRepoSnapshot,
    related_paths: &BTreeSet<&str>,
    entity_type: &str,
    entity_id: &str,
    entity_label: &str,
    change_kind: &str,
    node_refs: &[String],
    source_refs: &[String],
    evidence_refs: &[String],
    target_section: &str,
    reason: &str,
) {
    for page in snapshot.wiki_pages.iter().filter(|page| {
        wiki_page_matches_entity(page, related_paths, node_refs, source_refs, evidence_refs)
    }) {
        let key = (
            entity_type.to_string(),
            entity_id.to_string(),
            page.path.clone(),
            target_section.to_string(),
        );
        targets
            .entry(key)
            .or_insert_with(|| MarkdownWikiUpdateTarget {
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                entity_label: entity_label.to_string(),
                change_kind: change_kind.to_string(),
                page_id: page.page_id.clone(),
                path: page.path.clone(),
                title: page.title.clone(),
                target_section: target_section.to_string(),
                reason: reason.to_string(),
                source_refs: merge_string_refs(source_refs, &page.source_refs),
                node_refs: merge_string_refs(node_refs, &page.node_refs),
                evidence_refs: merge_string_refs(evidence_refs, &page.evidence_refs),
            });
    }
}

fn wiki_page_matches_entity(
    page: &WikiPage,
    related_paths: &BTreeSet<&str>,
    node_refs: &[String],
    source_refs: &[String],
    evidence_refs: &[String],
) -> bool {
    related_paths.contains(page.path.as_str())
        || refs_overlap(&page.node_refs, node_refs)
        || refs_overlap(&page.source_refs, source_refs)
        || refs_overlap(&page.evidence_refs, evidence_refs)
}

fn refs_overlap(left: &[String], right: &[String]) -> bool {
    !left.is_empty()
        && left
            .iter()
            .any(|value| right.iter().any(|other| other == value))
}

fn collect_markdown_source_file_states(
    root: &Path,
    dir: &Path,
    sources: &mut Vec<MarkdownSourceFileState>,
) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed reading {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed reading entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed reading metadata for {}", path.display()))?;
        if file_type.is_dir() {
            collect_markdown_source_file_states(root, &path, sources)?;
            continue;
        }
        if !file_type.is_file() || !is_markdown_source_path(&path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .with_context(|| {
                format!(
                    "failed deriving markdown source path {} relative to {}",
                    path.display(),
                    root.display()
                )
            })?
            .to_path_buf();
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed reading metadata for {}", path.display()))?;
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        let contents =
            fs::read(&path).with_context(|| format!("failed reading {}", path.display()))?;
        sources.push(MarkdownSourceFileState {
            normalized_absolute_path: normalize_ingest_path_for_compare(
                &path.display().to_string(),
            ),
            normalized_relative_path: normalize_ingest_path_for_compare(
                &relative_path.display().to_string(),
            ),
            source_path: path,
            relative_path,
            size_bytes: metadata.len(),
            modified_at,
            content_hash: format!("{:016x}", fnv1a_hash(&contents)),
        });
    }
    Ok(())
}

fn read_markdown_source_state(paths: &MarkdownIngestPaths) -> Result<MarkdownSourceStateFile> {
    read_optional_json_artifact(&paths.workspace_root.join(MARKDOWN_SOURCE_STATE_PATH))
}

fn write_markdown_source_state(
    paths: &MarkdownIngestPaths,
    state: &MarkdownSourceStateFile,
) -> Result<()> {
    write_json_pretty(
        &paths.workspace_root.join(MARKDOWN_SOURCE_STATE_PATH),
        state,
    )
}

fn read_markdown_ingest_queue(paths: &MarkdownIngestPaths) -> Result<MarkdownIngestQueueFile> {
    read_optional_json_artifact(&paths.workspace_root.join(MARKDOWN_INGEST_QUEUE_PATH))
}

fn write_markdown_ingest_queue(
    paths: &MarkdownIngestPaths,
    queue: &MarkdownIngestQueueFile,
) -> Result<()> {
    write_json_pretty(
        &paths.workspace_root.join(MARKDOWN_INGEST_QUEUE_PATH),
        queue,
    )
}

fn markdown_queue_dedupe_key(record: &MarkdownIngestQueueRecord) -> String {
    format!("{}:{}", record.normalized_path, record.content_hash)
}

fn source_state_by_path(
    state: &MarkdownSourceStateFile,
) -> BTreeMap<String, MarkdownSourceStateRecord> {
    let mut by_path = BTreeMap::new();
    for record in &state.sources {
        by_path.insert(record.normalized_path.clone(), record.clone());
        by_path.insert(
            normalize_ingest_path_for_compare(&record.source_path),
            record.clone(),
        );
        by_path.insert(
            normalize_ingest_path_for_compare(&record.relative_path),
            record.clone(),
        );
    }
    by_path
}

fn is_markdown_source_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn normalize_ingest_path_for_compare(path: &str) -> String {
    let path = PathBuf::from(path);
    path.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_brain_workspace_config(workspace_root: &Path) -> Result<BrainWorkspaceConfig> {
    let path = workspace_root.join("brain-config.json");
    if !path.exists() {
        return Ok(BrainWorkspaceConfig::default());
    }
    read_json_artifact(&path)
}

fn resolve_workspace_config_path(
    workspace_root: &Path,
    configured: Option<&str>,
    fallback: &str,
) -> Result<PathBuf> {
    let raw = configured.unwrap_or(fallback).trim();
    if raw.is_empty() {
        bail!("configured workspace path cannot be empty");
    }
    let path = PathBuf::from(raw);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("configured workspace path cannot contain ..");
    }
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(workspace_root.join(path))
    }
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

fn search_terms(query: &str) -> Vec<String> {
    query
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_search_token)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let frequencies = search_token_frequencies(haystack);
    let mut matched_terms = 0usize;
    let mut score = 0usize;
    for term in terms {
        if let Some(frequency) = frequencies.get(term) {
            matched_terms += 1;
            score += 8 + frequency.saturating_mul(2);
            continue;
        }
        if term.len() > 3
            && frequencies
                .keys()
                .any(|token| token.starts_with(term) || term.starts_with(token))
        {
            matched_terms += 1;
            score += 3;
        }
    }
    score += matched_terms.saturating_mul(matched_terms);
    if matched_terms == terms.len() {
        score += 10;
    }
    (score > 0).then_some(score)
}

fn evidence_snippet(evidence_ids: &[String]) -> String {
    if evidence_ids.is_empty() {
        return "evidence: none".into();
    }
    format!("evidence: {}", evidence_ids.join(", "))
}

fn search_token_frequencies(text: &str) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for token in text
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_search_token)
    {
        *frequencies.entry(token).or_insert(0) += 1;
    }
    frequencies
}

fn normalize_search_token(raw: &str) -> Option<String> {
    let mut token = raw.trim().to_ascii_lowercase();
    if token.len() <= 1 {
        return None;
    }
    if token.ends_with("ies") && token.len() > 4 {
        token.truncate(token.len() - 3);
        token.push('y');
    } else if token.ends_with("ing") && token.len() > 5 {
        token.truncate(token.len() - 3);
    } else if token.ends_with("ed") && token.len() > 4 {
        token.truncate(token.len() - 2);
    } else if token.ends_with("es") && token.len() > 4 && !token.ends_with("ses") {
        token.truncate(token.len() - 2);
    } else if token.ends_with('s')
        && token.len() > 4
        && !token.ends_with("ss")
        && !token.ends_with("us")
    {
        token.truncate(token.len() - 1);
    }
    (token.len() > 1).then_some(token)
}

fn best_snippet(text: &str, terms: &[String]) -> String {
    let lower = text.to_ascii_lowercase();
    let start = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0)
        .saturating_sub(48);
    text.chars().skip(start).take(180).collect()
}

fn context_pack_warnings(
    nodes: &[BrainNodeRecord],
    evidence: &[EvidenceRef],
    budget: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if nodes.is_empty() {
        warnings.push(
            "No graph nodes matched the query; pack falls back to workspace wiki pages.".into(),
        );
    }
    if evidence.is_empty() {
        warnings.push("No direct evidence refs matched the query.".into());
    }
    if nodes
        .iter()
        .any(|node| node.confidence.unwrap_or(1.0) < 0.6)
    {
        warnings.push("Some selected nodes have low confidence.".into());
    }
    if budget < 2000 {
        warnings.push("Small budget may omit relevant wiki pages or graph context.".into());
    }
    warnings
}

fn trim_context_pack_to_budget(
    budget: usize,
    wiki_pages: &mut [WikiPage],
    nodes: &mut Vec<BrainNodeRecord>,
) {
    let mut remaining_chars = budget.saturating_mul(4);
    for page in wiki_pages.iter_mut() {
        if page.body.len() > remaining_chars {
            page.body = page.body.chars().take(remaining_chars).collect();
            remaining_chars = 0;
        } else {
            remaining_chars = remaining_chars.saturating_sub(page.body.len());
        }
    }
    if remaining_chars == 0 {
        nodes.truncate(nodes.len().min(3));
    }
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
    let code = if format!("{error:?}").contains("decode") {
        "invalid_request"
    } else if format!("{error:?}").contains("config") {
        "config_error"
    } else {
        "runtime_error"
    };
    EngineFailure::new(command, code, error.to_string())
}

fn build_markdown(title: String, pages: &[ParsedPage]) -> String {
    let mut markdown = format!("# {title}\n\n");
    for (idx, page) in pages.iter().enumerate() {
        markdown.push_str(&format!("## Page {}\n\n", idx + 1));
        if let Some(image_path) = &page.image_asset_path {
            markdown.push_str(&format!("![Page {}]({image_path})\n\n", idx + 1));
        }
        if let Some(body) = page
            .markdown
            .as_ref()
            .or(page.plain_text.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            markdown.push_str(body);
            markdown.push_str("\n\n");
        } else if let Some(error_message) = &page.error_message {
            markdown.push_str(&format!("_AI analysis unavailable: {error_message}_\n\n"));
        } else {
            markdown.push_str("_AI analysis unavailable._\n\n");
        }
    }
    markdown
}

fn export_output_package(
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<Option<SourceArtifactManifest>> {
    let Some(output) = &request.output else {
        return Ok(None);
    };

    let base_name = output
        .name
        .clone()
        .or_else(|| {
            Path::new(&request.input.path)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "document".to_string());
    let safe_name = sanitize_name(&base_name);
    let timestamp = chrono_like_timestamp();
    let output_roots = output_root_candidates(output)?;
    write_output_package_with_fallback(&output_roots, &safe_name, &timestamp, request, result)
        .map(Some)
}

fn output_root_candidates(
    output: &hyprduck_engine_types::ParseOutputTarget,
) -> Result<Vec<PathBuf>> {
    if let Some(root) = &output.root_dir {
        return Ok(vec![PathBuf::from(root)]);
    }

    let mut candidates = Vec::new();

    if let Some(override_root) = std::env::var_os("HYPRDUCK_OUTPUT_DIR") {
        candidates.push(PathBuf::from(override_root));
    } else {
        if let Some(application_support_root) = dirs::data_local_dir() {
            candidates.push(application_support_root.join("HyprDuck"));
        }
        candidates.push(std::env::temp_dir().join("HyprDuck"));
    }

    candidates.dedup();
    Ok(candidates)
}

fn write_output_package_with_fallback(
    output_roots: &[PathBuf],
    safe_name: &str,
    timestamp: &str,
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<SourceArtifactManifest> {
    let mut last_error = None;

    for output_root in output_roots {
        match write_output_package_to_root(output_root, safe_name, timestamp, request, result) {
            Ok(manifest) => return Ok(manifest),
            Err(error) => {
                eprintln!(
                    "output packaging failed under {}: {error:#}",
                    output_root.display()
                );
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("failed writing markdown package")))
}

fn write_output_package_to_root(
    output_root: &Path,
    safe_name: &str,
    timestamp: &str,
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<SourceArtifactManifest> {
    let workspace_id = request
        .output
        .as_ref()
        .and_then(|output| output.workspace_id.clone())
        .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
    let source_id = request
        .output
        .as_ref()
        .and_then(|output| output.source_id.clone())
        .unwrap_or_else(new_source_id);
    let workspace_root = output_root.join(&workspace_id);
    let sources_root = workspace_root.join("sources");
    let artifacts_root = workspace_root.join("artifacts");
    for required_dir in [
        &sources_root,
        &artifacts_root,
        &workspace_root.join("wiki"),
        &workspace_root.join("graph"),
        &workspace_root.join("reviews"),
    ] {
        fs::create_dir_all(required_dir)
            .with_context(|| format!("failed creating {}", required_dir.display()))?;
    }

    let source_dir = sources_root.join(&source_id);
    let output_dir = artifacts_root.join(&source_id);
    let images_dir = output_dir.join("images");
    let pages_dir = output_dir.join("pages");
    fs::create_dir_all(&source_dir)
        .with_context(|| format!("failed creating source directory {}", source_dir.display()))?;
    fs::create_dir_all(&images_dir).with_context(|| {
        format!(
            "failed creating image output directory {}",
            images_dir.display()
        )
    })?;
    fs::create_dir_all(&pages_dir).with_context(|| {
        format!(
            "failed creating page artifact directory {}",
            pages_dir.display()
        )
    })?;

    let source_filename = Path::new(&request.input.path)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{safe_name}.{timestamp}"));
    let source_path = source_dir.join(sanitize_name(&source_filename));
    fs::copy(&request.input.path, &source_path).with_context(|| {
        format!(
            "failed copying source document {} to {}",
            request.input.path,
            source_path.display()
        )
    })?;

    for asset in &result.assets {
        let target = output_dir.join(&asset.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating asset directory {}", parent.display()))?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&asset.base64)
            .with_context(|| format!("failed decoding asset {}", asset.relative_path))?;
        fs::write(&target, bytes)
            .with_context(|| format!("failed writing asset {}", target.display()))?;
    }

    let markdown_path = output_dir.join(format!("{safe_name}.md"));
    fs::write(&markdown_path, &result.markdown)
        .with_context(|| format!("failed writing markdown {}", markdown_path.display()))?;

    let mut page_artifacts = Vec::new();
    for page in &result.pages {
        let page_number = page.index + 1;
        let markdown_path = if let Some(markdown) = &page.markdown {
            let path = pages_dir.join(format!("page_{page_number}.md"));
            fs::write(&path, markdown)
                .with_context(|| format!("failed writing page markdown {}", path.display()))?;
            Some(path.display().to_string())
        } else {
            None
        };
        let plain_text_path = if let Some(plain_text) = &page.plain_text {
            let path = pages_dir.join(format!("page_{page_number}.txt"));
            fs::write(&path, plain_text)
                .with_context(|| format!("failed writing page text {}", path.display()))?;
            Some(path.display().to_string())
        } else {
            None
        };
        page_artifacts.push(PageArtifact {
            index: page.index,
            label: format!("Page {page_number}"),
            image_path: page
                .image_asset_path
                .as_ref()
                .map(|relative_path| output_dir.join(relative_path).display().to_string()),
            markdown_path,
            plain_text_path,
            error_message: page.error_message.clone(),
        });
    }

    let status = ingest_status_for_result(result);
    let now = unix_timestamp_seconds();
    let manifest_path = output_dir.join("source-manifest.json");
    let manifest = SourceArtifactManifest {
        workspace_id,
        source_id,
        original_path: request.input.path.clone(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: output_dir.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: request.input.format.clone(),
        output_name: safe_name.to_string(),
        status,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        pages: page_artifacts,
        created_at: now,
        updated_at: now,
    };
    write_source_manifest(&manifest)?;
    Ok(manifest)
}

fn ingest_status_for_result(result: &ParseResult) -> IngestStatus {
    if result.success_count == 0 && result.failed_count > 0 {
        IngestStatus::Failed
    } else if result.failed_count > 0 {
        IngestStatus::NeedsReview
    } else {
        IngestStatus::Ingested
    }
}

fn write_source_manifest(manifest: &SourceArtifactManifest) -> Result<()> {
    let json =
        serde_json::to_string_pretty(manifest).context("failed to encode source manifest")?;
    if let Some(parent) = Path::new(&manifest.manifest_path).parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating source manifest directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&manifest.manifest_path, json)
        .with_context(|| format!("failed writing source manifest {}", manifest.manifest_path))
}

fn load_source_manifest(request: &CompileProjectRequest) -> Result<Option<SourceArtifactManifest>> {
    let Some(path) = &request.source_manifest_path else {
        return Ok(None);
    };
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed reading source manifest {path}"))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed decoding source manifest {path}"))
        .map(Some)
}

fn resolved_source_ids(
    request: &CompileProjectRequest,
    manifest: Option<&SourceArtifactManifest>,
) -> Result<(WorkspaceId, SourceId)> {
    if let Some(manifest) = manifest {
        if let Some(request_workspace_id) = &request.workspace_id {
            if request_workspace_id != &manifest.workspace_id {
                bail!(
                    "compile_project workspace_id {} does not match source manifest workspace_id {}",
                    request_workspace_id,
                    manifest.workspace_id
                );
            }
        }
        if let Some(request_source_id) = &request.source_id {
            if request_source_id != &manifest.source_id {
                bail!(
                    "compile_project source_id {} does not match source manifest source_id {}",
                    request_source_id,
                    manifest.source_id
                );
            }
        }
        return Ok((manifest.workspace_id.clone(), manifest.source_id.clone()));
    }

    Ok((
        request
            .workspace_id
            .clone()
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string()),
        request
            .source_id
            .clone()
            .unwrap_or_else(|| build_source_id(&request.source_markdown_path, 0)),
    ))
}

fn build_source_id(seed: &str, timestamp: u64) -> SourceId {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{seed}|{timestamp}").as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("source-{hash:016x}")
}

fn new_source_id() -> SourceId {
    format!("source-{}", Uuid::now_v7())
}

fn source_summary_from_manifest(manifest: &SourceArtifactManifest) -> SourceSummary {
    let failed_count = manifest
        .pages
        .iter()
        .filter(|page| page.error_message.is_some())
        .count();
    SourceSummary {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: manifest.format.clone(),
        status: manifest.status.clone(),
        page_count: manifest.pages.len(),
        success_count: manifest.pages.len().saturating_sub(failed_count),
        failed_count,
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
    }
}

fn source_summary_from_sqlite_row(line: &str) -> Result<SourceSummary> {
    let columns: Vec<&str> = line.split('|').collect();
    if columns.len() != 11 && columns.len() != 12 {
        bail!(
            "expected 11 or 12 source summary columns from sqlite, got {}",
            columns.len()
        );
    }
    let manifest = columns
        .get(11)
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
    if columns.len() != 13 && columns.len() != 14 {
        bail!(
            "expected 13 or 14 stored source columns from sqlite, got {}",
            columns.len()
        );
    }
    let manifest = columns
        .get(13)
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
    if value.len() % 2 != 0 {
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

fn ingest_status_slug(status: &IngestStatus) -> &'static str {
    match status {
        IngestStatus::Added => "added",
        IngestStatus::Rendering => "rendering",
        IngestStatus::Ingesting => "ingesting",
        IngestStatus::Ingested => "ingested",
        IngestStatus::NeedsReview => "needs_review",
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
        "needs_review" => Ok(IngestStatus::NeedsReview),
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
        .replace('/', "-")
        .replace('\\', "-")
        .replace(':', "-")
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
        let new_path = root.join("HyprDuck/knowledge.sqlite3");
        let legacy_path = root.join("HyprDuck/knowledge.sqlite3");
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
            self.save_source(project, source_manifest)?;
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
            "SELECT hex(workspace_id), hex(source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(status), page_count, success_count, failed_count, updated_at, manifest_base64 \
             FROM sources WHERE workspace_id = '{}' ORDER BY updated_at DESC;",
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
            "SELECT hex(workspace_id), hex(source_id), hex(original_path), hex(source_path), hex(markdown_path), hex(format), hex(status), page_count, success_count, failed_count, updated_at, hex(project_id), hex(manifest_path), manifest_base64 \
             FROM sources WHERE workspace_id = '{}' ORDER BY updated_at DESC;",
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
        Ok(Some(aggregate_workspace_project(workspace_id, rows)))
    }

    fn save_source(
        &self,
        project: &KnowledgeProject,
        manifest: &SourceArtifactManifest,
    ) -> Result<()> {
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

    fn update_source_manifest_snapshot(&self, manifest: &SourceArtifactManifest) -> Result<()> {
        self.ensure_schema()?;
        let manifest_json =
            serde_json::to_string(manifest).context("failed to encode source manifest snapshot")?;
        let manifest_base64 = base64::engine::general_purpose::STANDARD.encode(manifest_json);
        let status = ingest_status_slug(&manifest.status);
        let sql = format!(
            "UPDATE sources SET status = '{status}', updated_at = {updated_at}, manifest_base64 = '{manifest_base64}' \
             WHERE source_id = '{source_id}';",
            status = status,
            updated_at = manifest.updated_at,
            manifest_base64 = manifest_base64,
            source_id = escape_sqlite(&manifest.source_id),
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
        write_materialized_brain_repo(&workspace_root, &snapshot)
    }

    fn ensure_schema(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
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

        Ok(String::from_utf8(output.stdout).context("sqlite3 output was not valid UTF-8")?)
    }
}

fn escape_sqlite(value: &str) -> String {
    value.replace('\'', "''")
}

fn unix_timestamp_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn resolve_binary(name: &str, common_paths: &[&str]) -> PathBuf {
    if let Some(path) = find_binary_on_path(name) {
        return path;
    }

    common_paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn find_binary_on_path(name: &str) -> Option<PathBuf> {
    if Path::new(name).components().count() > 1 {
        let path = PathBuf::from(name);
        return path.exists().then_some(path);
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.exists())
    })
}

#[cfg(test)]
mod tests;
