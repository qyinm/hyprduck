use anyhow::Result;
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

use super::super::artifacts::write_provider_graph_run_validation_report;
use super::super::prompt::build_source_chunk_graph_prompt;
use super::super::reports::{ProviderGraphMaterializationReport, ProviderGraphStageRunReport};
use super::workspace_rebuild_chunking::{
    source_chunk_evidence_refs, source_graph_chunk_batch_plan,
    write_graph_candidate_batch_artifact, write_source_chunk_run_artifact,
};
use super::workspace_rebuild_compaction::{
    compact_source_graph_snapshot, strip_source_of_relations,
};
use super::workspace_rebuild_events::{
    source_graph_build_materialized_event, write_graph_diff_artifact,
};
use super::workspace_rebuild_merge::{
    merge_raw_source_graph_snapshots, parse_and_validate_source_graph_response,
};
use super::workspace_rebuild_provider_stage::{
    mark_source_graph_compaction_failed, provider_graph_failure_reason,
    run_source_graph_chunk_provider_jobs, source_graph_materialized_status, source_graph_progress,
    update_materialized_counts, SourceGraphChunkJob,
};
use super::workspace_rebuild_types::SOURCE_GRAPH_CHUNK_PARALLELISM;
use crate::provider::EngineConfig;
use crate::{
    capture_materialized_file_snapshot, changed_materialized_files, merge_preserved_brain_events,
    read_materialized_brain_snapshot, unix_timestamp_seconds, write_json_pretty,
    write_materialized_brain_repo, BrainRepoSnapshot, ImportEvidenceContext,
    SourceArtifactManifest,
};

pub(super) struct SourceGraphStageInput<'a> {
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_id: &'a str,
    pub(super) manifest: &'a SourceArtifactManifest,
    pub(super) markdown: &'a str,
    pub(super) artifact_root: &'a Path,
    pub(super) context: &'a ImportEvidenceContext,
    pub(super) report_path: &'a Path,
    pub(super) baseline_snapshot: &'a BrainRepoSnapshot,
    pub(super) config: &'a EngineConfig,
}

pub(super) struct SourceGraphStageOutput {
    pub(super) report: ProviderGraphMaterializationReport,
    pub(super) source_graph_snapshot: BrainRepoSnapshot,
    pub(super) linked_baseline: BrainRepoSnapshot,
    pub(super) source_graph_run_id: String,
}

