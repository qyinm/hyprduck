use super::common::*;
use super::*;

#[test]
fn provider_workspace_rebuild_response_materializes_complete_graph_snapshot() {
    let generated_at = 42;
    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at,
        sources: vec![
            SourceRecord {
                source_id: "source-alpha".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "/tmp/alpha.md".into(),
                source_path: "/tmp/alpha.md".into(),
                markdown_path: "/tmp/alpha.md".into(),
                format: "markdown".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: generated_at,
            },
            SourceRecord {
                source_id: "source-beta".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "/tmp/beta.md".into(),
                source_path: "/tmp/beta.md".into(),
                markdown_path: "/tmp/beta.md".into(),
                format: "markdown".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: generated_at,
            },
        ],
        evidence: vec![
            EvidenceRef {
                id: "ev-alpha".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Alpha source describes agent-ready workspace graphs.".into(),
                source_path: Some("/tmp/alpha.md".into()),
                source_id: Some("source-alpha".into()),
                markdown_path: Some("/tmp/alpha.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            },
            EvidenceRef {
                id: "ev-beta".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Beta source connects the graph to workspace rebuilds.".into(),
                source_path: Some("/tmp/beta.md".into()),
                source_id: Some("source-beta".into()),
                markdown_path: Some("/tmp/beta.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            },
        ],
        nodes: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    let raw = serde_json::json!({
        "materializedGraph": {
            "generatedAt": generated_at,
            "sources": [],
            "evidence": [],
            "nodes": [
                {
                    "nodeId": "source:source-alpha",
                    "kind": "source",
                    "label": "Alpha",
                    "scope": "project",
                    "aliases": [],
                    "evidenceIds": ["ev-alpha"],
                    "sourceIds": ["source-alpha"],
                    "confidence": 1.0,
                    "updatedAt": generated_at
                },
                {
                    "nodeId": "source:source-beta",
                    "kind": "source",
                    "label": "Beta",
                    "scope": "project",
                    "aliases": [],
                    "evidenceIds": ["ev-beta"],
                    "sourceIds": ["source-beta"],
                    "confidence": 1.0,
                    "updatedAt": generated_at
                },
                {
                    "nodeId": "concept-workspace-rebuild",
                    "kind": "concept",
                    "label": "Workspace rebuild",
                    "scope": "project",
                    "aliases": [],
                    "evidenceIds": ["ev-alpha", "ev-beta"],
                    "sourceIds": ["source-alpha", "source-beta"],
                    "confidence": 0.86,
                    "updatedAt": generated_at
                }
            ],
            "edges": [
                {
                    "relationId": "edge-alpha-beta-rebuild",
                    "kind": "related_to",
                    "sourceNodeId": "source:source-alpha",
                    "targetNodeId": "concept-workspace-rebuild",
                    "label": "supports workspace rebuild",
                    "evidenceIds": ["ev-alpha", "ev-beta"],
                    "confidence": 0.81,
                    "updatedAt": generated_at
                }
            ],
            "claims": [
                {
                    "claimId": "claim-workspace-rebuild",
                    "workspaceId": "default",
                    "statement": "The workspace graph is rebuilt from all source evidence.",
                    "topicRefs": ["concept-workspace-rebuild"],
                    "sourceRefs": ["source-alpha", "source-beta"],
                    "evidenceRefs": ["ev-alpha", "ev-beta"],
                    "status": "supported",
                    "updatedAt": generated_at
                }
            ],
            "memories": [],
            "wikiPages": [],
            "entities": [],
            "extractions": []
        }
    })
    .to_string();

    let mut snapshot =
        parse_provider_workspace_rebuild_snapshot(&raw).expect("parse workspace rebuild");
    normalize_provider_workspace_rebuild_snapshot(
        &mut snapshot,
        DEFAULT_WORKSPACE_ID,
        &baseline,
        generated_at,
    );

    validate_provider_workspace_rebuild_snapshot(&snapshot, &baseline)
        .expect("provider rebuild snapshot is valid");
    assert_eq!(snapshot.sources, baseline.sources);
    assert_eq!(snapshot.evidence, baseline.evidence);
    assert!(snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-alpha-beta-rebuild"));
}

#[test]
fn provider_workspace_rebuild_drops_invalid_refs_without_losing_valid_graph() {
    let generated_at = 42;
    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at,
        sources: vec![SourceRecord {
            source_id: "source-alpha".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/tmp/alpha.md".into(),
            source_path: "/tmp/alpha.md".into(),
            markdown_path: "/tmp/alpha.md".into(),
            format: "markdown".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: generated_at,
        }],
        evidence: vec![EvidenceRef {
            id: "ev-alpha".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Alpha source describes a durable signal.".into(),
            source_path: Some("/tmp/alpha.md".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/alpha.md".into()),
            image_path: None,
            provenance: Some("test".into()),
        }],
        nodes: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    let raw = serde_json::json!({
        "materializedGraph": {
            "sources": [],
            "evidence": [],
            "nodes": [
                {
                    "nodeId": "source:source-alpha",
                    "kind": "source",
                    "label": "Alpha",
                    "scope": "project",
                    "evidenceIds": ["ev-alpha"],
                    "sourceIds": ["source-alpha"],
                    "updatedAt": generated_at
                },
                {
                    "nodeId": "concept-durable-signal",
                    "kind": "concept",
                    "label": "Durable Signal",
                    "scope": "project",
                    "evidenceIds": ["ev-alpha", "retrieved:stale:chunk"],
                    "sourceIds": ["source-alpha", "source-stale"],
                    "updatedAt": generated_at
                }
            ],
            "edges": [
                {
                    "relationId": "edge-alpha-signal",
                    "kind": "related_to",
                    "sourceNodeId": "source:source-alpha",
                    "targetNodeId": "concept-durable-signal",
                    "label": "describes",
                    "evidenceIds": ["ev-alpha", "ev-stale"],
                    "updatedAt": generated_at
                }
            ],
            "claims": [],
            "memories": [
                {
                    "memoryId": "memory-stale-only",
                    "workspaceId": "default",
                    "scope": "project",
                    "title": "Stale provider memory",
                    "body": "This memory cites only invalid refs.",
                    "sourceRefs": ["source-stale"],
                    "evidenceRefs": ["retrieved:stale:chunk"],
                    "createdAt": generated_at,
                    "updatedAt": generated_at
                }
            ],
            "wikiPages": [],
            "entities": [],
            "extractions": []
        }
    })
    .to_string();

    let mut snapshot =
        parse_provider_workspace_rebuild_snapshot(&raw).expect("parse workspace rebuild");
    normalize_provider_workspace_rebuild_snapshot(
        &mut snapshot,
        DEFAULT_WORKSPACE_ID,
        &baseline,
        generated_at,
    );

    validate_provider_workspace_rebuild_snapshot(&snapshot, &baseline)
        .expect("provider rebuild snapshot is valid after stale refs are removed");
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-durable-signal"));
    assert!(snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-alpha-signal"));
    assert!(snapshot.memories.is_empty());
}

#[test]
fn provider_source_local_graph_adds_source_edges_for_imported_concepts() {
    let generated_at = 42;
    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at,
        sources: vec![SourceRecord {
            source_id: "source-alpha".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/tmp/alpha.md".into(),
            source_path: "/tmp/alpha.md".into(),
            markdown_path: "/tmp/alpha.md".into(),
            format: "markdown".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: generated_at,
        }],
        evidence: vec![EvidenceRef {
            id: "ev-alpha".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Alpha source describes graph traversal.".into(),
            source_path: Some("/tmp/alpha.md".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/alpha.md".into()),
            image_path: None,
            provenance: Some("test".into()),
        }],
        nodes: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    let raw = serde_json::json!({
        "materializedGraph": {
            "sources": [],
            "evidence": [],
            "nodes": [
                {
                    "nodeId": "concept-traversal",
                    "kind": "concept",
                    "label": "Graph traversal",
                    "scope": "project",
                    "evidenceIds": ["ev-alpha"],
                    "sourceIds": ["source-alpha"],
                    "updatedAt": generated_at
                }
            ],
            "edges": [],
            "claims": [],
            "memories": [],
            "wikiPages": [],
            "entities": [],
            "extractions": []
        }
    })
    .to_string();

    let mut snapshot = parse_provider_workspace_rebuild_snapshot(&raw).expect("parse source graph");
    normalize_provider_source_local_graph_snapshot(
        &mut snapshot,
        DEFAULT_WORKSPACE_ID,
        &baseline,
        "source-alpha",
        generated_at,
    );

    validate_provider_source_local_graph_snapshot(&snapshot, "source-alpha")
        .expect("source-local graph is valid");
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "source:source-alpha"));
    assert!(snapshot.relations.iter().any(|relation| {
        relation.kind == BrainRelationKind::SourceOf
            && relation.source_node_id == "source:source-alpha"
            && relation.target_node_id == "concept-traversal"
    }));
}

#[test]
fn provider_workspace_linking_keeps_only_cross_source_relations() {
    let generated_at = 42;
    let mut baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at,
        sources: vec![
            SourceRecord {
                source_id: "source-alpha".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "/tmp/alpha.md".into(),
                source_path: "/tmp/alpha.md".into(),
                markdown_path: "/tmp/alpha.md".into(),
                format: "markdown".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: generated_at,
            },
            SourceRecord {
                source_id: "source-beta".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "/tmp/beta.md".into(),
                source_path: "/tmp/beta.md".into(),
                markdown_path: "/tmp/beta.md".into(),
                format: "markdown".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: generated_at,
            },
        ],
        evidence: vec![
            EvidenceRef {
                id: "ev-alpha".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Alpha source describes traversal.".into(),
                source_path: Some("/tmp/alpha.md".into()),
                source_id: Some("source-alpha".into()),
                markdown_path: Some("/tmp/alpha.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            },
            EvidenceRef {
                id: "ev-beta".into(),
                page_label: "Page 1".into(),
                page_index: Some(0),
                snippet: "Beta source describes workspace graphs.".into(),
                source_path: Some("/tmp/beta.md".into()),
                source_id: Some("source-beta".into()),
                markdown_path: Some("/tmp/beta.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            },
        ],
        nodes: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    baseline.nodes = vec![
        BrainNodeRecord {
            node_id: "concept-alpha".into(),
            kind: BrainNodeKind::Concept,
            label: "Traversal".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["ev-alpha".into()],
            source_ids: vec!["source-alpha".into()],
            confidence: Some(0.9),
            updated_at: generated_at,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        },
        BrainNodeRecord {
            node_id: "concept-beta".into(),
            kind: BrainNodeKind::Concept,
            label: "Workspace graph".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["ev-beta".into()],
            source_ids: vec!["source-beta".into()],
            confidence: Some(0.9),
            updated_at: generated_at,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        },
        BrainNodeRecord {
            node_id: "concept-shared".into(),
            kind: BrainNodeKind::Concept,
            label: "Already shared graph".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["ev-alpha".into(), "ev-beta".into()],
            source_ids: vec!["source-alpha".into(), "source-beta".into()],
            confidence: Some(0.9),
            updated_at: generated_at,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        },
    ];
    let raw = serde_json::json!({
        "materializedGraph": {
            "sources": [],
            "evidence": [],
            "nodes": [],
            "edges": [
                {
                    "relationId": "edge-cross",
                    "kind": "related_to",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-beta",
                    "label": "connects to",
                    "evidenceIds": ["ev-alpha", "ev-beta"],
                    "updatedAt": generated_at
                },
                {
                    "relationId": "edge-local",
                    "kind": "related_to",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-alpha",
                    "label": "local only",
                    "evidenceIds": ["ev-alpha"],
                    "updatedAt": generated_at
                },
                {
                    "relationId": "edge-undercovered",
                    "kind": "related_to",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-beta",
                    "label": "missing beta evidence",
                    "evidenceIds": ["ev-alpha"],
                    "updatedAt": generated_at
                },
                {
                    "relationId": "edge-multisource",
                    "kind": "related_to",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-shared",
                    "label": "already shared endpoint",
                    "evidenceIds": ["ev-alpha", "ev-beta"],
                    "updatedAt": generated_at
                },
                {
                    "relationId": "edge-provider-source-of",
                    "kind": "source_of",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-beta",
                    "label": "provider source edge",
                    "evidenceIds": ["ev-alpha", "ev-beta"],
                    "updatedAt": generated_at
                }
            ],
            "claims": [],
            "memories": [],
            "wikiPages": [],
            "entities": [],
            "extractions": []
        }
    })
    .to_string();

    let mut snapshot =
        parse_provider_workspace_rebuild_snapshot(&raw).expect("parse workspace linking");
    normalize_provider_workspace_linking_snapshot(
        &mut snapshot,
        DEFAULT_WORKSPACE_ID,
        &baseline,
        "source-alpha",
        generated_at,
    );

    validate_provider_workspace_linking_snapshot(&snapshot, &baseline, "source-alpha")
        .expect("workspace linking graph is valid");
    assert!(snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-cross"));
    assert!(!snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-local"));
    assert!(!snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-undercovered"));
    assert!(!snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-multisource"));
    assert!(!snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-provider-source-of"));

    let mut relation_undercovered = snapshot.clone();
    relation_undercovered
        .relations
        .iter_mut()
        .find(|relation| relation.relation_id == "edge-cross")
        .expect("cross relation")
        .evidence_ids = vec!["ev-alpha".into()];
    let relation_error = validate_provider_workspace_linking_snapshot(
        &relation_undercovered,
        &baseline,
        "source-alpha",
    )
    .expect_err("cross-source relation must be backed by evidence from both endpoint sources");
    assert!(format!("{relation_error:#}")
        .contains("evidence does not cover both endpoint source sides"));

    let mut claim_undercovered = snapshot.clone();
    claim_undercovered.claims.push(ClaimRecord {
        claim_id: "claim-undercovered".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Alpha and beta are linked.".into(),
        topic_refs: vec!["concept-alpha".into()],
        source_refs: vec!["source-alpha".into(), "source-beta".into()],
        evidence_refs: vec!["ev-alpha".into()],
        status: "active".into(),
        updated_at: generated_at,
    });
    let claim_error = validate_provider_workspace_linking_snapshot(
        &claim_undercovered,
        &baseline,
        "source-alpha",
    )
    .expect_err("claim source refs must be covered by supporting evidence");
    assert!(format!("{claim_error:#}")
        .contains("claim claim-undercovered evidence does not cover all source refs"));

    let mut memory_undercovered = snapshot.clone();
    memory_undercovered.memories.push(MemoryRecord {
        memory_id: "memory-undercovered".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Undercovered memory".into(),
        body: "Alpha and beta memory.".into(),
        source_refs: vec!["source-alpha".into(), "source-beta".into()],
        evidence_refs: vec!["ev-alpha".into()],
        created_at: generated_at,
        updated_at: generated_at,
    });
    let memory_error = validate_provider_workspace_linking_snapshot(
        &memory_undercovered,
        &baseline,
        "source-alpha",
    )
    .expect_err("memory source refs must be covered by supporting evidence");
    assert!(format!("{memory_error:#}")
        .contains("memory memory-undercovered evidence does not cover all source refs"));

    let mut wiki_undercovered = snapshot.clone();
    wiki_undercovered.wiki_pages.push(WikiPage {
        page_id: "wiki-undercovered".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/undercovered.md".into(),
        title: "Undercovered Wiki".into(),
        body: "Alpha and beta wiki page.".into(),
        node_refs: vec!["concept-alpha".into()],
        source_refs: vec!["source-alpha".into(), "source-beta".into()],
        evidence_refs: vec!["ev-alpha".into()],
        updated_at: generated_at,
    });
    let wiki_error =
        validate_provider_workspace_linking_snapshot(&wiki_undercovered, &baseline, "source-alpha")
            .expect_err("wiki source refs must be covered by supporting evidence");
    assert!(format!("{wiki_error:#}")
        .contains("wiki page wiki/undercovered.md evidence does not cover all source refs"));
}

#[test]
fn provider_workspace_linking_caps_relations_claims_and_drops_wiki_pages() {
    let generated_at = 42;
    let mut baseline = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    baseline.generated_at = generated_at;
    baseline.sources = vec![
        SourceRecord {
            source_id: "source-alpha".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/tmp/alpha.md".into(),
            source_path: "/tmp/alpha.md".into(),
            markdown_path: "/tmp/alpha.md".into(),
            format: "markdown".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: generated_at,
        },
        SourceRecord {
            source_id: "source-beta".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/tmp/beta.md".into(),
            source_path: "/tmp/beta.md".into(),
            markdown_path: "/tmp/beta.md".into(),
            format: "markdown".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: generated_at,
        },
    ];
    baseline.evidence = vec![
        EvidenceRef {
            id: "ev-alpha".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Alpha evidence.".into(),
            source_path: Some("/tmp/alpha.md".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some("/tmp/alpha.md".into()),
            image_path: None,
            provenance: Some("test".into()),
        },
        EvidenceRef {
            id: "ev-beta".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Beta evidence.".into(),
            source_path: Some("/tmp/beta.md".into()),
            source_id: Some("source-beta".into()),
            markdown_path: Some("/tmp/beta.md".into()),
            image_path: None,
            provenance: Some("test".into()),
        },
    ];
    baseline.nodes = vec![
        BrainNodeRecord {
            node_id: "concept-alpha".into(),
            kind: BrainNodeKind::Concept,
            label: "Alpha".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["ev-alpha".into()],
            source_ids: vec!["source-alpha".into()],
            confidence: Some(0.9),
            updated_at: generated_at,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
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
            updated_at: generated_at,
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        },
    ];
    let edges = (0..30)
        .map(|index| {
            serde_json::json!({
                "relationId": format!("edge-cross-{index:02}"),
                "kind": "related_to",
                "sourceNodeId": "concept-alpha",
                "targetNodeId": "concept-beta",
                "label": "connects",
                "evidenceIds": ["ev-alpha", "ev-beta"],
                "confidence": 0.9,
                "updatedAt": generated_at
            })
        })
        .collect::<Vec<_>>();
    let claims = (0..10)
        .map(|index| {
            serde_json::json!({
                "claimId": format!("claim-cross-{index:02}"),
                "workspaceId": DEFAULT_WORKSPACE_ID,
                "statement": format!("Alpha and beta claim {index}."),
                "topicRefs": ["concept-alpha"],
                "sourceRefs": ["source-alpha", "source-beta"],
                "evidenceRefs": ["ev-alpha", "ev-beta"],
                "status": "active",
                "updatedAt": generated_at
            })
        })
        .collect::<Vec<_>>();
    let wiki_pages = (0..5)
        .map(|index| {
            serde_json::json!({
                "pageId": format!("wiki-cross-{index:02}"),
                "workspaceId": DEFAULT_WORKSPACE_ID,
                "path": format!("wiki/cross-{index:02}.md"),
                "title": format!("Cross {index}"),
                "body": "Cross-source summary.",
                "nodeRefs": ["concept-alpha"],
                "sourceRefs": ["source-alpha", "source-beta"],
                "evidenceRefs": ["ev-alpha", "ev-beta"],
                "updatedAt": generated_at
            })
        })
        .collect::<Vec<_>>();
    let memories = (0..10)
        .map(|index| {
            serde_json::json!({
                "memoryId": format!("memory-cross-{index:02}"),
                "workspaceId": DEFAULT_WORKSPACE_ID,
                "scope": "project",
                "title": format!("Cross memory {index}"),
                "body": "Workspace linking memories are not materialized from providers.",
                "sourceRefs": ["source-alpha", "source-beta"],
                "evidenceRefs": ["ev-alpha", "ev-beta"],
                "createdAt": generated_at,
                "updatedAt": generated_at
            })
        })
        .collect::<Vec<_>>();
    let raw = serde_json::json!({
        "materializedGraph": {
            "sources": [],
            "evidence": [],
            "nodes": [],
            "edges": edges,
            "claims": claims,
            "memories": memories,
            "wikiPages": wiki_pages,
            "entities": [],
            "extractions": []
        }
    })
    .to_string();

    let mut snapshot =
        parse_provider_workspace_rebuild_snapshot(&raw).expect("parse workspace linking");
    normalize_provider_workspace_linking_snapshot(
        &mut snapshot,
        DEFAULT_WORKSPACE_ID,
        &baseline,
        "source-alpha",
        generated_at,
    );

    validate_provider_workspace_linking_snapshot(&snapshot, &baseline, "source-alpha")
        .expect("capped workspace linking graph is valid");
    assert_eq!(snapshot.relations.len(), 24);
    assert_eq!(snapshot.claims.len(), 8);
    assert!(snapshot.memories.is_empty());
    assert!(snapshot.wiki_pages.is_empty());
}

#[test]
fn workspace_linking_prompt_uses_only_active_workspace_source_chunks() {
    let temp = tempdir().expect("tempdir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let import_manifest = sample_manifest_with_source(&temp, "source-import", "import", 1);
    let active_manifest = sample_manifest_with_source(&temp, "source-active", "active", 1);
    let stale_manifest = sample_manifest_with_source(&temp, "source-stale", "stale", 1);
    let import_markdown = "# Import\n\nShared alpha imported source text.";
    let active_markdown = "# Active\n\nActive candidate alpha source text.";
    let stale_markdown = "# Stale\n\nStale alpha source-index text must not enter rebuild prompt.";
    let import_chunks = chunk_source_markdown(&import_manifest, import_markdown);
    let active_chunks = chunk_source_markdown(&active_manifest, active_markdown);
    let stale_chunks = chunk_source_markdown(&stale_manifest, stale_markdown);
    upsert_source_chunks(&workspace_root, &import_manifest, &import_chunks)
        .expect("write import chunks");
    upsert_source_chunks(&workspace_root, &active_manifest, &active_chunks)
        .expect("write active chunks");
    upsert_source_chunks(&workspace_root, &stale_manifest, &stale_chunks)
        .expect("write stale chunks");

    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![
            SourceRecord {
                source_id: import_manifest.source_id.clone(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: import_manifest.original_path.clone(),
                source_path: import_manifest.source_path.clone(),
                markdown_path: import_manifest.markdown_path.clone(),
                format: "pdf".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
            SourceRecord {
                source_id: active_manifest.source_id.clone(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: active_manifest.original_path.clone(),
                source_path: active_manifest.source_path.clone(),
                markdown_path: active_manifest.markdown_path.clone(),
                format: "pdf".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
        ],
        evidence: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    let context = build_import_evidence_context(
        &workspace_root,
        &import_manifest,
        import_markdown,
        &baseline,
        &import_chunks,
    )
    .expect("context");

    let prompt = build_workspace_linking_prompt(
        &workspace_root,
        DEFAULT_WORKSPACE_ID,
        &import_manifest,
        import_markdown,
        &baseline,
        &context,
    )
    .expect("prompt");

    assert!(prompt.contains("Active candidate alpha source text."));
    assert!(
        prompt.contains("Every returned edge must cite evidence from both endpoint source sides")
    );
    assert!(prompt.contains("Do not return source_of edges"));
    assert!(prompt.contains("Return memories as []"));
    assert!(prompt.contains("grounded cross-source links such as shared concepts"));
    assert!(!prompt.contains("algorithms, data structures"));
    assert!(!prompt.contains("Stale source-index text must not enter rebuild prompt."));
    assert!(!prompt.contains("source-stale"));
}

#[test]
fn workspace_linking_prompt_uses_canonical_graph_only() {
    let temp = tempdir().expect("tempdir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("artifacts/source-import/provider-graph-candidates"))
        .expect("candidate artifact dir");
    fs::write(
        workspace_root.join("artifacts/source-import/provider-graph-candidates/raw.json"),
        r#"{"rawNodeId":"raw-candidate-must-not-enter-linking-prompt"}"#,
    )
    .expect("raw candidate artifact");

    let import_manifest = sample_manifest_with_source(&temp, "source-import", "import", 1);
    let active_manifest = sample_manifest_with_source(&temp, "source-active", "active", 1);
    let import_markdown = "# Import\n\nShared canonical imported source text.";
    let active_markdown = "# Active\n\nShared canonical active source text.";
    let import_chunks = chunk_source_markdown(&import_manifest, import_markdown);
    let active_chunks = chunk_source_markdown(&active_manifest, active_markdown);
    upsert_source_chunks(&workspace_root, &import_manifest, &import_chunks)
        .expect("write import chunks");
    upsert_source_chunks(&workspace_root, &active_manifest, &active_chunks)
        .expect("write active chunks");

    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![
            SourceRecord {
                source_id: import_manifest.source_id.clone(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: import_manifest.original_path.clone(),
                source_path: import_manifest.source_path.clone(),
                markdown_path: import_manifest.markdown_path.clone(),
                format: "pdf".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
            SourceRecord {
                source_id: active_manifest.source_id.clone(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: active_manifest.original_path.clone(),
                source_path: active_manifest.source_path.clone(),
                markdown_path: active_manifest.markdown_path.clone(),
                format: "pdf".into(),
                status: "ingested".into(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
        ],
        evidence: Vec::new(),
        nodes: vec![
            BrainNodeRecord {
                node_id: "concept:source-import:canonical-alpha".into(),
                kind: BrainNodeKind::Concept,
                label: "Canonical alpha".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: Vec::new(),
                source_ids: vec![import_manifest.source_id.clone()],
                confidence: Some(0.9),
                updated_at: 1,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            },
            BrainNodeRecord {
                node_id: "concept:source-active:canonical-beta".into(),
                kind: BrainNodeKind::Concept,
                label: "Canonical beta".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: Vec::new(),
                source_ids: vec![active_manifest.source_id.clone()],
                confidence: Some(0.9),
                updated_at: 1,
                valid_from: 0,
                valid_to: None,
                superseded_by: None,
            },
        ],
        relations: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    let context = build_import_evidence_context(
        &workspace_root,
        &import_manifest,
        import_markdown,
        &baseline,
        &import_chunks,
    )
    .expect("context");

    let prompt = build_workspace_linking_prompt(
        &workspace_root,
        DEFAULT_WORKSPACE_ID,
        &import_manifest,
        import_markdown,
        &baseline,
        &context,
    )
    .expect("prompt");

    assert!(prompt.contains("concept:source-import:canonical-alpha"));
    assert!(prompt.contains("concept:source-active:canonical-beta"));
    assert!(!prompt.contains("raw-candidate-must-not-enter-linking-prompt"));
    assert!(!prompt.contains("provider-graph-candidates"));
}
