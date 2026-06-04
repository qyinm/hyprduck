use super::super::*;

#[test]
fn knowledge_store_creates_canonical_schema_and_graphqlite_gate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");

    assert!(store.path().ends_with("hyprduck.sqlite"));

    let health = store.health().expect("health");
    assert_eq!(health.db_schema_version, 1);
    assert_eq!(health.graph_schema_version, 1);
    assert!(health.graphqlite_loaded);
    assert!(health.graphqlite_transactional);
}

#[test]
fn graphqlite_gate_rejects_non_transactional_graph_mutations() {
    let temp = tempfile::tempdir().expect("temp dir");
    let graph = Graph::open(temp.path().join("hyprduck.sqlite")).expect("open graph");
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
