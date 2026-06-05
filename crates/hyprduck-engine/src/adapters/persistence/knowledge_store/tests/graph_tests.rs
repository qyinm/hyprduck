use super::super::super::row_decode::sql_literal;
use super::super::*;

#[test]
fn graph_snapshot_appends_brain_events_in_graph_transaction() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");

    let first_snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
    store
        .persist_graph_snapshot(&first_snapshot)
        .expect("persist first graph snapshot");
    assert_eq!(
        brain_event_count(&store, "workspace-default").expect("first event count"),
        1
    );

    let second_snapshot = single_event_snapshot("event-b", "source-b", "evidence-b", "Beta");
    store
        .persist_graph_snapshot(&second_snapshot)
        .expect("persist second graph snapshot");

    assert_eq!(
        brain_event_count(&store, "workspace-default").expect("second event count"),
        2
    );
    assert_eq!(
        store
            .graph_snapshot_counts("workspace-default")
            .expect("graph counts"),
        KnowledgeGraphPersistReport {
            node_count: 2,
            relation_count: 0,
        }
    );
}

#[test]
fn graph_snapshot_marks_citation_ready_import_job_graph_ready_after_commit() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    insert_citation_ready_import_job(&store, "source-a");

    let snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
    store
        .persist_graph_snapshot(&snapshot)
        .expect("persist graph snapshot");

    assert_eq!(import_job_readiness(&store, "source-a"), (1, 1));
    assert_eq!(
        import_job_status(&store, "source-a"),
        "context_ready",
        "graph-ready commits should keep import lifecycle status consistent"
    );
}

#[test]
fn graph_snapshot_versions_logical_records_and_keeps_live_relation_endpoints() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");

    let mut first_snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
    first_snapshot.nodes.push(target_node("Target", 10));
    first_snapshot.relations = vec![shared_relation(10)];
    store
        .persist_graph_snapshot(&first_snapshot)
        .expect("persist first graph snapshot");

    let mut second_snapshot =
        single_event_snapshot("event-b", "source-a", "evidence-a", "Alpha v2");
    second_snapshot.generated_at = 20;
    second_snapshot.sources[0].updated_at = 20;
    second_snapshot.nodes[0].updated_at = 20;
    second_snapshot.nodes[0].valid_from = 20;
    second_snapshot.nodes.push(target_node("Target v2", 20));
    second_snapshot.relations = vec![shared_relation(20)];
    store
        .persist_graph_snapshot(&second_snapshot)
        .expect("persist second graph snapshot");

    assert_eq!(
        logical_node_version_count(&store, "node-source-a"),
        2,
        "each graph snapshot event should create a durable node version"
    );
    assert_eq!(
        live_logical_node_version_count(&store, "node-source-a"),
        1,
        "only one node version should remain live"
    );
    assert_eq!(live_logical_node_label(&store, "node-source-a"), "Alpha v2");
    assert_eq!(
        logical_relation_version_count(&store, "rel-shared"),
        2,
        "relation versions should be preserved when endpoint versions change"
    );
    assert_eq!(live_logical_relation_version_count(&store, "rel-shared"), 1);

    let (nodes, relations, _) = store
        .read_graph_canvas_projection_from_db("workspace-default")
        .expect("read graph canvas projection")
        .expect("graph canvas projection");
    let projected_node = nodes
        .iter()
        .find(|node| node.node_id == "node-source-a")
        .expect("project latest logical node");
    assert_eq!(projected_node.label, "Alpha v2");
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.node_id == "node-source-a")
            .count(),
        1
    );
    let projected_relation = relations
        .iter()
        .find(|relation| relation.relation_id == "rel-shared")
        .expect("project latest logical relation");
    assert_eq!(projected_relation.source_node_id, "node-source-a");
    assert_eq!(projected_relation.target_node_id, "node-target");

    let read_node = store
        .read_node_from_db("workspace-default", "node-source-a")
        .expect("read logical node")
        .expect("node response");
    assert_eq!(read_node.node.label, "Alpha v2");
    assert!(read_node.relations.iter().any(|relation| {
        relation.relation_id == "rel-shared"
            && relation.source_node_id == "node-source-a"
            && relation.target_node_id == "node-target"
    }));
}

