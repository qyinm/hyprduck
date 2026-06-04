use std::path::Path;

#[path = "workspace_rebuild/chunking.rs"]
mod workspace_rebuild_chunking;
#[path = "workspace_rebuild/compaction.rs"]
mod workspace_rebuild_compaction;
#[path = "workspace_rebuild/events.rs"]
mod workspace_rebuild_events;
#[path = "workspace_rebuild/fingerprint.rs"]
mod workspace_rebuild_fingerprint;
#[path = "workspace_rebuild/linking_stage.rs"]
mod workspace_rebuild_linking_stage;
#[path = "workspace_rebuild/merge.rs"]
mod workspace_rebuild_merge;
#[path = "workspace_rebuild/provider_stage.rs"]
mod workspace_rebuild_provider_stage;
#[path = "workspace_rebuild/source_stage.rs"]
mod workspace_rebuild_source_stage;
#[path = "workspace_rebuild/types.rs"]
mod workspace_rebuild_types;

#[cfg(test)]
use self::workspace_rebuild_chunking::{
    source_chunk_evidence_refs, source_graph_chunk_batch_plan, source_graph_chunk_batches,
    write_source_chunk_run_artifact,
};
#[cfg(test)]
use self::workspace_rebuild_compaction::{
    compact_source_graph_snapshot, normalize_concept_label, strip_source_of_relations,
};
#[cfg(test)]
use self::workspace_rebuild_events::source_graph_build_materialized_event;
#[cfg(test)]
use self::workspace_rebuild_events::workspace_linking_materialized_event;
use self::workspace_rebuild_fingerprint::provider_graph_input_fingerprint;
use self::workspace_rebuild_linking_stage::{
    materialize_workspace_linking_stage, WorkspaceLinkingStageInput,
};
#[cfg(test)]
use self::workspace_rebuild_merge::{
    merge_source_graph_snapshots, parse_and_validate_source_graph_response,
};
#[cfg(test)]
use self::workspace_rebuild_provider_stage::update_materialized_counts;
#[cfg(test)]
use self::workspace_rebuild_provider_stage::{
    mark_source_graph_compaction_failed, mark_workspace_linking_failed,
    provider_graph_failure_reason, source_graph_materialized_status, source_graph_progress,
};
use self::workspace_rebuild_provider_stage::{
    provider_graph_generation_disabled_for_process, provider_graph_report_is_reusable,
    provider_unavailable_reason,
};
use self::workspace_rebuild_source_stage::{materialize_source_graph_stage, SourceGraphStageInput};
pub(crate) use self::workspace_rebuild_types::{
    GraphCandidateBatch, GraphCandidateNode, GraphCandidateRelation, SourceGraphCompactionReport,
};
use self::workspace_rebuild_types::{
    PROVIDER_GRAPH_PROMPT_VERSION, PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
    PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION, SOURCE_GRAPH_AUTO_BATCH_LIMIT,
    SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS, SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS,
    SOURCE_GRAPH_HARD_MAX_CONCEPTS, SOURCE_GRAPH_HARD_MAX_RELATIONS, SOURCE_GRAPH_MAX_CLAIMS,
    SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE, SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION,
    SOURCE_GRAPH_TARGET_CONCEPTS,
};
#[cfg(test)]
use super::prompt::build_source_chunk_graph_prompt;
#[cfg(test)]
use super::reports::ProviderGraphStageRunReport;
use super::reports::{
    ProviderGraphMaterializationInputFingerprint, ProviderGraphMaterializationReport,
};
#[cfg(test)]
use crate::provider::ProviderKind;
use crate::provider::{EngineConfig, EngineConfigStore};
use crate::*;

