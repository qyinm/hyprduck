use super::super::*;

#[test]
fn graph_snapshot_is_persisted_as_current_graphqlite_workspace_graph() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    let snapshot = BrainRepoSnapshot {
        workspace_id: "workspace-default".into(),
        generated_at: 10,
        sources: vec![SourceRecord {
            source_id: "source-a".into(),
            workspace_id: "workspace-default".into(),
            original_path: "/Users/hyprduck/private/original-source-a.pdf".into(),
            source_path: "/Users/hyprduck/private/source-a.pdf".into(),
            markdown_path: "/Users/hyprduck/private/source-a.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 2,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 10,
        }],
        nodes: vec![
            BrainNodeRecord {
                node_id: "node-a".into(),
                kind: BrainNodeKind::Concept,
                label: "Alpha".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["evidence-a".into()],
                source_ids: vec!["source-a".into()],
                confidence: Some(0.9),
                updated_at: 10,
            },
            BrainNodeRecord {
                node_id: "node-b".into(),
                kind: BrainNodeKind::Concept,
                label: "Beta".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["evidence-b".into()],
                source_ids: vec!["source-a".into()],
                confidence: None,
                updated_at: 10,
            },
            BrainNodeRecord {
                node_id: "docs/private/unsafe-node".into(),
                kind: BrainNodeKind::Concept,
                label: "docs/private/roadmap.md".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["evidence-a".into()],
                source_ids: vec!["source-a".into()],
                confidence: None,
                updated_at: 10,
            },
            BrainNodeRecord {
                node_id: "node-windows-path".into(),
                kind: BrainNodeKind::Concept,
                label: "C:\\Users\\alice\\Secret.pdf".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["evidence-a".into()],
                source_ids: vec!["source-a".into()],
                confidence: None,
                updated_at: 10,
            },
            BrainNodeRecord {
                node_id: "node-embedded-path".into(),
                kind: BrainNodeKind::Concept,
                label: "Project at /Users/alice/private/source.pdf".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["evidence-a".into()],
                source_ids: vec!["source-a".into()],
                confidence: None,
                updated_at: 10,
            },
            BrainNodeRecord {
                node_id: "wiki-unsafe-alias".into(),
                kind: BrainNodeKind::WikiPage,
                label: "Unsafe Alias Wiki".into(),
                scope: BrainScope::Project,
                aliases: vec!["wiki/DOCS/PRIVATE/roadmap.md".into()],
                evidence_ids: vec!["evidence-a".into()],
                source_ids: vec!["source-a".into()],
                confidence: None,
                updated_at: 10,
            },
        ],
        relations: vec![
            BrainRelationRecord {
                relation_id: "rel-a".into(),
                kind: BrainRelationKind::RelatedTo,
                source_node_id: "node-a".into(),
                target_node_id: "node-b".into(),
                label: "relates".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            },
            BrainRelationRecord {
                relation_id: "rel-cites".into(),
                kind: BrainRelationKind::Cites,
                source_node_id: "claim-alpha".into(),
                target_node_id: "source:source-a".into(),
                label: "cites".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            },
            BrainRelationRecord {
                relation_id: "rel-links".into(),
                kind: BrainRelationKind::LinksTo,
                source_node_id: "wiki-alpha".into(),
                target_node_id: "entity-alpha".into(),
                label: "links".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            },
            BrainRelationRecord {
                relation_id: "docs/private/rel".into(),
                kind: BrainRelationKind::RelatedTo,
                source_node_id: "node-a".into(),
                target_node_id: "docs/private/unsafe-node".into(),
                label: "docs/private/relation.md".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            },
            BrainRelationRecord {
                relation_id: "rel-embedded-path".into(),
                kind: BrainRelationKind::Supports,
                source_node_id: "node-a".into(),
                target_node_id: "claim-alpha".into(),
                label: "See C:\\Users\\alice\\Secret.pdf".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            },
        ],
        evidence: vec![
            EvidenceRef {
                id: "evidence-a".into(),
                page_label: "p1".into(),
                page_index: Some(0),
                snippet: "Alpha relates to beta.".into(),
                source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                source_id: Some("source-a".into()),
                markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                provenance: Some("test".into()),
            },
            EvidenceRef {
                id: "evidence-b".into(),
                page_label: "p2".into(),
                page_index: Some(1),
                snippet: "Beta neighbor evidence.".into(),
                source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                source_id: Some("source-a".into()),
                markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                provenance: Some("test".into()),
            },
        ],
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-alpha".into(),
            workspace_id: "workspace-default".into(),
            path: "wiki/alpha".into(),
            title: "Alpha Wiki".into(),
            body: "# Overview\nAlpha wiki body.\n## Evidence\nAlpha cites source evidence.".into(),
            node_refs: Vec::new(),
            source_refs: vec!["source-a".into()],
            evidence_refs: vec!["evidence-a".into()],
            updated_at: 10,
        }],
        entities: vec![EntityRecord {
            entity_id: "entity-alpha".into(),
            workspace_id: "workspace-default".into(),
            kind: BrainNodeKind::Company,
            name: "Alpha Inc".into(),
            aliases: vec!["Alpha".into()],
            source_refs: vec!["source-a".into()],
            evidence_refs: vec!["evidence-a".into()],
            updated_at: 10,
        }],
        claims: vec![ClaimRecord {
            claim_id: "claim-alpha".into(),
            workspace_id: "workspace-default".into(),
            statement: "Alpha relates to beta.".into(),
            topic_refs: vec!["Alpha".into()],
            source_refs: vec!["source-a".into()],
            evidence_refs: vec!["evidence-a".into()],
            status: "supported".into(),
            updated_at: 10,
        }],
        extractions: vec![StructuredExtractionArtifact {
            artifact_id: "provider-run-alpha".into(),
            workspace_id: "workspace-default".into(),
            source_id: "source-a".into(),
            extractor: "test-extractor".into(),
            extractor_model: Some("test-model".into()),
            source_refs: vec!["source-a".into()],
            page_refs: Vec::new(),
            entities: Vec::new(),
            topics: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            memories: Vec::new(),
            evidence_refs: vec![EvidenceRef {
                id: "evidence-a".into(),
                page_label: "p1".into(),
                page_index: Some(0),
                snippet: "Alpha relates to beta.".into(),
                source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                source_id: Some("source-a".into()),
                markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                provenance: Some("test extraction".into()),
            }],
            confidence: Some(0.7),
            provenance: "test".into(),
            created_at: 10,
        }],
        events: vec![test_brain_event(
            "event-a",
            "workspace-default",
            &["evidence-a"],
        )],
    };

    let report = store
        .persist_graph_snapshot(&snapshot)
        .expect("persist graph snapshot");

    assert_eq!(
        report,
        KnowledgeGraphPersistReport {
            node_count: 10,
            relation_count: 5,
        }
    );
    assert_eq!(
        store
            .graph_snapshot_counts("workspace-default")
            .expect("graph counts"),
        report
    );
    assert_eq!(
        store
            .state_summary("workspace-default")
            .expect("state summary"),
        KnowledgeStoreStateSummary {
            evidence_item_count: 2,
            wiki_page_count: 1,
            graph_node_count: 10,
            graph_relation_count: 5,
        }
    );
    assert_graph_node_metadata(&store, "node-a");
    assert_graph_wiki_page_node(&store, "wiki-alpha");
    assert_wiki_relational_content(&store, "wiki-alpha");
    assert_source_page_fts_content(&store);
    assert_graph_edge_metadata(&store, "claim-alpha", "source:source-a", "CITES");
    assert_relational_proof_ignores_graph_metadata_tamper(&store);
    let hits = store
        .hybrid_retrieve("workspace-default", "Alpha", 5)
        .expect("hybrid retrieve");
    assert_eq!(hits.len(), 2);
    let alpha_hit = hits
        .iter()
        .find(|hit| hit.evidence_id == "evidence-a")
        .expect("alpha evidence hit");
    assert_eq!(alpha_hit.graph_neighbor_count, 1);
    let beta_neighbor_hit = hits
        .iter()
        .find(|hit| hit.evidence_id == "evidence-b")
        .expect("graph neighbor evidence hit");
    assert_eq!(beta_neighbor_hit.snippet, "Beta neighbor evidence.");
    update_source_context_pack_metadata(&store, "source-a");
    let context_pack = store
        .assemble_context_pack_v1_from_db(
            "workspace-default",
            "Alpha",
            5,
            "ctx_db_alpha".into(),
            "2026-05-29T09:53:26Z".into(),
        )
        .expect("assemble DB context pack v1");
    assert_eq!(
        context_pack.schema_version,
        hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION
    );
    assert_eq!(
        context_pack.retrieval_trace.strategy,
        "sqlite-graphqlite-fts5-hybrid"
    );
    assert!(context_pack
        .selected_evidence
        .iter()
        .any(|evidence| evidence.evidence_ref == "evidence-a"));
    let alpha_evidence = context_pack
        .selected_evidence
        .iter()
        .find(|evidence| evidence.evidence_ref == "evidence-a")
        .expect("alpha selected evidence");
    let graph_trail = alpha_evidence
        .graph_trail
        .as_ref()
        .expect("graph trail for alpha evidence");
    assert!(graph_trail
        .direct
        .iter()
        .any(|record| record.record_type == ContextPackGraphRecordKindV1::Node));
    assert!(graph_trail.direct.iter().any(|record| {
        record.record_type == ContextPackGraphRecordKindV1::Relation && record.id == "rel-a"
    }));
    assert!(graph_trail.direct.iter().any(|record| {
        record.record_type == ContextPackGraphRecordKindV1::WikiPage && record.id == "wiki-alpha"
    }));
    assert!(graph_trail.direct.iter().any(|record| {
        record.record_type == ContextPackGraphRecordKindV1::Claim && record.id == "claim-alpha"
    }));
    assert!(graph_trail
        .direct
        .iter()
        .chain(graph_trail.adjacent.iter())
        .all(|record| !record.id.contains("docs/private")));
    assert!(graph_trail
        .direct
        .iter()
        .chain(graph_trail.adjacent.iter())
        .all(|record| record.id != "node-windows-path"));
    assert!(graph_trail
        .direct
        .iter()
        .chain(graph_trail.adjacent.iter())
        .all(|record| record.id != "node-embedded-path" && record.id != "rel-embedded-path"));
    assert!(graph_trail.adjacent.iter().any(|record| {
        record.record_type == ContextPackGraphRecordKindV1::Node && record.id == "node-b"
    }));
    assert!(graph_trail.follow_up.iter().any(|follow_up| {
        follow_up.tool == ContextPackGraphFollowUpToolV1::ReadNode
            && follow_up.handle_type == ContextPackGraphHandleTypeV1::Node
            && matches!(
                &follow_up.arguments,
                ContextPackGraphFollowUpArgumentsV1::ReadNode(arguments)
                    if arguments.node_id == "node-a"
            )
    }));
    assert!(graph_trail.follow_up.iter().any(|follow_up| {
        follow_up.tool == ContextPackGraphFollowUpToolV1::ReadPageEvidence
            && follow_up.handle_type == ContextPackGraphHandleTypeV1::PageEvidence
            && matches!(
                &follow_up.arguments,
                ContextPackGraphFollowUpArgumentsV1::ReadPageEvidence(arguments)
                    if arguments.source_id == "source-a" && arguments.page == 1
            )
    }));
    assert!(graph_trail.follow_up.iter().any(|follow_up| {
        follow_up.tool == ContextPackGraphFollowUpToolV1::ReadWikiPage
            && follow_up.handle_type == ContextPackGraphHandleTypeV1::WikiPage
            && matches!(
                &follow_up.arguments,
                ContextPackGraphFollowUpArgumentsV1::ReadWikiPage(arguments)
                    if arguments.path == "wiki/alpha"
            )
    }));
    let context_pack_json = serde_json::to_string(&context_pack).expect("serialize context pack");
    assert!(!context_pack_json.contains("/Users/hyprduck/private"));
    assert!(!context_pack_json.contains("docs/private"));
    assert!(!context_pack_json.contains("DOCS/PRIVATE"));
    assert!(!context_pack_json.contains("/Users/alice"));
    assert!(!context_pack_json.contains("C:\\Users"));
    assert!(!context_pack_json.contains("../"));
    assert!(context_pack
        .retrieval_trace
        .evidence_type_trace
        .selected
        .get("text")
        .is_some_and(|count| *count >= 1));
    let source_response = store
        .read_source_from_db("workspace-default", "source-a", false)
        .expect("read source from DB")
        .expect("source response");
    assert_eq!(source_response.source.source_id, "source-a");
    assert_eq!(
        source_response.source.original_path,
        "original-source-a.pdf"
    );
    assert_eq!(source_response.source.source_path, "source-a.pdf");
    assert_eq!(source_response.source.markdown_path, "source-a.md");
    assert_eq!(source_response.evidence.len(), 2);
    let source_response_json =
        serde_json::to_string(&source_response).expect("serialize source response");
    assert!(!source_response_json.contains("/Users/hyprduck/private"));
    assert!(source_response
        .evidence
        .iter()
        .all(|evidence| evidence.source_path.as_deref() == Some("source-a.pdf")));
    assert!(source_response.evidence.iter().all(|evidence| {
        evidence.markdown_path.as_deref() == Some("source-a.md")
            && evidence.image_path.as_deref() == Some("source-a.png")
    }));
    assert_eq!(
        source_response
            .wiki_page
            .as_ref()
            .map(|page| page.page_id.as_str()),
        Some("wiki-alpha")
    );
    let page_response = store
        .read_page_evidence_from_db("workspace-default", "source-a", Some(1), false)
        .expect("read page evidence from DB")
        .expect("page evidence response");
    assert_eq!(page_response.source.source_id, "source-a");
    assert_eq!(page_response.evidence.len(), 1);
    assert_eq!(page_response.evidence[0].evidence_ref, "evidence-a");
    let page_response_json =
        serde_json::to_string(&page_response).expect("serialize page evidence response");
    assert!(!page_response_json.contains("/Users/hyprduck/private"));
    assert_eq!(
        page_response.evidence[0].markdown_path.as_deref(),
        Some("source-a.md")
    );
    assert_eq!(
        page_response.evidence[0].image_path.as_deref(),
        Some("source-a.png")
    );
    let wiki_page = store
        .read_wiki_page_from_db("workspace-default", "wiki/alpha")
        .expect("read wiki page from DB")
        .expect("wiki page");
    assert_eq!(wiki_page.page_id, "wiki-alpha");
    let node_response = store
        .read_node_from_db("workspace-default", "node-a")
        .expect("read node from DB")
        .expect("node response");
    assert_eq!(node_response.node.node_id, "node-a");
    assert!(node_response
        .relations
        .iter()
        .any(|relation| relation.relation_id == "rel-a"));
    assert!(node_response
        .relations
        .iter()
        .all(|relation| !relation.evidence_ids.is_empty()));
    let node_response_json =
        serde_json::to_string(&node_response).expect("serialize node response");
    assert!(!node_response_json.contains("/Users/hyprduck/private"));
    let (graph_nodes, graph_relations, graph_wiki_pages) = store
        .read_graph_canvas_projection_from_db("workspace-default")
        .expect("read graph canvas projection")
        .expect("graph canvas projection");
    assert!(graph_nodes.iter().any(|node| node.node_id == "node-a"));
    assert!(graph_nodes
        .iter()
        .any(|node| node.node_id == "source:source-a"));
    assert!(graph_relations
        .iter()
        .any(|relation| relation.relation_id == "rel-a"));
    assert_eq!(graph_wiki_pages.len(), 1);
    assert_eq!(graph_wiki_pages[0].page_id, "wiki-alpha");
    update_evidence_status(&store, "evidence-b", "failed");
    let filtered_hits = store
        .hybrid_retrieve("workspace-default", "Alpha", 5)
        .expect("filtered hybrid retrieve");
    assert!(filtered_hits
        .iter()
        .all(|hit| hit.evidence_id != "evidence-b"));
    let filtered_context_pack = store
        .assemble_context_pack_v1_from_db(
            "workspace-default",
            "Alpha",
            5,
            "ctx_db_alpha_filtered".into(),
            "2026-05-29T09:54:26Z".into(),
        )
        .expect("assemble filtered DB context pack v1");
    let filtered_alpha_trail = filtered_context_pack
        .selected_evidence
        .iter()
        .find(|evidence| evidence.evidence_ref == "evidence-a")
        .and_then(|evidence| evidence.graph_trail.as_ref())
        .expect("filtered alpha graph trail");
    assert!(!filtered_alpha_trail
        .adjacent
        .iter()
        .any(|record| record.id == "node-b"));
    let wiki_hits = store
        .hybrid_retrieve("workspace-default", "source evidence", 5)
        .expect("wiki hybrid retrieve");
    assert_eq!(wiki_hits.len(), 1);
    assert_eq!(wiki_hits[0].evidence_id, "evidence-a");
    assert_eq!(wiki_hits[0].source_id, "wiki-alpha");
    assert_eq!(wiki_hits[0].evidence_type, "wiki_evidence");
    assert_eq!(
        brain_event_count(&store, "workspace-default").expect("brain event count"),
        1
    );
    assert_eq!(
        graph_checkpoint_count(&store, "workspace-default").expect("checkpoint count"),
        1
    );
    assert_graph_checkpoint_metadata(&store, "workspace-default");
}

