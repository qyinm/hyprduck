use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use hyprduck_engine_client::{EngineClient, SubprocessEngineClient};
use hyprduck_engine_types::*;
use serde_json::{json, Map, Value};

use super::args::optional_string;
use super::policy::redact_local_path_text;
use super::protocol::McpServerState;

#[derive(Clone, Default)]
pub(super) struct ImportJobRegistry {
    jobs: Arc<Mutex<BTreeMap<String, ImportJobSnapshot>>>,
}

#[derive(Debug, Clone)]
pub(super) struct ImportJobSnapshot {
    pub(super) job_id: String,
    pub(super) workspace_id: String,
    pub(super) root_dir: Option<String>,
    pub(super) status: ImportLifecycleStatus,
    pub(super) phase: ImportLifecyclePhase,
    pub(super) progress_percent: u8,
    pub(super) source_id: Option<String>,
    pub(super) source_markdown_path: Option<String>,
    pub(super) source_document_path: Option<String>,
    pub(super) source_manifest_path: Option<String>,
    pub(super) page_count: Option<usize>,
    pub(super) evidence_count: Option<usize>,
    pub(super) citation_ready: bool,
    pub(super) graph_ready: bool,
    pub(super) graph_status: Option<String>,
    pub(super) graph_error_category: Option<String>,
    pub(super) graph_generation_skipped_reason: Option<String>,
    pub(super) graph_generation_error_message: Option<String>,
    pub(super) retryable: bool,
    pub(super) retry_attempt: u8,
    pub(super) max_retry_attempts: u8,
    pub(super) next_retry_at: Option<u64>,
    pub(super) manual_retry_available: bool,
    pub(super) warnings: Vec<String>,
    pub(super) error: Option<String>,
    pub(super) cancel_requested: bool,
    pub(super) created_at: u64,
    pub(super) updated_at: u64,
}

impl ImportJobRegistry {
    pub(super) fn insert(&self, job: ImportJobSnapshot) {
        self.jobs
            .lock()
            .expect("import job registry lock poisoned")
            .insert(job.job_id.clone(), job);
    }

    pub(super) fn get(&self, job_id: &str) -> Option<ImportJobSnapshot> {
        self.jobs
            .lock()
            .expect("import job registry lock poisoned")
            .get(job_id)
            .cloned()
    }

    pub(super) fn update<F>(&self, job_id: &str, apply: F)
    where
        F: FnOnce(&mut ImportJobSnapshot),
    {
        if let Some(job) = self
            .jobs
            .lock()
            .expect("import job registry lock poisoned")
            .get_mut(job_id)
        {
            apply(job);
            job.updated_at = unix_timestamp_seconds();
        }
    }

    pub(super) fn update_active<F>(&self, job_id: &str, apply: F)
    where
        F: FnOnce(&mut ImportJobSnapshot),
    {
        self.update(job_id, |job| {
            if job.status.is_terminal() {
                return;
            }
            if job.cancel_requested {
                job.status = ImportLifecycleStatus::Cancelled;
                job.phase = ImportLifecyclePhase::Cancelled;
                job.progress_percent = 100;
                return;
            }
            apply(job);
        });
    }

    pub(super) fn cancel(&self, job_id: &str) -> Result<ImportJobSnapshot> {
        let mut jobs = self.jobs.lock().expect("import job registry lock poisoned");
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| anyhow!("import job not found: {job_id}"))?;
        if !job.status.is_terminal() {
            job.cancel_requested = true;
            job.warnings.push(
                "cancel_requested; running engine steps may finish before cancellation".into(),
            );
            if matches!(job.phase, ImportLifecyclePhase::Imported) {
                job.status = ImportLifecycleStatus::Cancelled;
                job.phase = ImportLifecyclePhase::Cancelled;
                job.progress_percent = 100;
            }
            job.updated_at = unix_timestamp_seconds();
        }
        Ok(job.clone())
    }

    fn mark_cancelled_if_requested(&self, job_id: &str) -> bool {
        let mut jobs = self.jobs.lock().expect("import job registry lock poisoned");
        let Some(job) = jobs.get_mut(job_id) else {
            return false;
        };
        if !job.cancel_requested {
            return false;
        }
        if !job.status.is_terminal() {
            job.status = ImportLifecycleStatus::Cancelled;
            job.phase = ImportLifecyclePhase::Cancelled;
            job.progress_percent = 100;
            job.updated_at = unix_timestamp_seconds();
        }
        true
    }
}

