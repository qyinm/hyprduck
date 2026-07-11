use super::super::*;
use super::{
    candidate_node, candidate_relation, source_raw_snapshot,
    synthetic_eight_batch_success_snapshots, test_baseline, test_import_context, test_manifest,
};
use std::time::{Duration, Instant};

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
    let chunks = source_graph_chunk_batches(&manifest, "## Durable Signal\nstable source evidence");
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
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
    let (mut chunk_snapshot, provider_source_of_count) = parse_and_validate_source_graph_response(
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
    let (mut chunk_snapshot, provider_source_of_count) = parse_and_validate_source_graph_response(
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
                (index + 1 + (index / SOURCE_GRAPH_TARGET_CONCEPTS)) % SOURCE_GRAPH_TARGET_CONCEPTS
            ),
            label: "depends_on".into(),
            evidence_ids: vec![format!(
                "ev-source-alpha-{}",
                SOURCE_GRAPH_TARGET_CONCEPTS + index
            )],
            confidence: Some(0.5),
            updated_at: 1,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
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
    let first =
        merge_source_graph_snapshots("default", &baseline, "source-alpha", snapshots.clone(), 0)
            .expect("first compaction");
    let elapsed = start.elapsed();
    let second = merge_source_graph_snapshots("default", &baseline, "source-alpha", snapshots, 0)
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
