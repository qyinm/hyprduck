use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use duckdocs_engine_types::{
    AnswerProjectRequest, AnswerProjectResponseData, AnswerResponse, AnswerStatus,
    ApplyCorrectionRequest, ApplyCorrectionResponseData, BrainActor, BrainActorType,
    BrainContextPack, BrainEvent, BrainEventKind, BrainHealthStatus, BrainNodeKind,
    BrainNodeRecord, BrainProposalKind, BrainProposalStatus, BrainReadScope, BrainRelationKind,
    BrainRelationRecord, BrainRepoSnapshot, BrainReviewDecision, BrainReviewItem, BrainScope,
    BrainSearchResult, BrainSearchResultKind, BrainUpdateProposal, ClaimRecord,
    CompileProjectRequest, CompileProjectResponseData, CorrectionAction, CorrectionKind,
    DocumentFormat, EngineCommand, EngineConfigPayload, EngineFailure, EngineRequest,
    EngineRuntimeEvent, EngineRuntimeFailure, EngineRuntimeRequest, EngineRuntimeResponse,
    EngineSuccess, EntityRecord, EvidenceRef, GetBrainHealthRequest, GetBrainHealthResponseData,
    GetContextPackRequest, GetContextPackResponseData, GraphNodeDetail, GraphNodeKind,
    GraphNodePosition, GraphNodeSummary, IngestStatus, KnowledgeProject,
    ListBrainReviewItemsRequest, ListBrainReviewItemsResponseData, LoadConfigRequest,
    LoadProjectRequest, LoadProjectResponseData, MemoryRecord, OutputAsset, PageArtifact,
    ParseEvent, ParseInput, ParseMetadata, ParseOptions, ParseRequest, ParseResponseData,
    ParseResult, ParsedPage, ProjectOverview, ProjectStatus, ProposeBrainUpdateRequest,
    ProposeBrainUpdateResponseData, ProviderModelCatalogResponseData, ProviderOption,
    ReadNodeRequest, ReadNodeResponseData, ReadRecentEventsRequest, ReadRecentEventsResponseData,
    ReadSourceRequest, ReadSourceResponseData, ReadWikiPageRequest, ReadWikiPageResponseData,
    ReadinessCheck, RelationEdgeDetail, RelationEdgeSummary, RelationKind,
    ResolveBrainReviewItemRequest, ResolveBrainReviewItemResponseData,
    RuntimeReadinessResponseData, SaveConfigRequest, SaveConfigResponseData, SearchBrainRequest,
    SearchBrainResponseData, SourceArtifactManifest, SourceBacking, SourceId, SourceRecord,
    SourceSummary, StructuredExtractionArtifact, StructuredExtractionClaim,
    StructuredExtractionEntity, StructuredExtractionPageRef, StructuredExtractionRelation,
    StructuredExtractionTopic, SuggestedAction, SuggestedActionKind, ValidateProviderRequest,
    ValidateProviderResponseData, ValidationIssue, WikiPage, WorkspaceCorrection, WorkspaceId,
};
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempfile::tempdir;
use uuid::{Uuid, Version};

const DEFAULT_WORKSPACE_ID: &str = "default";
const PROJECT_SNAPSHOT_BATCH_SIZE: usize = 200;

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
        EngineRequest::GetContextPack(_) => EngineCommand::GetContextPack,
        EngineRequest::ProposeBrainUpdate(_) => EngineCommand::ProposeBrainUpdate,
        EngineRequest::ListBrainReviewItems(_) => EngineCommand::ListBrainReviewItems,
        EngineRequest::ResolveBrainReviewItem(_) => EngineCommand::ResolveBrainReviewItem,
        EngineRequest::GetBrainHealth(_) => EngineCommand::GetBrainHealth,
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

#[derive(Debug)]
struct ParsedDocument {
    pages: Vec<ParsedPage>,
    assets: Vec<OutputAsset>,
    page_count: usize,
    success_count: usize,
    failed_count: usize,
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
    Ok(CompileProjectResponseData {
        project_id: project.summary.project_id,
        workspace_id,
        source_id,
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
        bail!("workspace {workspace_id} was not found");
    }

    let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
    let selected_detail = aggregate
        .details_by_node_id
        .get(&request.node_id)
        .ok_or_else(|| anyhow!("workspace node {} was not found", request.node_id))?;
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
        CorrectionKind::KeepSeparate | CorrectionKind::Rename => None,
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
        .ok_or_else(|| anyhow!("workspace {workspace_id} was not found after correction"))?;
    Ok(ApplyCorrectionResponseData { project })
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
        events: reader.recent_events(request.limit.unwrap_or(20)),
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

    let auto_apply = is_auto_apply_brain_proposal(request.kind);
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
        target_node_id: request.target_node_id.clone(),
        target_source_id: request.target_source_id.clone(),
        relation_kind: request.relation_kind,
        source_refs,
        node_refs,
        evidence_refs: request.evidence_refs.clone(),
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
            BrainProposalKind::Claim | BrainProposalKind::Link | BrainProposalKind::WikiPage
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
    #[serde(default)]
    repairs: Vec<String>,
    #[serde(default)]
    issues: Vec<BrainLintIssue>,
}

fn run_brain_maintenance(scope: &BrainReadScope) -> Result<BrainMaintenanceReport> {
    let root = resolve_brain_workspace_root(scope)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    let snapshot = read_materialized_brain_snapshot(&root, &scope.workspace_id)?;
    let mut report = lint_brain_snapshot(&snapshot);
    report.repair_count += repair_generated_brain_artifacts(&root, &snapshot, &mut report.repairs)?;
    report.new_review_count += write_lint_review_items(&writer, &snapshot, &report.issues)?;
    report.issue_count = report.issues.len();
    write_json_pretty(&root.join("reviews/lint-reports/latest.json"), &report)?;
    if report.repair_count > 0 || report.new_review_count > 0 {
        writer.append_event(&brain_maintenance_event(&snapshot, &report)?)?;
    }
    Ok(report)
}

fn read_materialized_brain_snapshot(root: &Path, workspace_id: &str) -> Result<BrainRepoSnapshot> {
    let manifest_path = root.join("brain-manifest.json");
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
        repairs: Vec::new(),
        issues,
    }
}

fn missing_refs(refs: &[String], existing: &BTreeSet<String>) -> Vec<String> {
    refs.iter()
        .filter(|value| !existing.contains(*value))
        .cloned()
        .collect()
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
                actor_id: "duckdocs-maintenance".into(),
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
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewCreated,
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        relation_refs: issue.relation_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(issue)
            .context("failed to encode lint review event payload")?,
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
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::BrainMaintenanceRun,
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "duckdocs-maintenance".into(),
        },
        source_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: serde_json::to_string(report)
            .context("failed to encode maintenance event payload")?,
        confidence: None,
        policy_result: if report.issue_count == 0 {
            "auto_repaired".into()
        } else {
            "attention_needed".into()
        },
        created_at: report.generated_at,
    })
}

fn is_auto_apply_brain_proposal(kind: BrainProposalKind) -> bool {
    matches!(
        kind,
        BrainProposalKind::Memory | BrainProposalKind::Observation | BrainProposalKind::SourceNote
    )
}

fn validate_brain_update_proposal(request: &ProposeBrainUpdateRequest) -> Result<()> {
    if request.title.trim().is_empty() {
        bail!("brain update proposal title cannot be empty");
    }
    if request.body.trim().is_empty() {
        bail!("brain update proposal body cannot be empty");
    }
    match request.kind {
        BrainProposalKind::Link => {
            if request
                .target_node_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                bail!("link proposal needs --target-node");
            }
            if request.node_refs.is_empty() {
                bail!("link proposal needs at least one --node ref");
            }
            if request.relation_kind.is_none() {
                bail!("link proposal needs --relation");
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
                bail!("source note proposal needs --source");
            }
        }
        BrainProposalKind::Memory
        | BrainProposalKind::Claim
        | BrainProposalKind::Observation
        | BrainProposalKind::WikiPage => {}
    }
    Ok(())
}

fn brain_event_for_proposal(proposal: &BrainUpdateProposal) -> Result<BrainEvent> {
    let event_type = match proposal.kind {
        BrainProposalKind::Memory => BrainEventKind::MemoryProposed,
        BrainProposalKind::Claim => BrainEventKind::ClaimProposed,
        BrainProposalKind::Link => BrainEventKind::LinkProposed,
        BrainProposalKind::Observation => BrainEventKind::ObservationAppended,
        BrainProposalKind::SourceNote => BrainEventKind::SourceNoteProposed,
        BrainProposalKind::WikiPage => BrainEventKind::WikiPageProposed,
    };
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type,
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(proposal)
            .context("failed to encode proposal event payload")?,
        confidence: None,
        policy_result: "needs_review".into(),
        created_at: proposal.created_at,
    })
}

fn memory_record_for_proposal(proposal: &BrainUpdateProposal) -> MemoryRecord {
    MemoryRecord {
        memory_id: format!("memory-{}", brain_proposal_fingerprint(proposal)),
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        source_refs: proposal.source_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        created_at: proposal.created_at,
        updated_at: proposal.created_at,
    }
}

fn brain_memory_accepted_event(proposal: &BrainUpdateProposal) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{}", Uuid::now_v7()),
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::MemoryAccepted,
        actor: proposal.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(proposal)
            .context("failed to encode accepted memory event payload")?,
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
        workspace_id: proposal.workspace_id.clone(),
        scope: proposal.scope,
        event_type: BrainEventKind::ReviewResolved,
        actor: request.actor.clone(),
        source_refs: proposal.source_refs.clone(),
        node_refs: proposal.node_refs.clone(),
        relation_refs: Vec::new(),
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&json!({
            "proposalId": proposal.proposal_id,
            "decision": decision,
            "reason": request.reason,
            "status": proposal.status,
        }))
        .context("failed to encode review resolved event payload")?,
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
    root: PathBuf,
    _lock: WorkspaceLock,
}

impl BrainWorkspaceWriter {
    fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).with_context(|| format!("failed creating {}", root.display()))?;
        let lock = WorkspaceLock::acquire(root.join("brain.lock"))?;
        for dir in [
            root.join("events"),
            root.join("memory"),
            root.join("reviews/proposed-updates"),
        ] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed creating {}", dir.display()))?;
        }
        Ok(Self { root, _lock: lock })
    }

    fn write_proposal(&self, proposal: &BrainUpdateProposal) -> Result<PathBuf> {
        let path = self.proposal_path(&proposal.proposal_id);
        write_json_pretty(&path, proposal)?;
        Ok(path)
    }

    fn proposal_path(&self, proposal_id: &str) -> PathBuf {
        self.root
            .join("reviews/proposed-updates")
            .join(format!("{proposal_id}.json"))
    }

    fn append_event(&self, event: &BrainEvent) -> Result<()> {
        append_brain_event_jsonl(&self.root.join("events/brain_events.jsonl"), event)
    }

    fn upsert_memory_record(&self, memory: MemoryRecord) -> Result<()> {
        let path = self.root.join("memory/records.json");
        let mut memories = read_memory_records(&self.root)?;
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
        write_json_pretty(&path, &memories)
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
            .root
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
        if let Some(output_root) = self.root.parent() {
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
        let manifest_path = self.root.join("brain-manifest.json");
        if !manifest_path.exists() {
            return Ok(());
        }
        let mut snapshot: BrainRepoSnapshot = read_json_artifact(&manifest_path)?;
        apply_accepted_proposal_to_snapshot(proposal, &mut snapshot)?;
        write_json_pretty(&manifest_path, &snapshot)?;
        match proposal.kind {
            BrainProposalKind::Claim => {
                write_json_pretty(&self.root.join("graph/claims.json"), &snapshot.claims)?;
            }
            BrainProposalKind::Link => {
                write_json_pretty(&self.root.join("graph/edges.json"), &snapshot.relations)?;
            }
            BrainProposalKind::WikiPage => {
                let page = wiki_page_for_proposal(proposal);
                let page_path = self.root.join(&page.path);
                write_file_atomic(&page_path, page.body.as_bytes())?;
            }
            BrainProposalKind::Memory
            | BrainProposalKind::Observation
            | BrainProposalKind::SourceNote => {}
        }
        Ok(())
    }

    fn update_brain_manifest_source(&self, manifest: &SourceArtifactManifest) -> Result<()> {
        let path = self.root.join("brain-manifest.json");
        if !path.exists() {
            return Ok(());
        }
        let mut snapshot: BrainRepoSnapshot = read_json_artifact(&path)?;
        if let Some(source) = snapshot
            .sources
            .iter_mut()
            .find(|source| source.source_id == manifest.source_id)
        {
            source.description = manifest.description.clone();
            source.user_context = manifest.user_context.clone();
            source.ingest_instruction = manifest.ingest_instruction.clone();
            source.updated_at = manifest.updated_at;
            write_json_pretty(&path, &snapshot)?;
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
    for (proposal, _) in read_brain_update_proposals(root)? {
        if proposal.workspace_id == snapshot.workspace_id
            && proposal.status == BrainProposalStatus::Accepted
        {
            apply_accepted_proposal_to_snapshot(&proposal, snapshot)?;
        }
    }
    Ok(())
}

fn apply_accepted_proposal_to_snapshot(
    proposal: &BrainUpdateProposal,
    snapshot: &mut BrainRepoSnapshot,
) -> Result<()> {
    match proposal.kind {
        BrainProposalKind::Claim => {
            let claim = claim_record_for_proposal(proposal);
            if let Some(existing) = snapshot
                .claims
                .iter_mut()
                .find(|existing| existing.claim_id == claim.claim_id)
            {
                *existing = claim;
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
            let relation = relation_record_for_proposal(proposal)?;
            if let Some(existing) = snapshot
                .relations
                .iter_mut()
                .find(|existing| existing.relation_id == relation.relation_id)
            {
                *existing = relation;
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
            let page = wiki_page_for_proposal(proposal);
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
        BrainProposalKind::Memory
        | BrainProposalKind::Observation
        | BrainProposalKind::SourceNote => {}
    }
    Ok(())
}

fn claim_record_for_proposal(proposal: &BrainUpdateProposal) -> ClaimRecord {
    ClaimRecord {
        claim_id: format!("claim-{}", brain_proposal_fingerprint(proposal)),
        workspace_id: proposal.workspace_id.clone(),
        statement: proposal.body.clone(),
        topic_refs: proposal.node_refs.clone(),
        source_refs: proposal.source_refs.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        status: if proposal.evidence_refs.is_empty() {
            "accepted".into()
        } else {
            "supported".into()
        },
        updated_at: proposal.created_at,
    }
}

fn relation_record_for_proposal(proposal: &BrainUpdateProposal) -> Result<BrainRelationRecord> {
    let source_node_id = proposal
        .node_refs
        .first()
        .cloned()
        .context("accepted link proposal needs a source node ref")?;
    let target_node_id = proposal
        .target_node_id
        .clone()
        .context("accepted link proposal needs a target node ref")?;
    Ok(BrainRelationRecord {
        relation_id: format!("relation-{}", brain_proposal_fingerprint(proposal)),
        kind: proposal
            .relation_kind
            .unwrap_or(BrainRelationKind::RelatedTo),
        source_node_id,
        target_node_id,
        label: proposal.title.clone(),
        evidence_ids: proposal.evidence_refs.clone(),
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

struct WorkspaceLock {
    path: PathBuf,
}

impl WorkspaceLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    writeln!(file, "pid={}", std::process::id())
                        .with_context(|| format!("failed writing {}", path.display()))?;
                    file.sync_all()
                        .with_context(|| format!("failed syncing {}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() > Duration::from_secs(5) {
                        bail!(
                            "timed out waiting for workspace brain lock {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed acquiring {}", path.display()));
                }
            }
        }
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct BrainReader {
    root: PathBuf,
    snapshot: BrainRepoSnapshot,
    events: Vec<BrainEvent>,
}

impl BrainReader {
    fn open(scope: &BrainReadScope) -> Result<Self> {
        let root = resolve_brain_workspace_root(scope)?;
        let manifest_path = root.join("brain-manifest.json");
        let manifest_json = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed reading {}", manifest_path.display()))?;
        let mut snapshot: BrainRepoSnapshot = serde_json::from_str(&manifest_json)
            .with_context(|| format!("failed decoding {}", manifest_path.display()))?;
        if snapshot.workspace_id != scope.workspace_id {
            bail!(
                "brain manifest workspace_id {} does not match requested workspace {}",
                snapshot.workspace_id,
                scope.workspace_id
            );
        }
        snapshot.nodes = read_json_artifact(&root.join("graph/nodes.json"))?;
        snapshot.relations = read_json_artifact(&root.join("graph/edges.json"))?;
        snapshot.evidence = read_json_artifact(&root.join("graph/evidence.json"))?;
        if let Ok(entities) = read_json_artifact(&root.join("graph/entities.json")) {
            snapshot.entities = entities;
        }
        if let Ok(claims) = read_json_artifact(&root.join("graph/claims.json")) {
            snapshot.claims = claims;
        }
        snapshot.memories = read_memory_records(&root)?;
        let events = read_brain_events_jsonl(&root.join("events/brain_events.jsonl"))?;
        snapshot.events = events.clone();
        Ok(Self {
            root,
            snapshot,
            events,
        })
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
        let path = self.root.join(&page.path);
        page.body = fs::read_to_string(&path)
            .with_context(|| format!("failed reading {}", path.display()))?;
        Ok(page)
    }

    fn recent_events(&self, limit: usize) -> Vec<BrainEvent> {
        let mut events = self.events.clone();
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.event_id.cmp(&left.event_id))
        });
        events.truncate(limit);
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
        let recent_events = self.recent_events(5);
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
    if let Some(output_root) = std::env::var_os("DUCKDOCS_OUTPUT_DIR") {
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

fn read_json_artifact<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let json =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("failed decoding {}", path.display()))
}

fn read_optional_json_artifact<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    read_json_artifact(path)
}

fn read_memory_records(root: &Path) -> Result<Vec<MemoryRecord>> {
    read_optional_json_artifact(&root.join("memory/records.json"))
}

fn read_brain_events_jsonl(path: &Path) -> Result<Vec<BrainEvent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("failed decoding brain event JSONL row"))
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
        return store
            .load_workspace_project(workspace_id)?
            .ok_or_else(|| anyhow!("workspace {workspace_id} was not found"));
    }

    store
        .load_project(Some(project_id))?
        .ok_or_else(|| anyhow!("project {project_id} was not found"))
}

