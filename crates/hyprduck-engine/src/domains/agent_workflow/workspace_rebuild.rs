use std::path::Path;
use std::time::Duration;

use super::artifacts::{
    provider_workspace_rebuild_response_schema, write_provider_graph_run_artifacts,
    write_provider_graph_run_validation_report,
};
use super::prompt::{build_source_local_graph_prompt, build_workspace_linking_prompt};
use super::response::{
    normalize_provider_source_local_graph_snapshot, normalize_provider_workspace_linking_snapshot,
    parse_provider_workspace_rebuild_snapshot,
};
use super::validation::{
    validate_provider_source_local_graph_snapshot, validate_provider_workspace_linking_snapshot,
};
use crate::provider::EngineConfig;
use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphProposalGenerationReport {
    pub(crate) status: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) source_id: String,
    pub(crate) proposal_count: usize,
    pub(crate) applied_count: usize,
    pub(crate) failed_count: usize,
    #[serde(default)]
    pub(crate) proposal_ids: Vec<String>,
    #[serde(default)]
    pub(crate) applied_proposal_ids: Vec<String>,
    #[serde(default)]
    pub(crate) failed_proposals: Vec<AgentProposalFailureReport>,
    #[serde(default)]
    pub(crate) skipped_reason: Option<String>,
    #[serde(default)]
    pub(crate) error_message: Option<String>,
    #[serde(default)]
    pub(crate) provider_run_id: Option<String>,
    #[serde(default)]
    pub(crate) source_graph_run_id: Option<String>,
    #[serde(default)]
    pub(crate) workspace_linking_run_id: Option<String>,
    #[serde(default)]
    pub(crate) source_graph_materialized: bool,
    #[serde(default)]
    pub(crate) workspace_linking_materialized: bool,
    pub(crate) updated_at: u64,
}

pub(crate) fn maybe_generate_provider_graph_proposals(
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
        if existing.status == "linked" && existing.source_id == manifest.source_id {
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
                source_graph_run_id: None,
                workspace_linking_run_id: None,
                source_graph_materialized: false,
                workspace_linking_materialized: false,
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
        source_graph_run_id: None,
        workspace_linking_run_id: None,
        source_graph_materialized: false,
        workspace_linking_materialized: false,
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
    let source_graph_run_id = format!("provider-source-graph-{}", Uuid::now_v7());
    report.provider_run_id = Some(source_graph_run_id.clone());
    report.source_graph_run_id = Some(source_graph_run_id.clone());
    let source_graph_prompt =
        build_source_local_graph_prompt(workspace_id, manifest, markdown, &snapshot, context)?;
    let source_graph_response = match run_provider_graph_stage(
        workspace_root,
        workspace_id,
        manifest,
        &config,
        &source_graph_run_id,
        &source_graph_prompt,
    ) {
        Ok(response) => response,
        Err(error) => {
            report.status = "failed".into();
            report.error_message = Some(format!("{error:#}"));
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    let mut source_graph_snapshot =
        match parse_provider_workspace_rebuild_snapshot(&source_graph_response) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                report.status = "failed".into();
                report.error_message = Some(format!("{error:#}"));
                write_provider_graph_run_validation_report(
                    workspace_root,
                    &source_graph_run_id,
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
    normalize_provider_source_local_graph_snapshot(
        &mut source_graph_snapshot,
        workspace_id,
        &snapshot,
        &manifest.source_id,
        unix_timestamp_seconds(),
    );
    if let Err(error) =
        validate_provider_source_local_graph_snapshot(&source_graph_snapshot, &manifest.source_id)
    {
        report.status = "failed".into();
        report.error_message = Some(format!("{error:#}"));
        write_provider_graph_run_validation_report(
            workspace_root,
            &source_graph_run_id,
            workspace_id,
            &manifest.source_id,
            "failed",
            source_graph_snapshot.nodes.len(),
            Some(format!("{error:#}")),
        )?;
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

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
    report.status = "source_graph_materialized".into();
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
    let workspace_linking_run_id = format!("provider-workspace-linking-{}", Uuid::now_v7());
    report.provider_run_id = Some(workspace_linking_run_id.clone());
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

    report.status = "linked".into();
    report.proposal_count = 0;
    report.applied_count = final_snapshot.nodes.len();
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(&report_path, &report)?;
    Ok(report)
}

fn mark_workspace_linking_failed(
    report: &mut ProviderGraphProposalGenerationReport,
    error: &anyhow::Error,
) {
    report.status = if report.source_graph_materialized {
        "source_graph_materialized_linking_failed".into()
    } else {
        "failed".into()
    };
    report.workspace_linking_materialized = false;
    report.error_message = Some(format!("{error:#}"));
    report.updated_at = unix_timestamp_seconds();
}

fn run_provider_graph_stage(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    config: &EngineConfig,
    run_id: &str,
    prompt: &str,
) -> Result<String> {
    let response = match parse_openai_compatible_json_schema_with_timeout(
        config,
        prompt,
        provider_workspace_rebuild_response_schema(),
        Some(Duration::from_secs(
            PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
        )),
    ) {
        Ok(response) => response,
        Err(error) => {
            write_provider_graph_run_artifacts(
                workspace_root,
                run_id,
                workspace_id,
                manifest,
                "failed",
                Some(prompt),
                None,
                Some(format!("{error:#}")),
            )?;
            return Err(error);
        }
    };
    write_provider_graph_run_artifacts(
        workspace_root,
        run_id,
        workspace_id,
        manifest,
        "received",
        Some(prompt),
        Some(&response),
        None,
    )?;
    Ok(response)
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
    provider_graph_materialized_event(
        workspace_id,
        run_id,
        manifest,
        snapshot,
        "workspace_linking",
        "workspace-linking",
        "provider_workspace_linking",
    )
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
            caused_by_proposal_id: None,
            snapshot_id: Some(format!("snapshot-{workspace_id}-{}", snapshot.generated_at)),
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

    #[test]
    fn linking_failure_after_source_materialization_is_reported_as_partial_commit() {
        let mut report = ProviderGraphProposalGenerationReport {
            status: "source_graph_materialized".into(),
            provider: "openai".into(),
            model: "test-model".into(),
            source_id: "source-alpha".into(),
            proposal_count: 0,
            applied_count: 0,
            failed_count: 0,
            proposal_ids: Vec::new(),
            applied_proposal_ids: Vec::new(),
            failed_proposals: Vec::new(),
            skipped_reason: None,
            error_message: None,
            provider_run_id: Some("provider-workspace-linking-test".into()),
            source_graph_run_id: Some("provider-source-graph-test".into()),
            workspace_linking_run_id: Some("provider-workspace-linking-test".into()),
            source_graph_materialized: true,
            workspace_linking_materialized: false,
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
    }
}
