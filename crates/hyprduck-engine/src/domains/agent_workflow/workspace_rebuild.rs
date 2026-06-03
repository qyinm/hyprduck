use std::path::Path;
use std::time::Duration;

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
use self::workspace_rebuild_fingerprint::provider_graph_input_fingerprint;
#[cfg(test)]
use self::workspace_rebuild_merge::merge_source_graph_snapshots;
use self::workspace_rebuild_merge::{
    merge_raw_source_graph_snapshots, parse_and_validate_source_graph_response,
};
use super::artifacts::{
    provider_workspace_linking_response_schema, provider_workspace_rebuild_response_schema,
    write_provider_graph_run_artifacts, write_provider_graph_run_validation_report,
    ProviderGraphRunArtifact,
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
use crate::provider::{EngineConfig, EngineConfigStore, ProviderKind};
use crate::*;

const PROVIDER_GRAPH_PROMPT_VERSION: u32 = 2;
const PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION: u32 = 1;
const PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION: u32 = 1;
const SOURCE_GRAPH_CHUNK_BATCH_MAX_CHARS: usize = 80_000;
const SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS: usize = 40;
const SOURCE_GRAPH_AUTO_BATCH_LIMIT: usize = 8;
const SOURCE_GRAPH_CHUNK_PARALLELISM: usize = 4;
const SOURCE_GRAPH_TARGET_CONCEPTS: usize = 18;
const SOURCE_GRAPH_HARD_MAX_CONCEPTS: usize = 32;
const SOURCE_GRAPH_HARD_MAX_RELATIONS: usize = 48;
const SOURCE_GRAPH_MAX_CLAIMS: usize = 12;
const SOURCE_GRAPH_MAX_EVIDENCE_PER_NODE: usize = 8;
const SOURCE_GRAPH_MAX_EVIDENCE_PER_RELATION: usize = 6;

#[derive(Debug, Clone)]
struct SourceGraphChunkJob {
    batch_index: usize,
    run_id: String,
    chunk_ids: Vec<String>,
    evidence_refs: Vec<EvidenceRef>,
    prompt: String,
}

struct SourceGraphChunkProviderResult {
    batch_index: usize,
    run_id: String,
    chunk_ids: Vec<String>,
    evidence_refs: Vec<EvidenceRef>,
    response: Result<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateBatch {
    pub(crate) run_id: String,
    pub(crate) chunk_run_id: String,
    pub(crate) source_id: String,
    #[serde(default)]
    pub(crate) chunk_ids: Vec<String>,
    #[serde(default)]
    pub(crate) nodes: Vec<GraphCandidateNode>,
    #[serde(default)]
    pub(crate) relations: Vec<GraphCandidateRelation>,
    #[serde(default)]
    pub(crate) claims: Vec<ClaimRecord>,
    #[serde(default)]
    pub(crate) raw_response_ref: Option<String>,
    pub(crate) created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateNode {
    pub(crate) raw_node_id: String,
    pub(crate) label: String,
    pub(crate) kind: BrainNodeKind,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) page_refs: Vec<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
    #[serde(default)]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphCandidateRelation {
    pub(crate) raw_relation_id: String,
    pub(crate) source_raw_node_id: String,
    pub(crate) target_raw_node_id: String,
    pub(crate) kind: BrainRelationKind,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceGraphCompactionReport {
    pub(crate) raw_node_count: usize,
    pub(crate) raw_relation_count: usize,
    pub(crate) deduped_node_count: usize,
    pub(crate) deduped_relation_count: usize,
    pub(crate) materialized_node_count: usize,
    pub(crate) materialized_relation_count: usize,
    pub(crate) dropped_node_count: usize,
    pub(crate) dropped_relation_count: usize,
    #[serde(default)]
    pub(crate) drop_reasons: BTreeMap<String, usize>,
    #[serde(default)]
    pub(crate) candidate_to_canonical_map: BTreeMap<String, Option<String>>,
}

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

fn source_graph_progress(report: &ProviderGraphMaterializationReport) -> f32 {
    if report.chunk_total == 0 {
        return 0.0;
    }
    let completed = report.chunk_succeeded + report.chunk_failed;
    (completed as f32 / report.chunk_total as f32).clamp(0.0, 1.0)
}

fn provider_graph_failure_reason(error: &anyhow::Error) -> &'static str {
    let message = format!("{error:#}");
    if message.contains("provider_timeout") {
        "provider_timeout"
    } else if message.contains("provider_unavailable") {
        "provider_unavailable"
    } else {
        "provider_error"
    }
}

fn source_graph_materialized_status(report: &ProviderGraphMaterializationReport) -> &'static str {
    match (report.chunk_failed > 0, report.chunk_skipped > 0) {
        (true, true) => "source_partial_with_failures_and_skipped_chunks",
        (true, false) => "source_partial_with_failures",
        (false, true) => "source_partial_with_skipped_chunks",
        (false, false) => "source_graph_materialized",
    }
}

fn provider_unavailable_reason(config: &EngineConfig) -> &'static str {
    match &config.provider {
        ProviderKind::Unknown(_) => "unsupported_provider",
        ProviderKind::OpenRouter if config.api_key.trim().is_empty() => "provider_config",
        _ => "provider_config",
    }
}

fn update_materialized_counts(
    report: &mut ProviderGraphMaterializationReport,
    snapshot: &BrainRepoSnapshot,
) {
    report.materialized_node_count = snapshot.nodes.len();
    report.materialized_relation_count = snapshot.relations.len();
    report.materialized_claim_count = snapshot.claims.len();
    report.materialized_memory_count = snapshot.memories.len();
}

fn provider_graph_report_is_reusable(
    report: &ProviderGraphMaterializationReport,
    manifest: &SourceArtifactManifest,
    input_fingerprint: &ProviderGraphMaterializationInputFingerprint,
) -> bool {
    report.status == "linked"
        && report.source_id == manifest.source_id
        && report.input_fingerprint.as_ref() == Some(input_fingerprint)
}