#[test]
fn import_job_graph_pending_state_round_trips_for_source_retry() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    insert_citation_ready_import_job(&store, "source-a");

    assert!(store
        .update_import_job_graph_status_from_mcp(
            "workspace-default",
            "source-a",
            "citation_ready_graph_pending",
            "pending",
            Some("db_busy"),
            Some("database is busy"),
            true,
            1,
            2,
            Some(123),
            true,
        )
        .expect("update graph pending state"));

    let job = store
        .read_import_job("workspace-default", None, Some("source-a"))
        .expect("read import job")
        .expect("job should exist");
    assert_eq!(job.status, "citation_ready_graph_pending");
    assert!(job.citation_ready);
    assert!(!job.graph_ready);
    assert_eq!(job.graph_status, "pending");
    assert_eq!(job.graph_error_category, "db_busy");
    assert_eq!(job.graph_error_message_redacted, "database is busy");
    assert!(job.graph_retryable);
    assert_eq!(job.graph_retry_attempt, 1);
    assert_eq!(job.graph_max_retry_attempts, 2);
    assert_eq!(job.graph_next_retry_at, Some(123));
    assert!(job.manual_retry_available);
}

#[test]
fn graphqlite_mutation_failure_rolls_back_relational_graph_audit_writes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    insert_citation_ready_import_job(&store, "source-a");
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .execute_batch(
            "CREATE TRIGGER fail_graph_node_insert
                 BEFORE INSERT ON nodes
                 BEGIN
                   SELECT RAISE(FAIL, 'forced GraphQLite node failure');
                 END;",
        )
        .expect("install GraphQLite failure trigger");

    let snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
    let error = store
        .persist_graph_snapshot(&snapshot)
        .expect_err("GraphQLite node mutation should fail");

    assert!(error
        .to_string()
        .contains("failed upserting GraphQLite node"));
    assert_eq!(
        evidence_item_count(&store, "workspace-default").expect("evidence count"),
        0
    );
    assert_eq!(
        brain_event_count(&store, "workspace-default").expect("brain event count"),
        0
    );
    assert_eq!(
        store
            .graph_snapshot_counts("workspace-default")
            .expect("graph counts"),
        KnowledgeGraphPersistReport {
            node_count: 0,
            relation_count: 0,
        }
    );
    assert_eq!(import_job_readiness(&store, "source-a"), (1, 0));
}

fn insert_citation_ready_import_job(store: &KnowledgeStore, source_id: &str) {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
            .connection()
            .sqlite_connection()
            .execute_batch(&format!(
                "INSERT INTO import_jobs
                   (job_id, workspace_id, source_id, status, citation_ready, graph_ready, created_at, updated_at, error_message)
                 VALUES ({job_id}, 'workspace-default', {source_id}, 'completed', 1, 0, 1, 1, NULL);",
                job_id = sql_literal(&format!("import:{source_id}")),
                source_id = sql_literal(source_id),
            ))
            .expect("insert citation-ready import job");
}

fn import_job_readiness(store: &KnowledgeStore, source_id: &str) -> (i64, i64) {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT citation_ready, graph_ready FROM import_jobs WHERE source_id = ?1",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read import job readiness")
}

fn import_job_status(store: &KnowledgeStore, source_id: &str) -> String {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT status FROM import_jobs WHERE source_id = ?1",
            [source_id],
            |row| row.get(0),
        )
        .expect("read import job status")
}

fn brain_event_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM brain_events WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query brain event count")?;
    Ok(count)
}

fn evidence_item_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM evidence_items WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query evidence item count")?;
    Ok(count)
}

fn target_node(label: &str, timestamp: u64) -> BrainNodeRecord {
    BrainNodeRecord {
        node_id: "node-target".into(),
        kind: BrainNodeKind::Concept,
        label: label.into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec!["evidence-a".into()],
        source_ids: vec!["source-a".into()],
        confidence: Some(0.8),
        updated_at: timestamp,
        valid_from: timestamp,
        valid_to: None,
        superseded_by: None,
    }
}

fn shared_relation(timestamp: u64) -> BrainRelationRecord {
    BrainRelationRecord {
        relation_id: "rel-shared".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: "node-source-a".into(),
        target_node_id: "node-target".into(),
        label: "relates".into(),
        evidence_ids: vec!["evidence-a".into()],
        confidence: Some(0.8),
        updated_at: timestamp,
        valid_from: timestamp,
        valid_to: None,
        superseded_by: None,
    }
}

fn logical_node_version_count(store: &KnowledgeStore, logical_id: &str) -> i64 {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*)
             FROM node_props_text logical
             JOIN property_keys logical_key ON logical_key.id = logical.key_id
             WHERE logical_key.key = 'logical_id'
               AND logical.value = ?1",
            [logical_id],
            |row| row.get(0),
        )
        .expect("count logical node versions")
}

