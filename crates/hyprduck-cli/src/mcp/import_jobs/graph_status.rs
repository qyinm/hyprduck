use anyhow::Result;
use hyprduck_engine_client::EngineClient;
use hyprduck_engine_types::{
    ImportLifecyclePhase, ImportLifecycleStatus, UpdateImportJobGraphStatusRequest,
};

use super::{ImportJobRegistry, ImportJobRequest};
use crate::mcp::policy::redact_local_path_text;

pub(in crate::mcp) struct GraphFailureClassification {
    pub(in crate::mcp) category: &'static str,
    pub(in crate::mcp) retryable: bool,
}

pub(in crate::mcp) fn classify_graph_failure(message: &str) -> GraphFailureClassification {
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

pub(in crate::mcp) fn mark_graph_pending(
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

pub(in crate::mcp) fn sanitize_graph_error_message(message: &str) -> String {
    redact_local_path_text(message.lines().next().unwrap_or(message).trim())
}

pub(in crate::mcp) fn persist_import_job_graph_status(
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

pub(in crate::mcp) fn record_graph_status_persist_result(
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