fn mark_workspace_linking_failed(
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

fn mark_source_graph_compaction_failed(
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

fn run_provider_graph_stage(
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

fn run_source_graph_chunk_provider_jobs(
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

fn write_graph_diff_artifact(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    source_id: &str,
    changed_files: &[String],
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("graph-diff.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": source_id,
            "changedFiles": changed_files,
            "nodeCount": snapshot.nodes.len(),
            "relationCount": snapshot.relations.len(),
            "claimCount": snapshot.claims.len(),
            "memoryCount": snapshot.memories.len(),
            "updatedAt": snapshot.generated_at,
        }),
    )
}

fn source_graph_build_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    provider_graph_materialized_event(
        workspace_id,
        run_id,
        manifest,
        snapshot,
        "source_graph_build",
        "source-graph-build",
        "provider_source_graph_build",
    )
}

fn workspace_linking_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    let endpoint_node_ids = snapshot
        .relations
        .iter()
        .flat_map(|relation| {
            [
                relation.source_node_id.clone(),
                relation.target_node_id.clone(),
            ]
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(BrainEvent {
        event_id: format!("evt-{run_id}"),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("workspace_linking".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: format!("{PROVIDER_GRAPH_AGENT_ID}:workspace-linking"),
        },
        source_refs: snapshot
            .relations
            .iter()
            .flat_map(|relation| {
                snapshot
                    .nodes
                    .iter()
                    .filter(move |node| {
                        node.node_id == relation.source_node_id
                            || node.node_id == relation.target_node_id
                    })
                    .flat_map(|node| node.source_ids.clone())
            })
            .chain(std::iter::once(manifest.source_id.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_markdown_refs: vec![manifest.markdown_path.clone()],
        node_refs: endpoint_node_ids,
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
        target_node_ids: Vec::new(),
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
        evidence_refs: Vec::new(),
        payload_json: materialized_graph_event_payload_json(
            snapshot.generated_at,
            &[],
            &[],
            &snapshot.relations,
            &[],
            &snapshot.memories,
            &snapshot.wiki_pages,
            &[],
            &snapshot.claims,
            &[],
        )?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            caused_by_event_ids: Vec::new(),
            snapshot_id: Some(format!("snapshot-{run_id}")),
            previous_snapshot_id: None,
            materialized_version: Some(snapshot.generated_at),
            schema_version: 1,
        },
        confidence: Some("provider_workspace_linking".into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    })
}

fn provider_graph_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
    operation_type: &str,
    actor_suffix: &str,
    confidence: &str,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{run_id}"),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some(operation_type.into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: format!("{PROVIDER_GRAPH_AGENT_ID}:{actor_suffix}"),
        },
        source_refs: snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: vec![manifest.markdown_path.clone()],
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
        payload_json: materialized_graph_event_payload_json(
            snapshot.generated_at,
            &snapshot.sources,
            &snapshot.nodes,
            &snapshot.relations,
            &snapshot.evidence,
            &snapshot.memories,
            &snapshot.wiki_pages,
            &snapshot.entities,
            &snapshot.claims,
            &snapshot.extractions,
        )?,
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest.source_id.clone()],
            caused_by_event_ids: Vec::new(),
            snapshot_id: Some(format!("snapshot-{run_id}")),
            previous_snapshot_id: None,
            materialized_version: Some(snapshot.generated_at),
            schema_version: 1,
        },
        confidence: Some(confidence.into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    })
}

