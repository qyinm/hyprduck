use std::path::Path;
use std::time::Duration;

use super::artifacts::{
    provider_workspace_rebuild_response_schema, write_provider_graph_run_artifacts,
    write_provider_graph_run_validation_report,
};
use super::prompt::build_full_workspace_graph_rebuild_prompt;
use super::response::{
    normalize_provider_workspace_rebuild_snapshot, parse_provider_workspace_rebuild_snapshot,
};
use super::validation::validate_provider_workspace_rebuild_snapshot;
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
        if !existing.proposal_ids.is_empty() && existing.source_id == manifest.source_id {
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
    let prompt = build_full_workspace_graph_rebuild_prompt(
        workspace_root,
        workspace_id,
        manifest,
        markdown,
        &snapshot,
        context,
    )?;
    let provider_run_id = format!("provider-workspace-rebuild-{}", Uuid::now_v7());
    report.provider_run_id = Some(provider_run_id.clone());
    let provider_response = match parse_openai_compatible_json_schema_with_timeout(
        &config,
        &prompt,
        provider_workspace_rebuild_response_schema(),
        Some(Duration::from_secs(
            PROVIDER_GRAPH_GENERATION_TIMEOUT_SECONDS,
        )),
    ) {
        Ok(response) => response,
        Err(error) => {
            report.status = "failed".into();
            report.error_message = Some(format!("{error:#}"));
            write_provider_graph_run_artifacts(
                workspace_root,
                &provider_run_id,
                workspace_id,
                manifest,
                "failed",
                Some(&prompt),
                None,
                Some(format!("{error:#}")),
            )?;
            write_json_pretty(&report_path, &report)?;
            return Ok(report);
        }
    };
    write_provider_graph_run_artifacts(
        workspace_root,
        &provider_run_id,
        workspace_id,
        manifest,
        "received",
        Some(&prompt),
        Some(&provider_response),
        None,
    )?;

    let mut rebuilt_snapshot = match parse_provider_workspace_rebuild_snapshot(&provider_response) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            report.status = "failed".into();
            report.error_message = Some(format!("{error:#}"));
            write_provider_graph_run_validation_report(
                workspace_root,
                &provider_run_id,
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

    normalize_provider_workspace_rebuild_snapshot(
        &mut rebuilt_snapshot,
        workspace_id,
        &snapshot,
        unix_timestamp_seconds(),
    );
    if let Err(error) = validate_provider_workspace_rebuild_snapshot(&rebuilt_snapshot, &snapshot) {
        report.status = "failed".into();
        report.error_message = Some(format!("{error:#}"));
        write_provider_graph_run_validation_report(
            workspace_root,
            &provider_run_id,
            workspace_id,
            &manifest.source_id,
            "failed",
            rebuilt_snapshot.nodes.len(),
            Some(format!("{error:#}")),
        )?;
        write_json_pretty(&report_path, &report)?;
        return Ok(report);
    }

    let before = capture_materialized_file_snapshot(workspace_root)?;
    let event = full_workspace_rebuild_materialized_event(
        workspace_id,
        &provider_run_id,
        manifest,
        &rebuilt_snapshot,
    )?;
    rebuilt_snapshot.events = merge_preserved_brain_events(vec![event], &snapshot.events);
    refresh_materialized_wiki_pages(&mut rebuilt_snapshot);
    persist_reconstructed_brain_snapshot(workspace_root, &rebuilt_snapshot)?;
    let after = capture_materialized_file_snapshot(workspace_root)?;
    let changed_files = changed_materialized_files(&before, &after);

    write_provider_graph_run_validation_report(
        workspace_root,
        &provider_run_id,
        workspace_id,
        &manifest.source_id,
        "materialized",
        rebuilt_snapshot.nodes.len(),
        None,
    )?;
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(&provider_run_id)
            .join("graph-diff.json"),
        &json!({
            "runId": provider_run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "changedFiles": changed_files,
            "nodeCount": rebuilt_snapshot.nodes.len(),
            "relationCount": rebuilt_snapshot.relations.len(),
            "claimCount": rebuilt_snapshot.claims.len(),
            "memoryCount": rebuilt_snapshot.memories.len(),
            "updatedAt": rebuilt_snapshot.generated_at,
        }),
    )?;

    report.status = "rebuilt".into();
    report.proposal_count = 0;
    report.applied_count = rebuilt_snapshot.nodes.len();
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(&report_path, &report)?;
    Ok(report)
}

fn full_workspace_rebuild_materialized_event(
    workspace_id: &str,
    run_id: &str,
    manifest: &SourceArtifactManifest,
    snapshot: &BrainRepoSnapshot,
) -> Result<BrainEvent> {
    Ok(BrainEvent {
        event_id: format!("evt-{run_id}"),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.to_string(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("full_workspace_rebuild".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: format!("{PROVIDER_GRAPH_AGENT_ID}:full-workspace-rebuild"),
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
        confidence: Some("provider_full_workspace_rebuild".into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    })
}

fn provider_graph_generation_disabled_for_process() -> bool {
    std::env::var_os("HYPRDUCK_DISABLE_PROVIDER_GRAPH").is_some()
        || (cfg!(test) && std::env::var_os("HYPRDUCK_TEST_ENABLE_PROVIDER_GRAPH").is_none())
}