fn assert_wiki_relational_content(store: &KnowledgeStore, wiki_page_id: &str) {
    let graph = Graph::open(store.path()).expect("open graph");
    let sqlite = graph.connection().sqlite_connection();
    let page = sqlite
        .query_row(
            "SELECT path, approval_status, revision, evidence_refs_json
                 FROM wiki_pages
                 WHERE wiki_page_id = ?1",
            [wiki_page_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .expect("wiki page row");
    assert_eq!(page.0, "wiki/alpha");
    assert_eq!(page.1, "materialized");
    assert_eq!(page.2, 1);
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&page.3).expect("wiki evidence refs"),
        vec!["evidence-a"]
    );

    let revision = sqlite
        .query_row(
            "SELECT approval_status, diff_json, body
                 FROM wiki_revisions
                 WHERE wiki_page_id = ?1 AND revision = 1",
            [wiki_page_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("wiki revision row");
    assert_eq!(revision.0, "materialized");
    assert_eq!(revision.1, "{}");
    assert!(revision.2.contains("Alpha wiki body."));

    let sections = sqlite
        .query_row(
            "SELECT count(*) FROM wiki_sections WHERE wiki_page_id = ?1 AND revision = 1",
            [wiki_page_id],
            |row| row.get::<_, i64>(0),
        )
        .expect("wiki section count");
    assert_eq!(sections, 2);

    let fts_hits = sqlite
        .query_row(
            "SELECT count(*) FROM wiki_fts WHERE wiki_fts MATCH 'source'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("wiki fts count");
    assert_eq!(fts_hits, 1);
}

fn update_evidence_status(store: &KnowledgeStore, evidence_id: &str, status: &str) {
    let graph = Graph::open(store.path()).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .execute(
            "UPDATE evidence_items SET status = ?2 WHERE evidence_id = ?1",
            (evidence_id, status),
        )
        .expect("update evidence status");
}

fn update_source_context_pack_metadata(store: &KnowledgeStore, source_id: &str) {
    let graph = Graph::open(store.path()).expect("open graph");
    graph
        .connection()
        .sqlite_connection()
        .execute(
            "UPDATE sources
                 SET provider_route = 'test-local',
                     provider_locality = 'local',
                     content_hash = 'sha256:test-context-pack'
                 WHERE source_id = ?1",
            [source_id],
        )
        .expect("update source context metadata");
}

fn assert_source_page_fts_content(store: &KnowledgeStore) {
    let graph = Graph::open(store.path()).expect("open graph");
    let sqlite = graph.connection().sqlite_connection();
    let fts_hits = sqlite
        .query_row(
            "SELECT count(*) FROM source_page_fts WHERE source_page_fts MATCH 'relates'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("source page fts count");
    assert_eq!(fts_hits, 1);
}

#[test]
fn graph_snapshot_rejects_missing_relational_evidence_refs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    let snapshot = BrainRepoSnapshot {
        workspace_id: "workspace-default".into(),
        generated_at: 10,
        sources: Vec::new(),
        nodes: vec![BrainNodeRecord {
            node_id: "node-a".into(),
            kind: BrainNodeKind::Concept,
            label: "Alpha".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["missing-evidence".into()],
            source_ids: Vec::new(),
            confidence: None,
            updated_at: 10,
        }],
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![test_brain_event(
            "event-invalid",
            "workspace-default",
            &["missing-evidence"],
        )],
    };

    let error = store
        .persist_graph_snapshot(&snapshot)
        .expect_err("missing evidence ref should fail graph publish");

    assert!(error
        .to_string()
        .contains("references missing relational evidence row missing-evidence"));
    assert_eq!(
        brain_event_count(&store, "workspace-default").expect("brain event count"),
        0
    );
}

#[test]
fn wiki_content_rejects_missing_evidence_before_durable_rows_commit() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
        .expect("open knowledge store");
    let snapshot = BrainRepoSnapshot {
        workspace_id: "workspace-default".into(),
        generated_at: 10,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-missing-evidence".into(),
            workspace_id: "workspace-default".into(),
            path: "wiki/missing-evidence".into(),
            title: "Missing Evidence Wiki".into(),
            body: "This durable wiki content cites evidence that is not relationally present."
                .into(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: vec!["missing-evidence".into()],
            updated_at: 10,
        }],
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };

    let error = store
        .persist_graph_snapshot(&snapshot)
        .expect_err("wiki evidence ref validation should fail before commit");

    assert!(error
            .to_string()
            .contains("wiki page wiki-missing-evidence references missing relational evidence row missing-evidence"));
    assert_eq!(
        wiki_page_count(&store, "workspace-default").expect("wiki page count"),
        0
    );
    assert_eq!(
        wiki_revision_count(&store, "workspace-default").expect("wiki revision count"),
        0
    );
}

fn graph_checkpoint_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM graph_checkpoints WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query graph checkpoint count")?;
    Ok(count)
}