fn provider_graph_generation_disabled_for_process() -> bool {
    std::env::var_os("HYPRDUCK_DISABLE_PROVIDER_GRAPH").is_some()
        || (cfg!(test) && std::env::var_os("HYPRDUCK_TEST_ENABLE_PROVIDER_GRAPH").is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprduck_engine_types::{SourceFormat, SourceStatus};
    use std::time::{Duration, Instant};

    fn test_fingerprint(
        source_id: &str,
        markdown_hash: &str,
    ) -> ProviderGraphMaterializationInputFingerprint {
        ProviderGraphMaterializationInputFingerprint {
            workspace_id: "default".into(),
            source_id: source_id.into(),
            manifest_updated_at: 1,
            markdown_hash: markdown_hash.into(),
            provider: "open_router".into(),
            model: "openai/gpt-4.1-mini".into(),
            source_graph_schema_version: PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
            workspace_linking_schema_version: PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION,
            prompt_version: PROVIDER_GRAPH_PROMPT_VERSION,
            baseline_snapshot_id: Some("snapshot-a".into()),
            baseline_event_id: Some("evt-a".into()),
            baseline_materialized_at: Some(1),
        }
    }

    fn test_manifest(source_id: &str) -> SourceArtifactManifest {
        SourceArtifactManifest {
            workspace_id: "default".into(),
            source_id: source_id.into(),
            original_path: "/tmp/source.pdf".into(),
            source_path: "/tmp/source.pdf".into(),
            markdown_path: "/tmp/source.md".into(),
            artifact_root: "/tmp/artifacts/source".into(),
            manifest_path: "/tmp/artifacts/source/source-manifest.json".into(),
            format: DocumentFormat::Pdf,
            output_name: "source".into(),
            status: IngestStatus::Ingested,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            pages: Vec::new(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn test_report(
        source_id: &str,
        fingerprint: Option<ProviderGraphMaterializationInputFingerprint>,
    ) -> ProviderGraphMaterializationReport {
        ProviderGraphMaterializationReport {
            status: "linked".into(),
            provider: "open_router".into(),
            model: "openai/gpt-4.1-mini".into(),
            source_id: source_id.into(),
            input_fingerprint: fingerprint,
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
            source_graph_materialized: true,
            workspace_linking_materialized: true,
            retryable: false,
            stage: "linked".into(),
            progress: 1.0,
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
            updated_at: 1,
        }
    }

    fn test_baseline(source_id: &str, evidence_count: usize) -> BrainRepoSnapshot {
        let mut snapshot = empty_replayed_brain_snapshot("default");
        snapshot.sources.push(SourceRecord {
            source_id: source_id.into(),
            workspace_id: "default".into(),
            original_path: "/tmp/source.pdf".into(),
            source_path: "/tmp/source.pdf".into(),
            markdown_path: "/tmp/source.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: evidence_count.max(1),
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        });
        snapshot.evidence = (0..evidence_count)
            .map(|index| EvidenceRef {
                id: format!("ev-{source_id}-{index}"),
                page_label: format!("{}", index + 1),
                page_index: Some(index),
                snippet: format!("Evidence {index}"),
                source_path: Some("/tmp/source.pdf".into()),
                source_id: Some(source_id.into()),
                markdown_path: Some("/tmp/source.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            })
            .collect();
        snapshot.nodes.push(BrainNodeRecord {
            node_id: format!("source:{source_id}"),
            kind: BrainNodeKind::Source,
            label: "source.pdf".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: snapshot
                .evidence
                .iter()
                .map(|evidence| evidence.id.clone())
                .collect(),
            source_ids: vec![source_id.into()],
            confidence: Some(1.0),
            updated_at: 1,
        });
        snapshot
    }

    fn candidate_node(node_id: &str, label: &str, evidence_id: Option<String>) -> BrainNodeRecord {
        BrainNodeRecord {
            node_id: node_id.into(),
            kind: BrainNodeKind::Concept,
            label: label.into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: evidence_id.into_iter().collect(),
            source_ids: vec!["source-alpha".into()],
            confidence: Some(0.8),
            updated_at: 1,
        }
    }

    fn source_raw_snapshot(source_id: &str, evidence_count: usize) -> BrainRepoSnapshot {
        test_baseline(source_id, evidence_count)
    }

    fn candidate_relation(
        relation_id: &str,
        source_node_id: &str,
        target_node_id: &str,
        evidence_id: &str,
    ) -> BrainRelationRecord {
        BrainRelationRecord {
            relation_id: relation_id.into(),
            kind: BrainRelationKind::RelatedTo,
            source_node_id: source_node_id.into(),
            target_node_id: target_node_id.into(),
            label: "related_to".into(),
            evidence_ids: vec![evidence_id.into()],
            confidence: Some(0.7),
            updated_at: 1,
        }
    }

    fn synthetic_candidate_chunk_snapshot(
        source_id: &str,
        batch_index: usize,
        nodes_per_batch: usize,
    ) -> BrainRepoSnapshot {
        let mut snapshot = source_raw_snapshot(source_id, 0);
        snapshot.evidence = (0..nodes_per_batch)
            .map(|node_index| EvidenceRef {
                id: format!("ev-{source_id}-{batch_index:02}-{node_index:02}"),
                page_label: format!("Synthetic Page {}", batch_index + 1),
                page_index: Some(batch_index),
                snippet: format!(
                    "Synthetic evidence for batch {batch_index} node {node_index} keeps graph extraction grounded."
                ),
                source_path: Some("/tmp/source.pdf".into()),
                source_id: Some(source_id.into()),
                markdown_path: Some("/tmp/source.md".into()),
                image_path: None,
                provenance: Some("synthetic eight-batch fixture".into()),
            })
            .collect();
        for node_index in 0..nodes_per_batch {
            let global_index = batch_index * nodes_per_batch + node_index;
            let mut node = candidate_node(
                &format!("node-{batch_index:02}-{node_index:02}"),
                if global_index % 45 == 0 {
                    "Shared Anchor"
                } else if global_index % 37 == 0 {
                    "Evidence Carrier"
                } else {
                    "Important Synthetic Concept"
                },
                (node_index != nodes_per_batch - 1)
                    .then(|| format!("ev-{source_id}-{batch_index:02}-{node_index:02}")),
            );
            if global_index % 45 == 0 {
                node.aliases.push("shared anchors".into());
            } else {
                node.label = format!("{} {global_index:03}", node.label);
            }
            snapshot.nodes.push(node);
        }
        for relation_index in 0..nodes_per_batch {
            let source_node_index = relation_index;
            let target_node_index = (relation_index + 1) % nodes_per_batch;
            snapshot.relations.push(candidate_relation(
                &format!("rel-{batch_index:02}-{relation_index:02}"),
                &format!("node-{batch_index:02}-{source_node_index:02}"),
                &format!("node-{batch_index:02}-{target_node_index:02}"),
                &format!("ev-{source_id}-{batch_index:02}-{relation_index:02}"),
            ));
        }
        snapshot
    }

    fn synthetic_eight_batch_success_snapshots() -> Vec<BrainRepoSnapshot> {
        (0..8)
            .map(|batch_index| synthetic_candidate_chunk_snapshot("source-alpha", batch_index, 19))
            .collect()
    }

    fn synthetic_six_batch_success_snapshots() -> Vec<BrainRepoSnapshot> {
        (0..6)
            .map(|batch_index| synthetic_candidate_chunk_snapshot("source-alpha", batch_index, 25))
            .collect()
    }

    fn test_import_context(source_id: &str) -> ImportEvidenceContext {
        ImportEvidenceContext {
            schema_version: 1,
            workspace_id: "default".into(),
            trigger_source_id: source_id.into(),
            new_source: crate::domains::retrieval::import_context::NewSourceContext {
                source_id: source_id.into(),
                source_title: "source.pdf".into(),
                source_path: "/tmp/source.pdf".into(),
                markdown_path: "/tmp/source.md".into(),
                chunks: Vec::new(),
            },
            retrieval_queries: Vec::new(),
            retrieved_source_evidence: Vec::new(),
            workspace_source_outline: Vec::new(),
            existing_graph_context:
                crate::domains::retrieval::import_context::ExistingGraphContext {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    claims: Vec::new(),
                },
        }
    }

    #[test]
    fn linked_report_reuse_requires_matching_input_fingerprint() {
        let manifest = test_manifest("source-alpha");
        let fingerprint = test_fingerprint("source-alpha", "hash-a");
        let matching_report = test_report("source-alpha", Some(fingerprint.clone()));

        assert!(provider_graph_report_is_reusable(
            &matching_report,
            &manifest,
            &fingerprint
        ));

        let legacy_report_without_fingerprint = test_report("source-alpha", None);
        assert!(!provider_graph_report_is_reusable(
            &legacy_report_without_fingerprint,
            &manifest,
            &fingerprint
        ));

        let changed_markdown = test_fingerprint("source-alpha", "hash-b");
        assert!(!provider_graph_report_is_reusable(
            &matching_report,
            &manifest,
            &changed_markdown
        ));
    }

    #[test]
    fn provider_unavailable_reason_uses_provider_taxonomy() {
        let missing_openrouter = EngineConfig {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: String::new(),
            base_url: None,
            prompt_template: "General".into(),
        };
        let unknown_provider = EngineConfig {
            provider: ProviderKind::Unknown("legacy_ai".into()),
            model_id: "legacy-model".into(),
            api_key: "legacy-key".into(),
            base_url: None,
            prompt_template: "General".into(),
        };

        assert_eq!(
            provider_unavailable_reason(&missing_openrouter),
            "provider_config"
        );
        assert_eq!(
            provider_unavailable_reason(&unknown_provider),
            "unsupported_provider"
        );
    }

    #[test]
    fn source_graph_batches_split_markdown_into_bounded_runs() {
        let manifest = test_manifest("source-alpha");
        let markdown = (0..20)
            .map(|index| format!("## Heading {index}\n{}", "alpha ".repeat(2_000)))
            .collect::<Vec<_>>()
            .join("\n");

        let batches = source_graph_chunk_batches(&manifest, &markdown);

        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .all(|batch| batch.len() <= SOURCE_GRAPH_CHUNK_BATCH_MAX_CHUNKS));
        assert!(batches
            .iter()
            .flatten()
            .all(|chunk| chunk.source_id == "source-alpha"));
    }

    #[test]
    fn source_graph_batch_plan_reports_skipped_batches() {
        let manifest = test_manifest("source-alpha");
        let markdown = (0..120)
            .map(|index| format!("## Heading {index}\n{}", "alpha ".repeat(2_000)))
            .collect::<Vec<_>>()
            .join("\n");

        let plan = source_graph_chunk_batch_plan(&manifest, &markdown);

        assert!(plan.discovered_batch_count > SOURCE_GRAPH_AUTO_BATCH_LIMIT);
        assert_eq!(plan.batches.len(), SOURCE_GRAPH_AUTO_BATCH_LIMIT);
        assert_eq!(
            plan.skipped_batch_count,
            plan.discovered_batch_count - SOURCE_GRAPH_AUTO_BATCH_LIMIT
        );
    }

    #[test]
    fn source_graph_prompt_contains_candidate_caps_and_source_of_prohibition() {
        let manifest = test_manifest("source-alpha");
        let baseline = test_baseline("source-alpha", 1);
        let chunks =
            source_graph_chunk_batches(&manifest, "## Durable Signal\nstable source evidence");
        let prompt = build_source_chunk_graph_prompt(
            "default",
            &manifest,
            chunks.first().expect("chunk batch"),
            &source_chunk_evidence_refs(chunks.first().expect("chunk batch")),
            &baseline,
            &test_import_context("source-alpha"),
        )
        .expect("prompt");

        assert!(prompt.contains("Return at most 8 non-source concept/topic nodes"));
        assert!(prompt.contains("Return at most 10 non-source relations"));
        assert!(prompt.contains("Do not emit source_of edges"));
        assert!(prompt.contains("Do not perform exhaustive term extraction"));
        assert!(prompt.contains("retrieved:source-alpha:chunk-source-alpha"));
        assert!(!prompt.contains("ev-source-alpha-0"));
    }

    #[test]
    fn duplicate_chunk_nodes_merge_into_one_canonical_node() {
        let baseline = test_baseline("source-alpha", 3);
        let mut raw = source_raw_snapshot("source-alpha", 3);
        raw.nodes.push(candidate_node(
            "node-a",
            "Durable Signal",
            Some("ev-source-alpha-0".into()),
        ));
        let mut duplicate = candidate_node(
            "node-b",
            "concept:durable-signals",
            Some("ev-source-alpha-1".into()),
        );
        duplicate.aliases.push("durable signal".into());
        raw.nodes.push(duplicate);

        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        let concept_nodes = canonical
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .collect::<Vec<_>>();
        assert_eq!(concept_nodes.len(), 1);
        assert_eq!(
            report.candidate_to_canonical_map["node-a"],
            report.candidate_to_canonical_map["node-b"]
        );
        let source_edges = canonical
            .relations
            .iter()
            .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
            .count();
        assert_eq!(source_edges, 1);
    }

    #[test]
    fn raw_candidates_are_capped_before_materialization() {
        let baseline = test_baseline("source-alpha", 160);
        let mut raw = source_raw_snapshot("source-alpha", 160);
        for index in 0..150 {
            raw.nodes.push(candidate_node(
                &format!("node-{index}"),
                &format!("Important Concept {index}"),
                Some(format!("ev-source-alpha-{index}")),
            ));
        }

        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        assert!(canonical.nodes.len() <= SOURCE_GRAPH_TARGET_CONCEPTS + 1);
        assert!(canonical.nodes.len() <= SOURCE_GRAPH_HARD_MAX_CONCEPTS + 1);
        assert!(canonical.relations.len() <= SOURCE_GRAPH_HARD_MAX_RELATIONS);
        assert_eq!(report.raw_node_count, 151);
        assert!(report.dropped_node_count >= 132);
        assert!(report
            .drop_reasons
            .get("capped_out")
            .is_some_and(|count| *count >= 132));
    }

    #[test]
    fn relation_dedupe_remaps_to_canonical_endpoints() {
        let baseline = test_baseline("source-alpha", 8);
        let mut raw = source_raw_snapshot("source-alpha", 8);
        raw.nodes.push(candidate_node(
            "node-alpha",
            "Alpha Concept",
            Some("ev-source-alpha-0".into()),
        ));
        raw.nodes.push(candidate_node(
            "node-beta",
            "Beta Concept",
            Some("ev-source-alpha-1".into()),
        ));
        for index in 0..5 {
            raw.relations.push(BrainRelationRecord {
                relation_id: format!("rel-{index}"),
                kind: BrainRelationKind::DependsOn,
                source_node_id: "node-alpha".into(),
                target_node_id: "node-beta".into(),
                label: "depends_on".into(),
                evidence_ids: vec![format!("ev-source-alpha-{}", index + 2)],
                confidence: Some(0.8),
                updated_at: 1,
            });
        }

        let (canonical, _report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        let depends_on = canonical
            .relations
            .iter()
            .filter(|relation| relation.kind == BrainRelationKind::DependsOn)
            .collect::<Vec<_>>();
        assert_eq!(depends_on.len(), 1);
        assert_eq!(depends_on[0].evidence_ids.len(), 5);
    }

    #[test]
    fn shared_alias_and_evidence_merge_canonical_nodes() {
        let baseline = test_baseline("source-alpha", 4);
        let mut raw = source_raw_snapshot("source-alpha", 4);
        let mut alias_node = candidate_node(
            "node-alias-a",
            "Minimum Spanning Tree",
            Some("ev-source-alpha-0".into()),
        );
        alias_node.aliases.push("MST".into());
        raw.nodes.push(alias_node);
        let mut same_alias = candidate_node(
            "node-alias-b",
            "Spanning Tree Optimization",
            Some("ev-source-alpha-1".into()),
        );
        same_alias.aliases.push("MST".into());
        raw.nodes.push(same_alias);
        raw.nodes.push(candidate_node(
            "node-evidence-a",
            "Cut Property",
            Some("ev-source-alpha-2".into()),
        ));
        raw.nodes.push(candidate_node(
            "node-evidence-b",
            "Safe Edge Property",
            Some("ev-source-alpha-2".into()),
        ));

        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        let concept_count = canonical
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .count();
        assert_eq!(concept_count, 2);
        assert_eq!(
            report.candidate_to_canonical_map["node-alias-a"],
            report.candidate_to_canonical_map["node-alias-b"]
        );
        assert_eq!(
            report.candidate_to_canonical_map["node-evidence-a"],
            report.candidate_to_canonical_map["node-evidence-b"]
        );
    }

    #[test]
    fn source_of_relations_are_stripped_from_raw_and_accounted() {
        let baseline = test_baseline("source-alpha", 2);
        let mut raw = source_raw_snapshot("source-alpha", 2);
        raw.nodes.push(candidate_node(
            "node-alpha",
            "Alpha Concept",
            Some("ev-source-alpha-0".into()),
        ));
        raw.relations.push(BrainRelationRecord {
            relation_id: "provider-source-of".into(),
            kind: BrainRelationKind::SourceOf,
            source_node_id: "source:source-alpha".into(),
            target_node_id: "node-alpha".into(),
            label: "source_of".into(),
            evidence_ids: vec!["ev-source-alpha-0".into()],
            confidence: Some(1.0),
            updated_at: 1,
        });
        let stripped = strip_source_of_relations(&mut raw);

        assert_eq!(stripped, 1);
        assert!(raw
            .relations
            .iter()
            .all(|relation| relation.kind != BrainRelationKind::SourceOf));
        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, stripped)
                .expect("compact");
        assert_eq!(report.drop_reasons.get("provider_source_of"), Some(&1));
        assert_eq!(report.raw_relation_count, 0);
        assert_eq!(report.dropped_relation_count, 0);
        assert_eq!(
            canonical
                .relations
                .iter()
                .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
                .count(),
            1
        );
    }

    #[test]
    fn merged_raw_snapshot_has_no_source_of_but_canonical_does() {
        let baseline = test_baseline("source-alpha", 2);
        let mut chunk_snapshot = source_raw_snapshot("source-alpha", 2);
        chunk_snapshot.nodes.push(candidate_node(
            "node-alpha",
            "Alpha Concept",
            Some("ev-source-alpha-0".into()),
        ));
        chunk_snapshot.relations.push(BrainRelationRecord {
            relation_id: "provider-source-of".into(),
            kind: BrainRelationKind::SourceOf,
            source_node_id: "source:source-alpha".into(),
            target_node_id: "node-alpha".into(),
            label: "source_of".into(),
            evidence_ids: vec!["ev-source-alpha-0".into()],
            confidence: Some(1.0),
            updated_at: 1,
        });
        let stripped = strip_source_of_relations(&mut chunk_snapshot);

        let result = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            vec![chunk_snapshot],
            stripped,
        )
        .expect("merge");

        assert!(result
            .raw_snapshot
            .relations
            .iter()
            .all(|relation| relation.kind != BrainRelationKind::SourceOf));
        assert_eq!(
            result
                .canonical_snapshot
                .relations
                .iter()
                .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
                .count(),
            1
        );
        assert_eq!(
            result.report.drop_reasons.get("provider_source_of"),
            Some(&1)
        );
        assert_eq!(result.report.raw_relation_count, 0);
        assert_eq!(result.report.dropped_relation_count, 0);
    }

    #[test]
    fn engine_generated_source_edges_are_not_counted_as_provider_source_of() {
        let baseline = test_baseline("source-alpha", 1);
        let raw = json!({
            "materializedGraph": {
                "nodes": [
                    {
                        "nodeId": "node-alpha",
                        "kind": "concept",
                        "label": "Alpha Concept",
                        "scope": "project",
                        "aliases": [],
                        "evidenceIds": ["ev-source-alpha-0"],
                        "sourceIds": ["source-alpha"],
                        "confidence": 0.8,
                        "updatedAt": 1
                    }
                ],
                "edges": [],
                "claims": [],
                "memories": [],
                "wikiPages": [],
                "entities": [],
                "extractions": []
            }
        });
        let (mut chunk_snapshot, provider_source_of_count) =
            parse_and_validate_source_graph_response(
                &raw.to_string(),
                "default",
                &baseline,
                "source-alpha",
                &baseline.evidence,
            )
            .expect("parse and validate source graph");

        assert_eq!(provider_source_of_count, 0);
        assert_eq!(
            chunk_snapshot
                .relations
                .iter()
                .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
                .count(),
            1
        );
        assert_eq!(strip_source_of_relations(&mut chunk_snapshot), 1);
        let result = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            vec![chunk_snapshot],
            provider_source_of_count,
        )
        .expect("merge");

        assert!(!result
            .report
            .drop_reasons
            .contains_key("provider_source_of"));
        assert_eq!(result.report.raw_relation_count, 0);
        assert_eq!(result.report.dropped_relation_count, 0);
        assert_eq!(
            result
                .canonical_snapshot
                .relations
                .iter()
                .filter(|relation| relation.kind == BrainRelationKind::SourceOf)
                .count(),
            1
        );
    }

    #[test]
    fn provider_source_edges_are_reported_without_inflating_raw_relation_count() {
        let baseline = test_baseline("source-alpha", 1);
        let raw = json!({
            "materializedGraph": {
                "nodes": [
                    {
                        "nodeId": "node-alpha",
                        "kind": "concept",
                        "label": "Alpha Concept",
                        "scope": "project",
                        "aliases": [],
                        "evidenceIds": ["ev-source-alpha-0"],
                        "sourceIds": ["source-alpha"],
                        "confidence": 0.8,
                        "updatedAt": 1
                    }
                ],
                "edges": [
                    {
                        "relationId": "provider-source-of",
                        "kind": "source_of",
                        "sourceNodeId": "source:source-alpha",
                        "targetNodeId": "node-alpha",
                        "label": "source_of",
                        "evidenceIds": ["ev-source-alpha-0"],
                        "confidence": 1.0,
                        "updatedAt": 1
                    }
                ],
                "claims": [],
                "memories": [],
                "wikiPages": [],
                "entities": [],
                "extractions": []
            }
        });
        let (mut chunk_snapshot, provider_source_of_count) =
            parse_and_validate_source_graph_response(
                &raw.to_string(),
                "default",
                &baseline,
                "source-alpha",
                &baseline.evidence,
            )
            .expect("parse and validate source graph");

        assert_eq!(provider_source_of_count, 1);
        assert_eq!(strip_source_of_relations(&mut chunk_snapshot), 1);
        let result = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            vec![chunk_snapshot],
            provider_source_of_count,
        )
        .expect("merge");

        assert_eq!(
            result.report.drop_reasons.get("provider_source_of"),
            Some(&1)
        );
        assert_eq!(result.report.raw_relation_count, 0);
        assert_eq!(result.report.dropped_relation_count, 0);
    }

    #[test]
    fn merged_raw_snapshot_preserves_chunk_evidence_refs() {
        let baseline = test_baseline("source-alpha", 1);
        let mut chunk_snapshot = source_raw_snapshot("source-alpha", 0);
        chunk_snapshot.evidence = vec![EvidenceRef {
            id: "retrieved:source-alpha:chunk-a".into(),
            page_label: "Lines 1-3".into(),
            page_index: None,
            snippet: "Alpha evidence".into(),
            source_path: Some("/tmp/source.pdf".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/source.md".into()),
            image_path: None,
            provenance: Some("chunk".into()),
        }];
        chunk_snapshot.nodes.push(candidate_node(
            "node-alpha",
            "Alpha Concept",
            Some("retrieved:source-alpha:chunk-a".into()),
        ));

        let result = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            vec![chunk_snapshot],
            0,
        )
        .expect("merge");

        assert!(result
            .raw_snapshot
            .evidence
            .iter()
            .any(|evidence| evidence.id == "retrieved:source-alpha:chunk-a"));
        assert!(result
            .canonical_snapshot
            .evidence
            .iter()
            .any(|evidence| evidence.id == "retrieved:source-alpha:chunk-a"));
        assert!(result.canonical_snapshot.nodes.iter().any(|node| {
            node.evidence_ids
                .iter()
                .any(|evidence_id| evidence_id == "retrieved:source-alpha:chunk-a")
        }));
    }

    #[test]
    fn chunk_level_evidence_overlap_does_not_merge_distinct_labels() {
        let baseline = test_baseline("source-alpha", 1);
        let mut raw = source_raw_snapshot("source-alpha", 0);
        raw.evidence = vec![EvidenceRef {
            id: "retrieved:source-alpha:chunk-a".into(),
            page_label: "Lines 1-3".into(),
            page_index: None,
            snippet: "Alpha and Beta evidence".into(),
            source_path: Some("/tmp/source.pdf".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/source.md".into()),
            image_path: None,
            provenance: Some("chunk".into()),
        }];
        raw.nodes.push(candidate_node(
            "node-alpha",
            "Alpha Concept",
            Some("retrieved:source-alpha:chunk-a".into()),
        ));
        raw.nodes.push(candidate_node(
            "node-beta",
            "Beta Concept",
            Some("retrieved:source-alpha:chunk-a".into()),
        ));

        let (canonical, _report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        let concept_count = canonical
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .count();
        assert_eq!(concept_count, 2);
    }

    #[test]
    fn raw_relations_are_capped_before_materialization() {
        let baseline = test_baseline("source-alpha", 200);
        let mut raw = source_raw_snapshot("source-alpha", 200);
        for index in 0..SOURCE_GRAPH_TARGET_CONCEPTS {
            raw.nodes.push(candidate_node(
                &format!("node-{index}"),
                &format!("Concept {index}"),
                Some(format!("ev-source-alpha-{index}")),
            ));
        }
        for index in 0..150 {
            raw.relations.push(BrainRelationRecord {
                relation_id: format!("rel-{index}"),
                kind: BrainRelationKind::DependsOn,
                source_node_id: format!("node-{}", index % SOURCE_GRAPH_TARGET_CONCEPTS),
                target_node_id: format!(
                    "node-{}",
                    (index + 1 + (index / SOURCE_GRAPH_TARGET_CONCEPTS))
                        % SOURCE_GRAPH_TARGET_CONCEPTS
                ),
                label: "depends_on".into(),
                evidence_ids: vec![format!(
                    "ev-source-alpha-{}",
                    SOURCE_GRAPH_TARGET_CONCEPTS + index
                )],
                confidence: Some(0.5),
                updated_at: 1,
            });
        }

        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        assert!(canonical.relations.len() <= SOURCE_GRAPH_HARD_MAX_RELATIONS);
        assert!(report.dropped_relation_count >= 100);
        assert!(report
            .drop_reasons
            .get("relation_capped_out")
            .is_some_and(|count| *count > 0));
    }

    #[test]
    fn eight_batch_fixture_compacts_caps_and_stays_deterministic() {
        let baseline = test_baseline("source-alpha", 160);
        let snapshots = synthetic_eight_batch_success_snapshots();
        assert_eq!(snapshots.len(), 8);

        let start = Instant::now();
        let first = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            snapshots.clone(),
            0,
        )
        .expect("first compaction");
        let elapsed = start.elapsed();
        let second =
            merge_source_graph_snapshots("default", &baseline, "source-alpha", snapshots, 0)
                .expect("second compaction");

        assert!(
            elapsed < Duration::from_millis(500),
            "compaction took {:?}",
            elapsed
        );
        assert!(first.report.raw_node_count >= 151);
        assert!(first.report.raw_relation_count >= 150);
        assert!(first.canonical_snapshot.nodes.len() <= SOURCE_GRAPH_TARGET_CONCEPTS + 1);
        assert!(first.canonical_snapshot.nodes.len() <= SOURCE_GRAPH_HARD_MAX_CONCEPTS + 1);
        assert!(first.canonical_snapshot.relations.len() <= SOURCE_GRAPH_HARD_MAX_RELATIONS);
        assert_eq!(first.report.drop_reasons.get("missing_evidence"), Some(&8));
        assert!(first
            .report
            .drop_reasons
            .get("capped_out")
            .is_some_and(|count| *count > 0));

        let first_node_ids = first
            .canonical_snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<BTreeSet<_>>();
        let second_node_ids = second
            .canonical_snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect::<BTreeSet<_>>();
        let first_relation_ids = first
            .canonical_snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect::<BTreeSet<_>>();
        let second_relation_ids = second
            .canonical_snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect::<BTreeSet<_>>();
        let canonical_labels = first
            .canonical_snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .map(|node| normalize_concept_label(&node.label))
            .collect::<Vec<_>>();
        let unique_labels = canonical_labels.iter().collect::<BTreeSet<_>>();

        assert_eq!(
            first.canonical_snapshot.nodes.len(),
            second.canonical_snapshot.nodes.len()
        );
        assert_eq!(first_node_ids, second_node_ids);
        assert_eq!(first_relation_ids, second_relation_ids);
        assert_eq!(canonical_labels.len(), unique_labels.len());
        assert_eq!(first.report, second.report);
    }

    #[test]
    fn partial_chunk_failures_materialize_successful_chunks_and_record_failed_artifacts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let artifact_root = temp.path().join("artifacts").join("source-alpha");
        let baseline = test_baseline("source-alpha", 160);
        let result = merge_source_graph_snapshots(
            "default",
            &baseline,
            "source-alpha",
            synthetic_six_batch_success_snapshots(),
            0,
        )
        .expect("successful chunks still compact");
        for failed_batch in ["06", "07"] {
            assert!(result
                .raw_snapshot
                .nodes
                .iter()
                .all(|node| !node.node_id.starts_with(&format!("node-{failed_batch}-"))));
            assert!(result
                .raw_snapshot
                .relations
                .iter()
                .all(|relation| !relation
                    .relation_id
                    .starts_with(&format!("rel-{failed_batch}-"))));
            assert!(result.raw_snapshot.evidence.iter().all(|evidence| !evidence
                .id
                .contains(&format!("source-alpha-{failed_batch}-"))));
            assert!(result
                .canonical_snapshot
                .evidence
                .iter()
                .all(|evidence| !evidence
                    .id
                    .contains(&format!("source-alpha-{failed_batch}-"))));
            assert!(result.canonical_snapshot.nodes.iter().all(|node| node
                .evidence_ids
                .iter()
                .all(
                    |evidence_id| !evidence_id.contains(&format!("source-alpha-{failed_batch}-"))
                )));
        }
        let mut report = test_report("source-alpha", None);
        report.chunk_total = 8;
        report.chunk_succeeded = 6;
        report.chunk_failed = 2;
        report.retryable = true;
        report.failed_reason = Some(
            provider_graph_failure_reason(&anyhow!("provider_timeout: chunk request timed out"))
                .into(),
        );
        report.source_graph_materialized = true;
        report.workspace_linking_materialized = false;
        report.stage_runs = (0..8)
            .map(|index| ProviderGraphStageRunReport {
                stage: "source_chunk_extract".into(),
                run_id: format!("provider-source-graph-test-chunk-{index:04}"),
                chunk_ids: vec![format!("chunk-source-alpha-{index:04}")],
                status: if index < 6 {
                    "validated".into()
                } else {
                    "failed".into()
                },
                retryable: index >= 6,
                node_count: if index < 6 { 25 } else { 0 },
                relation_count: if index < 6 { 25 } else { 0 },
                error_message: (index >= 6).then(|| "provider_timeout".into()),
            })
            .collect();
        report.status = source_graph_materialized_status(&report).into();
        report.progress = source_graph_progress(&report);
        report.source_graph_node_count = result.canonical_snapshot.nodes.len();
        report.source_graph_relation_count = result.canonical_snapshot.relations.len();

        write_source_chunk_run_artifact(
            &artifact_root,
            "provider-source-graph-test-chunk-0006",
            "default",
            "source-alpha",
            &["chunk-source-alpha-0006".into()],
            "failed",
            Some("provider_timeout".into()),
        )
        .expect("failed chunk artifact");
        let failed_artifact: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                artifact_root
                    .join("provider-graph-chunks")
                    .join("provider-source-graph-test-chunk-0006.json"),
            )
            .expect("failed artifact json"),
        )
        .expect("failed artifact value");

        assert_eq!(report.status, "source_partial_with_failures");
        assert_eq!(report.progress, 1.0);
        assert_eq!(report.chunk_total, 8);
        assert_eq!(report.chunk_succeeded, 6);
        assert_eq!(report.chunk_failed, 2);
        assert!(report.retryable);
        assert_eq!(report.failed_reason.as_deref(), Some("provider_timeout"));
        assert!(report.source_graph_materialized);
        assert!(result.canonical_snapshot.nodes.len() > 1);
        assert_eq!(failed_artifact["errorMessage"], "provider_timeout");
    }

    #[test]
    fn compaction_failure_report_preserves_raw_and_skips_canonical_artifacts() {
        let temp = tempfile::tempdir().expect("temp dir");
        let artifact_root = temp.path().join("artifacts").join("source-alpha");
        let report_path = artifact_root.join("provider-graph-materialization.json");
        let raw_path = artifact_root.join("provider-graph-source-raw-merged.json");
        let canonical_path = artifact_root.join("provider-graph-source-canonical.json");
        let raw = source_raw_snapshot("source-alpha", 2);
        write_json_pretty(
            &raw_path,
            &json!({
                "status": "validated",
                "snapshot": raw.clone(),
            }),
        )
        .expect("raw artifact");
        let mut report = test_report("source-alpha", None);
        report.source_graph_materialized = false;
        report.workspace_linking_materialized = false;

        mark_source_graph_compaction_failed(
            &mut report,
            &artifact_root,
            &report_path,
            &raw,
            &anyhow!("canonical source graph exceeds relation cap"),
        )
        .expect("mark compaction failed");
        let persisted_report: ProviderGraphMaterializationReport =
            read_json_artifact(&report_path).expect("persisted materialization report");
        let compaction_report: serde_json::Value =
            read_json_artifact(&artifact_root.join("provider-graph-compaction.json"))
                .expect("compaction report");

        assert!(raw_path.exists());
        assert!(!canonical_path.exists());
        assert_eq!(persisted_report.status, "failed_no_materialization");
        assert_eq!(
            persisted_report.failed_reason.as_deref(),
            Some("source_graph_compaction_failed")
        );
        assert!(!persisted_report.source_graph_materialized);
        assert_eq!(
            persisted_report.compaction_status.as_deref(),
            Some("failed")
        );
        assert_eq!(compaction_report["status"], "failed");
        assert!(compaction_report["errorMessage"]
            .as_str()
            .is_some_and(|message| message.contains("relation cap")));
    }

    #[test]
    fn evidence_backed_general_labels_are_not_hard_dropped() {
        let baseline = test_baseline("source-alpha", 2);
        let mut raw = source_raw_snapshot("source-alpha", 2);
        raw.nodes.push(candidate_node(
            "node-general",
            "Generic Heading",
            Some("ev-source-alpha-0".into()),
        ));
        raw.nodes
            .push(candidate_node("node-missing", "Useful Concept", None));

        let (canonical, report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");

        assert_eq!(canonical.nodes.len(), 2);
        assert!(canonical
            .nodes
            .iter()
            .any(|node| node.label == "Generic Heading"));
        assert_eq!(report.dropped_node_count, 1);
        assert_eq!(report.drop_reasons.get("missing_evidence"), Some(&1));
    }

    #[test]
    fn ranking_prefers_structural_evidence_over_one_off_mentions() {
        let baseline = test_baseline("source-alpha", 80);
        let mut raw = source_raw_snapshot("source-alpha", 80);
        raw.evidence.push(EvidenceRef {
            id: "retrieved:source-alpha:chunk-strong".into(),
            page_label: "Architecture Heading".into(),
            page_index: None,
            snippet: "Shared signal appears in a titled section with chunk support.".into(),
            source_path: Some("/tmp/source.pdf".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/source.md".into()),
            image_path: None,
            provenance: Some("synthetic ranking fixture".into()),
        });
        let mut strong = candidate_node("node-strong", "Shared Signal", None);
        strong.evidence_ids = vec![
            "ev-source-alpha-0".into(),
            "ev-source-alpha-1".into(),
            "ev-source-alpha-2".into(),
            "retrieved:source-alpha:chunk-strong".into(),
        ];
        raw.nodes.push(strong);
        for index in 0..30 {
            raw.nodes.push(candidate_node(
                &format!("node-weak-{index:02}"),
                &format!("One Off Mention {index:02}"),
                Some(format!("ev-source-alpha-{}", index + 3)),
            ));
        }
        for index in 0..10 {
            raw.relations.push(candidate_relation(
                &format!("rel-strong-{index:02}"),
                "node-strong",
                &format!("node-weak-{index:02}"),
                &format!("ev-source-alpha-{}", index + 40),
            ));
        }

        let (canonical, _report) =
            compact_source_graph_snapshot("default", &baseline, "source-alpha", &raw, 0)
                .expect("compact");
        let first_concept = canonical
            .nodes
            .iter()
            .find(|node| node.kind == BrainNodeKind::Concept)
            .expect("first concept");

        assert_eq!(first_concept.label, "Shared Signal");
    }

    #[test]
    fn materialization_report_serializes_progress_and_chunk_fields() {
        let mut report = test_report("source-alpha", None);
        report.status = "source_partial_with_failures".into();
        report.retryable = true;
        report.stage = "extracting_source_chunks".into();
        report.progress = 0.5;
        report.failed_reason = Some("provider_timeout".into());
        report.chunk_total = 2;
        report.chunk_succeeded = 1;
        report.chunk_failed = 1;
        report.stage_runs.push(ProviderGraphStageRunReport {
            stage: "source_chunk_extract".into(),
            run_id: "provider-source-graph-test-chunk-0001".into(),
            chunk_ids: vec!["chunk-source-alpha-0001".into()],
            status: "failed".into(),
            retryable: true,
            node_count: 0,
            relation_count: 0,
            error_message: Some("provider_timeout".into()),
        });

        let encoded = serde_json::to_value(&report).expect("encode report");

        assert_eq!(encoded["retryable"], true);
        assert_eq!(encoded["stage"], "extracting_source_chunks");
        assert_eq!(encoded["progress"], 0.5);
        assert_eq!(encoded["failedReason"], "provider_timeout");
        assert_eq!(encoded["chunkTotal"], 2);
        assert_eq!(encoded["chunkSucceeded"], 1);
        assert_eq!(encoded["chunkFailed"], 1);
        assert_eq!(encoded["stageRuns"][0]["stage"], "source_chunk_extract");
        assert_eq!(encoded["stageRuns"][0]["retryable"], true);
    }

    #[test]
    fn provider_graph_stage_snapshot_ids_use_run_ids() {
        let manifest = test_manifest("source-alpha");
        let mut snapshot = empty_replayed_brain_snapshot("default");
        snapshot.generated_at = 42;

        let source_event = source_graph_build_materialized_event(
            "default",
            "provider-source-graph-run-a",
            &manifest,
            &snapshot,
        )
        .expect("source graph event");
        let linking_event = workspace_linking_materialized_event(
            "default",
            "provider-workspace-linking-run-b",
            &manifest,
            &snapshot,
        )
        .expect("workspace linking event");

        assert_eq!(
            source_event.causality.snapshot_id.as_deref(),
            Some("snapshot-provider-source-graph-run-a")
        );
        assert_eq!(
            linking_event.causality.snapshot_id.as_deref(),
            Some("snapshot-provider-workspace-linking-run-b")
        );
        assert_ne!(
            source_event.causality.snapshot_id,
            linking_event.causality.snapshot_id
        );
    }

    #[test]
    fn workspace_linking_event_is_relation_only() {
        let manifest = test_manifest("source-alpha");
        let mut snapshot = empty_replayed_brain_snapshot("default");
        snapshot.generated_at = 42;
        snapshot.nodes = vec![
            BrainNodeRecord {
                node_id: "concept-alpha".into(),
                kind: BrainNodeKind::Concept,
                label: "Alpha".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["ev-alpha".into()],
                source_ids: vec!["source-alpha".into()],
                confidence: Some(0.9),
                updated_at: 42,
            },
            BrainNodeRecord {
                node_id: "concept-beta".into(),
                kind: BrainNodeKind::Concept,
                label: "Beta".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["ev-beta".into()],
                source_ids: vec!["source-beta".into()],
                confidence: Some(0.9),
                updated_at: 42,
            },
        ];
        snapshot.relations = vec![BrainRelationRecord {
            relation_id: "edge-alpha-beta".into(),
            kind: BrainRelationKind::RelatedTo,
            source_node_id: "concept-alpha".into(),
            target_node_id: "concept-beta".into(),
            label: "related".into(),
            evidence_ids: vec!["ev-alpha".into(), "ev-beta".into()],
            confidence: Some(0.8),
            updated_at: 42,
        }];

        let event = workspace_linking_materialized_event(
            "default",
            "provider-workspace-linking-run-a",
            &manifest,
            &snapshot,
        )
        .expect("workspace linking event");
        let payload: serde_json::Value =
            serde_json::from_str(&event.payload_json).expect("decode payload");

        assert!(event.target_node_ids.is_empty());
        assert_eq!(event.target_edge_ids, vec!["edge-alpha-beta"]);
        assert_eq!(event.node_refs, vec!["concept-alpha", "concept-beta"]);
        assert_eq!(event.source_refs, vec!["source-alpha", "source-beta"]);
        assert_eq!(event.causality.caused_by_source_ids, vec!["source-alpha"]);
        assert_eq!(payload["materializedGraph"]["sources"], json!([]));
        assert_eq!(payload["materializedGraph"]["nodes"], json!([]));
        assert_eq!(payload["materializedGraph"]["evidence"], json!([]));
        assert_eq!(
            payload["materializedGraph"]["edges"][0]["relationId"],
            "edge-alpha-beta"
        );
    }

    #[test]
    fn linking_failure_after_source_materialization_is_reported_as_partial_commit() {
        let mut report = ProviderGraphMaterializationReport {
            status: "source_graph_materialized".into(),
            provider: "openai".into(),
            model: "test-model".into(),
            source_id: "source-alpha".into(),
            input_fingerprint: None,
            source_graph_node_count: 1,
            source_graph_relation_count: 1,
            workspace_link_count: 0,
            materialized_node_count: 0,
            materialized_relation_count: 0,
            materialized_claim_count: 0,
            materialized_memory_count: 0,
            skipped_reason: None,
            error_message: None,
            provider_run_ids: vec![
                "provider-source-graph-test".into(),
                "provider-workspace-linking-test".into(),
            ],
            source_graph_run_id: Some("provider-source-graph-test".into()),
            workspace_linking_run_id: Some("provider-workspace-linking-test".into()),
            source_graph_materialized: true,
            workspace_linking_materialized: false,
            retryable: false,
            stage: "source_graph_materialized".into(),
            progress: 1.0,
            failed_reason: None,
            chunk_total: 1,
            chunk_succeeded: 1,
            chunk_failed: 0,
            chunk_discovered: 1,
            chunk_processed: 1,
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
            updated_at: 1,
        };

        mark_workspace_linking_failed(&mut report, &anyhow!("link validation failed"));

        assert_eq!(report.status, "source_graph_materialized_linking_failed");
        assert!(report.source_graph_materialized);
        assert!(!report.workspace_linking_materialized);
        assert!(report
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("link validation failed")));
        let encoded = serde_json::to_value(&report).expect("encode report");
        assert!(encoded.get("sourceGraphNodeCount").is_some());
        assert!(encoded.get("materializedNodeCount").is_some());
        assert_eq!(
            encoded["providerRunIds"],
            json!([
                "provider-source-graph-test",
                "provider-workspace-linking-test"
            ])
        );
        assert!(encoded.get("providerRunId").is_none());
        assert!(encoded.get("candidateCount").is_none());
        assert!(encoded.get("appliedCount").is_none());
    }

    #[test]
    fn legacy_provider_run_id_report_still_decodes() {
        let report: ProviderGraphMaterializationReport = serde_json::from_value(json!({
            "status": "linked",
            "provider": "openai",
            "model": "test-model",
            "sourceId": "source-alpha",
            "providerRunId": "legacy-single-run",
            "updatedAt": 1
        }))
        .expect("legacy report decodes");

        assert!(report.provider_run_ids.is_empty());
        let encoded = serde_json::to_value(&report).expect("encode report");
        assert!(encoded.get("providerRunId").is_none());
        assert!(encoded.get("providerRunIds").is_some());
    }

    #[test]
    fn materialized_report_counts_reflect_current_snapshot() {
        let mut report = test_report("source-alpha", None);
        let mut snapshot = empty_replayed_brain_snapshot("default");
        snapshot.nodes.push(BrainNodeRecord {
            node_id: "concept-a".into(),
            kind: BrainNodeKind::Concept,
            label: "A".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: Vec::new(),
            source_ids: Vec::new(),
            confidence: None,
            updated_at: 1,
        });
        snapshot.memories.push(MemoryRecord {
            memory_id: "memory-a".into(),
            workspace_id: "default".into(),
            scope: BrainScope::Project,
            title: "A".into(),
            body: "A".into(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            created_at: 1,
            updated_at: 1,
        });

        update_materialized_counts(&mut report, &snapshot);

        assert_eq!(report.materialized_node_count, 1);
        assert_eq!(report.materialized_relation_count, 0);
        assert_eq!(report.materialized_claim_count, 0);
        assert_eq!(report.materialized_memory_count, 1);
    }
}