#[derive(Debug, Clone)]
struct PageSection {
    page_index: usize,
    page_label: String,
    content: String,
    markdown_path: Option<String>,
    image_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ConceptAccumulator {
    id: String,
    label: String,
    aliases: BTreeSet<String>,
    evidence: Vec<EvidenceRef>,
    page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ExtractionEvidenceRef {
    id: String,
    page_index: usize,
    page_label: String,
    snippet: String,
    source_path: String,
    source_id: Option<String>,
    markdown_path: Option<String>,
    image_path: Option<String>,
    provenance: String,
}

#[derive(Debug, Clone)]
struct ExtractedConcept {
    id: String,
    label: String,
    aliases: BTreeSet<String>,
    evidence_ids: Vec<String>,
    page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ExtractedClaim {
    id: String,
    text: String,
    subject_concept_id: String,
    evidence_id: String,
}

#[derive(Debug, Clone)]
struct ExtractedRelation {
    source_concept_id: String,
    target_concept_id: String,
    evidence_ids: Vec<String>,
    page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ExtractionArtifact {
    concepts: Vec<ExtractedConcept>,
    claims: Vec<ExtractedClaim>,
    relations: Vec<ExtractedRelation>,
    evidence_refs: BTreeMap<String, ExtractionEvidenceRef>,
}

#[derive(Debug, Clone)]
struct PageConceptSet {
    page_index: usize,
    page_label: String,
    concept_ids: Vec<String>,
    snippet: String,
    markdown_path: Option<String>,
    image_path: Option<String>,
}

#[derive(Debug, Clone)]
struct CollectedConcepts {
    concepts: Vec<ConceptAccumulator>,
    page_concepts: Vec<PageConceptSet>,
}

#[derive(Debug, Clone)]
struct EdgeAccumulator {
    source_node_id: String,
    target_node_id: String,
    evidence: Vec<EvidenceRef>,
    page_labels: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct StoredSourceRow {
    summary: SourceSummary,
    project_id: String,
    manifest_path: String,
}

#[derive(Debug, Clone)]
struct WorkspaceConceptAccumulator {
    node_id: String,
    canonical_name: String,
    aliases: BTreeSet<String>,
    evidence: Vec<EvidenceRef>,
    confidence: Option<f32>,
}

fn compile_knowledge_project(
    request: &CompileProjectRequest,
    markdown: &str,
    source_manifest: Option<&SourceArtifactManifest>,
) -> KnowledgeProject {
    let title = infer_markdown_title(&request.source_markdown_path, markdown);
    let mut page_sections = extract_page_sections(markdown);
    attach_page_artifacts_to_sections(&mut page_sections, source_manifest);
    let source_path = request
        .source_document_path
        .clone()
        .unwrap_or_else(|| request.source_markdown_path.clone());
    let source_node_id = source_manifest
        .map(|manifest| source_node_id(&manifest.source_id))
        .unwrap_or_else(|| "document".into());
    let source_label = source_manifest
        .map(source_label_from_manifest)
        .unwrap_or_else(|| title.clone());
    let source_path_for_evidence = source_manifest
        .map(|manifest| manifest.source_path.clone())
        .unwrap_or_else(|| source_path.clone());
    let source_id_for_evidence = source_manifest.map(|manifest| manifest.source_id.clone());
    let project_id = source_manifest
        .map(|manifest| build_source_backed_project_id(&manifest.workspace_id, &manifest.source_id))
        .unwrap_or_else(|| build_project_id(request));

    let extraction = build_extraction_artifact(
        &page_sections,
        &source_path_for_evidence,
        source_id_for_evidence.as_deref(),
    );
    let collected = collected_concepts_from_artifact(&extraction);
    let concept_accumulators = collected.concepts;
    let concept_count = concept_accumulators.len();
    let mut document_node = GraphNodeSummary {
        id: source_node_id.clone(),
        label: source_label.clone(),
        kind: source_manifest
            .map(|_| GraphNodeKind::Source)
            .unwrap_or(GraphNodeKind::Document),
        confidence: Some(if concept_count > 0 { 0.78 } else { 0.42 }),
        related_count: 0,
        evidence_count: page_sections.len(),
        position: GraphNodePosition { x: 50.0, y: 14.0 },
    };

    let concept_positions = layout_concept_positions(concept_count.max(1));
    let mut concept_nodes = concept_accumulators
        .iter()
        .enumerate()
        .map(|(index, concept)| GraphNodeSummary {
            id: concept.id.clone(),
            label: concept.label.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some((0.62 + (concept.evidence.len().min(3) as f32 * 0.08)).min(0.91)),
            related_count: 0,
            evidence_count: concept.evidence.len(),
            position: concept_positions
                .get(index)
                .cloned()
                .unwrap_or(GraphNodePosition { x: 50.0, y: 54.0 }),
        })
        .collect::<Vec<_>>();

    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut details_by_node_id = BTreeMap::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut answer_by_node_id = BTreeMap::new();
    let document_evidence = page_sections
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, section)| EvidenceRef {
            id: format!("ev-document-{}", index + 1),
            page_label: section.page_label.clone(),
            page_index: Some(section.page_index),
            snippet: excerpt(&section.content, 180),
            source_path: Some(source_path_for_evidence.clone()),
            source_id: source_id_for_evidence.clone(),
            markdown_path: section.markdown_path.clone(),
            image_path: section.image_path.clone(),
            provenance: Some(format!(
                "Document-level evidence extracted from {}.",
                section.page_label
            )),
        })
        .collect::<Vec<_>>();

    let (edges, built_edge_details_by_id, related_count_by_node_id, connected_node_ids_by_node_id) =
        build_relation_edges(
            &document_node,
            &concept_accumulators,
            &collected.page_concepts,
            &source_path_for_evidence,
            source_id_for_evidence.as_deref(),
        );
    edge_details_by_id.extend(built_edge_details_by_id);
    document_node.related_count = related_count_by_node_id
        .get(document_node.id.as_str())
        .copied()
        .unwrap_or(0);
    for node in &mut concept_nodes {
        node.related_count = related_count_by_node_id
            .get(node.id.as_str())
            .copied()
            .unwrap_or(0);
    }

    details_by_node_id.insert(
        document_node.id.clone(),
        GraphNodeDetail {
            node: document_node.clone(),
            canonical_name: source_label.clone(),
            aliases: vec![if source_manifest.is_some() {
                "Immutable source".into()
            } else {
                "Imported document".into()
            }],
            description: format!(
                "HyprDuck compiled {} concept nodes from {} visible page sections. Every node below keeps direct evidence back to the imported document.",
                concept_count,
                page_sections.len()
            ),
            evidence: document_evidence.clone(),
            actions: Vec::new(),
            source: source_manifest.map(source_backing_from_manifest),
        },
    );
    answer_by_node_id.insert(
        document_node.id.clone(),
        AnswerResponse {
            status: if concept_count > 0 {
                AnswerStatus::Grounded
            } else {
                AnswerStatus::LowConfidence
            },
            text: Some(format!(
                "HyprDuck found {} concept nodes across {} page sections in this import.",
                concept_count,
                page_sections.len()
            )),
            explanation:
                "This document-level answer is grounded in the concept nodes and visible evidence HyprDuck compiled from the markdown package."
                    .into(),
            citations: document_evidence.clone(),
            related_node_ids: connected_node_ids_by_node_id
                .get(document_node.id.as_str())
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default(),
            suggested_actions: vec![
                SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Review the cited snippets before trusting the document-wide summary."
                            .into(),
                },
                SuggestedAction {
                    kind: SuggestedActionKind::AskDifferentQuestion,
                    label: "Ask a narrower question".into(),
                    description:
                        "Grounded answers get stronger when you focus on one concept at a time."
                            .into(),
                },
            ],
        },
    );

    for node in &concept_nodes {
        let concept = concept_by_id
            .get(&node.id)
            .expect("concept node should have backing accumulator");
        let aliases = concept
            .aliases
            .iter()
            .filter(|alias| alias.as_str() != concept.label)
            .cloned()
            .collect::<Vec<_>>();
        let actions = correction_actions_for_detail(&concept.label, &aliases);
        details_by_node_id.insert(
            node.id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: concept.label.clone(),
                aliases,
                description: format!(
                    "Compiled from {} evidence refs across {} page(s). HyprDuck is still conservative and only shows evidence-backed concept nodes.",
                    concept.evidence.len(),
                    concept.page_labels.len()
                ),
                evidence: concept.evidence.clone(),
                actions,
                source: None,
            },
        );
        answer_by_node_id.insert(
            node.id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some(format!(
                    "{} appears in {} evidence refs across {} page(s).",
                    concept.label,
                    concept.evidence.len(),
                    concept.page_labels.len()
                )),
                explanation:
                    "This answer is grounded in the evidence attached to the selected concept node."
                        .into(),
                citations: concept.evidence.iter().take(3).cloned().collect(),
                related_node_ids: connected_node_ids_by_node_id
                    .get(node.id.as_str())
                    .map(|related| related.iter().cloned().collect())
                    .unwrap_or_else(|| vec![source_node_id.clone()]),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the concept before acting on it.".into(),
                }],
            },
        );
    }

    let mut nodes = Vec::with_capacity(concept_nodes.len() + 1);
    nodes.push(document_node.clone());
    nodes.extend(concept_nodes.iter().cloned());

    let evidence_count = concept_accumulators
        .iter()
        .map(|concept| concept.evidence.len())
        .sum::<usize>()
        + document_evidence.len()
        + edges.iter().map(|edge| edge.evidence_count).sum::<usize>();

    KnowledgeProject {
        summary: ProjectOverview {
            project_id,
            title,
            status: if concept_count > 0 {
                ProjectStatus::Ready
            } else {
                ProjectStatus::Degraded
            },
            stale: false,
            summary: format!(
                "Compiled {} concept nodes from {} page sections. HyprDuck only shows nodes with visible evidence.",
                concept_count,
                page_sections.len()
            ),
            document_count: 1,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

fn aggregate_workspace_project(
    workspace_id: &str,
    rows: Vec<(StoredSourceRow, Option<KnowledgeProject>)>,
) -> KnowledgeProject {
    let source_count = rows.len();
    let mut source_nodes = Vec::new();
    let mut source_details = BTreeMap::new();
    let mut source_answers = BTreeMap::new();
    let mut concept_accumulators = BTreeMap::<String, WorkspaceConceptAccumulator>::new();
    let mut aggregate_key_by_concept_key = BTreeMap::<String, String>::new();
    let mut source_concept_edges = BTreeMap::<String, StoredEdgeAccumulator>::new();
    let mut relation_edges = BTreeMap::<String, StoredEdgeAccumulator>::new();

    for (source_index, (row, project)) in rows.iter().enumerate() {
        let source_node_id = source_node_id(&row.summary.source_id);
        let source_label = source_label_from_summary(&row.summary);
        let source_detail_from_project = project.as_ref().and_then(|project| {
            project.details_by_node_id.get(&source_node_id).or_else(|| {
                project
                    .details_by_node_id
                    .values()
                    .find(|detail| is_source_like_node_kind(detail.node.kind))
            })
        });
        let source_evidence = source_detail_from_project
            .map(|detail| detail.evidence.clone())
            .unwrap_or_default();
        let source_node = GraphNodeSummary {
            id: source_node_id.clone(),
            label: source_label.clone(),
            kind: GraphNodeKind::Source,
            confidence: Some(if project.is_some() { 0.72 } else { 0.28 }),
            related_count: 0,
            evidence_count: source_evidence.len().max(row.summary.success_count),
            position: source_node_position(source_index, source_count),
        };
        source_nodes.push(source_node.clone());
        source_details.insert(
            source_node_id.clone(),
            GraphNodeDetail {
                node: source_node.clone(),
                canonical_name: source_label.clone(),
                aliases: vec!["Workspace source".into()],
                description: format!(
                    "Immutable source in workspace {workspace_id}. HyprDuck keeps source artifacts addressable while graph evidence is aggregated across the workspace."
                ),
                evidence: source_evidence.clone(),
                actions: Vec::new(),
                source: Some(source_backing_from_summary(&row.summary, &row.manifest_path)),
            },
        );
        source_answers.insert(
            source_node_id.clone(),
            AnswerResponse {
                status: if project.is_some() {
                    AnswerStatus::LowConfidence
                } else {
                    AnswerStatus::Blocked
                },
                text: None,
                explanation: if project.is_some() {
                    "This source contributes evidence to the workspace graph.".into()
                } else {
                    "This source is registered in the workspace, but no compiled graph snapshot was found yet.".into()
                },
                citations: source_evidence.iter().take(3).cloned().collect(),
                related_node_ids: Vec::new(),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect source artifacts".into(),
                    description:
                        "Open the source detail inspector to review copied source and derived artifacts."
                            .into(),
                }],
            },
        );

        let Some(project) = project else {
            continue;
        };

        let mut concept_id_map = BTreeMap::<String, String>::new();
        for detail in project.details_by_node_id.values() {
            if detail.node.kind != GraphNodeKind::Concept {
                continue;
            }
            let concept_keys = concept_identity_keys(detail);
            let Some(canonical_key) = concept_keys.first().cloned() else {
                continue;
            };
            let existing_aggregate_keys = concept_keys
                .iter()
                .filter_map(|key| aggregate_key_by_concept_key.get(key).cloned())
                .collect::<BTreeSet<_>>();
            let aggregate_key = existing_aggregate_keys
                .iter()
                .next()
                .cloned()
                .unwrap_or(canonical_key);
            if !existing_aggregate_keys.is_empty() {
                merge_workspace_concept_groups(
                    &aggregate_key,
                    &existing_aggregate_keys,
                    &mut concept_accumulators,
                    &mut aggregate_key_by_concept_key,
                );
            }
            for key in &concept_keys {
                aggregate_key_by_concept_key.insert(key.clone(), aggregate_key.clone());
            }
            let aggregate_node_id = format!("concept-{aggregate_key}");
            concept_id_map.insert(detail.node.id.clone(), aggregate_node_id.clone());
            let accumulator = concept_accumulators
                .entry(aggregate_key)
                .or_insert_with(|| WorkspaceConceptAccumulator {
                    node_id: aggregate_node_id.clone(),
                    canonical_name: detail.canonical_name.clone(),
                    aliases: BTreeSet::new(),
                    evidence: Vec::new(),
                    confidence: detail.node.confidence,
                });
            accumulator.aliases.extend(detail.aliases.iter().cloned());
            if accumulator.canonical_name != detail.canonical_name {
                accumulator.aliases.insert(detail.canonical_name.clone());
            }
            accumulator.evidence.extend(detail.evidence.iter().cloned());
            accumulator.confidence = match (accumulator.confidence, detail.node.confidence) {
                (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };

            let edge_id = relation_edge_id(
                RelationKind::SourceDocument,
                &source_node_id,
                &aggregate_node_id,
            );
            let edge_accumulator =
                source_concept_edges
                    .entry(edge_id)
                    .or_insert_with(|| StoredEdgeAccumulator {
                        kind: RelationKind::SourceDocument,
                        source_node_id: source_node_id.clone(),
                        target_node_id: aggregate_node_id.clone(),
                        label: "Compiled from source".into(),
                        confidence: Some(0.76),
                        evidence: Vec::new(),
                    });
            edge_accumulator
                .evidence
                .extend(detail.evidence.iter().cloned());
        }

        for edge in &project.edges {
            if edge.kind != RelationKind::RelatedTo {
                continue;
            }
            let Some(left) = concept_id_map.get(&edge.source_node_id).cloned() else {
                continue;
            };
            let Some(right) = concept_id_map.get(&edge.target_node_id).cloned() else {
                continue;
            };
            if left == right {
                continue;
            }
            let (source_node_id, target_node_id) = if left <= right {
                (left, right)
            } else {
                (right, left)
            };
            let edge_id =
                relation_edge_id(RelationKind::RelatedTo, &source_node_id, &target_node_id);
            let evidence = project
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| detail.evidence.clone())
                .unwrap_or_default();
            let accumulator =
                relation_edges
                    .entry(edge_id)
                    .or_insert_with(|| StoredEdgeAccumulator {
                        kind: RelationKind::RelatedTo,
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        label: normalized_edge_label(RelationKind::RelatedTo, &edge.label),
                        confidence: edge.confidence,
                        evidence: Vec::new(),
                    });
            accumulator.label =
                preferred_edge_label(&accumulator.label, &edge.label, RelationKind::RelatedTo);
            accumulator.confidence = match (accumulator.confidence, edge.confidence) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };
            accumulator.evidence.extend(evidence.into_iter());
        }
    }

    source_concept_edges =
        remap_workspace_edge_accumulators(source_concept_edges, &aggregate_key_by_concept_key);
    relation_edges =
        remap_workspace_edge_accumulators(relation_edges, &aggregate_key_by_concept_key);
    for accumulator in concept_accumulators.values_mut() {
        accumulator.evidence = dedupe_evidence(std::mem::take(&mut accumulator.evidence));
    }

    let concept_positions = layout_concept_positions(concept_accumulators.len().max(1));
    let mut concept_nodes = Vec::new();
    let mut details_by_node_id = source_details;
    let mut answer_by_node_id = source_answers;
    for (index, accumulator) in concept_accumulators.values().enumerate() {
        let aliases = accumulator
            .aliases
            .iter()
            .filter(|alias| alias.as_str() != accumulator.canonical_name)
            .cloned()
            .collect::<Vec<_>>();
        let source_ids = accumulator
            .evidence
            .iter()
            .filter_map(|evidence| evidence.source_id.clone())
            .collect::<BTreeSet<_>>();
        let node = GraphNodeSummary {
            id: accumulator.node_id.clone(),
            label: accumulator.canonical_name.clone(),
            kind: GraphNodeKind::Concept,
            confidence: accumulator.confidence,
            related_count: 0,
            evidence_count: accumulator.evidence.len(),
            position: concept_positions
                .get(index)
                .cloned()
                .unwrap_or(GraphNodePosition { x: 50.0, y: 54.0 }),
        };
        concept_nodes.push(node.clone());
        details_by_node_id.insert(
            node.id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: accumulator.canonical_name.clone(),
                aliases,
                description: format!(
                    "Workspace concept compiled from {} evidence refs across {} source(s).",
                    accumulator.evidence.len(),
                    source_ids.len()
                ),
                evidence: accumulator.evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );
        answer_by_node_id.insert(
            node.id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some(format!(
                    "{} appears in {} evidence refs across {} source(s).",
                    accumulator.canonical_name,
                    accumulator.evidence.len(),
                    source_ids.len()
                )),
                explanation:
                    "This workspace answer is grounded in evidence aggregated from source-backed imports."
                        .into(),
                citations: accumulator.evidence.iter().take(3).cloned().collect(),
                related_node_ids: Vec::new(),
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the workspace concept before acting on it."
                            .into(),
                }],
            },
        );
    }

    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    for accumulator in source_concept_edges
        .into_values()
        .chain(relation_edges.into_values())
    {
        let edge_id = relation_edge_id(
            accumulator.kind,
            &accumulator.source_node_id,
            &accumulator.target_node_id,
        );
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id: accumulator.source_node_id,
            target_node_id: accumulator.target_node_id,
            kind: accumulator.kind,
            label: accumulator.label,
            confidence: accumulator.confidence,
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence: accumulator.evidence,
            },
        );
        edges.push(edge);
    }

    let mut nodes = Vec::with_capacity(source_nodes.len() + concept_nodes.len());
    nodes.extend(source_nodes);
    nodes.extend(concept_nodes);
    finalize_workspace_project(
        workspace_id,
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
        source_count,
    )
}

