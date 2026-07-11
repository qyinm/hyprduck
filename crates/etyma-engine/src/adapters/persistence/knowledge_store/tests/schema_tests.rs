use super::super::*;

#[test]
fn knowledge_store_creates_canonical_schema_and_graphqlite_gate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");

    assert!(store.path().ends_with("etyma.sqlite"));

    let health = store.health().expect("health");
    assert_eq!(health.db_schema_version, 2);
    assert_eq!(health.graph_schema_version, 2);
    assert!(health.graphqlite_loaded);
    assert!(health.graphqlite_transactional);
}

#[test]
fn graph_version_id_helpers_are_deterministic_and_kind_scoped() {
    let node = graph_node_version_identity("workspace-default", "node-a", "event-a");
    let same_node = graph_node_version_identity("workspace-default", "node-a", "event-a");
    let next_node = graph_node_version_identity("workspace-default", "node-a", "event-b");
    let relation = graph_relation_version_identity("workspace-default", "node-a", "event-a");

    assert_eq!(node, same_node);
    assert_eq!(node.logical_id, "node-a");
    assert_eq!(node.created_by_event_id, "event-a");
    assert!(node.version_id.starts_with("etyma-node-version-"));
    assert_ne!(node.version_id, next_node.version_id);
    assert_ne!(node.version_id, relation.version_id);
    assert_eq!(GRAPH_VERSION_LEGACY_EVENT_ID, "legacy-graphqlite-current");
}

#[test]
fn graphqlite_gate_rejects_non_transactional_graph_mutations() {
    let temp = tempfile::tempdir().expect("temp dir");
    let graph = Graph::open(temp.path().join("etyma.sqlite")).expect("open graph");
    let report = validate_graphqlite_gate(&graph).expect("gate");

    assert!(report.loaded);
    assert!(report.cypher_ready);
    assert!(report.rollback_ready);
}

#[test]
fn evidence_query_intent_boosts_requested_evidence_types() {
    let table_intent = EvidenceQueryIntent::from_query("show the financial table");
    assert!(table_intent.boost("table_evidence") > table_intent.boost("text_evidence"));

    let visual_intent = EvidenceQueryIntent::from_query("inspect the figure caption");
    assert!(visual_intent.boost("image_region_evidence") > visual_intent.boost("text_evidence"));

    let relationship_intent = EvidenceQueryIntent::from_query("which claims are connected");
    assert!(
        relationship_intent.boost("relationship_evidence")
            > relationship_intent.boost("text_evidence")
    );
}