fn assert_graph_checkpoint_metadata(store: &KnowledgeStore, workspace_id: &str) {
    let graph = Graph::open(&store.path).expect("open graph");
    let row = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT checkpoint_id,
                        reason,
                        actor_json,
                        related_event_id,
                        graph_schema_version,
                        graphqlite_extension_version,
                        node_count,
                        edge_count,
                        evidence_ref_count,
                        checksum,
                        storage_ref
                 FROM graph_checkpoints
                 WHERE workspace_id = ?1",
            [workspace_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .expect("graph checkpoint metadata row");
    assert!(row
        .0
        .starts_with("graph-checkpoint-workspace-default-event-a-"));
    assert_eq!(row.1, "graph_snapshot_commit");
    assert!(row.2.contains("hyprduck-knowledge-store"));
    assert_eq!(row.3, "event-a");
    assert_eq!(row.4, GRAPHQLITE_SCHEMA_VERSION);
    assert_eq!(row.5, env!("CARGO_PKG_VERSION"));
    assert_eq!(row.6, 10);
    assert_eq!(row.7, 5);
    assert_eq!(row.8, 2);
    assert_eq!(row.9.len(), 64);
    assert_eq!(row.10, "hyprduck.sqlite:graphqlite");
}

fn wiki_page_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM wiki_pages WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query wiki page count")?;
    Ok(count)
}