fn merge_workspace_concept_groups(
    aggregate_key: &str,
    existing_aggregate_keys: &BTreeSet<String>,
    concept_accumulators: &mut BTreeMap<String, WorkspaceConceptAccumulator>,
    aggregate_key_by_concept_key: &mut BTreeMap<String, String>,
) {
    if existing_aggregate_keys.len() <= 1 {
        return;
    }

    let mut merged_accumulator = concept_accumulators
        .remove(aggregate_key)
        .unwrap_or_else(|| WorkspaceConceptAccumulator {
            node_id: format!("concept-{aggregate_key}"),
            canonical_name: aggregate_key.to_string(),
            aliases: BTreeSet::new(),
            evidence: Vec::new(),
            confidence: None,
        });

    for stale_key in existing_aggregate_keys {
        if stale_key == aggregate_key {
            continue;
        }
        if let Some(stale_accumulator) = concept_accumulators.remove(stale_key) {
            merged_accumulator
                .aliases
                .insert(stale_accumulator.canonical_name.clone());
            merged_accumulator
                .aliases
                .extend(stale_accumulator.aliases.into_iter());
            merged_accumulator
                .evidence
                .extend(stale_accumulator.evidence.into_iter());
            merged_accumulator.confidence =
                match (merged_accumulator.confidence, stale_accumulator.confidence) {
                    (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
                    (Some(left), None) => Some(left),
                    (None, Some(right)) => Some(right),
                    (None, None) => None,
                };
        }
    }

    merged_accumulator.evidence = dedupe_evidence(merged_accumulator.evidence);
    for mapped_aggregate_key in aggregate_key_by_concept_key.values_mut() {
        if existing_aggregate_keys.contains(mapped_aggregate_key) {
            *mapped_aggregate_key = aggregate_key.to_string();
        }
    }
    concept_accumulators.insert(aggregate_key.to_string(), merged_accumulator);
}

fn remap_workspace_edge_accumulators(
    accumulators: BTreeMap<String, StoredEdgeAccumulator>,
    aggregate_key_by_concept_key: &BTreeMap<String, String>,
) -> BTreeMap<String, StoredEdgeAccumulator> {
    let mut remapped = BTreeMap::<String, StoredEdgeAccumulator>::new();
    for mut accumulator in accumulators.into_values() {
        accumulator.source_node_id = remap_workspace_concept_node_id(
            &accumulator.source_node_id,
            aggregate_key_by_concept_key,
        );
        accumulator.target_node_id = remap_workspace_concept_node_id(
            &accumulator.target_node_id,
            aggregate_key_by_concept_key,
        );
        if accumulator.source_node_id == accumulator.target_node_id {
            continue;
        }
        let edge_id = relation_edge_id(
            accumulator.kind,
            &accumulator.source_node_id,
            &accumulator.target_node_id,
        );
        let existing = remapped
            .entry(edge_id)
            .or_insert_with(|| StoredEdgeAccumulator {
                kind: accumulator.kind,
                source_node_id: accumulator.source_node_id.clone(),
                target_node_id: accumulator.target_node_id.clone(),
                label: accumulator.label.clone(),
                confidence: accumulator.confidence,
                evidence: Vec::new(),
            });
        existing.label =
            preferred_edge_label(&existing.label, &accumulator.label, accumulator.kind);
        existing.confidence = match (existing.confidence, accumulator.confidence) {
            (Some(left), Some(right)) => Some(left.max(right).min(0.94)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        existing.evidence.extend(accumulator.evidence.into_iter());
    }
    for accumulator in remapped.values_mut() {
        accumulator.evidence = dedupe_evidence(std::mem::take(&mut accumulator.evidence));
    }
    remapped
}

fn remap_workspace_concept_node_id(
    node_id: &str,
    aggregate_key_by_concept_key: &BTreeMap<String, String>,
) -> String {
    let Some(key) = node_id.strip_prefix("concept-") else {
        return node_id.to_string();
    };
    aggregate_key_by_concept_key
        .get(key)
        .map(|aggregate_key| format!("concept-{aggregate_key}"))
        .unwrap_or_else(|| node_id.to_string())
}

fn finalize_workspace_project(
    workspace_id: &str,
    mut nodes: Vec<GraphNodeSummary>,
    edges: Vec<RelationEdgeSummary>,
    mut details_by_node_id: BTreeMap<String, GraphNodeDetail>,
    mut edge_details_by_id: BTreeMap<String, RelationEdgeDetail>,
    mut answer_by_node_id: BTreeMap<String, AnswerResponse>,
    source_count: usize,
) -> KnowledgeProject {
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    for edge in &edges {
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
    }
    for node in &mut nodes {
        node.related_count = related_count_by_node_id.get(&node.id).copied().unwrap_or(0);
        if let Some(detail) = details_by_node_id.get_mut(&node.id) {
            detail.node = node.clone();
        }
        if let Some(answer) = answer_by_node_id.get_mut(&node.id) {
            answer.related_node_ids = connected_node_ids_by_node_id
                .get(&node.id)
                .map(|related| related.iter().cloned().collect())
                .unwrap_or_default();
        }
    }

    let label_by_node_id = nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    for edge in &edges {
        if let Some(detail) = edge_details_by_id.get_mut(&edge.id) {
            detail.edge = edge.clone();
            detail.explanation = edge_explanation(edge, &label_by_node_id, &detail.evidence);
        }
    }

    let concept_count = nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let evidence_count = details_by_node_id
        .values()
        .map(|detail| detail.evidence.len())
        .sum::<usize>()
        + edge_details_by_id
            .values()
            .map(|detail| detail.evidence.len())
            .sum::<usize>();

    KnowledgeProject {
        summary: ProjectOverview {
            project_id: workspace_project_id(workspace_id),
            title: "Workspace knowledge".into(),
            status: if concept_count > 0 {
                ProjectStatus::Ready
            } else {
                ProjectStatus::Degraded
            },
            stale: false,
            summary: format!(
                "Workspace contains {} sources, {} concept nodes, and {} evidence-backed relationships.",
                source_count,
                concept_count,
                edges.len()
            ),
            document_count: source_count,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

fn workspace_project_id(workspace_id: &str) -> String {
    format!("workspace:{workspace_id}")
}

fn workspace_id_from_project_id(project_id: &str) -> Option<&str> {
    project_id
        .strip_prefix("workspace:")
        .filter(|workspace_id| !workspace_id.trim().is_empty())
}

fn matching_source_concept_node_ids(
    project: &KnowledgeProject,
    aggregate_detail: &GraphNodeDetail,
) -> Vec<String> {
    let aggregate_keys = concept_identity_keys(aggregate_detail)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let aggregate_source_ids = aggregate_detail
        .evidence
        .iter()
        .filter_map(|evidence| evidence.source_id.as_deref())
        .collect::<BTreeSet<_>>();

    project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter(|detail| {
            concept_identity_keys(detail)
                .iter()
                .any(|key| aggregate_keys.contains(key))
                && (aggregate_source_ids.is_empty()
                    || detail.evidence.iter().any(|evidence| {
                        evidence
                            .source_id
                            .as_deref()
                            .is_some_and(|source_id| aggregate_source_ids.contains(source_id))
                    }))
        })
        .map(|detail| detail.node.id.clone())
        .collect()
}

fn source_label_from_summary(summary: &SourceSummary) -> String {
    Path::new(&summary.original_path)
        .file_name()
        .or_else(|| Path::new(&summary.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| summary.source_id.clone())
}

fn source_backing_from_summary(summary: &SourceSummary, manifest_path: &str) -> SourceBacking {
    SourceBacking {
        workspace_id: summary.workspace_id.clone(),
        source_id: summary.source_id.clone(),
        original_path: summary.original_path.clone(),
        source_path: summary.source_path.clone(),
        markdown_path: summary.markdown_path.clone(),
        format: document_format_slug(&summary.format).into(),
        status: ingest_status_slug(&summary.status).into(),
        page_count: summary.page_count,
        success_count: summary.success_count,
        failed_count: summary.failed_count,
        description: summary.description.clone(),
        user_context: summary.user_context.clone(),
        ingest_instruction: summary.ingest_instruction.clone(),
        updated_at: summary.updated_at,
        manifest_path: Some(manifest_path.to_string()),
    }
}

fn build_brain_repo_snapshot(
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    aggregate: &KnowledgeProject,
    corrections: &[WorkspaceCorrection],
) -> BrainRepoSnapshot {
    let generated_at = unix_timestamp_seconds();
    let sources = rows
        .iter()
        .map(|(row, _)| SourceRecord {
            source_id: row.summary.source_id.clone(),
            workspace_id: row.summary.workspace_id.clone(),
            original_path: row.summary.original_path.clone(),
            source_path: row.summary.source_path.clone(),
            markdown_path: row.summary.markdown_path.clone(),
            format: document_format_slug(&row.summary.format).into(),
            status: ingest_status_slug(&row.summary.status).into(),
            page_count: row.summary.page_count,
            description: row.summary.description.clone(),
            user_context: row.summary.user_context.clone(),
            ingest_instruction: row.summary.ingest_instruction.clone(),
            updated_at: row.summary.updated_at,
        })
        .collect::<Vec<_>>();

    let mut evidence_by_id = BTreeMap::<String, EvidenceRef>::new();
    for detail in aggregate.details_by_node_id.values() {
        for evidence in &detail.evidence {
            evidence_by_id.insert(evidence.id.clone(), evidence.clone());
        }
    }
    for detail in aggregate.edge_details_by_id.values() {
        for evidence in &detail.evidence {
            evidence_by_id.insert(evidence.id.clone(), evidence.clone());
        }
    }

    let nodes = aggregate
        .details_by_node_id
        .values()
        .map(|detail| {
            let evidence_ids = detail
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let source_ids = detail
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            BrainNodeRecord {
                node_id: detail.node.id.clone(),
                kind: brain_node_kind_for_graph_kind(detail.node.kind),
                label: detail.canonical_name.clone(),
                scope: BrainScope::Project,
                aliases: detail.aliases.clone(),
                evidence_ids,
                source_ids,
                confidence: detail.node.confidence,
                updated_at: generated_at,
            }
        })
        .collect::<Vec<_>>();

    let entities = nodes
        .iter()
        .filter(|node| matches!(node.kind, BrainNodeKind::Concept | BrainNodeKind::Topic))
        .map(|node| EntityRecord {
            entity_id: format!("ent-{}", node.node_id),
            workspace_id: workspace_id.to_string(),
            kind: node.kind,
            name: node.label.clone(),
            aliases: node.aliases.clone(),
            source_refs: node.source_ids.clone(),
            evidence_refs: node.evidence_ids.clone(),
            updated_at: generated_at,
        })
        .collect::<Vec<_>>();

    let claims = aggregate
        .details_by_node_id
        .values()
        .filter(|detail| {
            matches!(
                detail.node.kind,
                GraphNodeKind::Concept | GraphNodeKind::Page
            )
        })
        .filter(|detail| !detail.evidence.is_empty())
        .map(|detail| {
            let evidence_refs = detail
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let source_refs = detail
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let first_evidence = detail
                .evidence
                .first()
                .map(|evidence| evidence.snippet.trim())
                .filter(|snippet| !snippet.is_empty())
                .unwrap_or(&detail.description);
            ClaimRecord {
                claim_id: format!("claim-{}", detail.node.id),
                workspace_id: workspace_id.to_string(),
                statement: format!(
                    "{} is evidence-backed by: {}",
                    detail.canonical_name, first_evidence
                ),
                topic_refs: vec![detail.node.id.clone()],
                source_refs,
                evidence_refs,
                status: "supported".into(),
                updated_at: generated_at,
            }
        })
        .collect::<Vec<_>>();

    let relations = aggregate
        .edges
        .iter()
        .filter_map(|edge| {
            let evidence_ids = aggregate
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| {
                    detail
                        .evidence
                        .iter()
                        .map(|evidence| evidence.id.clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if evidence_ids.is_empty() {
                return None;
            }
            Some(BrainRelationRecord {
                relation_id: edge.id.clone(),
                kind: brain_relation_kind_for_relation_kind(edge.kind),
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                label: edge.label.clone(),
                evidence_ids,
                confidence: edge.confidence,
                updated_at: generated_at,
            })
        })
        .collect::<Vec<_>>();

    let extractions = build_structured_extraction_artifacts(workspace_id, rows, generated_at);
    let wiki_pages = build_materialized_wiki_pages(workspace_id, &sources, &nodes, generated_at);
    let mut events = vec![
        BrainEvent {
            event_id: format!("evt-{workspace_id}-graph-materialized-{generated_at}"),
            workspace_id: workspace_id.to_string(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::GraphMaterialized,
            actor: BrainActor {
                actor_type: BrainActorType::System,
                actor_id: "duckdocs-engine".into(),
            },
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect::<Vec<_>>(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            relation_refs: relations
                .iter()
                .map(|relation| relation.relation_id.clone())
                .collect(),
            evidence_refs: evidence_by_id.keys().cloned().collect(),
            payload_json: format!(
                "{{\"nodeCount\":{},\"relationCount\":{},\"sourceCount\":{}}}",
                nodes.len(),
                relations.len(),
                sources.len()
            ),
            confidence: None,
            policy_result: "materialized".into(),
            created_at: generated_at,
        },
        BrainEvent {
            event_id: format!("evt-{workspace_id}-wiki-materialized-{generated_at}"),
            workspace_id: workspace_id.to_string(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::WikiMaterialized,
            actor: BrainActor {
                actor_type: BrainActorType::System,
                actor_id: "duckdocs-engine".into(),
            },
            source_refs: Vec::new(),
            node_refs: wiki_pages
                .iter()
                .flat_map(|page| page.node_refs.clone())
                .collect(),
            relation_refs: Vec::new(),
            evidence_refs: wiki_pages
                .iter()
                .flat_map(|page| page.evidence_refs.clone())
                .collect(),
            payload_json: format!("{{\"pageCount\":{}}}", wiki_pages.len()),
            confidence: None,
            policy_result: "materialized".into(),
            created_at: generated_at,
        },
    ];
    events.extend(corrections.iter().map(|correction| {
        BrainEvent {
            event_id: format!("evt-{}", correction.id),
            workspace_id: correction.workspace_id.clone(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::CorrectionApplied,
            actor: BrainActor {
                actor_type: BrainActorType::User,
                actor_id: "local-user".into(),
            },
            source_refs: Vec::new(),
            node_refs: std::iter::once(correction.aggregate_node_id.clone())
                .chain(correction.target_node_id.clone())
                .chain(correction.source_node_ids.clone())
                .collect(),
            relation_refs: Vec::new(),
            evidence_refs: correction.evidence_ids.clone(),
            payload_json: format!(
                "{{\"kind\":\"{}\",\"value\":{}}}",
                correction_kind_slug(&correction.kind),
                correction
                    .value
                    .as_ref()
                    .map(|value| json!(value).to_string())
                    .unwrap_or_else(|| "null".into())
            ),
            confidence: None,
            policy_result: "applied".into(),
            created_at: correction.created_at,
        }
    }));

    BrainRepoSnapshot {
        workspace_id: workspace_id.to_string(),
        generated_at,
        sources,
        nodes,
        relations,
        evidence: evidence_by_id.into_values().collect(),
        memories: Vec::new(),
        wiki_pages,
        entities,
        claims,
        extractions,
        events,
    }
}

fn build_structured_extraction_artifacts(
    workspace_id: &str,
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
    generated_at: u64,
) -> Vec<StructuredExtractionArtifact> {
    rows.iter()
        .map(|(row, project)| {
            build_structured_extraction_artifact_for_source(
                workspace_id,
                row,
                project.as_ref(),
                generated_at,
            )
        })
        .collect()
}

fn build_structured_extraction_artifact_for_source(
    workspace_id: &str,
    row: &StoredSourceRow,
    project: Option<&KnowledgeProject>,
    generated_at: u64,
) -> StructuredExtractionArtifact {
    let source_id = row.summary.source_id.clone();
    let source_refs = vec![source_id.clone()];
    let Some(project) = project else {
        return StructuredExtractionArtifact {
            artifact_id: format!("extraction-{source_id}"),
            workspace_id: workspace_id.into(),
            source_id,
            extractor: "heuristic".into(),
            extractor_model: None,
            source_refs,
            page_refs: Vec::new(),
            entities: Vec::new(),
            topics: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            evidence_refs: Vec::new(),
            confidence: Some(0.0),
            provenance: "No compiled project snapshot was available for this source.".into(),
            created_at: generated_at,
        };
    };

    let mut evidence_by_id = BTreeMap::<String, EvidenceRef>::new();
    for detail in project.details_by_node_id.values() {
        for evidence in &detail.evidence {
            if evidence.source_id.as_deref() == Some(source_id.as_str()) {
                evidence_by_id.insert(evidence.id.clone(), evidence.clone());
            }
        }
    }
    for detail in project.edge_details_by_id.values() {
        for evidence in &detail.evidence {
            if evidence.source_id.as_deref() == Some(source_id.as_str()) {
                evidence_by_id.insert(evidence.id.clone(), evidence.clone());
            }
        }
    }

    let page_refs = page_refs_from_evidence(evidence_by_id.values());
    let entities = project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionEntity {
                entity_id: format!("ent-{}", detail.node.id),
                kind: BrainNodeKind::Concept,
                name: detail.canonical_name.clone(),
                aliases: detail.aliases.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                provenance: format!(
                    "Heuristic extractor promoted concept node '{}' from source-backed evidence.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();

    let topics = project
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionTopic {
                topic_id: detail.node.id.clone(),
                title: detail.canonical_name.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                provenance: format!(
                    "Heuristic extractor treated '{}' as a source-backed topic.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();

    let claims = project
        .details_by_node_id
        .values()
        .filter(|detail| matches!(detail.node.kind, GraphNodeKind::Concept | GraphNodeKind::Page))
        .filter_map(|detail| {
            let evidence = evidence_for_source(&detail.evidence, &source_id);
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            let first_evidence = evidence
                .first()
                .map(|evidence| evidence.snippet.trim())
                .filter(|snippet| !snippet.is_empty())
                .unwrap_or(&detail.description);
            Some(StructuredExtractionClaim {
                claim_id: format!("claim-{}", detail.node.id),
                statement: format!(
                    "{} is evidence-backed by: {}",
                    detail.canonical_name, first_evidence
                ),
                subject_refs: vec![detail.node.id.clone()],
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: detail.node.confidence,
                status: "supported".into(),
                provenance: format!(
                    "Heuristic extractor created this claim only because '{}' has direct source evidence.",
                    detail.canonical_name
                ),
            })
        })
        .collect::<Vec<_>>();

    let relations = project
        .edges
        .iter()
        .filter_map(|edge| {
            let evidence = project
                .edge_details_by_id
                .get(&edge.id)
                .map(|detail| evidence_for_source(&detail.evidence, &source_id))
                .unwrap_or_default();
            let evidence_refs = evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect::<Vec<_>>();
            if evidence_refs.is_empty() {
                return None;
            }
            Some(StructuredExtractionRelation {
                relation_id: edge.id.clone(),
                kind: brain_relation_kind_for_relation_kind(edge.kind),
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                label: edge.label.clone(),
                source_refs: source_refs_from_evidence(&evidence, &source_id),
                evidence_refs,
                page_refs: page_refs_from_evidence(evidence.iter().copied()),
                confidence: edge.confidence,
                provenance: format!(
                    "Heuristic extractor kept this relation only because it has source evidence in {}.",
                    source_id
                ),
            })
        })
        .collect::<Vec<_>>();

    StructuredExtractionArtifact {
        artifact_id: format!("extraction-{source_id}"),
        workspace_id: workspace_id.into(),
        source_id,
        extractor: "heuristic".into(),
        extractor_model: None,
        source_refs,
        page_refs,
        entities,
        topics,
        claims,
        relations,
        evidence_refs: evidence_by_id.into_values().collect(),
        confidence: Some(if project.summary.status == ProjectStatus::Ready {
            0.7
        } else {
            0.4
        }),
        provenance: "Structured extraction artifact generated from the current heuristic graph compiler fallback.".into(),
        created_at: generated_at,
    }
}

fn evidence_for_source<'a>(evidence: &'a [EvidenceRef], source_id: &str) -> Vec<&'a EvidenceRef> {
    evidence
        .iter()
        .filter(|evidence| evidence.source_id.as_deref() == Some(source_id))
        .collect()
}

fn source_refs_from_evidence(evidence: &[&EvidenceRef], fallback_source_id: &str) -> Vec<String> {
    let mut refs = evidence
        .iter()
        .filter_map(|evidence| evidence.source_id.clone())
        .collect::<BTreeSet<_>>();
    if refs.is_empty() {
        refs.insert(fallback_source_id.into());
    }
    refs.into_iter().collect()
}

fn page_refs_from_evidence<'a>(
    evidence: impl IntoIterator<Item = &'a EvidenceRef>,
) -> Vec<StructuredExtractionPageRef> {
    let mut refs = BTreeMap::<(String, Option<usize>), StructuredExtractionPageRef>::new();
    for evidence in evidence {
        refs.entry((evidence.page_label.clone(), evidence.page_index))
            .or_insert_with(|| StructuredExtractionPageRef {
                page_label: evidence.page_label.clone(),
                page_index: evidence.page_index,
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
    }
    refs.into_values().collect()
}

fn brain_node_kind_for_graph_kind(kind: GraphNodeKind) -> BrainNodeKind {
    match kind {
        GraphNodeKind::Source | GraphNodeKind::Document => BrainNodeKind::Source,
        GraphNodeKind::Page => BrainNodeKind::Topic,
        GraphNodeKind::Concept => BrainNodeKind::Concept,
    }
}

fn brain_relation_kind_for_relation_kind(kind: RelationKind) -> BrainRelationKind {
    match kind {
        RelationKind::SourceDocument => BrainRelationKind::DerivedFrom,
        RelationKind::RelatedTo => BrainRelationKind::RelatedTo,
    }
}

fn build_materialized_wiki_pages(
    workspace_id: &str,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    generated_at: u64,
) -> Vec<WikiPage> {
    let mut pages = vec![
        WikiPage {
            page_id: "wiki-overview".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/overview.md".into(),
            title: "Workspace Overview".into(),
            body: format!(
                "# Workspace Overview\n\n- Workspace: `{workspace_id}`\n- Sources: {}\n- Nodes: {}\n",
                sources.len(),
                nodes.len()
            ),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: nodes
                .iter()
                .flat_map(|node| node.evidence_ids.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-index".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/index.md".into(),
            title: "Brain Index".into(),
            body: String::new(),
            node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
            source_refs: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
        WikiPage {
            page_id: "wiki-log".into(),
            workspace_id: workspace_id.into(),
            path: "wiki/log.md".into(),
            title: "Brain Log".into(),
            body: String::new(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: generated_at,
        },
    ];
    pages.extend(sources.iter().map(|source| WikiPage {
        page_id: format!("wiki-source-{}", source.source_id),
        workspace_id: workspace_id.into(),
        path: format!("wiki/sources/{}.md", sanitize_name(&source.source_id)),
        title: source.source_id.clone(),
        body: format!(
            "# {}\n\n- Original: `{}`\n- Source: `{}`\n- Markdown: `{}`\n- Status: `{}`\n",
            source.source_id,
            source.original_path,
            source.source_path,
            source.markdown_path,
            source.status
        ),
        node_refs: Vec::new(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: Vec::new(),
        updated_at: generated_at,
    }));
    pages.extend(
        nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| WikiPage {
                page_id: format!("wiki-topic-{}", node.node_id),
                workspace_id: workspace_id.into(),
                path: format!("wiki/topics/{}.md", sanitize_name(&node.node_id)),
                title: node.label.clone(),
                body: format!(
                    "# {}\n\n- Node: `{}`\n- Sources: {}\n- Evidence refs: {}\n",
                    node.label,
                    node.node_id,
                    node.source_ids.join(", "),
                    node.evidence_ids.len()
                ),
                node_refs: vec![node.node_id.clone()],
                source_refs: node.source_ids.clone(),
                evidence_refs: node.evidence_ids.clone(),
                updated_at: generated_at,
            }),
    );
    pages
}

fn write_materialized_brain_repo(root: &Path, snapshot: &BrainRepoSnapshot) -> Result<()> {
    let writer = BrainWorkspaceWriter::open(root.to_path_buf())?;
    let root = writer.root.as_path();
    let wiki_root = root.join("wiki");
    for dir in [
        root.join("graph"),
        root.join("artifacts"),
        root.join("events"),
        root.join("memory"),
        root.join("reviews/proposed-updates"),
        root.join("reviews/lint-reports"),
        wiki_root.join("sources"),
        wiki_root.join("entities"),
        wiki_root.join("topics"),
        wiki_root.join("claims"),
        wiki_root.join("questions"),
    ] {
        fs::create_dir_all(&dir).with_context(|| format!("failed creating {}", dir.display()))?;
    }

    let mut effective_snapshot = snapshot.clone();
    effective_snapshot.memories = read_memory_records(root)?;
    effective_snapshot.events = merge_preserved_brain_events(
        snapshot.events.clone(),
        &read_brain_events_jsonl(&root.join("events/brain_events.jsonl")).unwrap_or_default(),
    );
    apply_accepted_proposals_to_snapshot(root, &mut effective_snapshot)?;

    write_json_pretty(&root.join("brain-manifest.json"), &effective_snapshot)?;
    write_json_pretty(&root.join("graph/nodes.json"), &effective_snapshot.nodes)?;
    write_json_pretty(
        &root.join("graph/edges.json"),
        &effective_snapshot.relations,
    )?;
    write_json_pretty(
        &root.join("graph/evidence.json"),
        &effective_snapshot.evidence,
    )?;
    write_json_pretty(
        &root.join("graph/entities.json"),
        &effective_snapshot.entities,
    )?;
    write_json_pretty(&root.join("graph/claims.json"), &effective_snapshot.claims)?;
    write_json_pretty(
        &root.join("memory/records.json"),
        &effective_snapshot.memories,
    )?;
    for extraction in &effective_snapshot.extractions {
        write_json_pretty(
            &root
                .join("artifacts")
                .join(sanitize_name(&extraction.source_id))
                .join("extraction.json"),
            extraction,
        )?;
    }
    write_brain_events_jsonl(
        &root.join("events/brain_events.jsonl"),
        &effective_snapshot.events,
    )?;

    for page in &effective_snapshot.wiki_pages {
        let path = root.join(&page.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        fs::write(
            &path,
            materialized_wiki_page_body(page, &effective_snapshot),
        )
        .with_context(|| format!("failed writing wiki page {}", path.display()))?;
    }
    fs::write(root.join("reviews/proposed-updates/.gitkeep"), "")
        .context("failed writing proposed updates placeholder")?;
    fs::write(root.join("reviews/lint-reports/.gitkeep"), "")
        .context("failed writing lint reports placeholder")?;
    fs::write(root.join("memory/.gitkeep"), "").context("failed writing memory placeholder")?;
    Ok(())
}

fn read_structured_extraction_artifacts(
    root: &Path,
    sources: &[SourceRecord],
) -> Result<Vec<StructuredExtractionArtifact>> {
    let mut artifacts = Vec::new();
    for source in sources {
        let path = root
            .join("artifacts")
            .join(sanitize_name(&source.source_id))
            .join("extraction.json");
        if path.exists() {
            artifacts.push(
                read_json_artifact(&path)
                    .with_context(|| format!("failed reading {}", path.display()))?,
            );
        }
    }
    Ok(artifacts)
}

fn merge_preserved_brain_events(
    mut materialized_events: Vec<BrainEvent>,
    existing_events: &[BrainEvent],
) -> Vec<BrainEvent> {
    let mut seen = materialized_events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    for event in existing_events {
        if is_preserved_brain_event(event.event_type) && seen.insert(event.event_id.clone()) {
            materialized_events.push(event.clone());
        }
    }
    materialized_events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    materialized_events
}

fn is_preserved_brain_event(event_type: BrainEventKind) -> bool {
    matches!(
        event_type,
        BrainEventKind::MemoryProposed
            | BrainEventKind::ClaimProposed
            | BrainEventKind::LinkProposed
            | BrainEventKind::ObservationAppended
            | BrainEventKind::SourceNoteProposed
            | BrainEventKind::WikiPageProposed
            | BrainEventKind::MemoryAccepted
            | BrainEventKind::ReviewCreated
            | BrainEventKind::ReviewResolved
            | BrainEventKind::BrainMaintenanceRun
    )
}

fn materialized_wiki_page_body(page: &WikiPage, snapshot: &BrainRepoSnapshot) -> String {
    if page.path == "wiki/index.md" {
        let source_links = snapshot
            .sources
            .iter()
            .map(|source| {
                format!(
                    "- [{}](sources/{}.md)",
                    source.source_id,
                    sanitize_name(&source.source_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let topic_links = snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| {
                format!(
                    "- [{}](topics/{}.md)",
                    node.label,
                    sanitize_name(&node.node_id)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "# Brain Index\n\n## Sources\n\n{}\n\n## Topics\n\n{}\n",
            source_links, topic_links
        );
    }
    if page.path == "wiki/log.md" {
        return snapshot
            .events
            .iter()
            .map(|event| {
                format!(
                    "- {} `{}` by `{}`",
                    event.created_at, event.event_id, event.actor.actor_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    page.body.clone()
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value).context("failed to encode JSON artifact")?;
    write_file_atomic(path, json.as_bytes())
}

fn write_brain_events_jsonl(path: &Path, events: &[BrainEvent]) -> Result<()> {
    let mut lines = String::new();
    for event in events {
        lines.push_str(
            &serde_json::to_string(event).context("failed to encode brain event JSONL row")?,
        );
        lines.push('\n');
    }
    write_file_atomic(path, lines.as_bytes())
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "artifact".into());
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::now_v7().as_simple()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed opening {}", temp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed writing {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed syncing {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed renaming {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}

fn append_brain_event_jsonl(path: &Path, event: &BrainEvent) -> Result<()> {
    let line = serde_json::to_string(event).context("failed to encode brain event JSONL row")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed opening {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed writing {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed writing {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed syncing {}", path.display()))
}

fn workspace_root_for_rows(
    rows: &[(StoredSourceRow, Option<KnowledgeProject>)],
) -> Option<PathBuf> {
    rows.iter()
        .find_map(|(row, _)| workspace_root_from_summary(&row.summary))
}

fn workspace_root_from_summary(summary: &SourceSummary) -> Option<PathBuf> {
    workspace_root_from_path_segments(&summary.source_path, "sources", &summary.source_id).or_else(
        || {
            workspace_root_from_path_segments(
                &summary.markdown_path,
                "artifacts",
                &summary.source_id,
            )
        },
    )
}

fn workspace_root_from_path_segments(path: &str, marker: &str, source_id: &str) -> Option<PathBuf> {
    let marker = format!("/{marker}/{source_id}");
    path.find(&marker)
        .filter(|index| *index > 0)
        .map(|index| PathBuf::from(&path[..index]))
}

fn fallback_workspace_root(store_path: &Path, workspace_id: &str) -> PathBuf {
    store_path
        .parent()
        .map(|parent| parent.join(workspace_id))
        .unwrap_or_else(|| PathBuf::from(workspace_id))
}

fn concept_identity_keys(detail: &GraphNodeDetail) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical_key = normalize_key(&detail.canonical_name);
    if !canonical_key.is_empty() {
        keys.push(canonical_key);
    }
    for alias in &detail.aliases {
        let key = normalize_key(alias);
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn source_node_position(index: usize, total: usize) -> GraphNodePosition {
    if total <= 1 {
        return GraphNodePosition { x: 50.0, y: 12.0 };
    }
    let x = 14.0 + (72.0 / (total.saturating_sub(1) as f32)) * (index as f32);
    GraphNodePosition { x, y: 12.0 }
}

fn source_node_id(source_id: &str) -> String {
    format!("source:{source_id}")
}

fn is_source_like_node_kind(kind: GraphNodeKind) -> bool {
    matches!(kind, GraphNodeKind::Source | GraphNodeKind::Document)
}

fn source_label_from_manifest(manifest: &SourceArtifactManifest) -> String {
    Path::new(&manifest.original_path)
        .file_name()
        .or_else(|| Path::new(&manifest.source_path).file_name())
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| manifest.output_name.clone())
}

fn source_backing_from_manifest(manifest: &SourceArtifactManifest) -> SourceBacking {
    SourceBacking {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: document_format_slug(&manifest.format).into(),
        status: ingest_status_slug(&manifest.status).into(),
        page_count: manifest.pages.len(),
        success_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_none())
            .count(),
        failed_count: manifest
            .pages
            .iter()
            .filter(|page| page.error_message.is_some())
            .count(),
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
        manifest_path: Some(manifest.manifest_path.clone()),
    }
}

fn build_extraction_artifact(
    page_sections: &[PageSection],
    source_path: &str,
    source_id: Option<&str>,
) -> ExtractionArtifact {
    let mut concepts = BTreeMap::<String, ExtractedConcept>::new();
    let mut claims = Vec::new();
    let mut evidence_refs = BTreeMap::new();
    let mut concept_ids_by_page = Vec::<(String, Vec<String>, Vec<String>)>::new();

    for section in page_sections {
        let mut seen_on_page = BTreeSet::new();
        let mut page_concept_ids = Vec::new();
        let mut page_evidence_ids = Vec::new();
        let candidates = concept_candidates(&section.content);
        for candidate in candidates {
            let key = normalize_key(&candidate);
            if key.is_empty() || !seen_on_page.insert(key.clone()) {
                continue;
            }
            let concept_id = format!("concept-{key}");
            let concept = concepts
                .entry(key.clone())
                .or_insert_with(|| ExtractedConcept {
                    id: concept_id.clone(),
                    label: candidate.clone(),
                    aliases: BTreeSet::new(),
                    evidence_ids: Vec::new(),
                    page_labels: BTreeSet::new(),
                });
            if concept.label != candidate {
                concept.aliases.insert(candidate.clone());
            }
            concept.page_labels.insert(section.page_label.clone());
            let evidence_id = format!("ev-{}-{}", key, concept.evidence_ids.len() + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Concept '{}' was extracted from {} because the page text produced a stable candidate label.",
                        candidate, section.page_label
                    ),
                },
            );
            concept.evidence_ids.push(evidence_id.clone());
            page_evidence_ids.push(evidence_id.clone());
            claims.push(ExtractedClaim {
                id: format!("claim-{}-{}", key, claims.len() + 1),
                text: candidate.clone(),
                subject_concept_id: concept_id.clone(),
                evidence_id,
            });
            page_concept_ids.push(concept_id);
        }
        if !page_concept_ids.is_empty() {
            concept_ids_by_page.push((
                section.page_label.clone(),
                page_concept_ids,
                page_evidence_ids,
            ));
        }
    }

    if concepts.is_empty() {
        for (index, section) in page_sections.iter().enumerate() {
            let label = fallback_concept_label(&section.content, &section.page_label);
            let key = normalize_key(&label);
            let concept_id = format!("concept-{key}");
            concepts.insert(
                key.clone(),
                ExtractedConcept {
                    id: concept_id.clone(),
                    label,
                    aliases: BTreeSet::new(),
                    evidence_ids: vec![format!("ev-fallback-{}", index + 1)],
                    page_labels: [section.page_label.clone()].into_iter().collect(),
                },
            );
            let evidence_id = format!("ev-fallback-{}", index + 1);
            evidence_refs.insert(
                evidence_id.clone(),
                ExtractionEvidenceRef {
                    id: evidence_id.clone(),
                    page_index: section.page_index,
                    page_label: section.page_label.clone(),
                    snippet: excerpt(&section.content, 180),
                    source_path: source_path.to_string(),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: section.markdown_path.clone(),
                    image_path: section.image_path.clone(),
                    provenance: format!(
                        "Fallback concept extracted from {} because no stronger concept candidates were found.",
                        section.page_label
                    ),
                },
            );
            claims.push(ExtractedClaim {
                id: format!("claim-fallback-{}", index + 1),
                text: fallback_concept_label(&section.content, &section.page_label),
                subject_concept_id: concept_id.clone(),
                evidence_id: evidence_id.clone(),
            });
            concept_ids_by_page.push((
                section.page_label.clone(),
                vec![concept_id],
                vec![evidence_id],
            ));
        }
    }

    let concepts = concepts.into_values().take(20).collect::<Vec<_>>();
    let allowed_ids = concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let mut relations = Vec::new();
    for (page_label, mut concept_ids, evidence_ids) in concept_ids_by_page {
        concept_ids.retain(|id| allowed_ids.contains(id));
        concept_ids.sort();
        concept_ids.dedup();
        for left_index in 0..concept_ids.len() {
            for right_index in (left_index + 1)..concept_ids.len() {
                let (source_concept_id, target_concept_id) =
                    if concept_ids[left_index] <= concept_ids[right_index] {
                        (
                            concept_ids[left_index].clone(),
                            concept_ids[right_index].clone(),
                        )
                    } else {
                        (
                            concept_ids[right_index].clone(),
                            concept_ids[left_index].clone(),
                        )
                    };
                relations.push(ExtractedRelation {
                    source_concept_id,
                    target_concept_id,
                    evidence_ids: evidence_ids.clone(),
                    page_labels: [page_label.clone()].into_iter().collect(),
                });
            }
        }
    }

    ExtractionArtifact {
        concepts,
        claims,
        relations,
        evidence_refs,
    }
}

fn collected_concepts_from_artifact(artifact: &ExtractionArtifact) -> CollectedConcepts {
    let allowed_ids = artifact
        .concepts
        .iter()
        .map(|concept| concept.id.clone())
        .collect::<BTreeSet<_>>();
    let concepts = artifact
        .concepts
        .iter()
        .map(|concept| ConceptAccumulator {
            id: concept.id.clone(),
            label: concept.label.clone(),
            aliases: concept.aliases.clone(),
            evidence: concept
                .evidence_ids
                .iter()
                .filter_map(|id| artifact.evidence_refs.get(id))
                .map(evidence_ref_from_extraction)
                .collect(),
            page_labels: concept.page_labels.clone(),
        })
        .collect::<Vec<_>>();
    let mut page_concepts_by_label = BTreeMap::<String, PageConceptSet>::new();
    let mut claims = artifact.claims.iter().collect::<Vec<_>>();
    claims.sort_by(|left, right| left.id.cmp(&right.id));
    for claim in claims {
        if !allowed_ids.contains(&claim.subject_concept_id) {
            continue;
        }
        let Some(evidence) = artifact.evidence_refs.get(&claim.evidence_id) else {
            continue;
        };
        let page = page_concepts_by_label
            .entry(evidence.page_label.clone())
            .or_insert_with(|| PageConceptSet {
                page_index: evidence.page_index,
                page_label: evidence.page_label.clone(),
                concept_ids: Vec::new(),
                snippet: evidence.snippet.clone(),
                markdown_path: evidence.markdown_path.clone(),
                image_path: evidence.image_path.clone(),
            });
        page.concept_ids.push(claim.subject_concept_id.clone());
        if claim.text.len() > page.snippet.len() {
            page.snippet = claim.text.clone();
        }
    }
    for relation in &artifact.relations {
        if !allowed_ids.contains(&relation.source_concept_id)
            || !allowed_ids.contains(&relation.target_concept_id)
        {
            continue;
        }
        for page_label in &relation.page_labels {
            let evidence_refs = relation
                .evidence_ids
                .iter()
                .filter_map(|id| artifact.evidence_refs.get(id))
                .collect::<Vec<_>>();
            let relation_evidence = evidence_refs
                .iter()
                .find(|evidence| &evidence.page_label == page_label)
                .copied()
                .or_else(|| evidence_refs.first().copied());
            let Some(evidence) = relation_evidence else {
                continue;
            };
            let page = page_concepts_by_label
                .entry(page_label.clone())
                .or_insert_with(|| PageConceptSet {
                    page_index: evidence.page_index,
                    page_label: page_label.clone(),
                    concept_ids: Vec::new(),
                    snippet: evidence.snippet.clone(),
                    markdown_path: evidence.markdown_path.clone(),
                    image_path: evidence.image_path.clone(),
                });
            page.concept_ids.push(relation.source_concept_id.clone());
            page.concept_ids.push(relation.target_concept_id.clone());
        }
    }
    let page_concepts = page_concepts_by_label
        .into_values()
        .filter_map(|mut page| {
            page.concept_ids.sort();
            page.concept_ids.dedup();
            (!page.concept_ids.is_empty()).then_some(page)
        })
        .collect();
    CollectedConcepts {
        concepts,
        page_concepts,
    }
}

fn evidence_ref_from_extraction(evidence: &ExtractionEvidenceRef) -> EvidenceRef {
    EvidenceRef {
        id: evidence.id.clone(),
        page_label: evidence.page_label.clone(),
        page_index: Some(evidence.page_index),
        snippet: evidence.snippet.clone(),
        source_path: Some(evidence.source_path.clone()),
        source_id: evidence.source_id.clone(),
        markdown_path: evidence.markdown_path.clone(),
        image_path: evidence.image_path.clone(),
        provenance: Some(evidence.provenance.clone()),
    }
}

fn build_relation_edges(
    document_node: &GraphNodeSummary,
    concept_accumulators: &[ConceptAccumulator],
    page_concepts: &[PageConceptSet],
    source_path: &str,
    source_id: Option<&str>,
) -> (
    Vec<RelationEdgeSummary>,
    BTreeMap<String, RelationEdgeDetail>,
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    let concept_by_id = concept_accumulators
        .iter()
        .map(|concept| (concept.id.clone(), concept))
        .collect::<BTreeMap<_, _>>();

    for concept in concept_accumulators {
        let edge = RelationEdgeSummary {
            id: relation_edge_id(RelationKind::SourceDocument, &document_node.id, &concept.id),
            source_node_id: document_node.id.clone(),
            target_node_id: concept.id.clone(),
            kind: RelationKind::SourceDocument,
            label: "Compiled from source".into(),
            confidence: Some(0.94),
            evidence_count: concept.evidence.iter().take(2).count(),
        };
        let evidence = concept.evidence.iter().take(2).cloned().collect::<Vec<_>>();
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked the source document to {} because this concept was compiled from cited snippets in the import.",
                    concept.label
                ),
                evidence,
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    let mut concept_edge_accumulators = BTreeMap::<(String, String), EdgeAccumulator>::new();
    for page in page_concepts {
        if page.concept_ids.len() < 2 {
            continue;
        }
        for left_index in 0..page.concept_ids.len() {
            for right_index in (left_index + 1)..page.concept_ids.len() {
                let left_id = &page.concept_ids[left_index];
                let right_id = &page.concept_ids[right_index];
                let (source_node_id, target_node_id) = if left_id <= right_id {
                    (left_id.clone(), right_id.clone())
                } else {
                    (right_id.clone(), left_id.clone())
                };
                let accumulator = concept_edge_accumulators
                    .entry((source_node_id.clone(), target_node_id.clone()))
                    .or_insert_with(|| EdgeAccumulator {
                        source_node_id: source_node_id.clone(),
                        target_node_id: target_node_id.clone(),
                        evidence: Vec::new(),
                        page_labels: BTreeSet::new(),
                    });
                accumulator.page_labels.insert(page.page_label.clone());
                accumulator.evidence.push(EvidenceRef {
                    id: format!(
                        "ev-edge-{}-{}-{}",
                        source_node_id,
                        target_node_id,
                        accumulator.evidence.len() + 1
                    ),
                    page_label: page.page_label.clone(),
                    page_index: Some(page.page_index),
                    snippet: page.snippet.clone(),
                    source_path: Some(source_path.to_string()),
                    source_id: source_id.map(ToString::to_string),
                    markdown_path: page.markdown_path.clone(),
                    image_path: page.image_path.clone(),
                    provenance: Some(format!(
                        "Relation evidence extracted because both concepts appeared in {}.",
                        page.page_label
                    )),
                });
            }
        }
    }

    let mut concept_edges = concept_edge_accumulators.into_values().collect::<Vec<_>>();
    concept_edges.sort_by(|left, right| {
        right
            .page_labels
            .len()
            .cmp(&left.page_labels.len())
            .then_with(|| left.source_node_id.cmp(&right.source_node_id))
            .then_with(|| left.target_node_id.cmp(&right.target_node_id))
    });

    for accumulator in concept_edges.into_iter().take(16) {
        let source_label = concept_by_id
            .get(&accumulator.source_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.source_node_id.clone());
        let target_label = concept_by_id
            .get(&accumulator.target_node_id)
            .map(|concept| concept.label.clone())
            .unwrap_or_else(|| accumulator.target_node_id.clone());
        let edge = RelationEdgeSummary {
            id: format!(
                "edge-{}-{}",
                accumulator.source_node_id, accumulator.target_node_id
            ),
            source_node_id: accumulator.source_node_id.clone(),
            target_node_id: accumulator.target_node_id.clone(),
            kind: RelationKind::RelatedTo,
            label: "Related in source".into(),
            confidence: Some(
                (0.56 + (accumulator.page_labels.len().min(3) as f32 * 0.08)).min(0.84),
            ),
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge.id.clone(),
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: format!(
                    "HyprDuck linked {} and {} because they appeared together in {} page section(s).",
                    source_label,
                    target_label,
                    accumulator.page_labels.len()
                ),
                evidence: accumulator.evidence.clone(),
            },
        );
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
        edges.push(edge);
    }

    (
        edges,
        edge_details_by_id,
        related_count_by_node_id,
        connected_node_ids_by_node_id,
    )
}

fn note_relation(
    related_count_by_node_id: &mut BTreeMap<String, usize>,
    connected_node_ids_by_node_id: &mut BTreeMap<String, BTreeSet<String>>,
    source_node_id: &str,
    target_node_id: &str,
) {
    *related_count_by_node_id
        .entry(source_node_id.to_string())
        .or_default() += 1;
    *related_count_by_node_id
        .entry(target_node_id.to_string())
        .or_default() += 1;
    connected_node_ids_by_node_id
        .entry(source_node_id.to_string())
        .or_default()
        .insert(target_node_id.to_string());
    connected_node_ids_by_node_id
        .entry(target_node_id.to_string())
        .or_default()
        .insert(source_node_id.to_string());
}

fn concept_candidates(content: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for line in content.lines() {
        let cleaned = clean_candidate_line(line);
        if cleaned.is_empty() {
            continue;
        }
        if let Some(label) = derive_concept_label(&cleaned) {
            if !labels
                .iter()
                .any(|existing| normalize_key(existing) == normalize_key(&label))
            {
                labels.push(label);
            }
        }
        if labels.len() >= 3 {
            break;
        }
    }
    labels
}

fn clean_candidate_line(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("![")
        || trimmed.starts_with("_AI analysis unavailable")
        || trimmed.starts_with("# ")
        || trimmed.starts_with("## Page ")
    {
        return String::new();
    }

    trimmed
        .trim_start_matches('#')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
        .trim()
        .replace('`', "")
        .replace('*', "")
}

fn derive_concept_label(value: &str) -> Option<String> {
    let first_clause = value
        .split(|char| matches!(char, '.' | ':' | ';' | '(' | ')' | '[' | ']'))
        .next()
        .unwrap_or(value)
        .trim();
    let mut words = first_clause
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|char: char| !char.is_alphanumeric() && char != '-' && char != '/')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    while matches!(words.first(), Some(word) if is_leading_stopword(word)) {
        words.remove(0);
    }

    if words.len() < 2 {
        return None;
    }

    let label = words.into_iter().take(6).collect::<Vec<_>>().join(" ");
    if label.len() < 10 {
        return None;
    }

    Some(label)
}

fn is_leading_stopword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "for"
            | "from"
            | "in"
            | "into"
            | "of"
            | "on"
            | "or"
            | "the"
            | "this"
            | "that"
            | "to"
            | "with"
    )
}

fn fallback_concept_label(content: &str, page_label: &str) -> String {
    derive_concept_label(content).unwrap_or_else(|| format!("{page_label} summary"))
}

fn normalize_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for char in value.chars() {
        if char.is_ascii_alphanumeric() {
            normalized.push(char.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

fn extract_page_sections(markdown: &str) -> Vec<PageSection> {
    let normalized = markdown.replace("\r\n", "\n");
    let headers = regex_like_page_headers(&normalized);
    if headers.is_empty() {
        return vec![PageSection {
            page_index: 0,
            page_label: "Imported text".into(),
            content: normalized,
            markdown_path: None,
            image_path: None,
        }];
    }

    let mut sections = Vec::with_capacity(headers.len());
    for index in 0..headers.len() {
        let (page_label, _, content_start) = &headers[index];
        let next_start = headers
            .get(index + 1)
            .map(|(_, next_start, _)| *next_start)
            .unwrap_or(normalized.len());
        sections.push(PageSection {
            page_index: index,
            page_label: page_label.clone(),
            content: normalized[*content_start..next_start].trim().to_string(),
            markdown_path: None,
            image_path: None,
        });
    }
    sections
}

fn attach_page_artifacts_to_sections(
    sections: &mut [PageSection],
    source_manifest: Option<&SourceArtifactManifest>,
) {
    let Some(manifest) = source_manifest else {
        return;
    };
    for section in sections {
        let artifact = manifest
            .pages
            .iter()
            .find(|page| page.label == section.page_label)
            .or_else(|| manifest.pages.get(section.page_index));
        if let Some(artifact) = artifact {
            section.page_index = artifact.index;
            section.markdown_path = artifact.markdown_path.clone();
            section.image_path = artifact.image_path.clone();
        }
    }
}

fn regex_like_page_headers(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in markdown.lines() {
        let line_len = line.len();
        if let Some(page_label) = line
            .strip_prefix("## Page ")
            .map(|page| format!("Page {}", page.trim()))
        {
            headers.push((page_label, offset, offset + line_len + 1));
        }
        offset += line_len + 1;
    }
    headers
}

fn infer_markdown_title(markdown_path: &str, markdown: &str) -> String {
    if let Some(heading) = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
    {
        return heading.to_string();
    }

    Path::new(markdown_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "HyprDuck import".into())
}

fn excerpt(value: &str, max_length: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "No visible evidence snippet is available yet.".into();
    }
    let compact_chars = compact.chars().count();
    if compact_chars <= max_length {
        return compact;
    }
    let truncated = compact
        .chars()
        .take(max_length.saturating_sub(1))
        .collect::<String>();
    format!("{}…", truncated.trim_end())
}

fn correction_actions_for_detail(
    _canonical_name: &str,
    aliases: &[String],
) -> Vec<CorrectionAction> {
    vec![
        CorrectionAction {
            kind: CorrectionKind::Merge,
            label: "Merge".into(),
            disabled_reason: None,
        },
        CorrectionAction {
            kind: CorrectionKind::KeepSeparate,
            label: "Keep Separate".into(),
            disabled_reason: if aliases.is_empty() {
                Some("No grouped aliases are available to split yet.".into())
            } else {
                None
            },
        },
        CorrectionAction {
            kind: CorrectionKind::Rename,
            label: "Rename".into(),
            disabled_reason: None,
        },
    ]
}

#[derive(Debug, Clone)]
struct StoredEdgeAccumulator {
    kind: RelationKind,
    source_node_id: String,
    target_node_id: String,
    label: String,
    confidence: Option<f32>,
    evidence: Vec<EvidenceRef>,
}

fn apply_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    match request.kind {
        CorrectionKind::Rename => apply_rename_correction(project, request)?,
        CorrectionKind::Merge => apply_merge_correction(project, request)?,
        CorrectionKind::KeepSeparate => apply_keep_separate_correction(project, request)?,
    }
    refresh_project_after_correction(project);
    Ok(())
}

fn answer_project(
    project: &KnowledgeProject,
    request: &AnswerProjectRequest,
) -> Result<AnswerResponse> {
    let question = request.question.trim();
    if question.is_empty() {
        return Ok(AnswerResponse {
            status: AnswerStatus::Blocked,
            text: None,
            explanation: "Ask a concrete question before HyprDuck tries to answer from the graph."
                .into(),
            citations: Vec::new(),
            related_node_ids: request
                .node_id
                .clone()
                .into_iter()
                .collect(),
            suggested_actions: vec![SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a concrete question".into(),
                description:
                    "Grounded answers work best when the question names a concept, action, or relationship."
                        .into(),
            }],
        });
    }

    let focal_node_id = select_focal_node_id(project, request, question)?;

    let detail = project
        .details_by_node_id
        .get(&focal_node_id)
        .ok_or_else(|| anyhow!("node detail {} was not found", focal_node_id))?;
    let base_answer = project
        .answer_by_node_id
        .get(&focal_node_id)
        .cloned()
        .unwrap_or_else(|| build_answer_for_detail(project, detail, Vec::new()));
    let citations = best_matching_evidence(question, detail);
    let status = if citations.is_empty() {
        if detail.evidence.is_empty() {
            AnswerStatus::Blocked
        } else {
            AnswerStatus::LowConfidence
        }
    } else {
        AnswerStatus::Grounded
    };
    let related_node_ids = if base_answer.related_node_ids.is_empty() {
        project
            .edges
            .iter()
            .filter_map(|edge| {
                if edge.source_node_id == focal_node_id {
                    Some(edge.target_node_id.clone())
                } else if edge.target_node_id == focal_node_id {
                    Some(edge.source_node_id.clone())
                } else {
                    None
                }
            })
            .collect()
    } else {
        base_answer.related_node_ids.clone()
    };

    Ok(AnswerResponse {
        status,
        text: Some(answer_text_for_question(
            project, detail, question, status, &citations,
        )),
        explanation: answer_explanation_for_question(detail, question, status, &citations),
        citations,
        related_node_ids,
        suggested_actions: answer_suggested_actions(status),
    })
}

fn select_focal_node_id(
    project: &KnowledgeProject,
    request: &AnswerProjectRequest,
    question: &str,
) -> Result<String> {
    if let Some(node_id) = request.node_id.as_deref() {
        if project.details_by_node_id.contains_key(node_id) {
            return Ok(node_id.to_string());
        }
        bail!(
            "node {node_id} was not found in project {}",
            request.project_id
        );
    }

    if project.summary.project_id.starts_with("workspace:") {
        if let Some(node_id) = best_matching_detail_node_id(project, question) {
            return Ok(node_id);
        }
    }

    project
        .nodes
        .iter()
        .find(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.id.clone())
        .ok_or_else(|| {
            anyhow!(
                "no answerable node was found in project {}",
                request.project_id
            )
        })
}

fn best_matching_detail_node_id(project: &KnowledgeProject, question: &str) -> Option<String> {
    let terms = question_terms(question);
    if terms.is_empty() {
        return None;
    }

    project
        .details_by_node_id
        .values()
        .map(|detail| {
            let mut detail_terms = text_terms(&detail.canonical_name);
            detail_terms.extend(detail.aliases.iter().flat_map(|alias| text_terms(alias)));
            for evidence in &detail.evidence {
                detail_terms.extend(text_terms(&evidence.snippet));
            }
            let score = terms.intersection(&detail_terms).count();
            (score, detail.node.id.clone())
        })
        .filter(|(score, _)| *score > 0)
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, node_id)| node_id)
}

fn apply_rename_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let next_name = request
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("rename needs a non-empty canonical name"))?;
    let node = project
        .nodes
        .iter_mut()
        .find(|node| node.id == request.node_id)
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    if node.kind != GraphNodeKind::Concept {
        bail!("only concept nodes can be renamed");
    }

    let detail = project
        .details_by_node_id
        .get_mut(&request.node_id)
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    let previous_name = detail.canonical_name.clone();
    if previous_name == next_name {
        return Ok(());
    }

    let mut aliases = detail.aliases.iter().cloned().collect::<BTreeSet<_>>();
    aliases.insert(previous_name.clone());
    aliases.remove(next_name);
    detail.aliases = aliases.into_iter().collect();
    detail.canonical_name = next_name.to_string();
    detail.description = format!(
        "Renamed from {} to {}. HyprDuck kept the previous canonical label as an alias so the evidence trail stays intact.",
        previous_name, next_name
    );
    node.label = next_name.to_string();
    detail.node = node.clone();

    Ok(())
}

fn apply_merge_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let target_node_id = request
        .target_node_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("merge needs a target concept"))?;
    if target_node_id == request.node_id {
        bail!("merge target must be different from the selected node");
    }

    let source_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    let target_node = project
        .nodes
        .iter()
        .find(|node| node.id == target_node_id)
        .cloned()
        .ok_or_else(|| anyhow!("target node {} was not found", target_node_id))?;
    if source_node.kind != GraphNodeKind::Concept || target_node.kind != GraphNodeKind::Concept {
        bail!("merge only supports concept nodes");
    }

    let source_detail = project
        .details_by_node_id
        .get(&request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    let target_name = project
        .details_by_node_id
        .get(target_node_id)
        .map(|detail| detail.canonical_name.clone())
        .ok_or_else(|| anyhow!("target node detail {} was not found", target_node_id))?;

    {
        let target_detail = project
            .details_by_node_id
            .get_mut(target_node_id)
            .ok_or_else(|| anyhow!("target node detail {} was not found", target_node_id))?;
        let mut aliases = target_detail
            .aliases
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        aliases.insert(source_detail.canonical_name.clone());
        aliases.extend(source_detail.aliases.iter().cloned());
        aliases.remove(&target_name);
        target_detail.aliases = aliases.into_iter().collect();
        target_detail.evidence = dedupe_evidence(
            target_detail
                .evidence
                .clone()
                .into_iter()
                .chain(source_detail.evidence.clone())
                .collect(),
        );
        target_detail.description = format!(
            "Merged {} into {}. HyprDuck kept all visible evidence on the surviving concept.",
            source_detail.canonical_name, target_name
        );
    }

    if let Some(node) = project
        .nodes
        .iter_mut()
        .find(|node| node.id == target_node_id)
    {
        node.evidence_count = project
            .details_by_node_id
            .get(target_node_id)
            .map(|detail| detail.evidence.len())
            .unwrap_or(node.evidence_count);
        node.confidence = Some(
            node.confidence
                .unwrap_or(0.72)
                .max(source_node.confidence.unwrap_or(0.72))
                .min(0.94),
        );
    }

    project.nodes.retain(|node| node.id != request.node_id);
    project.details_by_node_id.remove(&request.node_id);
    project.answer_by_node_id.remove(&request.node_id);
    rewrite_project_edges(project, Some((&request.node_id, target_node_id)));

    Ok(())
}

fn apply_keep_separate_correction(
    project: &mut KnowledgeProject,
    request: &ApplyCorrectionRequest,
) -> Result<()> {
    let source_node = project
        .nodes
        .iter()
        .find(|node| node.id == request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node {} was not found", request.node_id))?;
    if source_node.kind != GraphNodeKind::Concept {
        bail!("keep separate only supports concept nodes");
    }

    let source_detail = project
        .details_by_node_id
        .get(&request.node_id)
        .cloned()
        .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
    if source_detail.aliases.is_empty() {
        bail!("keep separate needs at least one grouped alias");
    }

    {
        let detail = project
            .details_by_node_id
            .get_mut(&request.node_id)
            .ok_or_else(|| anyhow!("node detail {} was not found", request.node_id))?;
        detail.aliases.clear();
        detail.description = format!(
            "HyprDuck kept the previous aliases under {} as distinct concept nodes after a manual correction.",
            detail.canonical_name
        );
    }

    let split_evidence = source_detail.evidence.clone();
    for (index, alias) in source_detail.aliases.iter().enumerate() {
        let new_node_id = unique_manual_node_id(project, alias);
        let new_node = GraphNodeSummary {
            id: new_node_id.clone(),
            label: alias.clone(),
            kind: GraphNodeKind::Concept,
            confidence: Some(source_node.confidence.unwrap_or(0.68).min(0.82)),
            related_count: 0,
            evidence_count: split_evidence.len(),
            position: manual_split_position(&source_node.position, index),
        };
        project.nodes.push(new_node.clone());
        project.details_by_node_id.insert(
            new_node_id.clone(),
            GraphNodeDetail {
                node: new_node.clone(),
                canonical_name: alias.clone(),
                aliases: Vec::new(),
                description: format!(
                    "Created from a keep separate correction on {}. HyprDuck preserved the supporting evidence while treating this as its own concept.",
                    source_detail.canonical_name
                ),
                evidence: split_evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );

        for source_node_id in source_like_node_ids_for_concept(project, &request.node_id) {
            let document_evidence = split_evidence.iter().take(2).cloned().collect::<Vec<_>>();
            let document_edge = RelationEdgeSummary {
                id: relation_edge_id(RelationKind::SourceDocument, &source_node_id, &new_node_id),
                source_node_id: source_node_id.clone(),
                target_node_id: new_node_id.clone(),
                kind: RelationKind::SourceDocument,
                label: "Compiled from source".into(),
                confidence: Some(0.76),
                evidence_count: document_evidence.len(),
            };
            project.edges.push(document_edge.clone());
            project.edge_details_by_id.insert(
                document_edge.id.clone(),
                RelationEdgeDetail {
                    edge: document_edge,
                    explanation: String::new(),
                    evidence: document_evidence,
                },
            );
        }

        let (source_node_id, target_node_id) = if request.node_id <= new_node_id {
            (request.node_id.clone(), new_node_id.clone())
        } else {
            (new_node_id.clone(), request.node_id.clone())
        };
        let relation_evidence = split_evidence.iter().take(2).cloned().collect::<Vec<_>>();
        let relation_edge = RelationEdgeSummary {
            id: relation_edge_id(RelationKind::RelatedTo, &source_node_id, &target_node_id),
            source_node_id,
            target_node_id,
            kind: RelationKind::RelatedTo,
            label: "Separated by correction".into(),
            confidence: Some(0.68),
            evidence_count: relation_evidence.len(),
        };
        project.edges.push(relation_edge.clone());
        project.edge_details_by_id.insert(
            relation_edge.id.clone(),
            RelationEdgeDetail {
                edge: relation_edge,
                explanation: String::new(),
                evidence: relation_evidence,
            },
        );
    }

    Ok(())
}

fn refresh_project_after_correction(project: &mut KnowledgeProject) {
    rewrite_project_edges(project, None);

    let node_ids = project
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    project
        .details_by_node_id
        .retain(|node_id, _| node_ids.contains(node_id));
    project
        .answer_by_node_id
        .retain(|node_id, _| node_ids.contains(node_id));
    project.edges.retain(|edge| {
        node_ids.contains(&edge.source_node_id)
            && node_ids.contains(&edge.target_node_id)
            && edge.source_node_id != edge.target_node_id
    });
    project.edge_details_by_id.retain(|edge_id, detail| {
        node_ids.contains(&detail.edge.source_node_id)
            && node_ids.contains(&detail.edge.target_node_id)
            && detail.edge.source_node_id != detail.edge.target_node_id
            && edge_id == &detail.edge.id
    });

    let mut related_count_by_node_id = BTreeMap::<String, usize>::new();
    let mut connected_node_ids_by_node_id = BTreeMap::<String, BTreeSet<String>>::new();
    for edge in &project.edges {
        note_relation(
            &mut related_count_by_node_id,
            &mut connected_node_ids_by_node_id,
            &edge.source_node_id,
            &edge.target_node_id,
        );
    }

    for node in &mut project.nodes {
        if let Some(detail) = project.details_by_node_id.get_mut(&node.id) {
            if node.kind == GraphNodeKind::Concept {
                node.label = detail.canonical_name.clone();
                node.evidence_count = detail.evidence.len();
                detail.actions =
                    correction_actions_for_detail(&detail.canonical_name, &detail.aliases);
            } else {
                detail.actions = Vec::new();
            }
            node.related_count = related_count_by_node_id.get(&node.id).copied().unwrap_or(0);
            detail.node = node.clone();
        }
    }

    let label_by_node_id = project
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    for edge in &project.edges {
        if let Some(detail) = project.edge_details_by_id.get_mut(&edge.id) {
            detail.edge = edge.clone();
            detail.explanation = edge_explanation(edge, &label_by_node_id, &detail.evidence);
        }
    }

    for node in &project.nodes {
        if let Some(detail) = project.details_by_node_id.get(&node.id).cloned() {
            let related_node_ids = connected_node_ids_by_node_id
                .get(&node.id)
                .map(|related| related.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            project.answer_by_node_id.insert(
                node.id.clone(),
                build_answer_for_detail(project, &detail, related_node_ids),
            );
        }
    }

    let concept_count = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();
    let document_count = project
        .nodes
        .iter()
        .filter(|node| is_source_like_node_kind(node.kind))
        .count();
    let relationship_count = project.edges.len();
    let evidence_count = project
        .details_by_node_id
        .values()
        .map(|detail| detail.evidence.len())
        .sum::<usize>()
        + project
            .edge_details_by_id
            .values()
            .map(|detail| detail.evidence.len())
            .sum::<usize>();
    if let Some(document_title) = project
        .nodes
        .iter()
        .find(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.label.clone())
    {
        project.summary.title = document_title;
    }
    project.summary.status = if concept_count > 0 {
        ProjectStatus::Ready
    } else {
        ProjectStatus::Degraded
    };
    project.summary.document_count = document_count;
    project.summary.node_count = project.nodes.len();
    project.summary.relationship_count = relationship_count;
    project.summary.evidence_count = evidence_count;
    project.summary.summary = format!(
        "Workspace contains {} concept nodes and {} explainable relationships. Manual corrections keep the graph grounded in visible evidence.",
        concept_count, relationship_count
    );
}

fn rewrite_project_edges(project: &mut KnowledgeProject, redirect: Option<(&str, &str)>) {
    let mut previous_details = std::mem::take(&mut project.edge_details_by_id);
    let existing_edges = std::mem::take(&mut project.edges);
    let mut accumulators = BTreeMap::<String, StoredEdgeAccumulator>::new();
    let source_like_ids = source_like_node_ids(project);

    for edge in existing_edges {
        let mut source_node_id = edge.source_node_id.clone();
        let mut target_node_id = edge.target_node_id.clone();
        if let Some((from, to)) = redirect {
            if source_node_id == from {
                source_node_id = to.to_string();
            }
            if target_node_id == from {
                target_node_id = to.to_string();
            }
        }
        if source_node_id == target_node_id {
            continue;
        }
        if edge.kind == RelationKind::SourceDocument {
            if source_like_ids.contains(&target_node_id) {
                std::mem::swap(&mut source_node_id, &mut target_node_id);
            }
            if !source_like_ids.contains(&source_node_id) {
                continue;
            }
        } else if source_node_id > target_node_id {
            std::mem::swap(&mut source_node_id, &mut target_node_id);
        }

        let edge_id = relation_edge_id(edge.kind, &source_node_id, &target_node_id);
        let evidence = previous_details
            .remove(&edge.id)
            .map(|detail| detail.evidence)
            .unwrap_or_default();
        let accumulator =
            accumulators
                .entry(edge_id.clone())
                .or_insert_with(|| StoredEdgeAccumulator {
                    kind: edge.kind,
                    source_node_id: source_node_id.clone(),
                    target_node_id: target_node_id.clone(),
                    label: normalized_edge_label(edge.kind, &edge.label),
                    confidence: edge.confidence,
                    evidence: Vec::new(),
                });
        accumulator.label = preferred_edge_label(&accumulator.label, &edge.label, edge.kind);
        accumulator.confidence = match (accumulator.confidence, edge.confidence) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        accumulator.evidence = dedupe_evidence(
            accumulator
                .evidence
                .clone()
                .into_iter()
                .chain(evidence)
                .collect(),
        );
    }

    let mut edges = Vec::new();
    let mut edge_details_by_id = BTreeMap::new();
    for (edge_id, accumulator) in accumulators {
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id: accumulator.source_node_id,
            target_node_id: accumulator.target_node_id,
            kind: accumulator.kind,
            label: accumulator.label,
            confidence: accumulator.confidence,
            evidence_count: accumulator.evidence.len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence: accumulator.evidence,
            },
        );
        edges.push(edge);
    }

    project.edges = edges;
    project.edge_details_by_id = edge_details_by_id;
}

fn source_like_node_ids(project: &KnowledgeProject) -> BTreeSet<String> {
    project
        .nodes
        .iter()
        .filter(|node| is_source_like_node_kind(node.kind))
        .map(|node| node.id.clone())
        .collect()
}

fn source_like_node_ids_for_concept(
    project: &KnowledgeProject,
    concept_node_id: &str,
) -> BTreeSet<String> {
    let linked_source_ids = project
        .edges
        .iter()
        .filter(|edge| edge.kind == RelationKind::SourceDocument)
        .filter_map(|edge| {
            if edge.target_node_id == concept_node_id {
                Some(edge.source_node_id.clone())
            } else if edge.source_node_id == concept_node_id {
                Some(edge.target_node_id.clone())
            } else {
                None
            }
        })
        .filter(|node_id| {
            project
                .nodes
                .iter()
                .any(|node| node.id == *node_id && is_source_like_node_kind(node.kind))
        })
        .collect::<BTreeSet<_>>();

    if linked_source_ids.is_empty() {
        source_like_node_ids(project)
    } else {
        linked_source_ids
    }
}

fn build_answer_for_detail(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    related_node_ids: Vec<String>,
) -> AnswerResponse {
    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            let concept_relationship_count = project
                .edges
                .iter()
                .filter(|edge| edge.kind == RelationKind::RelatedTo)
                .count();
            AnswerResponse {
                status: if concept_count > 0 {
                    AnswerStatus::Grounded
                } else {
                    AnswerStatus::LowConfidence
                },
                text: Some(format!(
                    "HyprDuck currently tracks {} concept nodes and {} explainable concept links in this workspace.",
                    concept_count, concept_relationship_count
                )),
                explanation:
                    "This document-level answer reflects the current corrected graph and stays grounded in visible evidence.".into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![
                    SuggestedAction {
                        kind: SuggestedActionKind::InspectEvidence,
                        label: "Inspect evidence".into(),
                        description:
                            "Review the cited snippets before trusting the workspace-wide answer."
                                .into(),
                    },
                    SuggestedAction {
                        kind: SuggestedActionKind::AskDifferentQuestion,
                        label: "Ask a narrower question".into(),
                        description:
                            "Grounded answers get stronger when you focus on one concept at a time."
                                .into(),
                    },
                ],
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => {
            let page_count = detail
                .evidence
                .iter()
                .map(|evidence| evidence.page_label.clone())
                .collect::<BTreeSet<_>>()
                .len();
            AnswerResponse {
                status: if detail.evidence.is_empty() {
                    AnswerStatus::LowConfidence
                } else {
                    AnswerStatus::Grounded
                },
                text: Some(format!(
                    "{} currently has {} visible evidence refs across {} page(s).",
                    detail.canonical_name,
                    detail.evidence.len(),
                    page_count
                )),
                explanation:
                    "This answer reflects the current corrected concept node and its visible evidence."
                        .into(),
                citations: detail.evidence.iter().take(3).cloned().collect(),
                related_node_ids,
                suggested_actions: vec![SuggestedAction {
                    kind: SuggestedActionKind::InspectEvidence,
                    label: "Inspect evidence".into(),
                    description:
                        "Use the cited snippets to verify the corrected concept before acting on it."
                            .into(),
                }],
            }
        }
    }
}

fn best_matching_evidence(question: &str, detail: &GraphNodeDetail) -> Vec<EvidenceRef> {
    let question_terms = question_terms(question);
    if question_terms.is_empty() {
        return detail.evidence.iter().take(3).cloned().collect();
    }

    let mut scored = detail
        .evidence
        .iter()
        .map(|evidence| {
            let score = overlap_score(&question_terms, &evidence.snippet);
            (score, evidence.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.page_label.cmp(&right.1.page_label))
    });

    let matched = scored
        .iter()
        .filter(|(score, _)| *score > 0)
        .map(|(_, evidence)| evidence.clone())
        .take(3)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        detail.evidence.iter().take(2).cloned().collect()
    } else {
        matched
    }
}

fn answer_text_for_question(
    project: &KnowledgeProject,
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    let evidence_summary = citations
        .first()
        .map(|citation| citation.snippet.clone())
        .unwrap_or_else(|| "HyprDuck could not find a directly relevant snippet yet.".into());
    let page_count = detail
        .evidence
        .iter()
        .map(|evidence| evidence.page_label.clone())
        .collect::<BTreeSet<_>>()
        .len();

    match detail.node.kind {
        GraphNodeKind::Source | GraphNodeKind::Document => {
            let concept_count = project
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count();
            match status {
                AnswerStatus::Grounded => format!(
                    "For \"{}\", the strongest grounded reading is that this workspace currently contains {} concept nodes. Best visible support: {}",
                    question, concept_count, evidence_summary
                ),
                AnswerStatus::LowConfidence => format!(
                    "HyprDuck can partially answer \"{}\", but the graph only has weak snippet overlap. Closest visible support: {}",
                    question, evidence_summary
                ),
                AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                    "HyprDuck cannot safely answer \"{}\" from the current workspace yet.",
                    question
                ),
            }
        }
        GraphNodeKind::Concept | GraphNodeKind::Page => match status {
            AnswerStatus::Grounded => format!(
                "For \"{}\", {} is supported by {} visible evidence refs across {} page(s). Best visible support: {}",
                question,
                detail.canonical_name,
                detail.evidence.len(),
                page_count,
                evidence_summary
            ),
            AnswerStatus::LowConfidence => format!(
                "HyprDuck found {} evidence refs for {}, but the question \"{}\" only weakly matches those snippets. Closest visible support: {}",
                detail.evidence.len(),
                detail.canonical_name,
                question,
                evidence_summary
            ),
            AnswerStatus::Blocked | AnswerStatus::Stale => format!(
                "HyprDuck cannot safely answer \"{}\" for {} yet.",
                question, detail.canonical_name
            ),
        },
    }
}

fn answer_explanation_for_question(
    detail: &GraphNodeDetail,
    question: &str,
    status: AnswerStatus,
    citations: &[EvidenceRef],
) -> String {
    match status {
        AnswerStatus::Grounded => format!(
            "HyprDuck answered \"{}\" using {} visible citation(s) attached to {}.",
            question,
            citations.len(),
            detail.canonical_name
        ),
        AnswerStatus::LowConfidence => format!(
            "HyprDuck kept this answer cautious because the question \"{}\" only loosely overlaps with the visible evidence on {}.",
            question, detail.canonical_name
        ),
        AnswerStatus::Blocked => format!(
            "HyprDuck blocked this answer because it could not find enough grounded evidence for \"{}\".",
            question
        ),
        AnswerStatus::Stale => "HyprDuck is still reading from a stale workspace snapshot.".into(),
    }
}

fn answer_suggested_actions(status: AnswerStatus) -> Vec<SuggestedAction> {
    match status {
        AnswerStatus::Grounded => vec![SuggestedAction {
            kind: SuggestedActionKind::InspectEvidence,
            label: "Inspect evidence".into(),
            description: "Review the cited snippets if you want to verify the grounded answer."
                .into(),
        }],
        AnswerStatus::LowConfidence => vec![
            SuggestedAction {
                kind: SuggestedActionKind::InspectEvidence,
                label: "Inspect evidence".into(),
                description:
                    "Check the cited snippets to see where the question stopped matching strongly."
                        .into(),
            },
            SuggestedAction {
                kind: SuggestedActionKind::AskDifferentQuestion,
                label: "Ask a narrower question".into(),
                description:
                    "Use a concept name, relationship, or page label to get a more grounded answer."
                        .into(),
            },
        ],
        AnswerStatus::Blocked | AnswerStatus::Stale => vec![SuggestedAction {
            kind: SuggestedActionKind::AskDifferentQuestion,
            label: "Ask a narrower question".into(),
            description:
                "HyprDuck needs a more concrete, evidence-seeking question before it can answer."
                    .into(),
        }],
    }
}

fn question_terms(question: &str) -> BTreeSet<String> {
    text_terms(question)
}

fn text_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect()
}

fn overlap_score(question_terms: &BTreeSet<String>, haystack: &str) -> usize {
    let haystack_terms = haystack
        .split(|char: char| !char.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .collect::<BTreeSet<_>>();
    question_terms.intersection(&haystack_terms).count()
}

fn edge_explanation(
    edge: &RelationEdgeSummary,
    label_by_node_id: &BTreeMap<String, String>,
    evidence: &[EvidenceRef],
) -> String {
    let source_label = label_by_node_id
        .get(&edge.source_node_id)
        .cloned()
        .unwrap_or_else(|| edge.source_node_id.clone());
    let target_label = label_by_node_id
        .get(&edge.target_node_id)
        .cloned()
        .unwrap_or_else(|| edge.target_node_id.clone());

    match edge.kind {
        RelationKind::SourceDocument => format!(
            "HyprDuck linked the source document to {} because this concept is grounded in cited snippets from the import.",
            target_label
        ),
        RelationKind::RelatedTo if edge.label == "Separated by correction" => format!(
            "HyprDuck keeps {} and {} separate because you explicitly split them during correction review.",
            source_label, target_label
        ),
        RelationKind::RelatedTo => format!(
            "HyprDuck linked {} and {} because they share {} visible evidence ref(s).",
            source_label,
            target_label,
            evidence.len()
        ),
    }
}

fn relation_edge_id(kind: RelationKind, source_node_id: &str, target_node_id: &str) -> String {
    match kind {
        RelationKind::SourceDocument => format!("edge-{}-{}", source_node_id, target_node_id),
        RelationKind::RelatedTo => format!("edge-{}-{}", source_node_id, target_node_id),
    }
}

fn normalized_edge_label(kind: RelationKind, label: &str) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if label == "Separated by correction" => {
            "Separated by correction".into()
        }
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

fn preferred_edge_label(current: &str, incoming: &str, kind: RelationKind) -> String {
    match kind {
        RelationKind::SourceDocument => "Compiled from source".into(),
        RelationKind::RelatedTo if current == "Separated by correction" => current.into(),
        RelationKind::RelatedTo if incoming == "Separated by correction" => incoming.into(),
        RelationKind::RelatedTo => "Related in source".into(),
    }
}

fn dedupe_evidence(evidence: Vec<EvidenceRef>) -> Vec<EvidenceRef> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in evidence {
        let key = format!(
            "{}|{}|{}|{}",
            item.id,
            item.page_label,
            item.snippet,
            item.source_path.clone().unwrap_or_default()
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn unique_manual_node_id(project: &KnowledgeProject, label: &str) -> String {
    let base = normalize_key(label);
    let base_id = format!("concept-{base}");
    if !project.nodes.iter().any(|node| node.id == base_id) {
        return base_id;
    }

    let mut suffix = 2usize;
    loop {
        let candidate = format!("concept-{base}-manual-{suffix}");
        if !project.nodes.iter().any(|node| node.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn manual_split_position(base: &GraphNodePosition, index: usize) -> GraphNodePosition {
    let column = (index % 2) as f32;
    let row = (index / 2) as f32;
    GraphNodePosition {
        x: (base.x + 10.0 + column * 12.0).min(90.0),
        y: (base.y + row * 10.0).min(88.0),
    }
}

fn layout_concept_positions(count: usize) -> Vec<GraphNodePosition> {
    let per_row = if count > 9 { 4 } else { 3 };
    let row_count = ((count as f32) / (per_row as f32)).ceil() as usize;
    let row_spacing = if row_count > 1 {
        48.0 / (row_count.saturating_sub(1) as f32)
    } else {
        0.0
    };
    let mut positions = Vec::with_capacity(count);

    for index in 0..count {
        let row = index / per_row;
        let col = index % per_row;
        let columns_in_row = if row == row_count.saturating_sub(1) {
            let remainder = count % per_row;
            if remainder == 0 {
                per_row
            } else {
                remainder
            }
        } else {
            per_row
        };
        let x = if columns_in_row == 1 {
            50.0
        } else {
            18.0 + (64.0 / (columns_in_row.saturating_sub(1) as f32)) * (col as f32)
        };
        let y = 40.0 + row_spacing * (row as f32);
        positions.push(GraphNodePosition { x, y });
    }

    positions
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

fn parse_document(
    input: &ParseInput,
    template: &str,
    _options: &ParseOptions,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    match input.format {
        DocumentFormat::Pdf | DocumentFormat::Image => {
            parse_visual_document(input, template, config)
        }
        DocumentFormat::Docx | DocumentFormat::Doc => parse_text_document(input, template, config),
    }
}

fn parse_visual_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    let page_images = match input.format {
        DocumentFormat::Image => vec![PathBuf::from(&input.path)],
        DocumentFormat::Pdf => convert_pdf_to_pngs(Path::new(&input.path))?,
        _ => unreachable!(),
    };

    let total = page_images.len() as u32;
    let mut pages = Vec::new();
    let mut assets = Vec::new();
    let mut success_count = 0usize;
    let mut failed_count = 0usize;

    for (idx, image_path) in page_images.iter().enumerate() {
        emit_event(&ParseEvent::ConvertingPages {
            current: (idx + 1) as u32,
            total,
        })?;

        let image_bytes = fs::read(image_path)
            .with_context(|| format!("failed to read rendered image {}", image_path.display()))?;
        let relative_path = format!("images/page_{}.png", idx + 1);
        assets.push(OutputAsset {
            relative_path: relative_path.clone(),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(&image_bytes),
        });

        emit_event(&ParseEvent::Parsing {
            current: (idx + 1) as u32,
            total,
        })?;

        match parse_image_with_provider(config, &image_bytes, template) {
            Ok(markdown) => {
                success_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: Some(markdown.clone()),
                    plain_text: Some(markdown),
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: None,
                });
            }
            Err(error) => {
                failed_count += 1;
                pages.push(ParsedPage {
                    index: idx,
                    markdown: None,
                    plain_text: None,
                    svg: None,
                    image_asset_path: Some(relative_path),
                    error_message: Some(error.to_string()),
                });
            }
        }
    }

    Ok(ParsedDocument {
        page_count: pages.len(),
        pages,
        assets,
        success_count,
        failed_count,
    })
}

fn parse_text_document(
    input: &ParseInput,
    template: &str,
    config: &EngineConfig,
) -> Result<ParsedDocument> {
    let text = extract_text_via_textutil(Path::new(&input.path))?;
    emit_event(&ParseEvent::ConvertingPages {
        current: 1,
        total: 1,
    })?;
    emit_event(&ParseEvent::Parsing {
        current: 1,
        total: 1,
    })?;

    let page = match parse_text_with_provider(config, &text, template) {
        Ok(markdown) => ParsedPage {
            index: 0,
            markdown: Some(markdown.clone()),
            plain_text: Some(markdown),
            svg: None,
            image_asset_path: None,
            error_message: None,
        },
        Err(error) => ParsedPage {
            index: 0,
            markdown: None,
            plain_text: Some(text.clone()),
            svg: None,
            image_asset_path: None,
            error_message: Some(error.to_string()),
        },
    };

    let failed_count = usize::from(page.markdown.is_none());
    Ok(ParsedDocument {
        pages: vec![page],
        assets: Vec::new(),
        page_count: 1,
        success_count: 1usize.saturating_sub(failed_count),
        failed_count,
    })
}

fn convert_pdf_to_pngs(path: &Path) -> Result<Vec<PathBuf>> {
    let temp = tempdir().context("failed to create temp directory for pdf conversion")?;
    let prefix = temp.path().join("page");
    let status = Command::new(resolve_binary(
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
    ))
    .arg("-png")
    .arg(path)
    .arg(&prefix)
    .status()
    .context("failed to launch pdftoppm")?;
    if !status.success() {
        bail!("pdftoppm failed for {}", path.display());
    }

    let mut outputs = fs::read_dir(temp.path())
        .with_context(|| {
            format!(
                "failed listing converted PDF pages in {}",
                temp.path().display()
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    outputs.sort();

    if outputs.is_empty() {
        bail!("pdf conversion produced no pages for {}", path.display());
    }

    let persisted_root = temp.keep();
    Ok(outputs
        .into_iter()
        .map(|path| persisted_root.join(path.file_name().unwrap()))
        .collect())
}

fn extract_text_via_textutil(path: &Path) -> Result<String> {
    let output = Command::new(resolve_binary("textutil", &["/usr/bin/textutil"]))
        .arg("-convert")
        .arg("txt")
        .arg("-stdout")
        .arg(path)
        .output()
        .context("failed to launch textutil")?;

    if !output.status.success() {
        bail!("textutil failed for {}", path.display());
    }

    let text = String::from_utf8(output.stdout).context("textutil output was not valid UTF-8")?;
    if text.trim().is_empty() {
        bail!(
            "text extraction produced empty output for {}",
            path.display()
        );
    }
    Ok(text)
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
    output: &duckdocs_engine_types::ParseOutputTarget,
) -> Result<Vec<PathBuf>> {
    if let Some(root) = &output.root_dir {
        return Ok(vec![PathBuf::from(root)]);
    }

    let mut candidates = Vec::new();

    if let Some(override_root) = std::env::var_os("DUCKDOCS_OUTPUT_DIR") {
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
    }
}

fn correction_kind_from_slug(value: &str) -> Result<CorrectionKind> {
    match value {
        "merge" => Ok(CorrectionKind::Merge),
        "keep_separate" => Ok(CorrectionKind::KeepSeparate),
        "rename" => Ok(CorrectionKind::Rename),
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
    }
}

fn document_format_from_slug(value: &str) -> Result<DocumentFormat> {
    match value {
        "pdf" => Ok(DocumentFormat::Pdf),
        "docx" => Ok(DocumentFormat::Docx),
        "doc" => Ok(DocumentFormat::Doc),
        "image" => Ok(DocumentFormat::Image),
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
        if let Some(explicit_path) = std::env::var_os("DUCKDOCS_PROJECT_STORE") {
            return Ok(Self {
                path: PathBuf::from(explicit_path),
            });
        }

        let root = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("failed to resolve local data directory"))?;
        let new_path = root.join("HyprDuck/knowledge.sqlite3");
        let legacy_path = root.join("DuckDocs/knowledge.sqlite3");
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
            return Ok(());
        }
        let workspace_root = workspace_root_for_rows(&rows)
            .unwrap_or_else(|| fallback_workspace_root(&self.path, workspace_id));
        let aggregate = aggregate_workspace_project(workspace_id, rows.clone());
        let corrections = self.load_workspace_corrections(workspace_id)?;
        let snapshot = build_brain_repo_snapshot(workspace_id, &rows, &aggregate, &corrections);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EngineConfig {
    #[serde(deserialize_with = "ProviderKind::deserialize_unknown")]
    provider: ProviderKind,
    model_id: String,
    api_key: String,
    base_url: Option<String>,
    #[serde(default = "default_prompt_template")]
    prompt_template: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: None,
            prompt_template: default_prompt_template(),
        }
    }
}

fn default_prompt_template() -> String {
    "General".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderKind {
    OpenRouter,
    Ollama,
}

impl ProviderKind {
    /// Deserializes a provider slug, falling back to `OpenRouter` for unknown values.
    /// This handles legacy config files that may contain removed providers like `open_ai` or `anthropic`.
    fn deserialize_unknown<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let slug = String::deserialize(deserializer)?;
        Ok(Self::from_slug(&slug).unwrap_or(Self::OpenRouter))
    }
}

impl ProviderKind {
    fn id_slug(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router",
            Self::Ollama => "ollama",
        }
    }

    fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Self::Ollama => "http://127.0.0.1:11434/v1/chat/completions",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
        }
    }

    fn requires_api_key(&self) -> bool {
        !matches!(self, Self::Ollama)
    }

    fn supports_base_url(&self) -> bool {
        true
    }
}

struct EngineConfigStore {
    path: PathBuf,
}

impl EngineConfigStore {
    fn default() -> Result<Self> {
        if let Some(explicit_dir) = std::env::var_os("DUCKDOCS_CONFIG_DIR") {
            return Ok(Self {
                path: PathBuf::from(explicit_dir).join("engine-config.json"),
            });
        }

        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
        Ok(Self {
            path: home.join(".duckdocs/engine-config.json"),
        })
    }

    fn load(&self) -> Result<EngineConfig> {
        if !self.path.exists() {
            let config = EngineConfig::default();
            self.save(&config)?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("failed reading {}", self.path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed decoding {}", self.path.display()))
    }

    fn save(&self, config: &EngineConfig) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating config directory {}", parent.display())
            })?;
        }
        let payload =
            serde_json::to_string_pretty(config).context("failed encoding engine config")?;
        fs::write(&self.path, payload)
            .with_context(|| format!("failed writing {}", self.path.display()))
    }
}

impl EngineConfig {
    fn to_payload(&self) -> EngineConfigPayload {
        let provider_options = ProviderKind::all()
            .into_iter()
            .map(|provider| ProviderOption {
                id: provider.id_slug().to_string(),
                label: provider.label().to_string(),
                requires_api_key: provider.requires_api_key(),
                supports_base_url: provider.supports_base_url(),
            })
            .collect();

        EngineConfigPayload {
            provider: self.provider.id_slug().to_string(),
            model_id: self.model_id.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            prompt_template: self.prompt_template.clone(),
            provider_options,
            model_options: model_options_for(&self.provider)
                .into_iter()
                .map(str::to_string)
                .collect(),
            prompt_template_options: prompt_template_options()
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    fn from_payload(payload: EngineConfigPayload) -> Self {
        Self {
            provider: ProviderKind::from_slug(&payload.provider)
                .unwrap_or(ProviderKind::OpenRouter),
            model_id: payload.model_id,
            api_key: payload.api_key,
            base_url: payload.base_url,
            prompt_template: payload.prompt_template,
        }
    }
}

impl ProviderKind {
    fn all() -> [ProviderKind; 2] {
        [Self::OpenRouter, Self::Ollama]
    }

    fn from_slug(value: &str) -> Option<Self> {
        match value {
            "open_router" => Some(Self::OpenRouter),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

fn prompt_template_options() -> [&'static str; 6] {
    [
        "General",
        "API Documentation",
        "UI Flow",
        "Tutorial",
        "Code Snippets",
        "Data Tables",
    ]
}

fn model_options_for(provider: &ProviderKind) -> Vec<&'static str> {
    duckdocs_engine_types::model_options_for(provider.id_slug())
}

fn provider_model_catalog() -> ProviderModelCatalogResponseData {
    let provider_models = ProviderKind::all()
        .into_iter()
        .map(|provider| {
            (
                provider.id_slug().to_string(),
                model_options_for(&provider)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        })
        .collect();

    ProviderModelCatalogResponseData {
        provider_models,
        ollama_vision_prefixes: duckdocs_engine_types::ollama_vision_prefixes()
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn validate_provider(config: &EngineConfig) -> ValidateProviderResponseData {
    let mut issues = Vec::new();
    if config.provider.requires_api_key() && config.api_key.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_api_key".into(),
            message: format!("{} requires an API key.", config.provider.label()),
        });
    }

    if config.model_id.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "missing_model_id".into(),
            message: "A model ID is required.".into(),
        });
    }

    if let Some(base_url) = &config.base_url {
        if !base_url.trim().is_empty()
            && !(base_url.starts_with("http://") || base_url.starts_with("https://"))
        {
            issues.push(ValidationIssue {
                code: "invalid_base_url".into(),
                message: "Base URL must start with http:// or https://".into(),
            });
        }
    }

    ValidateProviderResponseData {
        ready: issues.is_empty(),
        issues,
    }
}

fn check_readiness(config_store: &EngineConfigStore) -> RuntimeReadinessResponseData {
    let mut checks = vec![ReadinessCheck {
        id: "runtime_process".into(),
        label: "Runtime process".into(),
        ready: true,
        required: true,
        message: "Runtime process is accepting commands.".into(),
    }];

    let config = match config_store.load() {
        Ok(config) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: true,
                required: true,
                message: format!("Loaded {}", config_store.path.display()),
            });
            config
        }
        Err(error) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: false,
                required: true,
                message: error.to_string(),
            });
            return RuntimeReadinessResponseData {
                ready: false,
                provider: "unknown".into(),
                model_id: String::new(),
                checks,
            };
        }
    };

    let validation = validate_provider(&config);
    checks.push(ReadinessCheck {
        id: "provider_config".into(),
        label: "Provider config".into(),
        ready: validation.ready,
        required: true,
        message: if validation.ready {
            format!(
                "{} is configured with model {}.",
                config.provider.label(),
                config.model_id
            )
        } else {
            validation
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        },
    });

    if matches!(config.provider, ProviderKind::Ollama) {
        checks.push(check_ollama_endpoint(&config));
    }

    checks.push(check_path_exists(
        "pdf_converter",
        "PDF converter",
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
        false,
    ));
    checks.push(check_path_exists(
        "text_converter",
        "DOC/DOCX text converter",
        "textutil",
        &["/usr/bin/textutil"],
        false,
    ));
    checks.push(check_path_exists(
        "knowledge_store",
        "Knowledge store",
        "sqlite3",
        &["/usr/bin/sqlite3"],
        true,
    ));

    RuntimeReadinessResponseData {
        ready: checks
            .iter()
            .filter(|check| check.required)
            .all(|check| check.ready),
        provider: config.provider.id_slug().into(),
        model_id: config.model_id,
        checks,
    }
}

fn check_path_exists(
    id: &str,
    label: &str,
    binary_name: &str,
    common_paths: &[&str],
    required: bool,
) -> ReadinessCheck {
    let path = resolve_binary(binary_name, common_paths);
    let ready = path.exists();
    ReadinessCheck {
        id: id.into(),
        label: label.into(),
        ready,
        required,
        message: if ready {
            format!("Found {}", path.display())
        } else {
            format!("Missing {binary_name} in PATH or common install locations")
        },
    }
}

fn check_ollama_endpoint(config: &EngineConfig) -> ReadinessCheck {
    let endpoint = ollama_models_endpoint(config);
    let result = Client::builder()
        .timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(1))
        .build()
        .and_then(|client| client.get(&endpoint).send())
        .and_then(|response| response.error_for_status().map(|_| ()));

    match result {
        Ok(()) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: true,
            required: true,
            message: format!("Ollama responded at {endpoint}."),
        },
        Err(error) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: false,
            required: true,
            message: format!("Ollama is not reachable at {endpoint}: {error}"),
        },
    }
}

fn ollama_models_endpoint(config: &EngineConfig) -> String {
    let raw = config
        .base_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string())
        .replace("/v1/chat/completions", "/v1/models")
        .replace("/api/generate", "/api/tags");

    if let Ok(mut url) = Url::parse(&raw) {
        let path = url.path().trim_end_matches('/');
        if path.is_empty() {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/v1" {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/api" {
            url.set_path("/api/tags");
            return url.to_string();
        }
    }

    raw
}

fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this page was packaged as an image-only placeholder.\n\n- Template: {}\n- Image bytes: {}\n",
            config.provider.id_slug(),
            template,
            image_bytes.len()
        ));
    }

    let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let prompt = format!(
        "Convert this document page into clean markdown. Template: {template}. Preserve headings, lists, tables, and code blocks where possible."
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, Some(image_base64))
        }
    }
}

fn parse_text_with_provider(config: &EngineConfig, text: &str, template: &str) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this document was returned from extracted text.\n\n- Template: {}\n\n{}",
            config.provider.id_slug(),
            template,
            text
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    match config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, None)
        }
    }
}

