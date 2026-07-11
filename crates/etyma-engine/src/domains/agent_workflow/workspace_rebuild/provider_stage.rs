//! Provider graph stage helpers.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::json;

use super::super::artifacts::{
    provider_workspace_rebuild_response_schema, write_provider_graph_run_artifacts,
    ProviderGraphRunArtifact,
};
use super::super::reports::{
    ProviderGraphMaterializationInputFingerprint, ProviderGraphMaterializationReport,
};
use super::{write_json_pretty, SourceArtifactManifest};
use crate::provider::{EngineConfig, ProviderKind};
use crate::{
    parse_openai_compatible_json_schema_with_timeout, unix_timestamp_seconds, BrainRepoSnapshot,
    EvidenceRef, PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
};

#[derive(Debug, Clone)]
pub(super) struct SourceGraphChunkJob {
    pub(super) batch_index: usize,
    pub(super) run_id: String,
    pub(super) chunk_ids: Vec<String>,
    pub(super) evidence_refs: Vec<EvidenceRef>,
    pub(super) prompt: String,
}

pub(super) struct SourceGraphChunkProviderResult {
    pub(super) batch_index: usize,
    pub(super) run_id: String,
    pub(super) chunk_ids: Vec<String>,
    pub(super) evidence_refs: Vec<EvidenceRef>,
    pub(super) response: Result<String>,
}

pub(super) fn source_graph_progress(report: &ProviderGraphMaterializationReport) -> f32 {
    if report.chunk_total == 0 {
        return 0.0;
    }
    let completed = report.chunk_succeeded + report.chunk_failed;
    (completed as f32 / report.chunk_total as f32).clamp(0.0, 1.0)
}

pub(super) fn provider_graph_failure_reason(error: &anyhow::Error) -> &'static str {
    let message = format!("{error:#}");
    if message.contains("provider_timeout") {
        "provider_timeout"
    } else if message.contains("provider_unavailable") {
        "provider_unavailable"
    } else {
        "provider_error"
    }
}

pub(super) fn source_graph_materialized_status(
    report: &ProviderGraphMaterializationReport,
) -> &'static str {
    match (report.chunk_failed > 0, report.chunk_skipped > 0) {
        (true, true) => "source_partial_with_failures_and_skipped_chunks",
        (true, false) => "source_partial_with_failures",
        (false, true) => "source_partial_with_skipped_chunks",
        (false, false) => "source_graph_materialized",
    }
}

pub(super) fn provider_unavailable_reason(config: &EngineConfig) -> &'static str {
    match &config.provider {
        ProviderKind::Unknown(_) => "unsupported_provider",
        ProviderKind::OpenRouter if config.api_key.trim().is_empty() => "provider_config",
        _ => "provider_config",
    }
}

pub(super) fn update_materialized_counts(
    report: &mut ProviderGraphMaterializationReport,
    snapshot: &BrainRepoSnapshot,
) {
    report.materialized_node_count = snapshot.nodes.len();
    report.materialized_relation_count = snapshot.relations.len();
    report.materialized_claim_count = snapshot.claims.len();
    report.materialized_memory_count = snapshot.memories.len();
}

pub(super) fn provider_graph_report_is_reusable(
    report: &ProviderGraphMaterializationReport,
    manifest: &SourceArtifactManifest,
    input_fingerprint: &ProviderGraphMaterializationInputFingerprint,
) -> bool {
    report.status == "linked"
        && report.source_id == manifest.source_id
        && report.input_fingerprint.as_ref() == Some(input_fingerprint)
}

pub(super) fn mark_workspace_linking_failed(
    report: &mut ProviderGraphMaterializationReport,
    error: &anyhow::Error,
) {
    report.status = if report.source_graph_materialized {
        "source_graph_materialized_linking_failed".into()
    } else {
        "failed".into()
    };
    report.workspace_linking_materialized = false;
    report.error_message = Some(format!("{error:#}"));
    report.retryable = true;
    report.stage = "workspace_linking_failed".into();
    report.failed_reason = Some(provider_graph_failure_reason(error).into());
    report.updated_at = unix_timestamp_seconds();
}