fn wiki_revision_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM wiki_revisions WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query wiki revision count")?;
    Ok(count)
}

fn assert_graph_node_metadata(store: &KnowledgeStore, node_id: &str) {
    let graph = Graph::open(&store.path).expect("open graph");
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n {id: $node_id})
                 RETURN n.evidence_ids_json AS evidence_ids_json,
                        n.source_ids_json AS source_ids_json,
                        n.producer_run_id AS producer_run_id,
                        n.producer_run_ids_json AS producer_run_ids_json,
                        n.confidence AS confidence,
                        n.status AS status,
                        n.updated_at AS updated_at",
        )
        .param("node_id", node_id)
        .run()
        .expect("query graph node metadata");
    let row = rows.get(0).expect("graph node metadata row");
    assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
    assert_string_array(row, "source_ids_json", &["source-a"]);
    assert_eq!(
        row.get::<String>("producer_run_id")
            .expect("producer run id"),
        "provider-run-alpha"
    );
    assert_string_array(row, "producer_run_ids_json", &["provider-run-alpha"]);
    assert_eq!(row.get::<f64>("confidence").expect("confidence"), 0.9);
    assert_eq!(row.get::<String>("status").expect("status"), "active");
    assert_eq!(row.get::<i64>("updated_at").expect("updated at"), 10);
}