fn provider_unavailable(config: &EngineConfig) -> bool {
    match config.provider {
        ProviderKind::OpenRouter => config.api_key.trim().is_empty(),
        ProviderKind::Ollama => false,
    }
}

fn parse_openai_compatible(
    config: &EngineConfig,
    prompt: &str,
    image_base64: Option<String>,
) -> Result<String> {
    let client = Client::builder()
        .timeout(None)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("failed to build provider HTTP client")?;
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(image_base64) = image_base64 {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/png;base64,{image_base64}") }
        }));
    }

    let body = serde_json::json!({
        "model": config.model_id,
        "messages": [{ "role": "user", "content": content }],
    });
    let endpoint = config
        .base_url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string());
    let response = client
        .post(&endpoint)
        .bearer_auth(config.api_key.clone())
        .json(&body)
        .send()
        .map_err(|error| anyhow!("failed to send provider request to {endpoint}: {error:#}"))?;
    let response = response
        .error_for_status()
        .map_err(|error| anyhow!("provider returned error status from {endpoint}: {error:#}"))?;
    let json: serde_json::Value = response.json().map_err(|error| {
        anyhow!("failed to decode provider response from {endpoint}: {error:#}")
    })?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("provider response did not include markdown text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_fixture_project(temp: &tempfile::TempDir, markdown: &str) -> KnowledgeProject {
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some("/tmp/source.pdf".into()),
            source_manifest_path: None,
            workspace_id: None,
            source_id: None,
        };

        let markdown = fs::read_to_string(&markdown_path).expect("read markdown");
        compile_knowledge_project(&request, &markdown, None)
    }

    fn compile_manifest_fixture_project(
        temp: &tempfile::TempDir,
        markdown: &str,
    ) -> (KnowledgeProject, SourceArtifactManifest) {
        compile_manifest_fixture_project_with_source(temp, markdown, "source-test", "source", 2)
    }

    fn compile_manifest_fixture_project_with_source(
        temp: &tempfile::TempDir,
        markdown: &str,
        source_id: &str,
        output_name: &str,
        updated_at: u64,
    ) -> (KnowledgeProject, SourceArtifactManifest) {
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest_with_source(temp, source_id, output_name, updated_at);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };

        (
            compile_knowledge_project(&request, markdown, Some(&manifest)),
            manifest,
        )
    }

    fn sample_parse_result() -> ParseResult {
        ParseResult {
            version: "1".into(),
            markdown: "# Sample import\n\n## Page 1\n\nGrounded evidence stays visible.\n".into(),
            pages: vec![ParsedPage {
                index: 0,
                markdown: Some("Grounded evidence stays visible.".into()),
                plain_text: Some("Grounded evidence stays visible.".into()),
                svg: None,
                image_asset_path: Some("images/page_1.png".into()),
                error_message: None,
            }],
            assets: vec![OutputAsset {
                relative_path: "images/page_1.png".into(),
                mime_type: "image/png".into(),
                base64: base64::engine::general_purpose::STANDARD.encode(b"png"),
            }],
            metadata: ParseMetadata {
                engine_id: "test/model".into(),
                duration_ms: 12,
                page_count: 1,
            },
            success_count: 1,
            failed_count: 0,
        }
    }

    fn sample_parse_request(temp: &tempfile::TempDir) -> ParseRequest {
        let source_path = temp.path().join("source.pdf");
        fs::write(&source_path, b"%PDF sample").expect("write source");
        ParseRequest {
            version: "1".into(),
            input: ParseInput {
                path: source_path.display().to_string(),
                format: DocumentFormat::Pdf,
            },
            template: "General".into(),
            options: ParseOptions::default(),
            output: None,
        }
    }

    fn sample_manifest(temp: &tempfile::TempDir) -> SourceArtifactManifest {
        sample_manifest_with_source(temp, "source-test", "source", 2)
    }

    fn sample_manifest_with_source(
        temp: &tempfile::TempDir,
        source_id: &str,
        output_name: &str,
        updated_at: u64,
    ) -> SourceArtifactManifest {
        SourceArtifactManifest {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            source_id: source_id.into(),
            original_path: temp
                .path()
                .join(format!("{output_name}.pdf"))
                .display()
                .to_string(),
            source_path: temp
                .path()
                .join(format!("default/sources/{source_id}/{output_name}.pdf"))
                .display()
                .to_string(),
            markdown_path: temp
                .path()
                .join(format!("default/artifacts/{source_id}/source.md"))
                .display()
                .to_string(),
            artifact_root: temp
                .path()
                .join(format!("default/artifacts/{source_id}"))
                .display()
                .to_string(),
            manifest_path: temp
                .path()
                .join(format!(
                    "default/artifacts/{source_id}/source-manifest.json"
                ))
                .display()
                .to_string(),
            format: DocumentFormat::Pdf,
            output_name: output_name.into(),
            status: IngestStatus::Ingested,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            pages: vec![PageArtifact {
                index: 0,
                label: "Page 1".into(),
                image_path: None,
                markdown_path: None,
                plain_text_path: None,
                error_message: None,
            }],
            created_at: 1,
            updated_at,
        }
    }

    fn multi_source_fixture_pages(temp: &tempfile::TempDir, source_id: &str) -> Vec<PageArtifact> {
        (0..2)
            .map(|index| PageArtifact {
                index,
                label: format!("Page {}", index + 1),
                image_path: Some(
                    temp.path()
                        .join(format!(
                            "default/artifacts/{source_id}/images/page_{}.png",
                            index + 1
                        ))
                        .display()
                        .to_string(),
                ),
                markdown_path: Some(
                    temp.path()
                        .join(format!(
                            "default/artifacts/{source_id}/pages/page_{}.md",
                            index + 1
                        ))
                        .display()
                        .to_string(),
                ),
                plain_text_path: None,
                error_message: None,
            })
            .collect()
    }

    fn rename_first_concept_for_test(
        project: &mut KnowledgeProject,
        canonical_name: &str,
        aliases: &[&str],
    ) {
        let concept_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        rename_concept_for_test(project, &concept_id, canonical_name, aliases);
    }

    fn rename_concept_for_test(
        project: &mut KnowledgeProject,
        concept_id: &str,
        canonical_name: &str,
        aliases: &[&str],
    ) {
        let node = project
            .nodes
            .iter_mut()
            .find(|node| node.id == concept_id)
            .expect("mutable concept node");
        node.label = canonical_name.into();
        let detail = project
            .details_by_node_id
            .get_mut(concept_id)
            .expect("concept detail");
        detail.canonical_name = canonical_name.into();
        detail.aliases = aliases.iter().map(|alias| (*alias).to_string()).collect();
        detail.node.label = canonical_name.into();
    }

    #[test]
    fn compile_and_store_project_round_trip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nHyprDuck compile path keeps evidence visible for every concept.\nExplainable graph view grounds answers in visible snippets.\n\n## Page 2\n\nEvidence inspector helps people trust the graph.\n",
        );
        let markdown_path = temp.path().join("sample.md");
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some("/tmp/source.pdf".into()),
            source_manifest_path: None,
            workspace_id: None,
            source_id: None,
        };
        assert_eq!(project.summary.status, ProjectStatus::Ready);
        assert!(project
            .nodes
            .iter()
            .any(|node| node.kind == GraphNodeKind::Concept));
        assert!(!project.edges.is_empty());
        assert!(project
            .edges
            .iter()
            .any(|edge| edge.kind == RelationKind::RelatedTo));

        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path);
        store
            .save_project(&project, &request, None)
            .expect("save project to sqlite");

        let loaded = store
            .load_project(Some(&project.summary.project_id))
            .expect("load project")
            .expect("stored project");
        assert_eq!(loaded.summary.project_id, project.summary.project_id);
        assert_eq!(loaded.summary.title, "Sample import");
        assert_eq!(loaded.nodes.len(), project.nodes.len());
        assert_eq!(loaded.edges.len(), project.edges.len());
        assert!(loaded.details_by_node_id.contains_key("document"));
        assert!(!loaded.edge_details_by_id.is_empty());
    }

    #[test]
    fn project_store_persists_workspace_source_manifest_summary() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown =
            "# Sample import\n\n## Page 1\n\nSource evidence belongs to the workspace.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some("/tmp/source.pdf".into()),
            source_manifest_path: Some(sample_manifest(&temp).manifest_path),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some("source-test".into()),
        };
        let manifest = sample_manifest(&temp);
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));

        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save project with source");

        let workspace_id = store
            .load_latest_workspace_id()
            .expect("load latest workspace")
            .expect("workspace id");
        let sources = store.load_sources(&workspace_id).expect("load sources");
        assert_eq!(workspace_id, DEFAULT_WORKSPACE_ID);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "source-test");
        assert_eq!(sources[0].page_count, 1);
        assert_eq!(sources[0].status, IngestStatus::Ingested);
        assert_eq!(sources[0].format, DocumentFormat::Pdf);
        assert_eq!(sources[0].success_count, 1);
        assert_eq!(sources[0].failed_count, 0);
    }

    #[test]
    fn project_store_materializes_brain_repo_artifacts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));

        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");

        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        assert!(workspace_root.join("brain-manifest.json").exists());
        assert!(workspace_root.join("graph/nodes.json").exists());
        assert!(workspace_root.join("graph/edges.json").exists());
        assert!(workspace_root.join("graph/evidence.json").exists());
        assert!(workspace_root.join("graph/entities.json").exists());
        assert!(workspace_root.join("graph/claims.json").exists());
        assert!(workspace_root
            .join("artifacts/source-test/extraction.json")
            .exists());
        assert!(workspace_root.join("events/brain_events.jsonl").exists());
        assert!(workspace_root.join("wiki/index.md").exists());
        assert!(workspace_root.join("wiki/log.md").exists());
        assert!(workspace_root.join("wiki/overview.md").exists());
        assert!(workspace_root
            .join("reviews/proposed-updates/.gitkeep")
            .exists());
        assert!(workspace_root
            .join("reviews/lint-reports/.gitkeep")
            .exists());

        let manifest_json =
            fs::read_to_string(workspace_root.join("brain-manifest.json")).expect("brain manifest");
        let snapshot: BrainRepoSnapshot =
            serde_json::from_str(&manifest_json).expect("decode brain manifest");
        assert_eq!(snapshot.workspace_id, DEFAULT_WORKSPACE_ID);
        assert_eq!(snapshot.sources.len(), 1);
        assert_eq!(snapshot.extractions.len(), 1);
        assert_eq!(snapshot.extractions[0].extractor, "heuristic");
        assert_eq!(snapshot.extractions[0].source_refs, vec!["source-test"]);
        assert!(!snapshot.extractions[0].page_refs.is_empty());
        assert!(snapshot.extractions[0].entities.iter().all(|entity| !entity
            .evidence_refs
            .is_empty()
            && !entity.source_refs.is_empty()
            && !entity.page_refs.is_empty()
            && !entity.provenance.is_empty()));
        assert!(snapshot.extractions[0]
            .claims
            .iter()
            .all(|claim| !claim.evidence_refs.is_empty()
                && !claim.source_refs.is_empty()
                && !claim.page_refs.is_empty()
                && !claim.provenance.is_empty()));
        assert!(snapshot.extractions[0]
            .relations
            .iter()
            .all(|relation| !relation.evidence_refs.is_empty()
                && !relation.source_refs.is_empty()
                && !relation.page_refs.is_empty()
                && !relation.provenance.is_empty()));
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.kind == BrainNodeKind::Concept));
        assert!(snapshot
            .entities
            .iter()
            .any(|entity| entity.kind == BrainNodeKind::Concept
                && !entity.evidence_refs.is_empty()
                && !entity.source_refs.is_empty()));
        assert!(snapshot.claims.iter().any(|claim| {
            claim.status == "supported"
                && claim.statement.contains("Agent brain")
                && !claim.topic_refs.is_empty()
                && !claim.evidence_refs.is_empty()
                && !claim.source_refs.is_empty()
        }));
        assert!(snapshot
            .relations
            .iter()
            .all(|relation| !relation.evidence_ids.is_empty()));
        assert!(snapshot
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::GraphMaterialized));
        let extraction_json =
            fs::read_to_string(workspace_root.join("artifacts/source-test/extraction.json"))
                .expect("read extraction artifact");
        let extraction: duckdocs_engine_types::StructuredExtractionArtifact =
            serde_json::from_str(&extraction_json).expect("decode extraction artifact");
        assert_eq!(extraction.extractor, "heuristic");
        assert_eq!(extraction.source_id, "source-test");
        assert!(extraction.created_at > 0);
        assert!(!extraction.entities.is_empty());
        assert!(!extraction.claims.is_empty());
        assert!(extraction
            .claims
            .iter()
            .all(|claim| !claim.evidence_refs.is_empty()));
        let index = fs::read_to_string(workspace_root.join("wiki/index.md")).expect("wiki index");
        assert!(index.contains("## Sources"));
        assert!(index.contains("## Topics"));
    }

    #[test]
    fn structured_extraction_does_not_trust_claims_or_relations_without_evidence() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (mut project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\nShared graph context keeps evidence visible.\n",
        );

        for detail in project.details_by_node_id.values_mut() {
            if detail.node.kind == GraphNodeKind::Concept {
                detail.evidence.clear();
            }
        }
        for detail in project.edge_details_by_id.values_mut() {
            detail.evidence.clear();
        }

        let row = StoredSourceRow {
            summary: source_summary_from_manifest(&manifest),
            project_id: project.summary.project_id.clone(),
            manifest_path: manifest.manifest_path.clone(),
        };
        let rows = vec![(row, Some(project.clone()))];
        let snapshot = build_brain_repo_snapshot(DEFAULT_WORKSPACE_ID, &rows, &project, &[]);

        assert!(snapshot.claims.is_empty());
        assert!(snapshot.relations.is_empty());
        assert_eq!(snapshot.extractions.len(), 1);
        assert!(snapshot.extractions[0].claims.is_empty());
        assert!(snapshot.extractions[0].relations.is_empty());
    }

    #[test]
    fn read_only_brain_api_reads_materialized_repo() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };

        let search = handle_search_brain(SearchBrainRequest {
            scope: scope.clone(),
            query: "agent brain".into(),
            limit: Some(20),
        })
        .expect("search brain");
        assert!(!search.results.is_empty());
        assert!(search
            .results
            .iter()
            .any(|result| result.kind == BrainSearchResultKind::Entity));
        assert!(search
            .results
            .iter()
            .any(|result| result.kind == BrainSearchResultKind::Claim));

        let source = handle_read_source(ReadSourceRequest {
            scope: scope.clone(),
            source_id: manifest.source_id.clone(),
        })
        .expect("read source");
        assert_eq!(source.source.source_id, manifest.source_id);
        assert!(!source.evidence.is_empty());

        let wiki = handle_read_wiki_page(ReadWikiPageRequest {
            scope: scope.clone(),
            path: "index.md".into(),
        })
        .expect("read wiki page");
        assert_eq!(wiki.page.path, "wiki/index.md");
        assert!(wiki.page.body.contains("Brain Index"));

        let node_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        let node = handle_read_node(ReadNodeRequest {
            scope: scope.clone(),
            node_id,
        })
        .expect("read node");
        assert_eq!(node.node.kind, BrainNodeKind::Concept);
        assert!(!node.evidence.is_empty());

        let events = handle_read_recent_events(ReadRecentEventsRequest {
            scope: scope.clone(),
            limit: Some(2),
        })
        .expect("read recent events");
        assert!(!events.events.is_empty());

        let context_pack = handle_get_context_pack(GetContextPackRequest {
            scope,
            query: "agent brain".into(),
            budget: Some(4000),
        })
        .expect("context pack")
        .context_pack;
        assert_eq!(context_pack.workspace_id, DEFAULT_WORKSPACE_ID);
        assert!(!context_pack.wiki_pages.is_empty());
        assert!(!context_pack.entities.is_empty());
        assert!(!context_pack.claims.is_empty());
        assert!(!context_pack.relations.is_empty());
        assert!(!context_pack.recent_events.is_empty());
    }

    #[test]
    fn brain_search_uses_tokenized_retrieval_and_context_expansion() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Planning memo\n\n## Page 1\n\nAgent brain context stays source backed.\nProject retrieval packs cite evidence for the graph.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };

        let search = handle_search_brain(SearchBrainRequest {
            scope: scope.clone(),
            query: "agents".into(),
            limit: Some(10),
        })
        .expect("search plural query");
        assert!(search.results.iter().any(|result| {
            result.kind == BrainSearchResultKind::Entity && result.title.contains("Agent brain")
        }));
        assert!(search
            .results
            .iter()
            .any(|result| result.kind == BrainSearchResultKind::Evidence));
        assert!(search.results.iter().any(|result| {
            result.kind == BrainSearchResultKind::Claim && result.snippet.contains("evidence:")
        }));

        let context_pack = handle_get_context_pack(GetContextPackRequest {
            scope,
            query: "agents".into(),
            budget: Some(4000),
        })
        .expect("context pack")
        .context_pack;
        assert!(context_pack
            .entities
            .iter()
            .any(|entity| entity.name.contains("Agent brain")));
        assert!(!context_pack.claims.is_empty());
        assert!(!context_pack.relations.is_empty());
        assert!(!context_pack.sources.is_empty());
        assert!(!context_pack.evidence.is_empty());
    }

    #[test]
    fn safe_brain_update_proposals_auto_apply_memory_records_without_mutating_graph_or_wiki() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        write_source_manifest(&manifest).expect("write source manifest");
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        let nodes_before =
            fs::read_to_string(workspace_root.join("graph/nodes.json")).expect("read nodes before");
        let edges_before =
            fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges before");
        let wiki_before =
            fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki before");

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let actor = BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        };
        let concept_node_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        let proposal_specs = [
            (
                BrainProposalKind::Memory,
                "Remember parser wedge",
                "HyprDuck starts from source-backed document parsing.",
                None,
                None,
                None,
                Vec::new(),
            ),
            (
                BrainProposalKind::Claim,
                "Claim source backing",
                "Every durable brain fact should point back to source evidence.",
                Some(concept_node_id.clone()),
                None,
                None,
                vec![concept_node_id.clone()],
            ),
            (
                BrainProposalKind::Link,
                "Relate document and concept",
                "The concept is derived from the imported document.",
                Some(concept_node_id.clone()),
                None,
                Some(BrainRelationKind::RelatedTo),
                vec!["document".into()],
            ),
            (
                BrainProposalKind::Observation,
                "Append session observation",
                "The agent saw the user prioritize a review queue.",
                None,
                None,
                None,
                Vec::new(),
            ),
            (
                BrainProposalKind::SourceNote,
                "Annotate source",
                "This source should seed the local brain.",
                None,
                Some(manifest.source_id.clone()),
                None,
                Vec::new(),
            ),
        ];

        let mut proposal_paths = Vec::new();
        for (kind, title, body, target_node_id, target_source_id, relation_kind, node_refs) in
            proposal_specs
        {
            let response = handle_propose_brain_update(ProposeBrainUpdateRequest {
                scope: scope.clone(),
                kind,
                title: title.into(),
                body: body.into(),
                actor: actor.clone(),
                target_node_id,
                target_source_id,
                relation_kind,
                source_description: None,
                source_user_context: None,
                source_ingest_instruction: None,
                source_refs: vec![manifest.source_id.clone()],
                node_refs,
                evidence_refs: Vec::new(),
            })
            .expect("propose brain update");
            if matches!(
                response.proposal.kind,
                BrainProposalKind::Memory
                    | BrainProposalKind::Observation
                    | BrainProposalKind::SourceNote
            ) {
                assert_eq!(response.proposal.status, BrainProposalStatus::Accepted);
                assert_eq!(response.event.policy_result, "auto_applied");
            } else {
                assert_eq!(response.proposal.status, BrainProposalStatus::PendingReview);
                assert_eq!(response.event.policy_result, "needs_review");
            }
            proposal_paths.push(PathBuf::from(response.proposal_path));
        }

        let mut accepted_count = 0usize;
        let mut pending_count = 0usize;
        for path in &proposal_paths {
            assert!(path.exists(), "missing proposal {}", path.display());
            let proposal_json = fs::read_to_string(path).expect("read proposal");
            let proposal: BrainUpdateProposal =
                serde_json::from_str(&proposal_json).expect("decode proposal");
            match proposal.status {
                BrainProposalStatus::Accepted => accepted_count += 1,
                BrainProposalStatus::PendingReview => pending_count += 1,
                BrainProposalStatus::Rejected => panic!("proposal should not be rejected"),
            }
        }
        assert_eq!(accepted_count, 3);
        assert_eq!(pending_count, 2);

        let memory_records: Vec<MemoryRecord> =
            read_json_artifact(&workspace_root.join("memory/records.json"))
                .expect("read memory records");
        assert_eq!(memory_records.len(), 2);
        assert!(memory_records
            .iter()
            .any(|memory| memory.title == "Remember parser wedge"));
        assert!(memory_records
            .iter()
            .any(|memory| memory.title == "Append session observation"));
        assert!(!memory_records
            .iter()
            .any(|memory| memory.title == "Annotate source"));
        let updated_manifest: SourceArtifactManifest =
            read_json_artifact(&PathBuf::from(&manifest.manifest_path))
                .expect("read updated source manifest");
        assert_eq!(
            updated_manifest.description,
            "This source should seed the local brain."
        );
        let stored_sources = store
            .load_sources(DEFAULT_WORKSPACE_ID)
            .expect("load source summaries after source note");
        assert_eq!(
            stored_sources[0].description,
            "This source should seed the local brain."
        );
        let read_source = handle_read_source(ReadSourceRequest {
            scope: scope.clone(),
            source_id: manifest.source_id.clone(),
        })
        .expect("read source after source note");
        assert_eq!(
            read_source.source.description,
            "This source should seed the local brain."
        );
        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::SourceNote,
            title: "Update source metadata fields".into(),
            body: "Fallback source note body.".into(),
            actor: actor.clone(),
            target_node_id: None,
            target_source_id: Some(manifest.source_id.clone()),
            relation_kind: None,
            source_description: Some("Reader guide".into()),
            source_user_context: Some("Imported for agent planning.".into()),
            source_ingest_instruction: Some("Extract decisions and open questions.".into()),
            source_refs: vec![manifest.source_id.clone()],
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
        })
        .expect("propose explicit source metadata");
        let explicit_manifest: SourceArtifactManifest =
            read_json_artifact(&PathBuf::from(&manifest.manifest_path))
                .expect("read explicit source manifest");
        assert_eq!(explicit_manifest.description, "Reader guide");
        assert_eq!(
            explicit_manifest.user_context,
            "Imported for agent planning."
        );
        assert_eq!(
            explicit_manifest.ingest_instruction,
            "Extract decisions and open questions."
        );

        let events = handle_read_recent_events(ReadRecentEventsRequest {
            scope: scope.clone(),
            limit: Some(10),
        })
        .expect("read proposal events");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::MemoryProposed));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::ClaimProposed));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::LinkProposed));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::ObservationAppended));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::SourceNoteProposed));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::MemoryAccepted));

        let search = handle_search_brain(SearchBrainRequest {
            scope: scope.clone(),
            query: "review queue".into(),
            limit: Some(10),
        })
        .expect("search brain memories");
        assert!(search.results.iter().any(|result| {
            result.kind == BrainSearchResultKind::Memory
                && result.title == "Append session observation"
        }));

        let context_pack = handle_get_context_pack(GetContextPackRequest {
            scope: scope.clone(),
            query: "source backed document parsing".into(),
            budget: Some(4000),
        })
        .expect("context pack")
        .context_pack;
        assert!(context_pack
            .memories
            .iter()
            .any(|memory| memory.title == "Remember parser wedge"));

        assert_eq!(
            fs::read_to_string(workspace_root.join("graph/nodes.json")).expect("read nodes after"),
            nodes_before
        );
        assert_eq!(
            fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges after"),
            edges_before
        );
        assert_eq!(
            fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki after"),
            wiki_before
        );

        store
            .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
            .expect("rematerialize workspace brain repo");
        let rematerialized_memory_records: Vec<MemoryRecord> =
            read_json_artifact(&workspace_root.join("memory/records.json"))
                .expect("read rematerialized memory records");
        assert_eq!(rematerialized_memory_records.len(), 2);
        let rematerialized_events =
            read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
                .expect("read rematerialized events");
        let accepted_event_ids = events
            .events
            .iter()
            .filter(|event| event.event_type == BrainEventKind::MemoryAccepted)
            .map(|event| event.event_id.clone())
            .collect::<BTreeSet<_>>();
        let claim_event_ids = events
            .events
            .iter()
            .filter(|event| event.event_type == BrainEventKind::ClaimProposed)
            .map(|event| event.event_id.clone())
            .collect::<BTreeSet<_>>();
        let rematerialized_event_ids = rematerialized_events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<BTreeSet<_>>();
        assert!(accepted_event_ids
            .iter()
            .all(|event_id| rematerialized_event_ids.contains(event_id)));
        assert!(claim_event_ids
            .iter()
            .all(|event_id| rematerialized_event_ids.contains(event_id)));
    }

    #[test]
    fn brain_review_health_lists_pending_claims_and_links_only() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let actor = BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        };
        let concept_node_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();

        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Memory,
            title: "Remember accepted memory".into(),
            body: "Safe memory proposals are auto-applied.".into(),
            actor: actor.clone(),
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
        })
        .expect("propose memory");
        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Claim,
            title: "Claim needs review".into(),
            body: "Durable claims require human attention before write-through.".into(),
            actor: actor.clone(),
            target_node_id: Some(concept_node_id.clone()),
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![concept_node_id.clone()],
            evidence_refs: Vec::new(),
        })
        .expect("propose claim");
        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Link,
            title: "Link needs review".into(),
            body: "Typed graph links require review before graph mutation.".into(),
            actor,
            target_node_id: Some(concept_node_id.clone()),
            target_source_id: None,
            relation_kind: Some(BrainRelationKind::RelatedTo),
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id],
            node_refs: vec!["document".into()],
            evidence_refs: Vec::new(),
        })
        .expect("propose link");

        let reviews = handle_list_brain_review_items(ListBrainReviewItemsRequest {
            scope: scope.clone(),
        })
        .expect("list reviews");
        assert_eq!(reviews.items.len(), 2);
        assert!(reviews
            .items
            .iter()
            .all(|item| item.status == BrainProposalStatus::PendingReview));
        assert!(reviews
            .items
            .iter()
            .any(|item| item.kind == BrainProposalKind::Claim));
        assert!(reviews
            .items
            .iter()
            .any(|item| item.kind == BrainProposalKind::Link));
        assert!(!reviews
            .items
            .iter()
            .any(|item| item.kind == BrainProposalKind::Memory));

        let health =
            handle_get_brain_health(GetBrainHealthRequest { scope }).expect("get brain health");
        assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
        assert_eq!(health.attention_count, 2);
        assert_eq!(health.review_items.len(), 2);
    }

    #[test]
    fn brain_health_is_clean_for_empty_workspace() {
        let temp = tempfile::tempdir().expect("temp dir");
        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };

        let health =
            handle_get_brain_health(GetBrainHealthRequest { scope }).expect("get brain health");

        assert_eq!(health.status, BrainHealthStatus::Clean);
        assert_eq!(health.attention_count, 0);
        assert!(health.review_items.is_empty());
        assert!(health.recent_events.is_empty());
    }

    #[test]
    fn brain_maintenance_repairs_generated_index_log_and_structured_artifacts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        fs::write(workspace_root.join("wiki/index.md"), "# Broken\n").expect("break index");
        fs::write(workspace_root.join("wiki/log.md"), "# Broken log\n").expect("break log");
        fs::remove_file(workspace_root.join("graph/nodes.json")).expect("remove nodes");
        fs::remove_file(workspace_root.join("graph/evidence.json")).expect("remove evidence");
        fs::remove_file(workspace_root.join("graph/claims.json")).expect("remove claims");

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let health = handle_get_brain_health(GetBrainHealthRequest { scope })
            .expect("health runs maintenance");
        assert_eq!(health.status, BrainHealthStatus::Clean);
        assert!(fs::read_to_string(workspace_root.join("wiki/index.md"))
            .expect("read repaired index")
            .contains("Brain Index"));
        assert!(fs::read_to_string(workspace_root.join("wiki/log.md"))
            .expect("read repaired log")
            .contains("graph-materialized"));
        let claims: Vec<ClaimRecord> =
            read_json_artifact(&workspace_root.join("graph/claims.json")).expect("read claims");
        assert!(!claims.is_empty());
        let nodes: Vec<BrainNodeRecord> =
            read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("read nodes");
        assert!(!nodes.is_empty());
        let evidence: Vec<EvidenceRef> =
            read_json_artifact(&workspace_root.join("graph/evidence.json")).expect("read evidence");
        assert!(!evidence.is_empty());
        let report: BrainMaintenanceReport =
            read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
                .expect("read lint report");
        assert!(report.repair_count >= 5);
        assert!(report.repairs.contains(&"wiki/index.md".into()));
        assert!(report.repairs.contains(&"wiki/log.md".into()));
        assert!(report.repairs.contains(&"graph/nodes.json".into()));
        assert!(report.repairs.contains(&"graph/evidence.json".into()));
        assert!(report.repairs.contains(&"graph/claims.json".into()));
    }

    #[test]
    fn brain_maintenance_promotes_risky_lint_findings_to_health_review() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        let claims_path = workspace_root.join("graph/claims.json");
        let mut claims: Vec<ClaimRecord> =
            read_json_artifact(&claims_path).expect("read materialized claims");
        claims.first_mut().expect("claim").evidence_refs.clear();
        write_json_pretty(&claims_path, &claims).expect("write broken claims");

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let health = handle_get_brain_health(GetBrainHealthRequest {
            scope: scope.clone(),
        })
        .expect("health runs maintenance");
        assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
        assert!(health
            .review_items
            .iter()
            .any(|item| item.title.contains("Claim needs evidence")));
        let report: BrainMaintenanceReport =
            read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
                .expect("read lint report");
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.kind == "missing_evidence"));
        let events = handle_read_recent_events(ReadRecentEventsRequest {
            scope,
            limit: Some(20),
        })
        .expect("read maintenance events");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::ReviewCreated));
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::BrainMaintenanceRun));
    }

    #[test]
    fn brain_maintenance_detects_orphan_conflict_and_stale_cases() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);

        let manifest_path = workspace_root.join("brain-manifest.json");
        let mut snapshot: BrainRepoSnapshot =
            read_json_artifact(&manifest_path).expect("read brain manifest");
        snapshot.sources[0].status = "stale".into();
        snapshot.sources[0].updated_at = snapshot.generated_at + 1;
        write_json_pretty(&manifest_path, &snapshot).expect("write stale manifest");

        let nodes_path = workspace_root.join("graph/nodes.json");
        let mut nodes: Vec<BrainNodeRecord> = read_json_artifact(&nodes_path).expect("read nodes");
        nodes.push(BrainNodeRecord {
            node_id: "orphan-concept".into(),
            kind: BrainNodeKind::Concept,
            label: "Orphan Concept".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: Vec::new(),
            source_ids: Vec::new(),
            confidence: None,
            updated_at: snapshot.generated_at,
        });
        write_json_pretty(&nodes_path, &nodes).expect("write orphan node");

        let claims_path = workspace_root.join("graph/claims.json");
        let mut claims: Vec<ClaimRecord> = read_json_artifact(&claims_path).expect("read claims");
        let base_claim = claims.first().expect("base claim").clone();
        claims.push(ClaimRecord {
            claim_id: "claim-positive-agent-context".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            statement: "Agent brain context is source backed".into(),
            topic_refs: base_claim.topic_refs.clone(),
            source_refs: base_claim.source_refs.clone(),
            evidence_refs: base_claim.evidence_refs.clone(),
            status: "supported".into(),
            updated_at: snapshot.generated_at,
        });
        claims.push(ClaimRecord {
            claim_id: "claim-negative-agent-context".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            statement: "Agent brain context is not source backed".into(),
            topic_refs: base_claim.topic_refs,
            source_refs: base_claim.source_refs,
            evidence_refs: base_claim.evidence_refs,
            status: "supported".into(),
            updated_at: snapshot.generated_at,
        });
        write_json_pretty(&claims_path, &claims).expect("write conflicting claims");

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let health = handle_get_brain_health(GetBrainHealthRequest { scope })
            .expect("health runs maintenance");
        assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
        let report: BrainMaintenanceReport =
            read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
                .expect("read lint report");
        assert!(report.issues.iter().any(|issue| issue.kind == "orphan"));
        assert!(report.issues.iter().any(|issue| issue.kind == "conflict"));
        assert!(report.issues.iter().any(|issue| issue.kind == "stale"));
        assert!(health
            .review_items
            .iter()
            .any(|item| item.kind == BrainProposalKind::Link));
        assert!(health
            .review_items
            .iter()
            .any(|item| item.title.contains("Claims may conflict")));
        assert!(health
            .review_items
            .iter()
            .any(|item| item.title.contains("Source may need recompilation")));
    }

    #[test]
    fn resolving_brain_review_applies_save_back_artifacts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        write_source_manifest(&manifest).expect("write source manifest");
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        let source_manifest_before =
            fs::read_to_string(&manifest.manifest_path).expect("read source manifest before");
        let memory_before = fs::read_to_string(workspace_root.join("memory/records.json"))
            .unwrap_or_else(|_| "[]".into());

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let actor = BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        };
        let concept_node_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        let concept_evidence_ids = project
            .details_by_node_id
            .get(&concept_node_id)
            .expect("concept detail")
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect::<Vec<_>>();
        let proposed = handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Claim,
            title: "Accepted source-backed claim".into(),
            body: "Accepted claim proposals become durable claim records.".into(),
            actor,
            target_node_id: Some(concept_node_id.clone()),
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![concept_node_id.clone()],
            evidence_refs: concept_evidence_ids.clone(),
        })
        .expect("propose claim");
        assert_eq!(proposed.proposal.status, BrainProposalStatus::PendingReview);

        let human = BrainActor {
            actor_type: BrainActorType::User,
            actor_id: "local-user".into(),
        };
        let resolved_claim = handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
            scope: scope.clone(),
            proposal_id: proposed.proposal.proposal_id.clone(),
            decision: BrainReviewDecision::Accept,
            actor: human.clone(),
            reason: Some("Evidence is enough for the proposal ledger.".into()),
        })
        .expect("resolve claim review");
        assert_eq!(
            resolved_claim.proposal.status,
            BrainProposalStatus::Accepted
        );
        assert_eq!(
            resolved_claim.event.event_type,
            BrainEventKind::ReviewResolved
        );
        assert_eq!(resolved_claim.event.policy_result, "accept");

        let persisted: BrainUpdateProposal =
            read_json_artifact(&PathBuf::from(&resolved_claim.proposal_path))
                .expect("read proposal");
        assert_eq!(persisted.status, BrainProposalStatus::Accepted);
        let claims_after_accept: Vec<ClaimRecord> =
            read_json_artifact(&workspace_root.join("graph/claims.json"))
                .expect("read claims after accept");
        assert!(claims_after_accept.iter().any(|claim| {
            claim.statement == "Accepted claim proposals become durable claim records."
                && claim.topic_refs.contains(&concept_node_id)
                && claim.evidence_refs == concept_evidence_ids
        }));

        let wiki = handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::WikiPage,
            title: "Accepted answer page".into(),
            body: "Accepted answer text becomes a durable wiki page.".into(),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "claude-code".into(),
            },
            target_node_id: Some(concept_node_id.clone()),
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![concept_node_id.clone()],
            evidence_refs: concept_evidence_ids.clone(),
        })
        .expect("propose wiki page");
        let resolved_wiki = handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
            scope: scope.clone(),
            proposal_id: wiki.proposal.proposal_id.clone(),
            decision: BrainReviewDecision::Accept,
            actor: human,
            reason: Some("Save answer as durable wiki page.".into()),
        })
        .expect("resolve wiki review");
        assert_eq!(resolved_wiki.proposal.status, BrainProposalStatus::Accepted);
        let saved_page = workspace_root.join("wiki/save-back/accepted-answer-page.md");
        assert!(saved_page.exists());
        assert!(fs::read_to_string(&saved_page)
            .expect("read saved wiki page")
            .contains("Accepted answer text becomes a durable wiki page."));

        store
            .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
            .expect("rematerialize workspace brain repo");
        let rematerialized_claims: Vec<ClaimRecord> =
            read_json_artifact(&workspace_root.join("graph/claims.json"))
                .expect("read rematerialized claims");
        assert!(rematerialized_claims.iter().any(|claim| {
            claim.statement == "Accepted claim proposals become durable claim records."
        }));
        assert!(saved_page.exists());

        let health = handle_get_brain_health(GetBrainHealthRequest {
            scope: scope.clone(),
        })
        .expect("get health after resolve");
        assert_eq!(health.status, BrainHealthStatus::Clean);
        assert_eq!(health.attention_count, 0);

        let events = handle_read_recent_events(ReadRecentEventsRequest {
            scope,
            limit: Some(20),
        })
        .expect("read events");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::ReviewResolved));
        assert_eq!(
            fs::read_to_string(workspace_root.join("memory/records.json"))
                .unwrap_or_else(|_| "[]".into()),
            memory_before
        );
        assert_eq!(
            fs::read_to_string(&manifest.manifest_path).expect("read source manifest after"),
            source_manifest_before
        );
    }

    #[test]
    fn brain_writer_bootstraps_missing_memory_and_events_files() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        fs::remove_file(workspace_root.join("memory/records.json")).expect("remove memory file");
        fs::remove_file(workspace_root.join("events/brain_events.jsonl")).expect("remove events");

        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let response = handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Memory,
            title: "Remember bootstrap behavior".into(),
            body: "Missing memory and event files should be recreated safely.".into(),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "claude-code".into(),
            },
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: Vec::new(),
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
        })
        .expect("propose memory with missing files");
        assert_eq!(response.proposal.status, BrainProposalStatus::Accepted);

        let memories = read_memory_records(&workspace_root).expect("read bootstrapped memories");
        assert_eq!(memories.len(), 1);
        assert!(workspace_root.join("events/brain_events.jsonl").exists());
        let events = handle_read_recent_events(ReadRecentEventsRequest {
            scope,
            limit: Some(10),
        })
        .expect("read bootstrapped events");
        assert!(events
            .events
            .iter()
            .any(|event| event.event_type == BrainEventKind::MemoryAccepted));
        assert!(!workspace_root.join("brain.lock").exists());
    }

    #[test]
    fn brain_writer_deduplicates_concurrent_safe_proposals_by_fingerprint() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source-backed project");
        let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
        let scope = BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let proposal_request = ProposeBrainUpdateRequest {
            scope,
            kind: BrainProposalKind::Memory,
            title: "Remember duplicate write".into(),
            body: "The same agent memory should collapse to one memory record.".into(),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "claude-code".into(),
            },
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
        };

        let handles = (0..8)
            .map(|_| {
                let request = proposal_request.clone();
                std::thread::spawn(move || {
                    handle_propose_brain_update(request).expect("concurrent safe proposal")
                })
            })
            .collect::<Vec<_>>();
        let responses = handles
            .into_iter()
            .map(|handle| handle.join().expect("join concurrent proposal"))
            .collect::<Vec<_>>();
        assert!(responses
            .iter()
            .all(|response| response.proposal.status == BrainProposalStatus::Accepted));

        let memories = read_memory_records(&workspace_root).expect("read memory records");
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].title, "Remember duplicate write");
        let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
            .expect("events remain valid JSONL");
        assert!(events.len() >= 16);
        assert!(!workspace_root.join("brain.lock").exists());
    }

    #[test]
    fn compile_project_uses_source_manifest_as_graph_node_backing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Sample import\n\n## Page 1\n\nSource evidence belongs to the graph.\n";
        let markdown_path = temp.path().join("sample.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some("/tmp/source.pdf".into()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
            source_id: Some(manifest.source_id.clone()),
        };

        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        let source_node_id = source_node_id(&manifest.source_id);
        let source_detail = project
            .details_by_node_id
            .get(&source_node_id)
            .expect("source node detail");

        assert_eq!(source_detail.node.kind, GraphNodeKind::Source);
        assert_eq!(
            source_detail
                .source
                .as_ref()
                .map(|source| source.source_id.as_str()),
            Some("source-test")
        );
        assert!(source_detail
            .evidence
            .iter()
            .all(|evidence| evidence.source_id.as_deref() == Some("source-test")));
        assert!(project
            .edges
            .iter()
            .any(|edge| edge.source_node_id == source_node_id));
    }

    #[test]
    fn structured_extraction_artifact_tracks_claims_relations_and_provenance() {
        let temp = tempfile::tempdir().expect("temp dir");
        let markdown = "# Source import\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\nEvidence Map links page images to markdown snippets.\n\n## Page 2\n\nShared Context Layer turns imported documents into agent-ready knowledge.\n";
        let mut sections = extract_page_sections(markdown);
        let mut manifest = sample_manifest(&temp);
        manifest.pages = vec![
            PageArtifact {
                index: 0,
                label: "Page 1".into(),
                image_path: Some(
                    temp.path()
                        .join("default/artifacts/source-test/images/page-1.png")
                        .display()
                        .to_string(),
                ),
                markdown_path: Some(
                    temp.path()
                        .join("default/artifacts/source-test/pages/page-1.md")
                        .display()
                        .to_string(),
                ),
                plain_text_path: None,
                error_message: None,
            },
            PageArtifact {
                index: 1,
                label: "Page 2".into(),
                image_path: Some(
                    temp.path()
                        .join("default/artifacts/source-test/images/page-2.png")
                        .display()
                        .to_string(),
                ),
                markdown_path: Some(
                    temp.path()
                        .join("default/artifacts/source-test/pages/page-2.md")
                        .display()
                        .to_string(),
                ),
                plain_text_path: None,
                error_message: None,
            },
        ];
        attach_page_artifacts_to_sections(&mut sections, Some(&manifest));

        let artifact =
            build_extraction_artifact(&sections, &manifest.source_path, Some(&manifest.source_id));

        assert!(artifact.concepts.len() >= 2);
        assert!(artifact.claims.len() >= 2);
        assert!(!artifact.relations.is_empty());
        assert!(artifact
            .relations
            .iter()
            .all(|relation| !relation.evidence_ids.is_empty()));
        let evidence = artifact
            .evidence_refs
            .values()
            .find(|evidence| evidence.page_label == "Page 1")
            .expect("page 1 evidence");
        assert_eq!(evidence.page_index, 0);
        assert!(evidence
            .markdown_path
            .as_deref()
            .is_some_and(|path| path.ends_with("page-1.md")));
        assert!(evidence
            .image_path
            .as_deref()
            .is_some_and(|path| path.ends_with("page-1.png")));
        assert!(evidence.provenance.contains("Page 1"));

        let collected = collected_concepts_from_artifact(&artifact);
        assert!(collected
            .page_concepts
            .iter()
            .any(|page| page.page_label == "Page 1" && page.concept_ids.len() >= 2));
    }

    #[test]
    fn structured_extraction_uses_unique_evidence_ids_across_pages() {
        let markdown = "# Source import\n\n## Page 1\n\nAlpha Planning Notes stay local.\nShared Context Layer keeps agents grounded.\n\n## Page 2\n\nShared Context Layer keeps agents grounded.\nEvidence Map links page images to markdown snippets.\n";
        let sections = extract_page_sections(markdown);
        let artifact = build_extraction_artifact(&sections, "/tmp/source.pdf", Some("source-test"));
        let shared = artifact
            .concepts
            .iter()
            .find(|concept| {
                normalize_key(&concept.label) == "shared-context-layer-keeps-agents-grounded"
            })
            .expect("shared context concept");

        assert_eq!(shared.evidence_ids.len(), 2);
        assert_eq!(
            shared.evidence_ids.iter().collect::<BTreeSet<_>>().len(),
            shared.evidence_ids.len()
        );
        for evidence_id in &shared.evidence_ids {
            assert!(artifact.evidence_refs.contains_key(evidence_id));
        }
        assert_eq!(
            artifact
                .evidence_refs
                .values()
                .filter(|evidence| evidence
                    .provenance
                    .contains("Shared Context Layer keeps agents"))
                .count(),
            2
        );
    }

    #[test]
    fn load_project_defaults_to_workspace_graph_aggregate() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\nAlpha Planning Notes mention source specific work.\n",
            "source-a",
            "alpha",
            10,
        );
        let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nShared context layer keeps agents grounded.\nBeta Review Notes mention separate work.\n",
            "source-b",
            "beta",
            20,
        );
        let request_a = CompileProjectRequest {
            source_markdown_path: manifest_a.markdown_path.clone(),
            source_document_path: Some(manifest_a.source_path.clone()),
            source_manifest_path: Some(manifest_a.manifest_path.clone()),
            workspace_id: Some(manifest_a.workspace_id.clone()),
            source_id: Some(manifest_a.source_id.clone()),
        };
        let request_b = CompileProjectRequest {
            source_markdown_path: manifest_b.markdown_path.clone(),
            source_document_path: Some(manifest_b.source_path.clone()),
            source_manifest_path: Some(manifest_b.manifest_path.clone()),
            workspace_id: Some(manifest_b.workspace_id.clone()),
            source_id: Some(manifest_b.source_id.clone()),
        };
        store
            .save_project(&project_a, &request_a, Some(&manifest_a))
            .expect("save project a");
        store
            .save_project(&project_b, &request_b, Some(&manifest_b))
            .expect("save project b");

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load workspace aggregate")
            .expect("workspace aggregate");
        assert_eq!(
            aggregate.summary.project_id,
            workspace_project_id(DEFAULT_WORKSPACE_ID)
        );
        assert_eq!(aggregate.summary.document_count, 2);
        assert!(aggregate
            .nodes
            .iter()
            .any(|node| node.id == "source:source-a"));
        assert!(aggregate
            .nodes
            .iter()
            .any(|node| node.id == "source:source-b"));

        let shared = aggregate
            .details_by_node_id
            .values()
            .find(|detail| {
                normalize_key(&detail.canonical_name)
                    == "shared-context-layer-keeps-agents-grounded"
            })
            .expect("shared aggregate concept");
        assert!(shared
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-a")));
        assert!(shared
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-b")));
        assert!(aggregate.edges.iter().any(|edge| {
            edge.kind == RelationKind::SourceDocument
                && edge.source_node_id == "source:source-a"
                && edge.target_node_id == shared.node.id
        }));
        assert!(aggregate.edges.iter().any(|edge| {
            edge.kind == RelationKind::SourceDocument
                && edge.source_node_id == "source:source-b"
                && edge.target_node_id == shared.node.id
        }));

        let loaded_source_project = store
            .load_project(Some(&project_a.summary.project_id))
            .expect("load exact project")
            .expect("exact source project");
        assert_eq!(
            loaded_source_project.summary.project_id,
            project_a.summary.project_id
        );
    }

    #[test]
    fn workspace_aggregate_smoke_uses_real_multi_source_markdown_fixtures() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("multisource");
        let fixture_a_path = fixture_root.join("agent-context.md");
        let fixture_b_path = fixture_root.join("review-notes.md");
        let markdown_a = fs::read_to_string(&fixture_a_path).expect("read agent context fixture");
        let markdown_b = fs::read_to_string(&fixture_b_path).expect("read review notes fixture");
        let mut manifest_a =
            sample_manifest_with_source(&temp, "source-agent-context", "agent-context", 10);
        let mut manifest_b =
            sample_manifest_with_source(&temp, "source-review-notes", "review-notes", 20);
        manifest_a.markdown_path = fixture_a_path.display().to_string();
        manifest_b.markdown_path = fixture_b_path.display().to_string();
        manifest_a.pages = multi_source_fixture_pages(&temp, "source-agent-context");
        manifest_b.pages = multi_source_fixture_pages(&temp, "source-review-notes");

        for (markdown, fixture_path, manifest) in [
            (&markdown_a, &fixture_a_path, &manifest_a),
            (&markdown_b, &fixture_b_path, &manifest_b),
        ] {
            let request = CompileProjectRequest {
                source_markdown_path: fixture_path.display().to_string(),
                source_document_path: Some(manifest.source_path.clone()),
                source_manifest_path: Some(manifest.manifest_path.clone()),
                workspace_id: Some(manifest.workspace_id.clone()),
                source_id: Some(manifest.source_id.clone()),
            };
            let project = compile_knowledge_project(&request, markdown, Some(manifest));
            store
                .save_project(&project, &request, Some(manifest))
                .expect("save source-backed fixture project");
        }

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");

        assert_eq!(aggregate.summary.document_count, 2);
        assert!(aggregate
            .nodes
            .iter()
            .any(|node| node.id == "source:source-agent-context"));
        assert!(aggregate
            .nodes
            .iter()
            .any(|node| node.id == "source:source-review-notes"));

        let shared = aggregate
            .details_by_node_id
            .values()
            .find(|detail| {
                normalize_key(&detail.canonical_name) == "shared-team-context-layer-keeps-agents"
            })
            .expect("shared team context layer concept");
        for source_id in ["source-agent-context", "source-review-notes"] {
            assert!(shared
                .evidence
                .iter()
                .any(|evidence| evidence.source_id.as_deref() == Some(source_id)));
        }
        assert!(shared.evidence.iter().any(|evidence| {
            evidence
                .markdown_path
                .as_deref()
                .is_some_and(|path| path.ends_with("page_1.md"))
                && evidence
                    .image_path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("page_1.png"))
        }));
        assert!(aggregate.edges.iter().any(|edge| {
            edge.kind == RelationKind::RelatedTo
                && (edge.source_node_id == shared.node.id || edge.target_node_id == shared.node.id)
                && edge.evidence_count > 0
        }));
    }

    #[test]
    fn source_backed_project_id_uses_manifest_identity() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let shared_original = temp
            .path()
            .join("shared-original.pdf")
            .display()
            .to_string();
        let markdown_a =
            "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n";
        let markdown_b =
            "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n";
        let markdown_path_a = temp.path().join("source-a.md");
        let markdown_path_b = temp.path().join("source-b.md");
        fs::write(&markdown_path_a, markdown_a).expect("write source a markdown");
        fs::write(&markdown_path_b, markdown_b).expect("write source b markdown");

        let mut manifest_a = sample_manifest_with_source(&temp, "source-a", "alpha", 10);
        let mut manifest_b = sample_manifest_with_source(&temp, "source-b", "beta", 11);
        manifest_a.original_path = shared_original.clone();
        manifest_b.original_path = shared_original.clone();
        let request_a = CompileProjectRequest {
            source_markdown_path: markdown_path_a.display().to_string(),
            source_document_path: Some(shared_original.clone()),
            source_manifest_path: Some(manifest_a.manifest_path.clone()),
            workspace_id: Some(manifest_a.workspace_id.clone()),
            source_id: Some(manifest_a.source_id.clone()),
        };
        let request_b = CompileProjectRequest {
            source_markdown_path: markdown_path_b.display().to_string(),
            source_document_path: Some(shared_original),
            source_manifest_path: Some(manifest_b.manifest_path.clone()),
            workspace_id: Some(manifest_b.workspace_id.clone()),
            source_id: Some(manifest_b.source_id.clone()),
        };

        let project_a = compile_knowledge_project(&request_a, markdown_a, Some(&manifest_a));
        let project_b = compile_knowledge_project(&request_b, markdown_b, Some(&manifest_b));
        assert_ne!(project_a.summary.project_id, project_b.summary.project_id);
        assert_eq!(
            project_a.summary.project_id,
            build_source_backed_project_id(DEFAULT_WORKSPACE_ID, "source-a")
        );
        assert_eq!(
            project_b.summary.project_id,
            build_source_backed_project_id(DEFAULT_WORKSPACE_ID, "source-b")
        );

        store
            .save_project(&project_a, &request_a, Some(&manifest_a))
            .expect("save source a project");
        store
            .save_project(&project_b, &request_b, Some(&manifest_b))
            .expect("save source b project");
        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        assert!(aggregate.details_by_node_id.values().any(|detail| detail
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-a"))));
        assert!(aggregate.details_by_node_id.values().any(|detail| detail
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-b"))));
    }

    #[test]
    fn source_rows_round_trip_paths_with_pipe_characters() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let markdown = "# Source A\n\n## Page 1\n\nPipe path evidence stays readable.\n";
        let markdown_path = temp.path().join("pipe-source.md");
        fs::write(&markdown_path, markdown).expect("write markdown");
        let mut manifest = sample_manifest_with_source(&temp, "source-pipe", "alpha", 10);
        manifest.original_path = temp.path().join("a|b.pdf").display().to_string();
        manifest.source_path = temp
            .path()
            .join("default/sources/source-pipe/a|b.pdf")
            .display()
            .to_string();
        manifest.markdown_path = temp
            .path()
            .join("default/artifacts/source-pipe/a|b.md")
            .display()
            .to_string();
        manifest.manifest_path = temp
            .path()
            .join("default/artifacts/source-pipe/source|manifest.json")
            .display()
            .to_string();
        let request = CompileProjectRequest {
            source_markdown_path: markdown_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(&manifest));
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save pipe path source");

        let sources = store
            .load_sources(DEFAULT_WORKSPACE_ID)
            .expect("load sources");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].original_path, manifest.original_path);
        assert_eq!(sources[0].source_path, manifest.source_path);
        assert_eq!(sources[0].markdown_path, manifest.markdown_path);

        let rows = store
            .load_source_rows(DEFAULT_WORKSPACE_ID)
            .expect("load source rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manifest_path, manifest.manifest_path);
        assert_eq!(rows[0].project_id, project.summary.project_id);
    }

    #[test]
    fn workspace_aggregate_merges_concepts_by_alias_identity() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nFoo keeps project knowledge grounded.\n",
            "source-a",
            "alpha",
            20,
        );
        let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nBar keeps project knowledge grounded.\n",
            "source-b",
            "beta",
            10,
        );
        let source_a_concept_id = project_a
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("source a concept")
            .id
            .clone();
        let source_a_node = project_a
            .nodes
            .iter_mut()
            .find(|node| node.id == source_a_concept_id)
            .expect("source a concept node");
        source_a_node.label = "Foo".into();
        let source_a_detail = project_a
            .details_by_node_id
            .get_mut(&source_a_concept_id)
            .expect("source a concept detail");
        source_a_detail.canonical_name = "Foo".into();
        source_a_detail.aliases = vec!["Bar".into()];
        source_a_detail.node.label = "Foo".into();
        let source_b_concept_id = project_b
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("source b concept")
            .id
            .clone();
        let source_b_node = project_b
            .nodes
            .iter_mut()
            .find(|node| node.id == source_b_concept_id)
            .expect("source b concept node");
        source_b_node.label = "Bar".into();
        let source_b_detail = project_b
            .details_by_node_id
            .get_mut(&source_b_concept_id)
            .expect("source b concept detail");
        source_b_detail.canonical_name = "Bar".into();
        source_b_detail.node.label = "Bar".into();
        let request_a = CompileProjectRequest {
            source_markdown_path: manifest_a.markdown_path.clone(),
            source_document_path: Some(manifest_a.source_path.clone()),
            source_manifest_path: Some(manifest_a.manifest_path.clone()),
            workspace_id: Some(manifest_a.workspace_id.clone()),
            source_id: Some(manifest_a.source_id.clone()),
        };
        let request_b = CompileProjectRequest {
            source_markdown_path: manifest_b.markdown_path.clone(),
            source_document_path: Some(manifest_b.source_path.clone()),
            source_manifest_path: Some(manifest_b.manifest_path.clone()),
            workspace_id: Some(manifest_b.workspace_id.clone()),
            source_id: Some(manifest_b.source_id.clone()),
        };
        store
            .save_project(&project_a, &request_a, Some(&manifest_a))
            .expect("save source a project");
        store
            .save_project(&project_b, &request_b, Some(&manifest_b))
            .expect("save source b project");

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        let merged = aggregate
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Foo")
            .expect("merged foo concept");
        assert!(merged.aliases.contains(&"Bar".into()));
        assert!(merged
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-a")));
        assert!(merged
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-b")));
        assert_eq!(
            aggregate
                .nodes
                .iter()
                .filter(|node| node.kind == GraphNodeKind::Concept)
                .count(),
            1
        );
    }

    #[test]
    fn workspace_aggregate_merges_transitive_alias_groups() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha concept keeps project knowledge grounded.\n",
            "source-a",
            "alpha",
            30,
        );
        let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nGamma concept keeps project knowledge grounded.\n",
            "source-b",
            "beta",
            20,
        );
        let (mut project_c, manifest_c) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source C\n\n## Page 1\n\nBeta concept bridges gamma evidence.\n",
            "source-c",
            "gamma",
            10,
        );

        rename_first_concept_for_test(&mut project_a, "Alpha", &["Beta"]);
        rename_first_concept_for_test(&mut project_b, "Gamma", &["Delta"]);
        rename_first_concept_for_test(&mut project_c, "Beta", &["Gamma"]);

        for (project, manifest) in [
            (&project_a, &manifest_a),
            (&project_b, &manifest_b),
            (&project_c, &manifest_c),
        ] {
            let request = CompileProjectRequest {
                source_markdown_path: manifest.markdown_path.clone(),
                source_document_path: Some(manifest.source_path.clone()),
                source_manifest_path: Some(manifest.manifest_path.clone()),
                workspace_id: Some(manifest.workspace_id.clone()),
                source_id: Some(manifest.source_id.clone()),
            };
            store
                .save_project(project, &request, Some(manifest))
                .expect("save source project");
        }

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        let concept_details = aggregate
            .details_by_node_id
            .values()
            .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
            .collect::<Vec<_>>();
        assert_eq!(concept_details.len(), 1);
        let merged = concept_details[0];
        assert!(merged.aliases.contains(&"Beta".into()));
        assert!(merged.aliases.contains(&"Gamma".into()));
        for source_id in ["source-a", "source-b", "source-c"] {
            assert!(merged
                .evidence
                .iter()
                .any(|evidence| evidence.source_id.as_deref() == Some(source_id)));
        }
    }

    #[test]
    fn handle_load_project_defaults_to_workspace_graph_aggregate() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let (project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\n",
            "source-a",
            "alpha",
            10,
        );
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save project");

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        let response = handle_load_project(LoadProjectRequest::default())
            .expect("load project through handler");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        let project = response.project.expect("workspace aggregate project");
        assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
        assert_eq!(
            project.summary.project_id,
            workspace_project_id(DEFAULT_WORKSPACE_ID)
        );
        assert!(project
            .nodes
            .iter()
            .any(|node| node.id == "source:source-a"));
    }

    #[test]
    fn default_load_project_falls_back_to_latest_legacy_project() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let project = compile_fixture_project(
            &temp,
            "# Legacy import\n\n## Page 1\n\nLegacy project snapshots remain visible.\n",
        );
        let request = CompileProjectRequest {
            source_markdown_path: temp.path().join("legacy.md").display().to_string(),
            source_document_path: None,
            source_manifest_path: None,
            workspace_id: None,
            source_id: None,
        };
        store
            .save_project(&project, &request, None)
            .expect("save legacy project");

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        let response =
            handle_load_project(LoadProjectRequest::default()).expect("load default project");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        assert_eq!(response.sources.len(), 0);
        assert_eq!(
            response.project.expect("legacy project").summary.project_id,
            project.summary.project_id
        );
    }

    #[test]
    fn workspace_rename_correction_replays_to_source_snapshots_and_ledger() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nShared context layer keeps agents grounded.\n",
            "source-a",
            "alpha",
            10,
        );
        let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nShared context layer keeps document evidence grounded.\n",
            "source-b",
            "beta",
            20,
        );
        rename_first_concept_for_test(&mut project_a, "Shared Context Layer", &["Agent Context"]);
        rename_first_concept_for_test(&mut project_b, "Shared Context Layer", &[]);

        for (project, manifest) in [(&project_a, &manifest_a), (&project_b, &manifest_b)] {
            let request = CompileProjectRequest {
                source_markdown_path: manifest.markdown_path.clone(),
                source_document_path: Some(manifest.source_path.clone()),
                source_manifest_path: Some(manifest.manifest_path.clone()),
                workspace_id: Some(manifest.workspace_id.clone()),
                source_id: Some(manifest.source_id.clone()),
            };
            store
                .save_project(project, &request, Some(manifest))
                .expect("save source project");
        }

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        let aggregate_detail = aggregate
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Shared Context Layer")
            .expect("workspace concept")
            .clone();

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        let response = handle_apply_correction(ApplyCorrectionRequest {
            project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
            node_id: aggregate_detail.node.id.clone(),
            kind: CorrectionKind::Rename,
            target_node_id: None,
            value: Some("Agent Brain Context".into()),
        })
        .expect("apply workspace rename correction");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        assert!(response
            .project
            .details_by_node_id
            .values()
            .any(|detail| detail.canonical_name == "Agent Brain Context"));
        for project_id in [&project_a.summary.project_id, &project_b.summary.project_id] {
            let source_project = store
                .load_project(Some(project_id))
                .expect("load source project")
                .expect("source project");
            let renamed = source_project
                .details_by_node_id
                .values()
                .find(|detail| detail.canonical_name == "Agent Brain Context")
                .expect("renamed source concept");
            assert!(renamed.aliases.contains(&"Shared Context Layer".into()));
        }

        let corrections = store
            .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
            .expect("load workspace corrections");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].aggregate_node_id, aggregate_detail.node.id);
        assert_eq!(corrections[0].kind, CorrectionKind::Rename);
        assert_eq!(corrections[0].source_node_ids.len(), 2);
        assert!(!corrections[0].evidence_ids.is_empty());

        let events_path = temp
            .path()
            .join(DEFAULT_WORKSPACE_ID)
            .join("events/brain_events.jsonl");
        let events = fs::read_to_string(events_path).expect("brain events");
        assert!(events.contains("\"eventType\":\"correction_applied\""));
        assert!(events.contains("\"policyResult\":\"applied\""));
    }

    #[test]
    fn workspace_merge_correction_replays_to_source_snapshot_and_ledger() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let (mut project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha planning context keeps agents grounded.\n\n## Page 2\n\nBeta review context keeps evidence visible.\n",
            "source-a",
            "alpha",
            10,
        );
        let concept_ids = project
            .nodes
            .iter()
            .filter(|node| node.kind == GraphNodeKind::Concept)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        assert!(concept_ids.len() >= 2);
        rename_concept_for_test(&mut project, &concept_ids[0], "Alpha Context", &[]);
        rename_concept_for_test(&mut project, &concept_ids[1], "Beta Context", &[]);
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source project");

        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        let source_detail = aggregate
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Alpha Context")
            .expect("workspace source concept")
            .clone();
        let target_detail = aggregate
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Beta Context")
            .expect("workspace target concept")
            .clone();
        let node_count_before = project.nodes.len();

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        handle_apply_correction(ApplyCorrectionRequest {
            project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
            node_id: source_detail.node.id.clone(),
            kind: CorrectionKind::Merge,
            target_node_id: Some(target_detail.node.id.clone()),
            value: None,
        })
        .expect("apply workspace merge correction");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        let source_project = store
            .load_project(Some(&project.summary.project_id))
            .expect("load source project")
            .expect("source project");
        assert_eq!(source_project.nodes.len(), node_count_before - 1);
        assert!(!source_project
            .details_by_node_id
            .values()
            .any(|detail| detail.canonical_name == "Alpha Context"));
        assert!(source_project
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Beta Context")
            .expect("merged target concept")
            .aliases
            .contains(&"Alpha Context".into()));

        let corrections = store
            .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
            .expect("load workspace corrections");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].kind, CorrectionKind::Merge);
        assert_eq!(corrections[0].target_node_id, Some(target_detail.node.id));
        assert_eq!(corrections[0].source_node_ids.len(), 1);
    }

    #[test]
    fn workspace_keep_separate_correction_replays_to_source_snapshot_and_ledger() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let (mut project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha context keeps agents grounded.\n",
            "source-a",
            "alpha",
            10,
        );
        rename_first_concept_for_test(&mut project, "Alpha Context", &["Beta Context"]);
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save source project");
        let aggregate = store
            .load_workspace_project(DEFAULT_WORKSPACE_ID)
            .expect("load aggregate")
            .expect("workspace aggregate");
        let aggregate_detail = aggregate
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Alpha Context")
            .expect("workspace concept")
            .clone();
        assert!(aggregate_detail.aliases.contains(&"Beta Context".into()));
        let node_count_before = project.nodes.len();

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        handle_apply_correction(ApplyCorrectionRequest {
            project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
            node_id: aggregate_detail.node.id.clone(),
            kind: CorrectionKind::KeepSeparate,
            target_node_id: None,
            value: None,
        })
        .expect("apply workspace keep separate correction");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        let source_project = store
            .load_project(Some(&project.summary.project_id))
            .expect("load source project")
            .expect("source project");
        assert_eq!(source_project.nodes.len(), node_count_before + 1);
        assert!(source_project
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Alpha Context")
            .expect("kept concept")
            .aliases
            .is_empty());
        assert!(source_project
            .details_by_node_id
            .values()
            .any(|detail| detail.canonical_name == "Beta Context"));

        let corrections = store
            .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
            .expect("load workspace corrections");
        assert_eq!(corrections.len(), 1);
        assert_eq!(corrections[0].kind, CorrectionKind::KeepSeparate);
        assert_eq!(corrections[0].source_node_ids.len(), 1);
    }

    #[test]
    fn exact_project_load_uses_project_workspace_sources() {
        static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("temp dir");
        let store_path = temp.path().join("knowledge.sqlite3");
        let store = KnowledgeProjectStore::new(store_path.clone());
        let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nWorkspace A evidence stays separate.\n",
            "source-a",
            "alpha",
            10,
        );
        let (project_b, mut manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nWorkspace B evidence is newer.\n",
            "source-b",
            "beta",
            99,
        );
        manifest_b.workspace_id = "workspace-b".into();
        let request_a = CompileProjectRequest {
            source_markdown_path: manifest_a.markdown_path.clone(),
            source_document_path: Some(manifest_a.source_path.clone()),
            source_manifest_path: Some(manifest_a.manifest_path.clone()),
            workspace_id: Some(manifest_a.workspace_id.clone()),
            source_id: Some(manifest_a.source_id.clone()),
        };
        let request_b = CompileProjectRequest {
            source_markdown_path: manifest_b.markdown_path.clone(),
            source_document_path: Some(manifest_b.source_path.clone()),
            source_manifest_path: Some(manifest_b.manifest_path.clone()),
            workspace_id: Some(manifest_b.workspace_id.clone()),
            source_id: Some(manifest_b.source_id.clone()),
        };
        store
            .save_project(&project_a, &request_a, Some(&manifest_a))
            .expect("save workspace a project");
        store
            .save_project(&project_b, &request_b, Some(&manifest_b))
            .expect("save workspace b project");

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        let response = handle_load_project(LoadProjectRequest {
            project_id: Some(project_a.summary.project_id.clone()),
            workspace_id: None,
        })
        .expect("load exact project");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }

        assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
        assert_eq!(response.sources.len(), 1);
        assert_eq!(response.sources[0].source_id, "source-a");
        assert_eq!(
            response.project.expect("exact project").summary.project_id,
            project_a.summary.project_id
        );

        let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
        std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
        let error = handle_load_project(LoadProjectRequest {
            project_id: Some(project_a.summary.project_id.clone()),
            workspace_id: Some("workspace-b".into()),
        })
        .expect_err("stale workspace should not hydrate exact project");
        match previous_store {
            Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
            None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
        }
        assert!(error
            .to_string()
            .contains("belongs to workspace default, not workspace-b"));
    }

    #[test]
    fn answer_project_supports_workspace_project_id() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let (project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\n",
            "source-a",
            "alpha",
            10,
        );
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(&project, &request, Some(&manifest))
            .expect("save project");
        let aggregate =
            load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
                .expect("load workspace answerable project");
        let answer = answer_project(
            &aggregate,
            &AnswerProjectRequest {
                project_id: aggregate.summary.project_id.clone(),
                node_id: None,
                question: "What does the shared context layer say?".into(),
            },
        )
        .expect("answer workspace project");

        assert_ne!(answer.status, AnswerStatus::Blocked);
        assert!(answer
            .citations
            .iter()
            .any(|citation| citation.source_id.as_deref() == Some("source-a")));
    }

    #[test]
    fn workspace_answer_without_node_uses_matching_source_evidence() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
        let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n",
            "source-a",
            "alpha",
            10,
        );
        let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n",
            "source-b",
            "beta",
            11,
        );
        let request_a = CompileProjectRequest {
            source_markdown_path: manifest_a.markdown_path.clone(),
            source_document_path: Some(manifest_a.source_path.clone()),
            source_manifest_path: Some(manifest_a.manifest_path.clone()),
            workspace_id: Some(manifest_a.workspace_id.clone()),
            source_id: Some(manifest_a.source_id.clone()),
        };
        let request_b = CompileProjectRequest {
            source_markdown_path: manifest_b.markdown_path.clone(),
            source_document_path: Some(manifest_b.source_path.clone()),
            source_manifest_path: Some(manifest_b.manifest_path.clone()),
            workspace_id: Some(manifest_b.workspace_id.clone()),
            source_id: Some(manifest_b.source_id.clone()),
        };
        store
            .save_project(&project_a, &request_a, Some(&manifest_a))
            .expect("save source a project");
        store
            .save_project(&project_b, &request_b, Some(&manifest_b))
            .expect("save source b project");
        let aggregate =
            load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
                .expect("load workspace answerable project");
        let answer = answer_project(
            &aggregate,
            &AnswerProjectRequest {
                project_id: aggregate.summary.project_id.clone(),
                node_id: None,
                question: "What does the beta architecture context say?".into(),
            },
        )
        .expect("answer workspace project");

        assert_ne!(answer.status, AnswerStatus::Blocked);
        assert!(answer
            .citations
            .iter()
            .any(|citation| citation.source_id.as_deref() == Some("source-b")));
        assert!(!answer
            .citations
            .iter()
            .any(|citation| citation.source_id.as_deref() == Some("source-a")));
    }

    #[test]
    fn compile_project_rejects_request_ids_that_conflict_with_manifest() {
        let temp = tempfile::tempdir().expect("temp dir");
        let manifest = sample_manifest(&temp);
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some("/tmp/source.pdf".into()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some("different-workspace".into()),
            source_id: Some(manifest.source_id.clone()),
        };

        let error = resolved_source_ids(&request, Some(&manifest)).expect_err("id mismatch");
        assert!(error
            .to_string()
            .contains("does not match source manifest workspace_id"));
    }

    #[test]
    fn output_packaging_falls_back_to_next_root_when_primary_root_is_unwritable() {
        let temp = tempfile::tempdir().expect("temp dir");
        let blocked_root = temp.path().join("blocked-root");
        fs::write(&blocked_root, "not a directory").expect("blocked root file");
        let fallback_root = temp.path().join("fallback-root");
        let request = sample_parse_request(&temp);

        let manifest = write_output_package_with_fallback(
            &[blocked_root.clone(), fallback_root.clone()],
            "sample-import",
            "123",
            &request,
            &sample_parse_result(),
        )
        .expect("fallback output manifest");

        assert!(Path::new(&manifest.markdown_path).starts_with(&fallback_root));
        assert!(Path::new(&manifest.markdown_path).exists());
        assert!(Path::new(&manifest.source_path).exists());
        assert!(Path::new(&manifest.manifest_path).exists());
        assert!(manifest.artifact_root.contains("/default/artifacts/"));
        assert!(manifest.source_path.contains("/default/sources/"));
        assert_eq!(manifest.pages[0].label, "Page 1");
        assert!(manifest.pages[0]
            .markdown_path
            .as_deref()
            .is_some_and(|path| Path::new(path).exists()));
    }

    #[test]
    fn output_packaging_uses_requested_workspace_and_source_ids() {
        let temp = tempfile::tempdir().expect("temp dir");
        let fallback_root = temp.path().join("output-root");
        let mut request = sample_parse_request(&temp);
        request.output = Some(duckdocs_engine_types::ParseOutputTarget {
            root_dir: Some(fallback_root.display().to_string()),
            name: Some("sample-import".into()),
            workspace_id: Some("workspace-alpha".into()),
            source_id: Some("source-alpha".into()),
        });

        let manifest = write_output_package_with_fallback(
            &[fallback_root.clone()],
            "sample-import",
            "123",
            &request,
            &sample_parse_result(),
        )
        .expect("output manifest");

        assert_eq!(manifest.workspace_id, "workspace-alpha");
        assert_eq!(manifest.source_id, "source-alpha");
        assert!(manifest
            .artifact_root
            .contains("/workspace-alpha/artifacts/source-alpha"));
        assert!(manifest
            .source_path
            .contains("/workspace-alpha/sources/source-alpha"));
    }

    #[test]
    fn rename_correction_updates_canonical_name_and_aliases() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n",
        );
        let concept_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        let previous_name = project
            .details_by_node_id
            .get(&concept_id)
            .expect("concept detail")
            .canonical_name
            .clone();
        let project_id = project.summary.project_id.clone();

        apply_correction(
            &mut project,
            &ApplyCorrectionRequest {
                project_id,
                node_id: concept_id.clone(),
                kind: CorrectionKind::Rename,
                target_node_id: None,
                value: Some("Graph Evidence View".into()),
            },
        )
        .expect("apply rename correction");

        let detail = project
            .details_by_node_id
            .get(&concept_id)
            .expect("renamed detail");
        assert_eq!(detail.canonical_name, "Graph Evidence View");
        assert!(detail.aliases.contains(&previous_name));
        assert_eq!(
            project
                .nodes
                .iter()
                .find(|node| node.id == concept_id)
                .expect("renamed node")
                .label,
            "Graph Evidence View"
        );
    }

    #[test]
    fn merge_correction_combines_concepts_and_redirects_edges() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n\n## Page 2\n\nEvidence inspector helps people trust the graph.\n",
        );
        let concept_ids = project
            .nodes
            .iter()
            .filter(|node| node.kind == GraphNodeKind::Concept)
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        let source_id = concept_ids[0].clone();
        let target_id = concept_ids[1].clone();
        let source_name = project
            .details_by_node_id
            .get(&source_id)
            .expect("source detail")
            .canonical_name
            .clone();
        let node_count_before = project.nodes.len();
        let project_id = project.summary.project_id.clone();

        apply_correction(
            &mut project,
            &ApplyCorrectionRequest {
                project_id,
                node_id: source_id.clone(),
                kind: CorrectionKind::Merge,
                target_node_id: Some(target_id.clone()),
                value: None,
            },
        )
        .expect("apply merge correction");

        assert_eq!(project.nodes.len(), node_count_before - 1);
        assert!(!project.nodes.iter().any(|node| node.id == source_id));
        assert!(project
            .edges
            .iter()
            .all(|edge| { edge.source_node_id != source_id && edge.target_node_id != source_id }));
        assert!(project
            .details_by_node_id
            .get(&target_id)
            .expect("target detail")
            .aliases
            .contains(&source_name));
    }

    #[test]
    fn keep_separate_correction_splits_aliases_into_new_nodes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded Graph View keeps answers cautious.\n\n## Page 2\n\nGrounded graph view keeps answers cautious.\n",
        );
        let concept_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        assert!(!project
            .details_by_node_id
            .get(&concept_id)
            .expect("detail")
            .aliases
            .is_empty());
        let node_count_before = project.nodes.len();
        let project_id = project.summary.project_id.clone();

        apply_correction(
            &mut project,
            &ApplyCorrectionRequest {
                project_id,
                node_id: concept_id.clone(),
                kind: CorrectionKind::KeepSeparate,
                target_node_id: None,
                value: None,
            },
        )
        .expect("apply keep separate correction");

        assert!(project.nodes.len() > node_count_before);
        assert!(project
            .details_by_node_id
            .get(&concept_id)
            .expect("detail")
            .aliases
            .is_empty());
        assert!(project
            .edges
            .iter()
            .any(|edge| edge.label == "Separated by correction"));
    }

    #[test]
    fn corrections_preserve_manifest_source_document_edges() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (mut project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded Graph View keeps answers cautious.\n\n## Page 2\n\nGrounded graph view keeps answers cautious.\n",
        );
        let source_node_id = source_node_id(&manifest.source_id);
        let concept_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();
        let project_id = project.summary.project_id.clone();

        apply_correction(
            &mut project,
            &ApplyCorrectionRequest {
                project_id,
                node_id: concept_id.clone(),
                kind: CorrectionKind::KeepSeparate,
                target_node_id: None,
                value: None,
            },
        )
        .expect("apply keep separate correction");

        assert!(project.edges.iter().any(|edge| {
            edge.kind == RelationKind::SourceDocument
                && edge.source_node_id == source_node_id
                && edge.target_node_id == concept_id
        }));
        assert!(project.edges.iter().any(|edge| {
            edge.kind == RelationKind::SourceDocument
                && edge.source_node_id == source_node_id
                && edge.target_node_id != concept_id
        }));
    }

    #[test]
    fn answer_project_blocks_empty_question() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\n",
        );

        let answer = answer_project(
            &project,
            &AnswerProjectRequest {
                project_id: project.summary.project_id.clone(),
                node_id: None,
                question: "   ".into(),
            },
        )
        .expect("answer project");

        assert_eq!(answer.status, AnswerStatus::Blocked);
        assert!(answer.citations.is_empty());
    }

    #[test]
    fn answer_project_returns_grounded_citations_for_matching_question() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n",
        );
        let concept_id = project
            .nodes
            .iter()
            .find(|node| node.kind == GraphNodeKind::Concept)
            .expect("concept node")
            .id
            .clone();

        let answer = answer_project(
            &project,
            &AnswerProjectRequest {
                project_id: project.summary.project_id.clone(),
                node_id: Some(concept_id),
                question: "What evidence keeps graph answers grounded?".into(),
            },
        )
        .expect("answer project");

        assert_eq!(answer.status, AnswerStatus::Grounded);
        assert!(!answer.citations.is_empty());
        assert!(answer
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("grounded"));
    }

    #[test]
    fn ollama_models_endpoint_normalizes_common_base_urls() {
        let mut config = EngineConfig {
            provider: ProviderKind::Ollama,
            model_id: "qwen3-vl:8b".into(),
            api_key: String::new(),
            base_url: Some("http://127.0.0.1:11434".into()),
            prompt_template: "General".into(),
        };

        assert_eq!(
            ollama_models_endpoint(&config),
            "http://127.0.0.1:11434/v1/models"
        );

        config.base_url = Some("http://127.0.0.1:11434/api/generate".into());
        assert_eq!(
            ollama_models_endpoint(&config),
            "http://127.0.0.1:11434/api/tags"
        );

        config.base_url = Some("http://127.0.0.1:11434/v1/chat/completions".into());
        assert_eq!(
            ollama_models_endpoint(&config),
            "http://127.0.0.1:11434/v1/models"
        );
    }

    #[test]
    fn resolve_binary_prefers_path_before_common_locations() {
        let temp = tempfile::tempdir().expect("temp dir");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("bin dir");
        let binary_path = bin_dir.join("duckdocs-test-bin");
        fs::write(&binary_path, "").expect("test bin");

        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &bin_dir);
        let resolved = resolve_binary("duckdocs-test-bin", &["/definitely/missing"]);
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        assert_eq!(resolved, binary_path);
    }
}
