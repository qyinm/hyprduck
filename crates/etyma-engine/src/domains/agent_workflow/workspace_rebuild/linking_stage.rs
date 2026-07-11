use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

use super::super::artifacts::{
    provider_workspace_linking_response_schema, write_provider_graph_run_validation_report,
};
use super::super::linking_policy::unverified_workspace_linking_relation_ids;
use super::super::prompt::build_workspace_linking_prompt;
use super::super::reports::ProviderGraphMaterializationReport;
use super::super::response::{
    normalize_provider_workspace_linking_snapshot, parse_provider_workspace_rebuild_snapshot,
};
use super::super::validation::validate_provider_workspace_linking_snapshot;
use super::workspace_rebuild_events::{
    workspace_linking_materialized_event, write_graph_diff_artifact,
};
use super::workspace_rebuild_provider_stage::{
    mark_workspace_linking_failed, run_provider_graph_stage, update_materialized_counts,
};
use crate::provider::EngineConfig;
use crate::{
    capture_materialized_file_snapshot, changed_materialized_files, merge_preserved_brain_events,
    read_materialized_brain_snapshot, unix_timestamp_seconds, write_json_pretty,
    write_materialized_brain_repo, BrainRepoSnapshot, ImportEvidenceContext,
    SourceArtifactManifest,
};

pub(super) struct WorkspaceLinkingStageInput<'a> {
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_id: &'a str,
    pub(super) manifest: &'a SourceArtifactManifest,
    pub(super) markdown: &'a str,
    pub(super) artifact_root: &'a Path,
    pub(super) context: &'a ImportEvidenceContext,
    pub(super) report_path: &'a Path,
    pub(super) source_graph_run_id: &'a str,
    pub(super) source_materialization_input: BrainRepoSnapshot,
    pub(super) linked_baseline: BrainRepoSnapshot,
    pub(super) config: &'a EngineConfig,
}

pub(super) fn materialize_workspace_linking_stage(
    input: WorkspaceLinkingStageInput<'_>,
    mut report: ProviderGraphMaterializationReport,
) -> Result<ProviderGraphMaterializationReport> {
    let WorkspaceLinkingStageInput {
        workspace_root,
        workspace_id,
        manifest,
        markdown,
        artifact_root: _artifact_root,
        context,
        report_path,
        source_graph_run_id: _source_graph_run_id,
        source_materialization_input: _source_materialization_input,
        linked_baseline,
        config,
    } = input;

    let workspace_linking_run_id = format!("provider-workspace-linking-{}", Uuid::now_v7());
    report
        .provider_run_ids
        .push(workspace_linking_run_id.clone());
    report.workspace_linking_run_id = Some(workspace_linking_run_id.clone());
    let workspace_linking_prompt = build_workspace_linking_prompt(
        workspace_root,
        workspace_id,
        manifest,
        markdown,
        &linked_baseline,
        context,
    )?;
    let workspace_linking_response = match run_provider_graph_stage(
        workspace_root,
        workspace_id,
        manifest,
        config,
        &workspace_linking_run_id,
        &workspace_linking_prompt,
        provider_workspace_linking_response_schema(),
    ) {
        Ok(response) => response,
        Err(error) => {
            mark_workspace_linking_failed(&mut report, &error);
            write_json_pretty(report_path, &report)?;
            return Ok(report);
        }
    };
    let mut linking_snapshot =
        match parse_provider_workspace_rebuild_snapshot(&workspace_linking_response) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                mark_workspace_linking_failed(&mut report, &error);
                write_provider_graph_run_validation_report(
                    workspace_root,
                    &workspace_linking_run_id,
                    workspace_id,
                    &manifest.source_id,
                    "failed",
                    0,
                    Some(format!("{error:#}")),
                )?;
                write_json_pretty(report_path, &report)?;
                return Ok(report);
            }
        };
    normalize_provider_workspace_linking_snapshot(
        &mut linking_snapshot,
        workspace_id,
        &linked_baseline,
        &manifest.source_id,
        unix_timestamp_seconds(),
    );
    report.workspace_link_count = linking_snapshot.relations.len();
    if let Err(error) = validate_provider_workspace_linking_snapshot(
        &linking_snapshot,
        &linked_baseline,
        &manifest.source_id,
    ) {
        mark_workspace_linking_failed(&mut report, &error);
        write_provider_graph_run_validation_report(
            workspace_root,
            &workspace_linking_run_id,
            workspace_id,
            &manifest.source_id,
            "failed",
            linking_snapshot.nodes.len(),
            Some(format!("{error:#}")),
        )?;
        write_json_pretty(report_path, &report)?;
        return Ok(report);
    }

    let before_linking = capture_materialized_file_snapshot(workspace_root)?;
    let invalidate_relation_ids =
        unverified_workspace_linking_relation_ids(&linked_baseline, &manifest.source_id);
    let workspace_linking_event = workspace_linking_materialized_event(
        workspace_id,
        &workspace_linking_run_id,
        manifest,
        &linking_snapshot,
        &invalidate_relation_ids,
    )?;
    let mut linking_materialization_input = linked_baseline.clone();
    linking_materialization_input.events =
        merge_preserved_brain_events(vec![workspace_linking_event], &linked_baseline.events);
    write_materialized_brain_repo(workspace_root, &linking_materialization_input)?;
    report.workspace_linking_materialized = true;
    report.retryable = report.chunk_failed > 0;
    report.failed_reason = if report.chunk_failed > 0 {
        Some("source_chunk_failures".into())
    } else if report.chunk_skipped > 0 {
        Some("source_chunk_batch_limit".into())
    } else {
        None
    };
    let after_linking = capture_materialized_file_snapshot(workspace_root)?;
    let workspace_linking_changed_files =
        changed_materialized_files(&before_linking, &after_linking);
    let final_snapshot = read_materialized_brain_snapshot(workspace_root, workspace_id)
        .unwrap_or(linking_materialization_input);
    write_provider_graph_run_validation_report(
        workspace_root,
        &workspace_linking_run_id,
        workspace_id,
        &manifest.source_id,
        "materialized",
        linking_snapshot.relations.len(),
        None,
    )?;
    write_graph_diff_artifact(
        workspace_root,
        &workspace_linking_run_id,
        workspace_id,
        &manifest.source_id,
        &workspace_linking_changed_files,
        &linking_snapshot,
    )?;

    report.status = if report.chunk_failed > 0 && report.chunk_skipped > 0 {
        "linked_with_source_chunk_failures_and_skipped_chunks".into()
    } else if report.chunk_failed > 0 {
        "linked_with_source_chunk_failures".into()
    } else if report.chunk_skipped > 0 {
        "linked_with_skipped_source_chunks".into()
    } else {
        "linked".into()
    };
    report.stage = "linked".into();
    report.progress = 1.0;
    update_materialized_counts(&mut report, &final_snapshot);
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(report_path, &report)?;
    Ok(report)
}
