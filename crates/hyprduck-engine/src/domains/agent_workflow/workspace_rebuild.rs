use std::path::Path;
use std::time::Duration;

use super::artifacts::{
    provider_workspace_linking_response_schema, provider_workspace_rebuild_response_schema,
    write_provider_graph_run_artifacts, write_provider_graph_run_validation_report,
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

const PROVIDER_GRAPH_PROMPT_VERSION: u32 = 2;
const PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION: u32 = 1;
const PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphMaterializationInputFingerprint {
    pub(crate) workspace_id: String,
    pub(crate) source_id: String,
    pub(crate) manifest_updated_at: u64,
    pub(crate) markdown_hash: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) source_graph_schema_version: u32,
    pub(crate) workspace_linking_schema_version: u32,
    pub(crate) prompt_version: u32,
    #[serde(default)]
    pub(crate) baseline_snapshot_id: Option<String>,
    #[serde(default)]
    pub(crate) baseline_event_id: Option<String>,
    #[serde(default)]
    pub(crate) baseline_materialized_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderGraphMaterializationReport {
    pub(crate) status: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) source_id: String,
    #[serde(default)]
    pub(crate) input_fingerprint: Option<ProviderGraphMaterializationInputFingerprint>,
    #[serde(default)]
    pub(crate) source_graph_node_count: usize,
    #[serde(default)]
    pub(crate) source_graph_relation_count: usize,
    #[serde(default)]
    pub(crate) workspace_link_count: usize,
    #[serde(default)]
    pub(crate) materialized_node_count: usize,
    #[serde(default)]
    pub(crate) materialized_relation_count: usize,
    #[serde(default)]
    pub(crate) materialized_claim_count: usize,
    #[serde(default)]
    pub(crate) materialized_memory_count: usize,
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

pub(crate) fn maybe_generate_provider_graph_materialization(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    artifact_root: &Path,
    context: &ImportEvidenceContext,
) -> Result<ProviderGraphMaterializationReport> {
    let report_path = artifact_root.join("provider-graph-materialization.json");
    let legacy_report_path = artifact_root.join("provider-graph-proposals.json");
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
    let input_fingerprint =
        provider_graph_input_fingerprint(workspace_root, workspace_id, manifest, markdown, &config);
    if let Ok(existing) = read_json_artifact::<ProviderGraphMaterializationReport>(&report_path)
        .or_else(|_| read_json_artifact::<ProviderGraphMaterializationReport>(&legacy_report_path))
    {
        if provider_graph_report_is_reusable(&existing, manifest, &input_fingerprint) {
            if !report_path.exists() {
                write_json_pretty(&report_path, &existing)?;
            }
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
        provider_workspace_rebuild_response_schema(),
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
    report.source_graph_node_count = source_graph_snapshot.nodes.len();
    report.source_graph_relation_count = source_graph_snapshot.relations.len();
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
    report.materialized_node_count = final_snapshot.nodes.len();
    report.materialized_relation_count = final_snapshot.relations.len();
    report.materialized_claim_count = final_snapshot.claims.len();
    report.materialized_memory_count = final_snapshot.memories.len();
    report.updated_at = unix_timestamp_seconds();
    write_json_pretty(&report_path, &report)?;
    Ok(report)
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

fn provider_graph_input_fingerprint(
    workspace_root: &Path,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    markdown: &str,
    config: &EngineConfig,
) -> ProviderGraphMaterializationInputFingerprint {
    let baseline_marker = read_latest_readable_graph_snapshot_marker(workspace_root)
        .ok()
        .flatten();
    ProviderGraphMaterializationInputFingerprint {
        workspace_id: workspace_id.into(),
        source_id: manifest.source_id.clone(),
        manifest_updated_at: manifest.updated_at,
        markdown_hash: stable_text_hash(markdown),
        provider: config.provider.id_slug().into(),
        model: config.model_id.clone(),
        source_graph_schema_version: PROVIDER_SOURCE_GRAPH_SCHEMA_VERSION,
        workspace_linking_schema_version: PROVIDER_WORKSPACE_LINKING_SCHEMA_VERSION,
        prompt_version: PROVIDER_GRAPH_PROMPT_VERSION,
        baseline_snapshot_id: baseline_marker
            .as_ref()
            .map(|marker| marker.snapshot_id.clone()),
        baseline_event_id: baseline_marker
            .as_ref()
            .map(|marker| marker.event_id.clone()),
        baseline_materialized_at: baseline_marker
            .as_ref()
            .map(|marker| marker.materialized_at),
    }
}

fn stable_text_hash(value: &str) -> String {
    format!("{:016x}", fnv1a_hash(value.as_bytes()))
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
    report.updated_at = unix_timestamp_seconds();
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
            provider_run_id: None,
            source_graph_run_id: None,
            workspace_linking_run_id: None,
            source_graph_materialized: true,
            workspace_linking_materialized: true,
            updated_at: 1,
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
        let encoded = serde_json::to_value(&report).expect("encode report");
        assert!(encoded.get("sourceGraphNodeCount").is_some());
        assert!(encoded.get("materializedNodeCount").is_some());
        assert!(encoded.get("proposalCount").is_none());
        assert!(encoded.get("appliedCount").is_none());
    }
}