pub(crate) fn maybe_generate_provider_graph_materialization(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    artifact_root: &Path,
    context: &ImportEvidenceContext,
) -> Result<ProviderGraphMaterializationReport> {
    let report_path = artifact_root.join("provider-graph-materialization.json");
    let config = match EngineConfigStore::default().and_then(|store| store.load()) {
        Ok(config) => config,
        Err(error) => {
            let report = ProviderGraphMaterializationReport {
                status: "failed".into(),
                provider: "unknown".into(),
                model: "unknown".into(),
                source_id: manifest.source_id.clone(),
                input_fingerprint: None,
                source_graph_node_count: 0,
                source_graph_relation_count: 0,
                workspace_link_count: 0,
                materialized_node_count: 0,
                materialized_relation_count: 0,
                materialized_claim_count: 0,
                materialized_memory_count: 0,
                skipped_reason: None,
                error_message: Some(format!("{error:#}")),
                provider_run_ids: Vec::new(),
                source_graph_run_id: None,
                workspace_linking_run_id: None,
                source_graph_materialized: false,
                workspace_linking_materialized: false,
                retryable: false,
                stage: "config".into(),
                progress: 0.0,
                failed_reason: Some("config_load_failed".into()),
                chunk_total: 0,
                chunk_succeeded: 0,
                chunk_failed: 0,
                chunk_discovered: 0,
                chunk_processed: 0,
                chunk_skipped: 0,
                stage_runs: Vec::new(),
                raw_source_graph_node_count: 0,
                raw_source_graph_relation_count: 0,
                canonical_source_graph_node_count: 0,
                canonical_source_graph_relation_count: 0,
                pruned_source_graph_node_count: 0,
                pruned_source_graph_relation_count: 0,
                compaction_status: None,
                compaction_report_path: None,
                updated_at: unix_timestamp_seconds(),
            };
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    let input_fingerprint =
        provider_graph_input_fingerprint(workspace_root, workspace_id, manifest, markdown, &config);
    if let Ok(existing) = read_json_artifact::<ProviderGraphMaterializationReport>(&report_path) {
        if provider_graph_report_is_reusable(&existing, manifest, &input_fingerprint) {
            return Ok(existing);
        }
    }
    let mut report = ProviderGraphMaterializationReport {
        status: "skipped".into(),
        provider: config.provider.id_slug().into(),
        model: config.model_id.clone(),
        source_id: manifest.source_id.clone(),
        input_fingerprint: Some(input_fingerprint),
        source_graph_node_count: 0,
        source_graph_relation_count: 0,
        workspace_link_count: 0,
        materialized_node_count: 0,
        materialized_relation_count: 0,
        materialized_claim_count: 0,
        materialized_memory_count: 0,
        skipped_reason: None,
        error_message: None,
        provider_run_ids: Vec::new(),
        source_graph_run_id: None,
        workspace_linking_run_id: None,
        source_graph_materialized: false,
        workspace_linking_materialized: false,
        retryable: false,
        stage: "queued".into(),
        progress: 0.0,
        failed_reason: None,
        chunk_total: 0,
        chunk_succeeded: 0,
        chunk_failed: 0,
        chunk_discovered: 0,
        chunk_processed: 0,
        chunk_skipped: 0,
        stage_runs: Vec::new(),
        raw_source_graph_node_count: 0,
        raw_source_graph_relation_count: 0,
        canonical_source_graph_node_count: 0,
        canonical_source_graph_relation_count: 0,
        pruned_source_graph_node_count: 0,
        pruned_source_graph_relation_count: 0,
        compaction_status: None,
        compaction_report_path: None,
        updated_at: unix_timestamp_seconds(),
    };

    if provider_graph_generation_disabled_for_process() {
        report.skipped_reason = Some("provider_graph_generation_disabled".into());
        report.stage = "skipped".into();
        report.progress = 1.0;
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    if provider_unavailable(&config) {
        report.skipped_reason = Some(provider_unavailable_reason(&config).into());
        report.stage = "skipped".into();
        report.progress = 1.0;
        report.failed_reason = report.skipped_reason.clone();
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)
        .unwrap_or_else(|_| empty_replayed_brain_snapshot(workspace_id));
    write_json_pretty(&artifact_root.join("provider-graph-context.json"), context)?;
    report.stage = "extracting_source_chunks".into();
    report.status = "extracting_source_chunks".into();
    write_json_pretty(&report_path, &report)?;

    let Some(source_stage) = materialize_source_graph_stage(
        SourceGraphStageInput {
            workspace_root,
            workspace_id,
            manifest,
            markdown,
            artifact_root,
            context,
            report_path: &report_path,
            baseline_snapshot: &snapshot,
            config: &config,
        },
        report,
    )?
    else {
        return read_json_artifact(&report_path)
            .context("source graph stage exited without a persisted report");
    };
    let report = source_stage.report;
    let source_materialization_input = source_stage.source_graph_snapshot;
    let linked_baseline = source_stage.linked_baseline;
    let source_graph_run_id = source_stage.source_graph_run_id;

    materialize_workspace_linking_stage(
        WorkspaceLinkingStageInput {
            workspace_root,
            workspace_id,
            manifest,
            markdown,
            artifact_root,
            context,
            report_path: &report_path,
            source_graph_run_id: &source_graph_run_id,
            source_materialization_input,
            linked_baseline,
            config: &config,
        },
        report,
    )
}

#[cfg(test)]
mod tests;