fn assert_graph_wiki_page_node(store: &KnowledgeStore, node_id: &str) {
    let graph = Graph::open(&store.path).expect("open graph");
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n:WikiPage {id: $node_id})
                 RETURN n.kind AS kind,
                        n.label AS label,
                        n.aliases_json AS aliases_json,
                        n.evidence_ids_json AS evidence_ids_json,
                        n.source_ids_json AS source_ids_json,
                        n.status AS status",
        )
        .param("node_id", node_id)
        .run()
        .expect("query wiki page graph node");
    let row = rows.get(0).expect("wiki page graph node row");
    assert_eq!(row.get::<String>("kind").expect("kind"), "wiki_page");
    assert_eq!(row.get::<String>("label").expect("label"), "Alpha Wiki");
    assert_string_array(row, "aliases_json", &["wiki/alpha"]);
    assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
    assert_string_array(row, "source_ids_json", &["source-a"]);
    assert_eq!(row.get::<String>("status").expect("status"), "active");
}

fn assert_graph_edge_metadata(
    store: &KnowledgeStore,
    source_node_id: &str,
    target_node_id: &str,
    relation_type: &str,
) {
    let graph = Graph::open(&store.path).expect("open graph");
    let rows = graph
        .connection()
        .cypher_builder(&format!(
            "MATCH (a {{id: $source_node_id}})-[r:{relation_type}]->(b {{id: $target_node_id}})
                 RETURN r.evidence_ids_json AS evidence_ids_json,
                        r.source_ids_json AS source_ids_json,
                        r.producer_run_id AS producer_run_id,
                        r.producer_run_ids_json AS producer_run_ids_json,
                        r.confidence AS confidence,
                        r.status AS status,
                        r.updated_at AS updated_at"
        ))
        .param("source_node_id", source_node_id)
        .param("target_node_id", target_node_id)
        .run()
        .expect("query graph edge metadata");
    let row = rows.get(0).expect("graph edge metadata row");
    assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
    assert_string_array(row, "source_ids_json", &["source-a"]);
    assert_eq!(
        row.get::<String>("producer_run_id")
            .expect("producer run id"),
        "provider-run-alpha"
    );
    assert_string_array(row, "producer_run_ids_json", &["provider-run-alpha"]);
    assert_eq!(row.get::<f64>("confidence").expect("confidence"), 0.8);
    assert_eq!(row.get::<String>("status").expect("status"), "active");
    assert_eq!(row.get::<i64>("updated_at").expect("updated at"), 10);
}