pub(super) fn mark_source_graph_compaction_failed(
    report: &mut ProviderGraphMaterializationReport,
    artifact_root: &Path,
    report_path: &Path,
    raw_snapshot: &BrainRepoSnapshot,
    error: &anyhow::Error,
) -> Result<()> {
    report.status = "failed_no_materialization".into();
    report.stage = "source_graph_compaction_failed".into();
    report.retryable = true;
    report.error_message = Some(format!("{error:#}"));
    report.failed_reason = Some("source_graph_compaction_failed".into());
    report.raw_source_graph_node_count = raw_snapshot.nodes.len();
    report.raw_source_graph_relation_count = raw_snapshot.relations.len();
    report.compaction_status = Some("failed".into());
    report.compaction_report_path = Some(
        artifact_root
            .join("provider-graph-compaction.json")
            .to_string_lossy()
            .into_owned(),
    );
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(
        &artifact_root.join("provider-graph-compaction.json"),
        &json!({
            "status": "failed",
            "rawNodeCount": raw_snapshot.nodes.len(),
            "rawRelationCount": raw_snapshot.relations.len(),
            "errorMessage": format!("{error:#}"),
            "updatedAt": report.updated_at,
        }),
    )?;
    write_json_pretty(report_path, report)
}

pub(super) fn run_provider_graph_stage(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    config: &EngineConfig,
    run_id: &str,
    prompt: &str,
    response_schema: async_openai::types::chat::ResponseFormatJsonSchema,
) -> Result<String> {
    let response = match parse_openai_compatible_json_schema_with_timeout(
        config,
        prompt,
        response_schema,
        Some(Duration::from_secs(
            PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
        )),
    ) {
        Ok(response) => response,
        Err(error) => {
            write_provider_graph_run_artifacts(ProviderGraphRunArtifact {
                workspace_root,
                run_id,
                workspace_id,
                manifest,
                status: "failed",
                prompt: Some(prompt),
                provider_response: None,
                error_message: Some(format!("{error:#}")),
            })?;
            return Err(error);
        }
    };
    write_provider_graph_run_artifacts(ProviderGraphRunArtifact {
        workspace_root,
        run_id,
        workspace_id,
        manifest,
        status: "received",
        prompt: Some(prompt),
        provider_response: Some(&response),
        error_message: None,
    })?;
    Ok(response)
}

pub(super) fn run_source_graph_chunk_provider_jobs(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    config: &EngineConfig,
    jobs: &[SourceGraphChunkJob],
) -> Vec<SourceGraphChunkProviderResult> {
    std::thread::scope(|scope| {
        let handles = jobs
            .iter()
            .map(|job| {
                scope.spawn(move || SourceGraphChunkProviderResult {
                    batch_index: job.batch_index,
                    run_id: job.run_id.clone(),
                    chunk_ids: job.chunk_ids.clone(),
                    evidence_refs: job.evidence_refs.clone(),
                    response: run_provider_graph_stage(
                        workspace_root,
                        workspace_id,
                        manifest,
                        config,
                        &job.run_id,
                        &job.prompt,
                        provider_workspace_rebuild_response_schema(),
                    ),
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .zip(jobs.iter())
            .map(|(handle, job)| match handle.join() {
                Ok(result) => result,
                Err(_) => SourceGraphChunkProviderResult {
                    batch_index: job.batch_index,
                    run_id: job.run_id.clone(),
                    chunk_ids: job.chunk_ids.clone(),
                    evidence_refs: job.evidence_refs.clone(),
                    response: Err(anyhow!("source chunk provider worker panicked")),
                },
            })
            .collect()
    })
}

pub(super) fn provider_graph_generation_disabled_for_process() -> bool {
    std::env::var_os("ETYMA_DISABLE_PROVIDER_GRAPH").is_some()
        || (cfg!(test) && std::env::var_os("ETYMA_TEST_ENABLE_PROVIDER_GRAPH").is_none())
}