fn live_logical_node_version_count(store: &KnowledgeStore, logical_id: &str) -> i64 {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*)
             FROM node_props_text logical
             JOIN property_keys logical_key ON logical_key.id = logical.key_id
             LEFT JOIN property_keys valid_to_key ON valid_to_key.key = 'valid_to'
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = logical.node_id
              AND valid_to.key_id = valid_to_key.id
             WHERE logical_key.key = 'logical_id'
               AND logical.value = ?1
               AND COALESCE(valid_to.value, 0) <= 0",
            [logical_id],
            |row| row.get(0),
        )
        .expect("count live logical node versions")
}

fn live_logical_node_label(store: &KnowledgeStore, logical_id: &str) -> String {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT label.value
             FROM node_props_text logical
             JOIN property_keys logical_key ON logical_key.id = logical.key_id
             JOIN property_keys label_key ON label_key.key = 'label'
             JOIN node_props_text label
               ON label.node_id = logical.node_id
              AND label.key_id = label_key.id
             LEFT JOIN property_keys valid_to_key ON valid_to_key.key = 'valid_to'
             LEFT JOIN node_props_int valid_to
               ON valid_to.node_id = logical.node_id
              AND valid_to.key_id = valid_to_key.id
             WHERE logical_key.key = 'logical_id'
               AND logical.value = ?1
               AND COALESCE(valid_to.value, 0) <= 0
             ORDER BY logical.node_id DESC
             LIMIT 1",
            [logical_id],
            |row| row.get(0),
        )
        .expect("read live logical node label")
}

fn logical_relation_version_count(store: &KnowledgeStore, logical_id: &str) -> i64 {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*)
             FROM edge_props_text relation
             JOIN property_keys relation_key ON relation_key.id = relation.key_id
             WHERE relation_key.key = 'relation_id'
               AND relation.value = ?1",
            [logical_id],
            |row| row.get(0),
        )
        .expect("count logical relation versions")
}

fn live_logical_relation_version_count(store: &KnowledgeStore, logical_id: &str) -> i64 {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*)
             FROM edge_props_text relation
             JOIN property_keys relation_key ON relation_key.id = relation.key_id
             LEFT JOIN property_keys valid_to_key ON valid_to_key.key = 'valid_to'
             LEFT JOIN edge_props_int valid_to
               ON valid_to.edge_id = relation.edge_id
              AND valid_to.key_id = valid_to_key.id
             WHERE relation_key.key = 'relation_id'
               AND relation.value = ?1
               AND COALESCE(valid_to.value, 0) <= 0",
            [logical_id],
            |row| row.get(0),
        )
        .expect("count live logical relation versions")
}

fn single_event_snapshot(
    event_id: &str,
    source_id: &str,
    evidence_id: &str,
    label: &str,
) -> BrainRepoSnapshot {
    BrainRepoSnapshot {
        workspace_id: "workspace-default".into(),
        generated_at: 10,
        sources: vec![SourceRecord {
            source_id: source_id.into(),
            workspace_id: "workspace-default".into(),
            original_path: format!("/tmp/{source_id}.pdf"),
            source_path: format!("sources/{source_id}.pdf"),
            markdown_path: format!("sources/{source_id}.md"),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 10,
        }],
        nodes: vec![BrainNodeRecord {
            node_id: format!("node-{source_id}"),
            kind: BrainNodeKind::Concept,
            label: label.into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence_id.into()],
            source_ids: vec![source_id.into()],
            confidence: Some(0.9),
            updated_at: 10,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        }],
        relations: Vec::new(),
        evidence: vec![EvidenceRef {
            id: evidence_id.into(),
            page_label: "p1".into(),
            page_index: Some(0),
            snippet: format!("{label} evidence."),
            source_path: Some(format!("sources/{source_id}.pdf")),
            source_id: Some(source_id.into()),
            markdown_path: Some(format!("sources/{source_id}.md")),
            image_path: None,
            provenance: Some("test".into()),
        }],
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![test_brain_event(
            event_id,
            "workspace-default",
            &[evidence_id],
        )],
    }
}

fn test_brain_event(event_id: &str, workspace_id: &str, evidence_refs: &[&str]) -> BrainEvent {
    BrainEvent {
        event_id: event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "test-agent".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: evidence_refs.iter().map(|value| (*value).into()).collect(),
        payload_json: "{}".into(),
        causality: BrainEventCausality::default(),
        confidence: None,
        policy_result: PolicyResult::materialized(),
        created_at: 10,
    }
}
