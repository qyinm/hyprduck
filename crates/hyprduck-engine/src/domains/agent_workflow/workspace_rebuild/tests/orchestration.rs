use super::super::*;
use super::{
    source_raw_snapshot, synthetic_six_batch_success_snapshots, test_baseline, test_fingerprint,
    test_manifest, test_report,
};

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
        assert!(result
            .canonical_snapshot
            .nodes
            .iter()
            .all(|node| node.evidence_ids.iter().all(
                |evidence_id| !evidence_id.contains(&format!("source-alpha-{failed_batch}-"))
            )));
    }
    let mut report = test_report("source-alpha", None);
    report.chunk_total = 8;
    report.chunk_succeeded = 6;
    report.chunk_failed = 2;
    report.retryable = true;
    report.failed_reason = Some(
        provider_graph_failure_reason(&anyhow!("provider_timeout: chunk request timed out")).into(),
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