fn assert_relational_proof_ignores_graph_metadata_tamper(store: &KnowledgeStore) {
    let graph = Graph::open(&store.path).expect("open graph");
    graph
        .upsert_node(
            "node-a",
            [
                ("workspace_id", "workspace-default"),
                ("evidence_ids_json", "[\"graph-only-evidence\"]"),
                ("source_ids_json", "[\"graph-only-source\"]"),
            ],
            "Concept",
        )
        .expect("tamper GraphQLite node metadata");

    let proof = store
        .resolve_evidence_proof("workspace-default", "evidence-a")
        .expect("resolve relational evidence proof");
    assert_eq!(proof.evidence_id, "evidence-a");
    assert_eq!(proof.source_id, "source-a");
    assert_eq!(proof.page_index, Some(0));
    assert_eq!(proof.page_label, "p1");
    assert_eq!(proof.evidence_type, "text_evidence");
    assert_eq!(proof.snippet, "Alpha relates to beta.");
    assert_eq!(proof.status, "active");

    let error = store
        .resolve_evidence_proof("workspace-default", "graph-only-evidence")
        .expect_err("graph-only evidence ref must not resolve proof");
    assert!(error
        .to_string()
        .contains("missing relational evidence row graph-only-evidence"));
}

fn assert_string_array(row: &graphqlite::Row, column: &str, expected: &[&str]) {
    let values = match row.get_value(column).expect("array column exists") {
        graphqlite::Value::Array(values) => values
            .iter()
            .map(|value| match value {
                graphqlite::Value::String(value) => value.clone(),
                other => panic!("unexpected array value for {column}: {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("unexpected value for {column}: {other:?}"),
    };
    assert_eq!(values, expected);
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
