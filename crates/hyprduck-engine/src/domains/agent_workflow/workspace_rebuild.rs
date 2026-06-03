use std::path::Path;

#[path = "workspace_rebuild/chunking.rs"]
mod workspace_rebuild_chunking;
#[path = "workspace_rebuild/compaction.rs"]
mod workspace_rebuild_compaction;
#[path = "workspace_rebuild/events.rs"]
mod workspace_rebuild_events;
#[path = "workspace_rebuild/fingerprint.rs"]
mod workspace_rebuild_fingerprint;
#[path = "workspace_rebuild/merge.rs"]
mod workspace_rebuild_merge;
#[path = "workspace_rebuild/provider_stage.rs"]
mod workspace_rebuild_provider_stage;
#[path = "workspace_rebuild/types.rs"]
mod workspace_rebuild_types;

#[cfg(test)]
use self::workspace_rebuild_chunking::source_graph_chunk_batches;
use self::workspace_rebuild_chunking::{
    source_chunk_evidence_refs, source_graph_chunk_batch_plan,
    write_graph_candidate_batch_artifact, write_source_chunk_run_artifact,
};
#[cfg(test)]
use self::workspace_rebuild_compaction::normalize_concept_label;
use self::workspace_rebuild_compaction::{
    compact_source_graph_snapshot, strip_source_of_relations,
};
use self::workspace_rebuild_events::{
    source_graph_build_materialized_event, workspace_linking_materialized_event,
    write_graph_diff_artifact,
};
use self::workspace_rebuild_fingerprint::provider_graph_input_fingerprint;
#[cfg(test)]
use self::workspace_rebuild_merge::merge_source_graph_snapshots;
use self::workspace_rebuild_merge::{
    merge_raw_source_graph_snapshots, parse_and_validate_source_graph_response,
};
use self::workspace_rebuild_provider_stage::{
    mark_source_graph_compaction_failed, mark_workspace_linking_failed,
    provider_graph_failure_reason, provider_graph_generation_disabled_for_process,
    provider_graph_report_is_reusable, provider_unavailable_reason, run_provider_graph_stage,
    run_source_graph_chunk_provider_jobs, source_graph_materialized_status, source_graph_progress,
    update_materialized_counts, SourceGraphChunkJob,
};
pub(crate) use self::workspace_rebuild_types::{
    GraphCandidateBatch, GraphCandidateNode, GraphCandidateRelation, SourceGraphCompactionReport,
};
use self::workspace_rebuild_types::{
    PROVIDER_GRAPH_PROMPT_VERSION, PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
    PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION, SOURCE_GRAPH_AUTO_BATCH_LIMIT,
    SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS, SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS,
    SOURCE_GRAPH_CHUNK_PARALLELISM, SOURCE_GRAPH_HARD_MAX_CONCEPTS,
    SOURCE_GRAPH_HARD_MAX_RELATIONS, SOURCE_GRAPH_MAX_CLAIMS, SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE,
    SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION, SOURCE_GRAPH_TARGET_CONCEPTS,
};
use super::artifacts::{
    provider_workspace_linking_response_schema, write_provider_graph_run_validation_report,
};
use super::prompt::{build_source_chunk_graph_prompt, build_workspace_linking_prompt};
use super::reports::{
    ProviderGraphMaterializationInputFingerprint, ProviderGraphMaterializationReport,
    ProviderGraphStageRunReport,
};
use super::response::{
    normalize_provider_workspace_linking_snapshot, parse_provider_workspace_rebuild_snapshot,
};
use super::validation::validate_provider_workspace_linking_snapshot;
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
                &snapshot,
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
    write_json_pretty(&report_path, &report)?;

    let mut source_graph_snapshots = Vec::new();
    let mut stripped_source_of_relation_count = 0usize;
    for job_batch in source_chunk_jobs.chunks(SOURCE_GRAPH_CHUNK_PARALLELISM) {
        let mut provider_results = run_source_graph_chunk_provider_jobs(
            workspace_root,
            workspace_id,
            manifest,
            &config,
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
                    &snapshot,
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
            write_json_pretty(&report_path, &report)?;
        }
    }

    if source_graph_snapshots.is_empty() {
        report.status = "failed_no_materialization".into();
        report.stage = "source_graph_failed".into();
        report.retryable = true;
        report.error_message = Some("no valid source graph chunk output to materialize".into());
        report.failed_reason = Some("no_valid_source_chunks".into());
        report.updated_at = unix_timestamp_seconds();
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let raw_source_graph_snapshot = match merge_raw_source_graph_snapshots(
        workspace_id,
        &snapshot,
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
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
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
        &snapshot,
        &manifest.source_id,
        &raw_source_graph_snapshot,
        stripped_source_of_relation_count,
    ) {
        Ok(result) => result,
        Err(error) => {
            mark_source_graph_compaction_failed(
                &mut report,
                artifact_root,
                &report_path,
                &raw_source_graph_snapshot,
                &error,
            )?;
            return Ok(report);
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
    write_json_pretty(&report_path, &report)?;

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
    write_json_pretty(&report_path, &report)?;
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
        &config,
        &workspace_linking_run_id,
        &workspace_linking_prompt,
        provider_workspace_linking_response_schema(),
    ) {
        Ok(response) => response,
        Err(error) => {
            mark_workspace_linking_failed(&mut report, &error);
            write_json_pretty(&report_path, &report)?;
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
                write_json_pretty(&report_path, &report)?;
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
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let before_linking = capture_materialized_file_snapshot(workspace_root)?;
    let workspace_linking_event = workspace_linking_materialized_event(
        workspace_id,
        &workspace_linking_run_id,
        manifest,
        &linking_snapshot,
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
    write_json_pretty(&report_path, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests;