pub(super) fn materialize_source_graph_stage(
    input: SourceGraphStageInput<'_>,
    mut report: ProviderGraphMaterializationReport,
) -> Result<Option<SourceGraphStageOutput>> {
    let SourceGraphStageInput {
        workspace_root,
        workspace_id,
        manifest,
        markdown,
        artifact_root,
        context,
        report_path,
        baseline_snapshot,
        config,
    } = input;
    let snapshot = baseline_snapshot;

    let source_graph_run_id = format!("provider-source-graph-{}", Uuid::now_v7());
    report.source_graph_run_id = Some(source_graph_run_id.clone());
    let source_chunk_plan = source_graph_chunk_batch_plan(manifest, markdown);
    let source_chunk_batches = source_chunk_plan.batches;
    report.chunk_discovered = source_chunk_plan.discovered_batch_count;
    report.chunk_processed = source_chunk_batches.len();
    report.chunk_skipped = source_chunk_plan.skipped_batch_count;
    report.chunk_total = source_chunk_batches.len();
    let source_chunk_jobs = source_chunk_batches
        .iter()
        .enumerate()
        .map(|(batch_index, batch)| {
            let run_id = format!("{source_graph_run_id}-chunk-{:04}", batch_index + 1);
            let chunk_ids = batch
                .iter()
                .map(|chunk| chunk.chunk_id.clone())
                .collect::<Vec<_>>();
            let evidence_refs = source_chunk_evidence_refs(batch);
            let prompt = build_source_chunk_graph_prompt(
                workspace_id,
                manifest,
                batch,
                &evidence_refs,
                snapshot,
                context,
            )?;
            Ok(SourceGraphChunkJob {
                batch_index,
                run_id,
                chunk_ids,
                evidence_refs,
                prompt,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    report.provider_run_ids.extend(
        source_chunk_jobs
            .iter()
            .map(|job| job.run_id.clone())
            .collect::<Vec<_>>(),
    );
    write_json_pretty(report_path, &report)?;

    let mut source_graph_snapshots = Vec::new();
    let mut stripped_source_of_relation_count = 0usize;
    for job_batch in source_chunk_jobs.chunks(SOURCE_GRAPH_CHUNK_PARALLELISM) {
        let mut provider_results = run_source_graph_chunk_provider_jobs(
            workspace_root,
            workspace_id,
            manifest,
            config,
            job_batch,
        );
        provider_results.sort_by_key(|result| result.batch_index);
        for provider_result in provider_results {
            let chunk_run_id = provider_result.run_id;
            let chunk_ids = provider_result.chunk_ids;
            let batch_evidence = provider_result.evidence_refs;
            match provider_result.response {
                Ok(response) => match parse_and_validate_source_graph_response(
                    &response,
                    workspace_id,
                    snapshot,
                    &manifest.source_id,
                    &batch_evidence,
                ) {
                    Ok((mut source_graph_snapshot, provider_source_of_relation_count)) => {
                        stripped_source_of_relation_count += provider_source_of_relation_count;
                        strip_source_of_relations(&mut source_graph_snapshot);
                        report.chunk_succeeded += 1;
                        report.stage_runs.push(ProviderGraphStageRunReport {
                            stage: "source_chunk_extract".into(),
                            run_id: chunk_run_id.clone(),
                            chunk_ids: chunk_ids.clone(),
                            status: "validated".into(),
                            retryable: false,
                            node_count: source_graph_snapshot.nodes.len(),
                            relation_count: source_graph_snapshot.relations.len(),
                            error_message: None,
                        });
                        write_provider_graph_run_validation_report(
                            workspace_root,
                            &chunk_run_id,
                            workspace_id,
                            &manifest.source_id,
                            "validated",
                            source_graph_snapshot.nodes.len(),
                            None,
                        )?;
                        write_json_pretty(
                            &artifact_root
                                .join("provider-graph-chunks")
                                .join(format!("{chunk_run_id}.json")),
                            &json!({
                                "runId": chunk_run_id,
                                "workspaceId": workspace_id,
                                "sourceId": manifest.source_id,
                                "stage": "source_chunk_extract",
                                "status": "validated",
                                "chunkIds": chunk_ids,
                                "nodeCount": source_graph_snapshot.nodes.len(),
                                "relationCount": source_graph_snapshot.relations.len(),
                                "snapshot": source_graph_snapshot.clone(),
                                "updatedAt": unix_timestamp_seconds(),
                            }),
                        )?;
                        write_graph_candidate_batch_artifact(
                            artifact_root,
                            &source_graph_run_id,
                            &chunk_run_id,
                            workspace_id,
                            &manifest.source_id,
                            &chunk_ids,
                            &source_graph_snapshot,
                        )?;
                        source_graph_snapshots.push(source_graph_snapshot);
                    }
                    Err(error) => {
                        report.chunk_failed += 1;
                        report.retryable = true;
                        report.failed_reason = Some("source_chunk_validation_failed".into());
                        report.stage_runs.push(ProviderGraphStageRunReport {
                            stage: "source_chunk_extract".into(),
                            run_id: chunk_run_id.clone(),
                            chunk_ids: chunk_ids.clone(),
                            status: "failed".into(),
                            retryable: true,
                            node_count: 0,
                            relation_count: 0,
                            error_message: Some(format!("{error:#}")),
                        });
                        write_provider_graph_run_validation_report(
                            workspace_root,
                            &chunk_run_id,
                            workspace_id,
                            &manifest.source_id,
                            "failed",
                            0,
                            Some(format!("{error:#}")),
                        )?;
                        write_source_chunk_run_artifact(
                            artifact_root,
                            &chunk_run_id,
                            workspace_id,
                            &manifest.source_id,
                            &chunk_ids,
                            "failed",
                            Some(format!("{error:#}")),
                        )?;
                    }
                },
                Err(error) => {
                    report.chunk_failed += 1;
                    report.retryable = true;
                    report.failed_reason = Some(provider_graph_failure_reason(&error).into());
                    report.stage_runs.push(ProviderGraphStageRunReport {
                        stage: "source_chunk_extract".into(),
                        run_id: chunk_run_id.clone(),
                        chunk_ids: chunk_ids.clone(),
                        status: "failed".into(),
                        retryable: true,
                        node_count: 0,
                        relation_count: 0,
                        error_message: Some(format!("{error:#}")),
                    });
                    write_source_chunk_run_artifact(
                        artifact_root,
                        &chunk_run_id,
                        workspace_id,
                        &manifest.source_id,
                        &chunk_ids,
                        "failed",
                        Some(format!("{error:#}")),
                    )?;
                }
            }
            report.progress = source_graph_progress(&report);
            report.updated_at = unix_timestamp_seconds();
            write_json_pretty(report_path, &report)?;
        }
    }

    if source_graph_snapshots.is_empty() {
        report.status = "failed_no_materialization".into();
        report.stage = "source_graph_failed".into();
        report.retryable = true;
        report.error_message = Some("no valid source graph chunk output to materialize".into());
        report.failed_reason = Some("no_valid_source_chunks".into());
        report.updated_at = unix_timestamp_seconds();
        write_json_pretty(report_path, &report)?;
        return Ok(None);
    }

    let raw_source_graph_snapshot = match merge_raw_source_graph_snapshots(
        workspace_id,
        snapshot,
        &manifest.source_id,
        source_graph_snapshots,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            report.status = "failed_no_materialization".into();
            report.stage = "source_graph_failed".into();
            report.retryable = true;
            report.error_message = Some(format!("{error:#}"));
            report.failed_reason = Some("raw_source_graph_merge_failed".into());
            report.updated_at = unix_timestamp_seconds();
            write_json_pretty(report_path, &report)?;
            return Ok(None);
        }
    };
    write_json_pretty(
        &artifact_root.join("provider-graph-source-raw-merged.json"),
        &json!({
            "runId": source_graph_run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "stage": "source_graph_raw_merge",
            "status": "validated",
            "nodeCount": raw_source_graph_snapshot.nodes.len(),
            "relationCount": raw_source_graph_snapshot.relations.len(),
            "snapshot": raw_source_graph_snapshot.clone(),
            "updatedAt": unix_timestamp_seconds(),
        }),
    )?;
    let (canonical_snapshot, compaction_report) = match compact_source_graph_snapshot(
        workspace_id,
        snapshot,
        &manifest.source_id,
        &raw_source_graph_snapshot,
        stripped_source_of_relation_count,
    ) {
        Ok(result) => result,
        Err(error) => {
            mark_source_graph_compaction_failed(
                &mut report,
                artifact_root,
                report_path,
                &raw_source_graph_snapshot,
                &error,
            )?;
            return Ok(None);
        }
    };
    let source_graph_snapshot = canonical_snapshot;

    report.source_graph_node_count = source_graph_snapshot.nodes.len();
    report.source_graph_relation_count = source_graph_snapshot.relations.len();
    report.raw_source_graph_node_count = compaction_report.raw_node_count;
    report.raw_source_graph_relation_count = compaction_report.raw_relation_count;
    report.canonical_source_graph_node_count = compaction_report.materialized_node_count;
    report.canonical_source_graph_relation_count = compaction_report.materialized_relation_count;
    report.pruned_source_graph_node_count = compaction_report.dropped_node_count;
    report.pruned_source_graph_relation_count = compaction_report.dropped_relation_count;
    report.compaction_status = Some("compacted".into());
    report.compaction_report_path = Some(
        artifact_root
            .join("provider-graph-compaction.json")
            .to_string_lossy()
            .into_owned(),
    );
    write_json_pretty(
        &artifact_root.join("provider-graph-compaction.json"),
        &compaction_report,
    )?;
    write_json_pretty(
        &artifact_root.join("provider-graph-source-canonical.json"),
        &json!({
            "runId": source_graph_run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "stage": "source_graph_canonical",
            "status": "validated",
            "nodeCount": source_graph_snapshot.nodes.len(),
            "relationCount": source_graph_snapshot.relations.len(),
            "snapshot": source_graph_snapshot.clone(),
            "updatedAt": unix_timestamp_seconds(),
        }),
    )?;
    write_json_pretty(
        &artifact_root.join("provider-graph-source-merged.json"),
        &json!({
            "runId": source_graph_run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "stage": "source_graph_canonical",
            "status": "validated",
            "chunkTotal": report.chunk_total,
            "chunkDiscovered": report.chunk_discovered,
            "chunkProcessed": report.chunk_processed,
            "chunkSkipped": report.chunk_skipped,
            "chunkSucceeded": report.chunk_succeeded,
            "chunkFailed": report.chunk_failed,
            "rawNodeCount": report.raw_source_graph_node_count,
            "rawRelationCount": report.raw_source_graph_relation_count,
            "nodeCount": source_graph_snapshot.nodes.len(),
            "relationCount": source_graph_snapshot.relations.len(),
            "snapshot": source_graph_snapshot.clone(),
            "updatedAt": unix_timestamp_seconds(),
        }),
    )?;

    let before_source_graph = capture_materialized_file_snapshot(workspace_root)?;
    let source_graph_event = source_graph_build_materialized_event(
        workspace_id,
        &source_graph_run_id,
        manifest,
        &source_graph_snapshot,
    )?;
    let mut source_materialization_input = snapshot.clone();
    source_materialization_input.events =
        merge_preserved_brain_events(vec![source_graph_event], &snapshot.events);
    write_materialized_brain_repo(workspace_root, &source_materialization_input)?;
    report.source_graph_materialized = true;
    report.stage = "source_graph_materialized".into();
    report.status = source_graph_materialized_status(&report).into();
    if report.chunk_skipped > 0 {
        report.failed_reason = Some("source_chunk_batch_limit".into());
    }
    update_materialized_counts(&mut report, &source_materialization_input);
    report.progress = 1.0;
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(report_path, &report)?;

    let after_source_graph = capture_materialized_file_snapshot(workspace_root)?;
    let source_graph_changed_files =
        changed_materialized_files(&before_source_graph, &after_source_graph);
    write_provider_graph_run_validation_report(
        workspace_root,
        &source_graph_run_id,
        workspace_id,
        &manifest.source_id,
        "materialized",
        source_graph_snapshot.nodes.len(),
        None,
    )?;
    write_graph_diff_artifact(
        workspace_root,
        &source_graph_run_id,
        workspace_id,
        &manifest.source_id,
        &source_graph_changed_files,
        &source_graph_snapshot,
    )?;

    let linked_baseline = read_materialized_brain_snapshot(workspace_root, workspace_id)
        .unwrap_or_else(|_| source_materialization_input.clone());
    update_materialized_counts(&mut report, &linked_baseline);
    report.stage = "workspace_linking".into();
    report.progress = 1.0;
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(report_path, &report)?;

    Ok(Some(SourceGraphStageOutput {
        report,
        source_graph_snapshot: source_materialization_input,
        linked_baseline,
        source_graph_run_id,
    }))
}