impl ImportJobSnapshot {
    pub(super) fn queued(job_id: String, scope: &BrainReadScope) -> Self {
        let now = unix_timestamp_seconds();
        Self {
            job_id,
            workspace_id: scope.workspace_id.clone(),
            root_dir: scope.root_dir.clone(),
            status: ImportLifecycleStatus::Imported,
            phase: ImportLifecyclePhase::Imported,
            progress_percent: 0,
            source_id: None,
            source_markdown_path: None,
            source_document_path: None,
            source_manifest_path: None,
            page_count: None,
            evidence_count: None,
            citation_ready: false,
            graph_ready: false,
            graph_status: None,
            graph_error_category: None,
            graph_generation_skipped_reason: None,
            graph_generation_error_message: None,
            retryable: false,
            retry_attempt: 0,
            max_retry_attempts: 2,
            next_retry_at: None,
            manual_retry_available: false,
            warnings: Vec::new(),
            error: None,
            cancel_requested: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub(super) fn to_value(&self) -> Value {
        json!({
            "jobId": self.job_id,
            "workspaceId": self.workspace_id,
            "status": self.status.as_str(),
            "phase": self.phase.as_str(),
            "progressPercent": self.progress_percent,
            "sourceId": self.source_id,
            "pageCount": self.page_count,
            "evidenceCount": self.evidence_count,
            "citationReady": self.citation_ready,
            "graphReady": self.graph_ready,
            "graphStatus": self.graph_status,
            "graphErrorCategory": self.graph_error_category,
            "graphGenerationSkippedReason": self.graph_generation_skipped_reason,
            "graphGenerationErrorMessage": self.graph_generation_error_message,
            "retryable": self.retryable,
            "retryAttempt": self.retry_attempt,
            "maxRetryAttempts": self.max_retry_attempts,
            "nextRetryAt": self.next_retry_at,
            "manualRetryAvailable": self.manual_retry_available,
            "warnings": self.warnings,
            "error": self.error,
            "cancelRequested": self.cancel_requested,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

fn import_job_snapshot_from_record(
    record: ImportJobRecord,
    scope: &BrainReadScope,
) -> ImportJobSnapshot {
    let lifecycle = ImportLifecycleState::from_persisted(
        &record.status,
        &record.graph_status,
        record.citation_ready,
        record.graph_ready,
        record.graph_retryable,
        record.manual_retry_available,
    );
    ImportJobSnapshot {
        job_id: record.job_id,
        workspace_id: record.workspace_id,
        root_dir: scope.root_dir.clone(),
        status: lifecycle.status,
        phase: lifecycle.phase,
        progress_percent: if lifecycle.terminal { 100 } else { 0 },
        source_id: Some(record.source_id),
        source_markdown_path: non_empty_string(record.source_markdown_path),
        source_document_path: non_empty_string(record.source_document_path),
        source_manifest_path: non_empty_string(record.source_manifest_path),
        page_count: None,
        evidence_count: None,
        citation_ready: lifecycle.citation_ready,
        graph_ready: lifecycle.graph_ready,
        graph_status: non_empty_string(record.graph_status),
        graph_error_category: non_empty_string(record.graph_error_category),
        graph_generation_skipped_reason: None,
        graph_generation_error_message: non_empty_string(record.graph_error_message_redacted),
        retryable: lifecycle.retryable,
        retry_attempt: record.graph_retry_attempt,
        max_retry_attempts: record.graph_max_retry_attempts.max(2),
        next_retry_at: record.graph_next_retry_at,
        manual_retry_available: lifecycle.manual_retry_available,
        warnings: Vec::new(),
        error: None,
        cancel_requested: false,
        created_at: record.updated_at,
        updated_at: record.updated_at,
    }
}

pub(super) fn non_empty_string(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

pub(super) struct ImportJobRequest {
    pub(super) job_id: String,
    pub(super) scope: BrainReadScope,
    pub(super) source_path: PathBuf,
    pub(super) format: DocumentFormat,
    pub(super) name: Option<String>,
    pub(super) skip_graph_generation: bool,
}

pub(super) fn next_import_job_id() -> String {
    format!("import-{}-{}", std::process::id(), unix_timestamp_millis())
}

pub(super) fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn spawn_import_job(registry: ImportJobRegistry, request: ImportJobRequest) {
    thread::spawn(move || {
        if let Err(error) = run_import_job(&registry, &request) {
            registry.update(&request.job_id, |job| {
                if !matches!(job.status, ImportLifecycleStatus::Cancelled) {
                    job.status = ImportLifecycleStatus::Failed;
                    job.phase = ImportLifecyclePhase::Failed;
                    job.progress_percent = 100;
                    job.error = Some(error.to_string());
                }
            });
        }
    });
}

fn run_import_job(registry: &ImportJobRegistry, request: &ImportJobRequest) -> Result<()> {
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let client = SubprocessEngineClient::default();
    registry.update_active(&request.job_id, |job| {
        job.status = ImportLifecycleStatus::Parsing;
        job.phase = ImportLifecyclePhase::Parsing;
        job.progress_percent = 5;
    });

    let parse = client.parse(
        ParseRequest {
            version: "1".into(),
            input: ParseInput {
                path: request.source_path.display().to_string(),
                format: request.format.clone(),
            },
            template: "General".into(),
            options: ParseOptions::default(),
            output: Some(ParseOutputTarget {
                root_dir: request.scope.root_dir.clone(),
                name: request.name.clone(),
                workspace_id: Some(request.scope.workspace_id.clone()),
                source_id: None,
            }),
        },
        &mut |progress| {
            let (phase, progress_percent) = import_phase_from_parse_progress(&progress);
            registry.update_active(&request.job_id, |job| {
                job.status = match phase {
                    ImportLifecyclePhase::Imported => ImportLifecycleStatus::Imported,
                    ImportLifecyclePhase::Parsing => ImportLifecycleStatus::Parsing,
                    ImportLifecyclePhase::Packaging => ImportLifecycleStatus::Packaging,
                    ImportLifecyclePhase::Failed => ImportLifecycleStatus::Failed,
                    ImportLifecyclePhase::CitationReady
                    | ImportLifecyclePhase::ContextMaterializing
                    | ImportLifecyclePhase::GraphRetryWaiting
                    | ImportLifecyclePhase::GraphPending
                    | ImportLifecyclePhase::GraphSkipped
                    | ImportLifecyclePhase::ContextReady
                    | ImportLifecyclePhase::Cancelled => job.status,
                };
                job.phase = phase;
                job.progress_percent = progress_percent;
            });
        },
    )?;
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let manifest = parse
        .source_manifest
        .ok_or_else(|| anyhow!("import_source parse did not produce a source manifest"))?;
    registry.update_active(&request.job_id, |job| {
        job.status = ImportLifecycleStatus::Packaging;
        job.phase = ImportLifecyclePhase::Packaging;
        job.progress_percent = 70;
        job.source_id = Some(manifest.source_id.clone());
        job.source_markdown_path = Some(manifest.markdown_path.clone());
        job.source_document_path = Some(manifest.source_path.clone());
        job.source_manifest_path = Some(manifest.manifest_path.clone());
        job.page_count = Some(parse.result.metadata.page_count);
    });

    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let compile = client.compile_project(CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(request.scope.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: Some(true),
    })?;
    let page_evidence = client.read_page_evidence(ReadPageEvidenceRequest {
        scope: request.scope.clone(),
        source_id: compile.source_id.clone(),
        page: None,
        include_local_paths: false,
    })?;
    let evidence_count = page_evidence.evidence.len();
    registry.update_active(&request.job_id, |job| {
        job.status = ImportLifecycleStatus::CitationReady;
        job.phase = ImportLifecyclePhase::CitationReady;
        job.progress_percent = if request.skip_graph_generation {
            100
        } else {
            82
        };
        job.source_id = Some(compile.source_id.clone());
        job.evidence_count = Some(evidence_count);
        job.citation_ready = evidence_count > 0;
        if request.skip_graph_generation {
            job.status = ImportLifecycleStatus::CitationReadyGraphSkipped;
            job.phase = ImportLifecyclePhase::GraphSkipped;
            job.graph_ready = false;
            job.graph_status = Some("skipped".into());
            job.graph_generation_skipped_reason = Some("skipGraphGeneration requested".into());
            job.manual_retry_available = true;
        }
    });
    if request.skip_graph_generation {
        persist_import_job_graph_status(registry, &client, request);
    }
    if registry.mark_cancelled_if_requested(&request.job_id) || request.skip_graph_generation {
        return Ok(());
    }

    registry.update_active(&request.job_id, |job| {
        job.status = ImportLifecycleStatus::CitationReady;
        job.phase = ImportLifecyclePhase::ContextMaterializing;
        job.progress_percent = 88;
    });
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let graph_compile = match compile_graph_stage_with_retry(registry, &client, request, &manifest)
    {
        Ok(graph_compile) => graph_compile,
        Err(error) => {
            mark_graph_pending(
                registry,
                Some(&client),
                request,
                &error.to_string(),
                0,
                None,
            );
            return Ok(());
        }
    };
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let graph_status = graph_compile.graph_generation_status.clone();
    let graph_ready = graph_status_is_ready(graph_status.as_deref());
    registry.update_active(&request.job_id, |job| {
        job.progress_percent = 100;
        job.graph_ready = graph_ready;
        job.graph_status = graph_status.or_else(|| Some("unknown".into()));
        job.graph_error_category = graph_compile.graph_generation_failed_reason;
        job.graph_generation_skipped_reason = graph_compile.graph_generation_skipped_reason;
        job.graph_generation_error_message = graph_compile.graph_generation_error_message;
        job.retryable = graph_compile.graph_generation_retryable.unwrap_or(false);
        if graph_ready {
            job.status = ImportLifecycleStatus::ContextReady;
            job.phase = ImportLifecyclePhase::ContextReady;
            job.retryable = false;
            job.next_retry_at = None;
            job.manual_retry_available = false;
        } else if matches!(job.graph_status.as_deref(), Some("skipped")) {
            job.status = ImportLifecycleStatus::CitationReadyGraphSkipped;
            job.phase = ImportLifecyclePhase::GraphSkipped;
            job.manual_retry_available = true;
        } else {
            job.status = ImportLifecycleStatus::CitationReadyGraphPending;
            job.phase = ImportLifecyclePhase::GraphPending;
            job.manual_retry_available = true;
        }
    });
    Ok(())
}

fn compile_graph_stage_with_retry(
    registry: &ImportJobRegistry,
    client: &SubprocessEngineClient,
    request: &ImportJobRequest,
    manifest: &SourceArtifactManifest,
) -> Result<CompileProjectResponseData> {
    let max_attempts = registry
        .get(&request.job_id)
        .map(|job| job.max_retry_attempts)
        .unwrap_or(2);
    let mut attempt = 0;
    loop {
        match compile_graph_stage(client, request, manifest) {
            Ok(response) => return Ok(response),
            Err(error) => {
                let message = error.to_string();
                let classification = classify_graph_failure(&message);
                if !classification.retryable || attempt >= max_attempts {
                    mark_graph_pending(registry, Some(client), request, &message, attempt, None);
                    return Ok(CompileProjectResponseData {
                        project_id: String::new(),
                        workspace_id: request.scope.workspace_id.clone(),
                        source_id: manifest.source_id.clone(),
                        graph_generation_status: Some("pending".into()),
                        graph_generation_skipped_reason: None,
                        graph_generation_error_message: Some(sanitize_graph_error_message(
                            &message,
                        )),
                        graph_generation_retryable: Some(classification.retryable),
                        graph_generation_failed_reason: Some(classification.category.into()),
                        graph_generation_stage: Some("graph_materialization".into()),
                    });
                }
                attempt = attempt.saturating_add(1);
                let next_retry_at = unix_timestamp_seconds().saturating_add(1);
                mark_graph_pending(
                    registry,
                    Some(client),
                    request,
                    &message,
                    attempt,
                    Some(next_retry_at),
                );
                thread::sleep(std::time::Duration::from_millis(150));
                registry.update_active(&request.job_id, |job| {
                    job.status = ImportLifecycleStatus::CitationReady;
                    job.phase = ImportLifecyclePhase::ContextMaterializing;
                    job.progress_percent = 88;
                    job.next_retry_at = None;
                });
            }
        }
    }
}

fn compile_graph_stage(
    client: &SubprocessEngineClient,
    request: &ImportJobRequest,
    manifest: &SourceArtifactManifest,
) -> Result<CompileProjectResponseData> {
    client.compile_project(CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(request.scope.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: Some(false),
    })
}

pub(super) struct GraphFailureClassification {
    pub(super) category: &'static str,
    pub(super) retryable: bool,
}

pub(super) fn classify_graph_failure(message: &str) -> GraphFailureClassification {
    let lower = message.to_ascii_lowercase();
    let retryable = lower.contains("database is locked")
        || lower.contains("database is busy")
        || lower.contains("sqlite_busy")
        || lower.contains("sqlite_locked")
        || lower.contains("provider_timeout")
        || lower.contains("provider_unavailable")
        || (lower.contains("readonly database")
            && (lower.contains("transaction") || lower.contains("connection")));
    let category = if lower.contains("provider_timeout") {
        "provider_timeout"
    } else if lower.contains("provider_unavailable") {
        "provider_unavailable"
    } else if lower.contains("database is locked") || lower.contains("sqlite_locked") {
        "db_locked"
    } else if lower.contains("database is busy") || lower.contains("sqlite_busy") {
        "db_busy"
    } else if lower.contains("readonly database") {
        "db_readonly"
    } else {
        "graph_materialization_failed"
    };
    GraphFailureClassification {
        category,
        retryable,
    }
}

fn mark_graph_pending(
    registry: &ImportJobRegistry,
    client: Option<&dyn EngineClient>,
    request: &ImportJobRequest,
    message: &str,
    retry_attempt: u8,
    next_retry_at: Option<u64>,
) {
    let classification = classify_graph_failure(message);
    registry.update_active(&request.job_id, |job| {
        job.status = if next_retry_at.is_some() {
            ImportLifecycleStatus::GraphRetryWaiting
        } else {
            ImportLifecycleStatus::CitationReadyGraphPending
        };
        job.phase = if next_retry_at.is_some() {
            ImportLifecyclePhase::GraphRetryWaiting
        } else {
            ImportLifecyclePhase::GraphPending
        };
        job.progress_percent = 100;
        job.graph_ready = false;
        job.graph_status = Some("pending".into());
        job.graph_error_category = Some(classification.category.into());
        job.graph_generation_error_message = Some(sanitize_graph_error_message(message));
        job.retryable = classification.retryable;
        job.retry_attempt = retry_attempt;
        job.next_retry_at = next_retry_at;
        job.manual_retry_available = true;
    });
    if let (Some(client), Some(job)) = (client, registry.get(&request.job_id)) {
        if let Some(source_id) = job.source_id.as_deref() {
            let result = client.update_import_job_graph_status(UpdateImportJobGraphStatusRequest {
                scope: request.scope.clone(),
                source_id: source_id.to_string(),
                status: job.status.as_str().into(),
                graph_status: job.graph_status.clone().unwrap_or_else(|| "pending".into()),
                graph_error_category: job.graph_error_category.clone(),
                graph_error_message_redacted: job.graph_generation_error_message.clone(),
                graph_retryable: job.retryable,
                graph_retry_attempt: job.retry_attempt,
                graph_max_retry_attempts: job.max_retry_attempts,
                graph_next_retry_at: job.next_retry_at,
                manual_retry_available: job.manual_retry_available,
            });
            record_graph_status_persist_result(registry, &request.job_id, result);
        }
    }
}

pub(super) fn sanitize_graph_error_message(message: &str) -> String {
    redact_local_path_text(message.lines().next().unwrap_or(message).trim())
}

fn persist_import_job_graph_status(
    registry: &ImportJobRegistry,
    client: &dyn EngineClient,
    request: &ImportJobRequest,
) {
    let Some(job) = registry.get(&request.job_id) else {
        return;
    };
    let Some(source_id) = job.source_id.as_deref() else {
        return;
    };
    let result = client.update_import_job_graph_status(UpdateImportJobGraphStatusRequest {
        scope: request.scope.clone(),
        source_id: source_id.to_string(),
        status: job.status.as_str().into(),
        graph_status: job.graph_status.clone().unwrap_or_else(|| "unknown".into()),
        graph_error_category: job.graph_error_category.clone(),
        graph_error_message_redacted: job.graph_generation_error_message.clone(),
        graph_retryable: job.retryable,
        graph_retry_attempt: job.retry_attempt,
        graph_max_retry_attempts: job.max_retry_attempts,
        graph_next_retry_at: job.next_retry_at,
        manual_retry_available: job.manual_retry_available,
    });
    record_graph_status_persist_result(registry, &request.job_id, result);
}

pub(super) fn record_graph_status_persist_result(
    registry: &ImportJobRegistry,
    job_id: &str,
    result: Result<bool>,
) {
    match result {
        Ok(true) => {}
        Ok(false) => registry.update(job_id, |job| {
            push_unique_warning(
                &mut job.warnings,
                "graph_status_persist_failed: citation-ready import job was not found",
            );
        }),
        Err(error) => {
            let message = sanitize_graph_error_message(&error.to_string());
            registry.update(job_id, |job| {
                push_unique_warning(
                    &mut job.warnings,
                    &format!("graph_status_persist_failed: {message}"),
                );
            });
        }
    }
}

fn push_unique_warning(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|existing| existing == warning) {
        warnings.push(warning.to_string());
    }
}

pub(super) fn retry_import_graph(
    registry: &ImportJobRegistry,
    job: &ImportJobSnapshot,
) -> Result<()> {
    if !job.citation_ready {
        return Err(anyhow!(
            "import graph retry requires a citation-ready source"
        ));
    }
    if job.graph_ready {
        return Ok(());
    }
    let source_markdown_path = job
        .source_markdown_path
        .clone()
        .ok_or_else(|| anyhow!("import graph retry is missing source markdown state"))?;
    let source_document_path = job.source_document_path.clone();
    let source_manifest_path = job
        .source_manifest_path
        .clone()
        .ok_or_else(|| anyhow!("import graph retry is missing source manifest state"))?;
    let source_id = job
        .source_id
        .clone()
        .ok_or_else(|| anyhow!("import graph retry is missing source id"))?;

    registry.update(&job.job_id, |job| {
        job.status = ImportLifecycleStatus::CitationReady;
        job.phase = ImportLifecyclePhase::ContextMaterializing;
        job.progress_percent = 88;
        job.next_retry_at = None;
    });
    let client = SubprocessEngineClient::default();
    match client.compile_project(CompileProjectRequest {
        source_markdown_path,
        source_document_path,
        source_manifest_path: Some(source_manifest_path),
        workspace_id: Some(job.workspace_id.clone()),
        source_id: Some(source_id.clone()),
        skip_graph_generation: Some(false),
    }) {
        Ok(response) => {
            let graph_status = response.graph_generation_status.clone();
            let graph_ready = graph_status_is_ready(graph_status.as_deref());
            registry.update(&job.job_id, |job| {
                job.progress_percent = 100;
                job.graph_ready = graph_ready;
                job.graph_status = graph_status.or_else(|| Some("unknown".into()));
                job.graph_error_category = response.graph_generation_failed_reason;
                job.graph_generation_skipped_reason = response.graph_generation_skipped_reason;
                job.graph_generation_error_message = response.graph_generation_error_message;
                job.retryable = response.graph_generation_retryable.unwrap_or(false);
                if graph_ready {
                    job.status = ImportLifecycleStatus::ContextReady;
                    job.phase = ImportLifecyclePhase::ContextReady;
                    job.manual_retry_available = false;
                    job.retryable = false;
                } else {
                    job.status = ImportLifecycleStatus::CitationReadyGraphPending;
                    job.phase = ImportLifecyclePhase::GraphPending;
                    job.manual_retry_available = true;
                }
            });
        }
        Err(error) => {
            mark_graph_pending(
                registry,
                Some(&client),
                &job_import_request_stub(job),
                &error.to_string(),
                0,
                None,
            );
        }
    }
    Ok(())
}

fn job_import_request_stub(job: &ImportJobSnapshot) -> ImportJobRequest {
    ImportJobRequest {
        job_id: job.job_id.clone(),
        scope: BrainReadScope {
            workspace_id: job.workspace_id.clone(),
            root_dir: job.root_dir.clone(),
        },
        source_path: PathBuf::new(),
        format: DocumentFormat::Markdown,
        name: None,
        skip_graph_generation: false,
    }
}

pub(super) fn import_phase_from_parse_progress(
    progress: &ParseProgress,
) -> (ImportLifecyclePhase, u8) {
    match progress {
        ParseProgress::Queued => (ImportLifecyclePhase::Imported, 2),
        ParseProgress::ConvertingPages { current, total } => (
            ImportLifecyclePhase::Parsing,
            scaled_progress(*current as usize, *total as usize, 10, 35),
        ),
        ParseProgress::Parsing { current, total } => (
            ImportLifecyclePhase::Parsing,
            scaled_progress(*current as usize, *total as usize, 35, 65),
        ),
        ParseProgress::Packaging => (ImportLifecyclePhase::Packaging, 68),
        ParseProgress::Completed => (ImportLifecyclePhase::Packaging, 70),
        ParseProgress::Failed { .. } => (ImportLifecyclePhase::Failed, 100),
    }
}

fn scaled_progress(current: usize, total: usize, start: u8, end: u8) -> u8 {
    if total == 0 {
        return start;
    }
    let span = end.saturating_sub(start) as usize;
    let bounded_current = current.min(total);
    (start as usize + (span * bounded_current / total)) as u8
}

pub(super) fn ensure_import_job_scope(
    job: &ImportJobSnapshot,
    scope: &BrainReadScope,
) -> Result<()> {
    if job.workspace_id != scope.workspace_id || job.root_dir != scope.root_dir {
        return Err(anyhow!("import job not found in requested workspace scope"));
    }
    Ok(())
}

pub(super) fn import_job_lookup(
    arguments: &Map<String, Value>,
) -> Result<(Option<String>, Option<String>)> {
    let job_id = optional_string(arguments, "jobId")?;
    let source_id = optional_string(arguments, "sourceId")?;
    if job_id.is_none() && source_id.is_none() {
        return Err(anyhow!("missing required argument: jobId or sourceId"));
    }
    Ok((job_id, source_id))
}

pub(super) fn resolve_import_job(
    client: &dyn EngineClient,
    state: &McpServerState,
    scope: &BrainReadScope,
    job_id: Option<String>,
    source_id: Option<String>,
) -> Result<ImportJobSnapshot> {
    if let Some(job_id) = job_id.as_deref() {
        if let Some(job) = state.import_jobs.get(job_id) {
            ensure_import_job_scope(&job, scope)?;
            return Ok(job);
        }
    }
    let record = client
        .read_import_job(ReadImportJobRequest {
            scope: scope.clone(),
            job_id,
            source_id,
        })?
        .ok_or_else(|| anyhow!("import job not found in requested workspace scope"))?;
    let job = import_job_snapshot_from_record(record, scope);
    ensure_import_job_scope(&job, scope)?;
    state.import_jobs.insert(job.clone());
    Ok(job)
}
