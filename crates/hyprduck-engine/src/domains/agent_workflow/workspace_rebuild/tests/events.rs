use super::super::*;
use super::test_manifest;

#[test]
fn provider_graph_materialized_events_use_distinct_operation_types() {
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
fn mark_workspace_linking_failed_preserves_source_graph_state() {
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
