use super::*;
use etyma_engine_types::{SourceFormat, SourceStatus};

mod compaction;
mod events;
mod orchestration;

pub(super) fn test_fingerprint(
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
pub(super) fn test_manifest(source_id: &str) -> SourceArtifactManifest {
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
pub(super) fn test_report(
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
pub(super) fn test_baseline(source_id: &str, evidence_count: usize) -> BrainRepoSnapshot {
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    snapshot
}
pub(super) fn candidate_node(
    node_id: &str,
    label: &str,
    evidence_id: Option<String>,
) -> BrainNodeRecord {
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    }
}
pub(super) fn source_raw_snapshot(source_id: &str, evidence_count: usize) -> BrainRepoSnapshot {
    test_baseline(source_id, evidence_count)
}
pub(super) fn candidate_relation(
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    }
}
pub(super) fn synthetic_candidate_chunk_snapshot(
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
pub(super) fn synthetic_eight_batch_success_snapshots() -> Vec<BrainRepoSnapshot> {
    (0..8)
        .map(|batch_index| synthetic_candidate_chunk_snapshot("source-alpha", batch_index, 19))
        .collect()
}
pub(super) fn synthetic_six_batch_success_snapshots() -> Vec<BrainRepoSnapshot> {
    (0..6)
        .map(|batch_index| synthetic_candidate_chunk_snapshot("source-alpha", batch_index, 25))
        .collect()
}
pub(super) fn test_import_context(source_id: &str) -> ImportEvidenceContext {
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
        existing_graph_context: crate::domains::retrieval::import_context::ExistingGraphContext {
            nodes: Vec::new(),
            edges: Vec::new(),
            claims: Vec::new(),
        },
    }
}
