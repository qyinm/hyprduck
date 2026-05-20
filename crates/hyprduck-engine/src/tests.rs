use super::*;
use crate::provider::{ollama_models_endpoint, ProviderKind};

static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn compile_fixture_project(temp: &tempfile::TempDir, markdown: &str) -> KnowledgeProject {
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some("/tmp/source.pdf".into()),
        source_manifest_path: None,
        workspace_id: None,
        source_id: None,
        skip_graph_generation: None,
    };

    let markdown = fs::read_to_string(&markdown_path).expect("read markdown");
    compile_knowledge_project(&request, &markdown, None)
}

fn compile_manifest_fixture_project_with_source(
    temp: &tempfile::TempDir,
    markdown: &str,
    source_id: &str,
    output_name: &str,
    updated_at: u64,
) -> (KnowledgeProject, SourceArtifactManifest) {
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest_with_source(temp, source_id, output_name, updated_at);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };

    (
        compile_knowledge_project(&request, markdown, Some(&manifest)),
        manifest,
    )
}

fn synthetic_projection_project(
    project_id: &str,
    source_count: usize,
    concept_count: usize,
    related_relation_count: usize,
) -> KnowledgeProject {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut details_by_node_id = BTreeMap::new();
    let mut edge_details_by_id = BTreeMap::new();
    let mut answer_by_node_id = BTreeMap::new();

    for source_index in 0..source_count {
        let node_id = format!("source:projection-{source_index}");
        let node = GraphNodeSummary {
            id: node_id.clone(),
            label: format!("Projection source {source_index}"),
            kind: GraphNodeKind::Source,
            confidence: Some(0.8),
            related_count: 0,
            evidence_count: 1,
            position: GraphNodePosition { x: 10.0, y: 10.0 },
        };
        nodes.push(node.clone());
        details_by_node_id.insert(
            node_id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: node.label.clone(),
                aliases: Vec::new(),
                description: "Synthetic source for projection tests.".into(),
                evidence: Vec::new(),
                actions: Vec::new(),
                source: None,
            },
        );
        answer_by_node_id.insert(
            node_id,
            AnswerResponse {
                status: AnswerStatus::LowConfidence,
                text: None,
                explanation: "Synthetic source answer.".into(),
                citations: Vec::new(),
                related_node_ids: Vec::new(),
                suggested_actions: Vec::new(),
            },
        );
    }

    let mut evidence_by_concept = Vec::new();
    for concept_index in 0..concept_count {
        let node_id = format!("concept-projection-{concept_index:03}");
        let evidence_count = concept_count - concept_index;
        let evidence = (0..evidence_count.min(5))
            .map(|evidence_index| EvidenceRef {
                id: format!("ev-projection-{concept_index:03}-{evidence_index}"),
                page_label: format!("Page {}", evidence_index + 1),
                page_index: Some(evidence_index),
                snippet: format!(
                    "Projection concept {concept_index} has evidence {evidence_index}."
                ),
                source_path: Some(format!("/tmp/projection/source-{concept_index}.pdf")),
                source_id: Some(format!(
                    "source-projection-{}",
                    concept_index % source_count.max(1)
                )),
                markdown_path: Some(format!("/tmp/projection/source-{concept_index}.md")),
                image_path: None,
                provenance: Some("synthetic projection fixture".into()),
            })
            .collect::<Vec<_>>();
        evidence_by_concept.push(evidence.clone());
        let node = GraphNodeSummary {
            id: node_id.clone(),
            label: format!("Projection Concept {concept_index:03}"),
            kind: GraphNodeKind::Concept,
            confidence: Some(0.9 - (concept_index as f32 * 0.001).min(0.5)),
            related_count: 0,
            evidence_count,
            position: GraphNodePosition { x: 50.0, y: 50.0 },
        };
        nodes.push(node.clone());
        details_by_node_id.insert(
            node_id.clone(),
            GraphNodeDetail {
                node: node.clone(),
                canonical_name: node.label.clone(),
                aliases: Vec::new(),
                description: "Synthetic concept for projection tests.".into(),
                evidence: evidence.clone(),
                actions: Vec::new(),
                source: None,
            },
        );
        answer_by_node_id.insert(
            node_id.clone(),
            AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some("Synthetic concept answer.".into()),
                explanation: "Synthetic answer.".into(),
                citations: evidence.iter().take(1).cloned().collect(),
                related_node_ids: Vec::new(),
                suggested_actions: Vec::new(),
            },
        );

        let source_node_id = format!("source:projection-{}", concept_index % source_count.max(1));
        let edge_id = format!("edge-source-projection-{concept_index:03}");
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id,
            target_node_id: node_id,
            kind: RelationKind::SourceDocument,
            label: "Compiled from source".into(),
            confidence: Some(0.8),
            evidence_count: evidence.len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence,
            },
        );
        edges.push(edge);
    }

    for relation_index in 0..related_relation_count {
        if concept_count < 2 {
            break;
        }
        let left = relation_index % concept_count;
        let right = (relation_index + 1) % concept_count;
        if left == right {
            continue;
        }
        let edge_id = format!("edge-related-projection-{relation_index:03}");
        let edge = RelationEdgeSummary {
            id: edge_id.clone(),
            source_node_id: format!("concept-projection-{left:03}"),
            target_node_id: format!("concept-projection-{right:03}"),
            kind: RelationKind::RelatedTo,
            label: "Related to".into(),
            confidence: Some(0.7),
            evidence_count: evidence_by_concept[left].len(),
        };
        edge_details_by_id.insert(
            edge_id,
            RelationEdgeDetail {
                edge: edge.clone(),
                explanation: String::new(),
                evidence: evidence_by_concept[left].clone(),
            },
        );
        edges.push(edge);
    }

    KnowledgeProject {
        summary: ProjectOverview {
            project_id: project_id.into(),
            title: "Synthetic projection project".into(),
            status: ProjectStatus::Ready,
            stale: false,
            summary: "Synthetic projection project.".into(),
            document_count: source_count,
            node_count: nodes.len(),
            relationship_count: edges.len(),
            evidence_count: details_by_node_id
                .values()
                .map(|detail| detail.evidence.len())
                .sum::<usize>(),
            hidden_concept_count: 0,
            hidden_relation_count: 0,
        },
        nodes,
        edges,
        details_by_node_id,
        edge_details_by_id,
        answer_by_node_id,
    }
}

fn assert_projection_has_no_dangling_edges(project: &KnowledgeProject) {
    let node_ids = project
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    for edge in &project.edges {
        assert!(
            node_ids.contains(edge.source_node_id.as_str()),
            "edge {} has missing source {}",
            edge.id,
            edge.source_node_id
        );
        assert!(
            node_ids.contains(edge.target_node_id.as_str()),
            "edge {} has missing target {}",
            edge.id,
            edge.target_node_id
        );
    }
}

fn assert_projection_details_match_visible_graph(project: &KnowledgeProject) {
    let node_ids = project
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let edge_ids = project
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        project
            .details_by_node_id
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        node_ids
    );
    assert_eq!(
        project
            .edge_details_by_id
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        edge_ids
    );
}

fn synthetic_context_pack_snapshot(record_count: usize) -> BrainRepoSnapshot {
    let source = SourceRecord {
        source_id: "source-alpha".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/source-alpha.pdf".into(),
        source_path: "/tmp/source-alpha.pdf".into(),
        markdown_path: "/tmp/source-alpha.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: record_count,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![source.clone()];
    for index in 0..record_count {
        let evidence_id = format!("ev-alpha-{index:03}");
        let node_id = format!("concept-alpha-{index:03}");
        snapshot.evidence.push(EvidenceRef {
            id: evidence_id.clone(),
            page_label: format!("Page {}", index + 1),
            page_index: Some(index),
            snippet: format!("Alpha projection evidence {index} supports a canonical graph fact."),
            source_path: Some(source.source_path.clone()),
            source_id: Some(source.source_id.clone()),
            markdown_path: Some(source.markdown_path.clone()),
            image_path: None,
            provenance: Some("synthetic context pack fixture".into()),
        });
        snapshot.nodes.push(BrainNodeRecord {
            node_id: node_id.clone(),
            kind: BrainNodeKind::Concept,
            label: format!("Alpha Projection Concept {index:03}"),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence_id.clone()],
            source_ids: vec![source.source_id.clone()],
            confidence: Some(0.9),
            updated_at: 1,
        });
        snapshot.claims.push(ClaimRecord {
            claim_id: format!("claim-alpha-{index:03}"),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            statement: format!("Alpha projection claim {index} is evidence backed."),
            topic_refs: vec![node_id.clone()],
            source_refs: vec![source.source_id.clone()],
            evidence_refs: vec![evidence_id.clone()],
            status: "supported".into(),
            updated_at: 1,
        });
        snapshot.relations.push(BrainRelationRecord {
            relation_id: format!("relation-alpha-{index:03}"),
            kind: BrainRelationKind::RelatedTo,
            source_node_id: node_id,
            target_node_id: format!("concept-alpha-{next:03}", next = (index + 1) % record_count),
            label: "Related to".into(),
            evidence_ids: vec![evidence_id],
            confidence: Some(0.7),
            updated_at: 1,
        });
    }
    snapshot
}

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
fn provider_workspace_linking_caps_relations_claims_and_wiki_pages() {
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
    assert_eq!(snapshot.wiki_pages.len(), 3);
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

fn sample_parse_result() -> ParseResult {
    ParseResult {
        version: "1".into(),
        markdown: "# Sample import\n\n## Page 1\n\nGrounded evidence stays visible.\n".into(),
        pages: vec![ParsedPage {
            index: 0,
            markdown: Some("Grounded evidence stays visible.".into()),
            plain_text: Some("Grounded evidence stays visible.".into()),
            svg: None,
            image_asset_path: Some("images/page_1.png".into()),
            error_message: None,
        }],
        assets: vec![OutputAsset {
            relative_path: "images/page_1.png".into(),
            mime_type: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(b"png"),
        }],
        metadata: ParseMetadata {
            engine_id: "test/model".into(),
            duration_ms: 12,
            page_count: 1,
        },
        success_count: 1,
        failed_count: 0,
    }
}

fn sample_parse_request(temp: &tempfile::TempDir) -> ParseRequest {
    let source_path = temp.path().join("source.pdf");
    fs::write(&source_path, b"%PDF sample").expect("write source");
    ParseRequest {
        version: "1".into(),
        input: ParseInput {
            path: source_path.display().to_string(),
            format: DocumentFormat::Pdf,
        },
        template: "General".into(),
        options: ParseOptions::default(),
        output: None,
    }
}

fn sample_engine_config() -> EngineConfig {
    EngineConfig {
        provider: ProviderKind::OpenRouter,
        model_id: "openai/gpt-4.1-mini".into(),
        api_key: String::new(),
        base_url: None,
        prompt_template: "General".into(),
    }
}

fn sample_manifest(temp: &tempfile::TempDir) -> SourceArtifactManifest {
    sample_manifest_with_source(temp, "source-test", "source", 2)
}

fn sample_manifest_with_source(
    temp: &tempfile::TempDir,
    source_id: &str,
    output_name: &str,
    updated_at: u64,
) -> SourceArtifactManifest {
    SourceArtifactManifest {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: source_id.into(),
        original_path: temp
            .path()
            .join(format!("{output_name}.pdf"))
            .display()
            .to_string(),
        source_path: temp
            .path()
            .join(format!("default/sources/{source_id}/{output_name}.pdf"))
            .display()
            .to_string(),
        markdown_path: temp
            .path()
            .join(format!("default/artifacts/{source_id}/source.md"))
            .display()
            .to_string(),
        artifact_root: temp
            .path()
            .join(format!("default/artifacts/{source_id}"))
            .display()
            .to_string(),
        manifest_path: temp
            .path()
            .join(format!(
                "default/artifacts/{source_id}/source-manifest.json"
            ))
            .display()
            .to_string(),
        format: DocumentFormat::Pdf,
        output_name: output_name.into(),
        status: IngestStatus::Ingested,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        pages: vec![PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: None,
            markdown_path: None,
            plain_text_path: None,
            error_message: None,
        }],
        created_at: 1,
        updated_at,
    }
}

#[test]
fn brain_health_is_clean_for_empty_workspace() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let health =
        handle_get_brain_health(GetBrainHealthRequest { scope }).expect("get brain health");

    assert_eq!(health.status, BrainHealthStatus::Clean);
    assert_eq!(health.attention_count, 0);
    assert!(health.source_reports.is_empty());
    assert!(health.recent_events.is_empty());
}

#[test]
fn brain_health_reports_source_readiness_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");
    fs::create_dir_all(workspace_root.join("artifacts/src-partial")).expect("artifact dir");

    let source = SourceRecord {
        source_id: "src-partial".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/private/docs/partial.pdf".into(),
        source_path: "/private/hyprduck/sources/src-partial/partial.pdf".into(),
        markdown_path: "/private/hyprduck/artifacts/src-partial/partial.md".into(),
        format: hyprduck_engine_types::SourceFormat::pdf(),
        status: SourceStatus::partial(),
        page_count: 2,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 20,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 20,
        sources: vec![source],
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write manifest");
    write_json_pretty::<Vec<BrainNodeRecord>>(&workspace_root.join("graph/nodes.json"), &vec![])
        .expect("write nodes");
    write_json_pretty::<Vec<BrainRelationRecord>>(
        &workspace_root.join("graph/edges.json"),
        &vec![],
    )
    .expect("write edges");
    write_json_pretty::<Vec<EvidenceRef>>(&workspace_root.join("graph/evidence.json"), &vec![])
        .expect("write evidence");

    let source_pack = SourcePackV0 {
        schema_version: hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION.into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: "src-partial".into(),
        original_filename: "partial.pdf".into(),
        original_path: "/private/docs/partial.pdf".into(),
        source_path: "/private/hyprduck/sources/src-partial/partial.pdf".into(),
        markdown_path: "/private/hyprduck/artifacts/src-partial/partial.md".into(),
        artifact_root: "/private/hyprduck/artifacts/src-partial".into(),
        content_hash: "sha256:source-pack".into(),
        format: DocumentFormat::Pdf,
        page_count: 2,
        ingestion_status: IngestStatus::Partial,
        provider_route: "openrouter:gpt-4.1".into(),
        local_only: false,
        pages: vec![
            hyprduck_engine_types::SourcePackPageV0 {
                page: 1,
                label: "Page 1".into(),
                image_path: None,
                markdown_path: Some("artifacts/src-partial/pages/page_1.md".into()),
                plain_text_path: None,
                error_message: None,
            },
            hyprduck_engine_types::SourcePackPageV0 {
                page: 2,
                label: "Page 2".into(),
                image_path: None,
                markdown_path: Some("artifacts/src-partial/pages/page_2.md".into()),
                plain_text_path: None,
                error_message: Some("page renderer failed".into()),
            },
        ],
        warnings: vec![hyprduck_engine_types::SourcePackWarningV0 {
            warning_type: "page_parse_failed".into(),
            severity: hyprduck_engine_types::ContextPackWarningSeverity::High,
            message: format!("page renderer failed at {}", temp.path().display()),
            page: Some(2),
        }],
        created_at: 10,
        updated_at: 20,
    };
    let evidence_index = EvidenceIndexV0 {
        schema_version: hyprduck_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION.into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: "src-partial".into(),
        content_hash: "sha256:evidence-index".into(),
        provider_route: "openrouter:gpt-4.1".into(),
        local_only: false,
        evidence: Vec::new(),
        warnings: Vec::new(),
        generated_at: 20,
    };
    write_json_pretty(
        &workspace_root.join("artifacts/src-partial/source_pack.json"),
        &source_pack,
    )
    .expect("write source pack");
    write_json_pretty(
        &workspace_root.join("artifacts/src-partial/evidence_index.json"),
        &evidence_index,
    )
    .expect("write evidence index");

    let health = handle_get_brain_health(GetBrainHealthRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("get health");

    assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
    assert!(health.attention_count >= 4);
    assert_eq!(health.source_reports.len(), 1);
    let report = &health.source_reports[0];
    assert_eq!(report.source_id, "src-partial");
    assert_eq!(report.status, SourceStatus::partial());
    assert_eq!(report.page_count, 2);
    assert_eq!(report.failed_page_count, 1);
    assert_eq!(report.provider_route, "openrouter:gpt-4.1");
    assert_eq!(report.local_only, Some(false));
    assert_eq!(report.content_hash.as_deref(), Some("sha256:source-pack"));
    assert_eq!(report.content_hash_status, "mismatch");
    assert!(report.warnings.contains(&"partial_import".into()));
    assert!(report.warnings.contains(&"content_hash_mismatch".into()));
    assert!(report
        .warnings
        .contains(&"1 page(s) failed during import".into()));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("page_parse_failed")));
    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains(&temp.path().display().to_string())));
}

#[test]
fn brain_health_rejects_cross_workspace_artifact_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");
    fs::create_dir_all(workspace_root.join("artifacts/src-cross")).expect("artifact dir");

    write_health_test_snapshot(
        &workspace_root,
        SourceRecord {
            source_id: "src-cross".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/private/docs/cross.pdf".into(),
            source_path: "/private/hyprduck/sources/src-cross/cross.pdf".into(),
            markdown_path: "/private/hyprduck/artifacts/src-cross/cross.md".into(),
            format: hyprduck_engine_types::SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 20,
        },
    );
    let source_pack = SourcePackV0 {
        schema_version: hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION.into(),
        workspace_id: "other-workspace".into(),
        source_id: "src-cross".into(),
        original_filename: "cross.pdf".into(),
        original_path: "/private/docs/cross.pdf".into(),
        source_path: "/private/hyprduck/sources/src-cross/cross.pdf".into(),
        markdown_path: "/private/hyprduck/artifacts/src-cross/cross.md".into(),
        artifact_root: "/private/hyprduck/artifacts/src-cross".into(),
        content_hash: "sha256:foreign-source-pack".into(),
        format: DocumentFormat::Pdf,
        page_count: 1,
        ingestion_status: IngestStatus::Ingested,
        provider_route: "openrouter:foreign".into(),
        local_only: false,
        pages: Vec::new(),
        warnings: Vec::new(),
        created_at: 10,
        updated_at: 20,
    };
    let evidence_index = EvidenceIndexV0 {
        schema_version: hyprduck_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION.into(),
        workspace_id: "other-workspace".into(),
        source_id: "src-cross".into(),
        content_hash: "sha256:foreign-evidence-index".into(),
        provider_route: "openrouter:foreign".into(),
        local_only: false,
        evidence: Vec::new(),
        warnings: Vec::new(),
        generated_at: 20,
    };
    write_json_pretty(
        &workspace_root.join("artifacts/src-cross/source_pack.json"),
        &source_pack,
    )
    .expect("write source pack");
    write_json_pretty(
        &workspace_root.join("artifacts/src-cross/evidence_index.json"),
        &evidence_index,
    )
    .expect("write evidence index");

    let health = handle_get_brain_health(GetBrainHealthRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("get health");

    let report = &health.source_reports[0];
    assert_eq!(report.provider_route, "unknown");
    assert_eq!(report.local_only, None);
    assert_eq!(report.content_hash, None);
    assert_eq!(report.content_hash_status, "unknown");
    assert!(report
        .warnings
        .contains(&"source_pack_workspace_mismatch".into()));
    assert!(report
        .warnings
        .contains(&"evidence_index_workspace_mismatch".into()));
}

#[test]
fn brain_health_keeps_unreadable_and_missing_artifacts_distinct() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");
    fs::create_dir_all(workspace_root.join("artifacts/src-broken")).expect("artifact dir");

    write_health_test_snapshot(
        &workspace_root,
        SourceRecord {
            source_id: "src-broken".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/private/docs/broken.pdf".into(),
            source_path: "/private/hyprduck/sources/src-broken/broken.pdf".into(),
            markdown_path: "/private/hyprduck/artifacts/src-broken/broken.md".into(),
            format: hyprduck_engine_types::SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 20,
        },
    );
    fs::write(
        workspace_root.join("artifacts/src-broken/source_pack.json"),
        "{not-json",
    )
    .expect("write invalid source pack");

    let health = handle_get_brain_health(GetBrainHealthRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("get health");

    let warnings = &health.source_reports[0].warnings;
    assert!(warnings.contains(&"source_pack_unreadable".into()));
    assert!(!warnings.contains(&"source_pack_missing".into()));
    assert!(warnings.contains(&"evidence_index_missing".into()));
    assert!(!warnings.contains(&"evidence_index_unreadable".into()));
}

fn write_health_test_snapshot(workspace_root: &Path, source: SourceRecord) {
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 20,
        sources: vec![source],
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write manifest");
    write_json_pretty::<Vec<BrainNodeRecord>>(&workspace_root.join("graph/nodes.json"), &vec![])
        .expect("write nodes");
    write_json_pretty::<Vec<BrainRelationRecord>>(
        &workspace_root.join("graph/edges.json"),
        &vec![],
    )
    .expect("write edges");
    write_json_pretty::<Vec<EvidenceRef>>(&workspace_root.join("graph/evidence.json"), &vec![])
        .expect("write evidence");
}

#[test]
fn brain_maintenance_preserves_existing_wiki_page_when_adding_missing_manifest_record() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown =
        "# Agent Graph Loop\n\n## Page 1\n\nAgent graph loop keeps wiki pages materialized.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let existing_wiki_path = "wiki/save-back/manual-page.md".to_string();
    let existing_wiki_file = workspace_root.join(&existing_wiki_path);
    fs::create_dir_all(existing_wiki_file.parent().expect("manual wiki parent"))
        .expect("create manual wiki parent");
    let original_body = "# Manual Page\n\nKeep this exact user-authored body.\n";
    fs::write(&existing_wiki_file, original_body).expect("write manual wiki page");
    let mut memories = read_memory_records(&workspace_root).expect("read memories");
    memories.push(MemoryRecord {
        memory_id: "memory-existing-wiki-ref".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Existing wiki ref".into(),
        body: "Existing user-authored pages should become readable without being replaced.".into(),
        source_refs: vec![existing_wiki_path.clone()],
        evidence_refs: Vec::new(),
        created_at: 10,
        updated_at: 10,
    });
    write_json_pretty(&workspace_root.join("memory/records.json"), &memories)
        .expect("write memory ref");
    let broken_snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read snapshot before repair");
    let missing_issues = lint_missing_materialized_wiki_refs(&workspace_root, &broken_snapshot);
    assert!(missing_issues.iter().any(|issue| {
        issue.kind == "missing_wiki_page" && issue.source_refs.contains(&existing_wiki_path)
    }));

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let read_only_health = handle_get_brain_health(GetBrainHealthRequest {
        scope: scope.clone(),
    })
    .expect("read-only health");
    assert_eq!(read_only_health.status, BrainHealthStatus::AttentionNeeded);
    assert!(!workspace_root
        .join("state/maintenance-latest.json")
        .exists());
    let still_broken_snapshot =
        read_json_artifact::<BrainRepoSnapshot>(&workspace_root.join("brain-manifest.json"))
            .expect("read manifest after health");
    assert!(!still_broken_snapshot
        .wiki_pages
        .iter()
        .any(|page| page.path == existing_wiki_path));

    let report = run_brain_maintenance(&scope).expect("maintenance runs");
    assert_eq!(report.issue_count, 0);
    let report: BrainMaintenanceReport =
        read_json_artifact(&workspace_root.join("state/maintenance-latest.json"))
            .expect("read maintenance report");
    assert!(report.repairs.contains(&existing_wiki_path));
    assert!(!report
        .issues
        .iter()
        .any(|issue| issue.kind == "missing_wiki_page"));
    assert_eq!(
        fs::read_to_string(&existing_wiki_file).expect("read manual wiki page after repair"),
        original_body
    );
    let repaired_snapshot: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read repaired manifest");
    let materialized_page = repaired_snapshot
        .wiki_pages
        .iter()
        .find(|page| page.path == existing_wiki_path)
        .expect("existing page added to manifest");
    assert_eq!(materialized_page.body, original_body);
    assert!(materialized_page.source_refs.contains(&existing_wiki_path));
}

#[test]
fn brain_writer_bootstraps_missing_memory_and_events_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::remove_file(workspace_root.join("memory/records.json")).expect("remove memory file");
    fs::remove_file(workspace_root.join("events/brain_events.jsonl")).expect("remove events");

    let memory = MemoryRecord {
        memory_id: "memory-bootstrap".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Remember bootstrap behavior".into(),
        body: "Missing memory and event files should be recreated safely.".into(),
        source_refs: Vec::new(),
        evidence_refs: Vec::new(),
        created_at: 42,
        updated_at: 42,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("open writer");
    writer
        .upsert_memory_record(memory.clone())
        .expect("write memory with missing file");
    writer
        .append_event(&BrainEvent {
            event_id: "evt-memory-bootstrap".into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::MemoryAccepted,
            operation_type: Some("memory_recorded".into()),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "test-agent".into(),
            },
            source_refs: Vec::new(),
            source_markdown_refs: Vec::new(),
            node_refs: Vec::new(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: vec![memory.memory_id.clone()],
            target_node_ids: Vec::new(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: vec![memory.memory_id.clone()],
            evidence_refs: Vec::new(),
            payload_json: serde_json::to_string(&memory).expect("memory payload"),
            causality: BrainEventCausality::default(),
            confidence: None,
            policy_result: "accepted".into(),
            created_at: 42,
        })
        .expect("append memory event");
    drop(writer);

    let memories = read_memory_records(&workspace_root).expect("read bootstrapped memories");
    assert_eq!(memories.len(), 1);
    assert!(workspace_root.join("events/brain_events.jsonl").exists());
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope,
        limit: Some(10),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read bootstrapped events");
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::MemoryAccepted));
    assert!(!workspace_root.join(BRAIN_LOCK_DIRECTORY_NAME).exists());
}

#[test]
fn materialization_fails_on_corrupt_existing_events_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure repo dirs");
    fs::write(
        workspace_root.join("events/brain_events.jsonl"),
        "{not valid json}\n",
    )
    .expect("write corrupt events file");
    let snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);

    let error = compute_effective_brain_snapshot(&workspace_root, &snapshot)
        .expect_err("corrupt existing events must fail materialization");

    assert!(format!("{error:#}").contains("failed reading existing events"));
}

#[test]
fn brain_event_jsonl_reader_ignores_unknown_legacy_event_types() {
    let temp = tempfile::tempdir().expect("temp dir");
    let events_path = temp.path().join("brain_events.jsonl");
    let legacy_event = serde_json::json!({
        "eventId": "legacy-event-1",
        "schemaVersion": BRAIN_EVENT_SCHEMA_VERSION,
        "workspaceId": DEFAULT_WORKSPACE_ID,
        "scope": "project",
        "eventType": "link_proposed",
        "actor": {
            "actorType": "agent",
            "actorId": "legacy-agent"
        },
        "policyResult": "materialized",
        "createdAt": 41
    });
    let known_event = serde_json::json!({
        "eventId": "event-1",
        "schemaVersion": BRAIN_EVENT_SCHEMA_VERSION,
        "workspaceId": DEFAULT_WORKSPACE_ID,
        "scope": "project",
        "eventType": "brain_maintenance_run",
        "actor": {
            "actorType": "system",
            "actorId": "system"
        },
        "policyResult": "accepted",
        "createdAt": 42
    });
    fs::write(
        &events_path,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&legacy_event).expect("legacy event"),
            serde_json::to_string(&known_event).expect("known event")
        ),
    )
    .expect("write events");

    let events = read_brain_events_jsonl(&events_path).expect("read events");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, "event-1");
}

#[test]
fn brain_writer_uses_directory_lock_without_pid_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("create workspace");
    fs::write(workspace_root.join("brain.lock"), "pid=999999999\n")
        .expect("write legacy lock file");

    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("open writer");
    assert!(workspace_root.join(BRAIN_LOCK_DIRECTORY_NAME).is_dir());
    drop(writer);

    assert!(!workspace_root.join(BRAIN_LOCK_DIRECTORY_NAME).exists());
    assert!(workspace_root.join("brain.lock").exists());
}

#[test]
fn context_pack_source_metadata_hashes_available_source_content() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let source_path = workspace_root.join("source.pdf");
    let markdown_path = workspace_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");

    let metadata = build_context_pack_source_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-context".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.pdf".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::pdf(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    let source = metadata.get("src-context").expect("source metadata");
    assert!(source.content_hash.starts_with("fnv64:"));
    assert_eq!(source.provider_route, "unknown");
    assert!(!source.local_only);
}

#[test]
fn context_pack_artifact_metadata_warns_when_source_pack_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let source_path = workspace_root.join("source.pdf");
    let markdown_path = workspace_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-context".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.pdf".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::pdf(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    let source = metadata
        .sources
        .get("src-context")
        .expect("source metadata");
    assert_eq!(source.provider_route, "unknown");
    assert!(metadata
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "source_pack_missing"));
}

#[test]
fn context_pack_artifact_metadata_sanitizes_unreadable_artifact_warnings() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let bad_source_pack_root = workspace_root.join("artifacts/src-bad-source-pack");
    let bad_source_pack_sources = workspace_root.join("sources/src-bad-source-pack");
    let bad_evidence_index_root = workspace_root.join("artifacts/src-bad-evidence-index");
    let bad_evidence_index_sources = workspace_root.join("sources/src-bad-evidence-index");
    fs::create_dir_all(&bad_source_pack_root).expect("bad source pack artifact root");
    fs::create_dir_all(&bad_source_pack_sources).expect("bad source pack source root");
    fs::create_dir_all(&bad_evidence_index_root).expect("bad evidence index artifact root");
    fs::create_dir_all(&bad_evidence_index_sources).expect("bad evidence index source root");
    let bad_source_pack_source_path = bad_source_pack_sources.join("source.md");
    let bad_source_pack_markdown_path = bad_source_pack_root.join("source.md");
    let bad_evidence_index_source_path = bad_evidence_index_sources.join("source.md");
    let bad_evidence_index_markdown_path = bad_evidence_index_root.join("source.md");
    fs::write(&bad_source_pack_source_path, b"source bytes").expect("write source");
    fs::write(&bad_source_pack_markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(&bad_evidence_index_source_path, b"source bytes").expect("write source");
    fs::write(&bad_evidence_index_markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(bad_source_pack_root.join("source_pack.json"), "{not json").expect("bad source pack");
    fs::write(
        bad_evidence_index_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-bad-evidence-index",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": bad_evidence_index_source_path.display().to_string(),
            "markdownPath": bad_evidence_index_markdown_path.display().to_string(),
            "artifactRoot": bad_evidence_index_root.display().to_string(),
            "contentHash": "fnv64:source",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        bad_evidence_index_root.join("evidence_index.json"),
        "{not json",
    )
    .expect("bad evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[
            SourceRecord {
                source_id: "src-bad-source-pack".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "source.md".into(),
                source_path: bad_source_pack_source_path.display().to_string(),
                markdown_path: bad_source_pack_markdown_path.display().to_string(),
                format: hyprduck_engine_types::SourceFormat::markdown(),
                status: hyprduck_engine_types::SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
            SourceRecord {
                source_id: "src-bad-evidence-index".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                original_path: "source.md".into(),
                source_path: bad_evidence_index_source_path.display().to_string(),
                markdown_path: bad_evidence_index_markdown_path.display().to_string(),
                format: hyprduck_engine_types::SourceFormat::markdown(),
                status: hyprduck_engine_types::SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
        ],
    );

    let temp_path = temp.path().display().to_string();
    let source_pack_warning = metadata
        .warnings
        .iter()
        .find(|warning| warning.warning_type == "source_pack_unreadable")
        .expect("source pack unreadable warning");
    assert_eq!(
        source_pack_warning.message,
        "Source Pack for src-bad-source-pack could not be read or decoded."
    );
    assert!(!source_pack_warning.message.contains(&temp_path));

    let evidence_index_warning = metadata
        .warnings
        .iter()
        .find(|warning| warning.warning_type == "evidence_index_unreadable")
        .expect("evidence index unreadable warning");
    assert_eq!(
        evidence_index_warning.message,
        "Evidence Index for src-bad-evidence-index could not be read or decoded."
    );
    assert!(!evidence_index_warning.message.contains(&temp_path));
}

#[test]
fn context_pack_artifact_metadata_prefers_source_pack_and_evidence_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-context");
    let source_root = workspace_root.join("sources/src-context");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-context",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:indexed-source",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-context",
            "contentHash": "fnv64:indexed-source",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-src-context-source-1",
                "sourceId": "src-context",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Indexed evidence quote.",
                "parseConfidence": "high",
                "contentHash": "fnv64:indexed-source",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-context".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    let source = metadata
        .sources
        .get("src-context")
        .expect("source metadata");
    assert_eq!(source.content_hash, "fnv64:indexed-source");
    assert_eq!(source.provider_route, "local_demo");
    assert!(source.local_only);
    let evidence = metadata
        .evidence
        .get("src-context")
        .and_then(|source_evidence| source_evidence.get("ev-src-context-source-1"))
        .expect("evidence metadata");
    assert_eq!(evidence.quoted_text, "Indexed evidence quote.");
    assert_eq!(evidence.span.as_deref(), Some("page"));
    assert_eq!(
        evidence.parse_confidence,
        hyprduck_engine_types::ContextPackParseConfidence::High
    );
}

#[test]
fn context_pack_artifact_metadata_rejects_cross_workspace_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-cross-workspace");
    let source_root = workspace_root.join("sources/src-cross-workspace");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": "other-workspace",
            "sourceId": "src-cross-workspace",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:cross-workspace-pack",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": "other-workspace",
            "sourceId": "src-cross-workspace",
            "contentHash": "fnv64:cross-workspace-pack",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-cross-workspace",
                "sourceId": "src-cross-workspace",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Cross-workspace evidence must not be trusted.",
                "parseConfidence": "high",
                "contentHash": "fnv64:cross-workspace-pack",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-cross-workspace".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    let source = metadata
        .sources
        .get("src-cross-workspace")
        .expect("fallback source metadata");
    assert_eq!(
        source.content_hash,
        format!("fnv64:{:016x}", fnv1a64(b"source bytes"))
    );
    assert_eq!(source.provider_route, "unknown");
    assert!(metadata
        .evidence
        .get("src-cross-workspace")
        .map_or(true, |source_evidence| source_evidence.is_empty()));
    assert!(metadata
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "source_pack_workspace_mismatch"));
    assert!(metadata
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "evidence_index_workspace_mismatch"));
}

#[test]
fn context_pack_artifact_metadata_warns_and_skips_stale_evidence_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-stale");
    let source_root = workspace_root.join("sources/src-stale");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-stale",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:current",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-stale",
            "contentHash": "fnv64:stale",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-src-stale-source-1",
                "sourceId": "src-stale",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Stale evidence quote.",
                "parseConfidence": "high",
                "contentHash": "fnv64:stale",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-stale".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    assert!(metadata
        .evidence
        .get("src-stale")
        .map_or(true, |source_evidence| source_evidence.is_empty()));
    assert!(metadata.warnings.iter().any(|warning| {
        warning.warning_type == "evidence_index_stale_content_hash"
            && warning.message.contains("fnv64:stale")
            && warning.message.contains("fnv64:current")
    }));
}

#[test]
fn context_pack_artifact_metadata_warns_and_skips_mismatched_evidence_item() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-mismatch");
    let source_root = workspace_root.join("sources/src-mismatch");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-mismatch",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:source",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-mismatch",
            "contentHash": "fnv64:source",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-mismatched-source",
                "sourceId": "other-source",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Wrong source evidence quote.",
                "parseConfidence": "high",
                "contentHash": "fnv64:source",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-mismatch".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    assert!(metadata
        .evidence
        .get("src-mismatch")
        .map_or(true, |source_evidence| source_evidence.is_empty()));
    let warning = metadata
        .warnings
        .iter()
        .find(|warning| warning.warning_type == "evidence_item_source_mismatch")
        .expect("source mismatch warning");
    assert_eq!(warning.page_refs.len(), 1);
    assert_eq!(warning.page_refs[0].source_id, "src-mismatch");
    assert_eq!(warning.page_refs[0].page, 1);
}

#[test]
fn context_pack_artifact_metadata_propagates_artifact_warnings_once() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-partial");
    let source_root = workspace_root.join("sources/src-partial");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("source.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    let warning = serde_json::json!({
        "type": "page_parse_failed",
        "severity": "medium",
        "message": "Page 2 failed during provider parsing.",
        "page": 2
    });
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-partial",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:partial",
            "format": "markdown",
            "pageCount": 2,
            "ingestionStatus": "partial",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [warning.clone()],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-partial",
            "contentHash": "fnv64:partial",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [],
            "warnings": [warning],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-partial".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: source_path.display().to_string(),
            markdown_path: markdown_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::partial(),
            page_count: 2,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    let warnings = metadata
        .warnings
        .iter()
        .filter(|warning| warning.warning_type == "page_parse_failed")
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].severity,
        hyprduck_engine_types::ContextPackWarningSeverity::Medium
    );
    assert_eq!(warnings[0].page_refs.len(), 1);
    assert_eq!(warnings[0].page_refs[0].source_id, "src-partial");
    assert_eq!(warnings[0].page_refs[0].page, 2);
}

#[test]
fn read_page_evidence_resolves_source_evidence_index_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-page-evidence");
    let source_root = workspace_root.join("sources/src-page-evidence");
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace_root.join("memory")).expect("memory dir");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("pages/page_1.md");
    let image_path = artifact_root.join("images/page_1.png");
    fs::create_dir_all(markdown_path.parent().expect("markdown parent")).expect("pages dir");
    fs::create_dir_all(image_path.parent().expect("image parent")).expect("images dir");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    fs::write(&image_path, b"image bytes").expect("write image");
    let source = SourceRecord {
        source_id: "src-page-evidence".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "source.md".into(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        format: hyprduck_engine_types::SourceFormat::markdown(),
        status: hyprduck_engine_types::SourceStatus::ingested(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![source.clone()],
        evidence: vec![EvidenceRef {
            id: "ev-src-page-evidence-source-1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Fallback internal snippet should not be returned.".into(),
            source_path: Some(source_path.display().to_string()),
            source_id: Some(source.source_id.clone()),
            markdown_path: Some(markdown_path.display().to_string()),
            image_path: Some(image_path.display().to_string()),
            provenance: None,
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
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("brain manifest");
    write_json_pretty(
        &workspace_root.join("graph/nodes.json"),
        &Vec::<BrainNodeRecord>::new(),
    )
    .expect("nodes");
    write_json_pretty(
        &workspace_root.join("graph/edges.json"),
        &Vec::<BrainRelationRecord>::new(),
    )
    .expect("edges");
    write_json_pretty(
        &workspace_root.join("graph/evidence.json"),
        &snapshot.evidence,
    )
    .expect("evidence");
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &Vec::<MemoryRecord>::new(),
    )
    .expect("memories");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-page-evidence",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:page-evidence",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-page-evidence",
            "contentHash": "fnv64:page-evidence",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-src-page-evidence-source-1",
                "sourceId": "src-page-evidence",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Indexed page evidence quote.",
                "parseConfidence": "high",
                "contentHash": "fnv64:page-evidence",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": image_path.display().to_string()
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let response = handle_read_page_evidence(ReadPageEvidenceRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        source_id: source.source_id,
        page: Some(1),
    })
    .expect("page evidence");

    assert_eq!(response.evidence.len(), 1);
    assert_eq!(
        response.evidence[0].evidence_ref,
        "ev-src-page-evidence-source-1"
    );
    assert_eq!(
        response.evidence[0].quoted_text,
        "Indexed page evidence quote."
    );
    assert_eq!(
        response.evidence[0].parse_confidence,
        hyprduck_engine_types::ContextPackParseConfidence::High
    );
    assert_eq!(response.evidence[0].content_hash, "fnv64:page-evidence");
    let expected_markdown_path = markdown_path.display().to_string();
    assert_eq!(
        response.evidence[0].markdown_path.as_deref(),
        Some(expected_markdown_path.as_str())
    );
    assert!(response.warnings.is_empty());
}

#[test]
fn read_page_evidence_rejects_cross_workspace_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-page-cross-workspace");
    let source_root = workspace_root.join("sources/src-page-cross-workspace");
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::create_dir_all(&source_root).expect("source root");
    let source_path = source_root.join("source.md");
    let markdown_path = artifact_root.join("pages/page_1.md");
    fs::create_dir_all(markdown_path.parent().expect("markdown parent")).expect("pages dir");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, b"markdown bytes").expect("write markdown");
    let source = SourceRecord {
        source_id: "src-page-cross-workspace".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "source.md".into(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        format: hyprduck_engine_types::SourceFormat::markdown(),
        status: hyprduck_engine_types::SourceStatus::ingested(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    write_health_test_snapshot(&workspace_root, source.clone());
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": "other-workspace",
            "sourceId": "src-page-cross-workspace",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:page-cross-workspace",
            "format": "markdown",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "local_demo",
            "localOnly": true,
            "pages": [],
            "warnings": [],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": "other-workspace",
            "sourceId": "src-page-cross-workspace",
            "contentHash": "fnv64:page-cross-workspace",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-src-page-cross-workspace-source-1",
                "sourceId": "src-page-cross-workspace",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Cross-workspace page evidence must not be returned.",
                "parseConfidence": "high",
                "contentHash": "fnv64:page-cross-workspace",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let response = handle_read_page_evidence(ReadPageEvidenceRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        source_id: source.source_id,
        page: Some(1),
    })
    .expect("page evidence");

    assert!(response.evidence.is_empty());
    assert!(response
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "source_pack_workspace_mismatch"));
    assert!(response
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "evidence_index_workspace_mismatch"));
}

#[test]
fn context_pack_source_metadata_skips_paths_outside_workspace_root() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let outside_path = temp.path().join("outside.md");
    fs::write(&outside_path, b"outside bytes").expect("outside");

    let metadata = build_context_pack_source_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-outside".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "outside.md".into(),
            source_path: outside_path.display().to_string(),
            markdown_path: outside_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    assert!(!metadata.contains_key("src-outside"));
}

#[test]
#[cfg(unix)]
fn context_pack_source_metadata_skips_symlink_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let outside_path = temp.path().join("outside.md");
    let symlink_path = workspace_root.join("source.md");
    fs::write(&outside_path, b"outside bytes").expect("outside");
    std::os::unix::fs::symlink(&outside_path, &symlink_path).expect("symlink");

    let metadata = build_context_pack_source_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-symlink".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "source.md".into(),
            source_path: symlink_path.display().to_string(),
            markdown_path: symlink_path.display().to_string(),
            format: hyprduck_engine_types::SourceFormat::markdown(),
            status: hyprduck_engine_types::SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
    );

    assert!(!metadata.contains_key("src-symlink"));
}

#[test]
fn context_pack_v0_persistence_writes_latest_and_history_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };
    let context_pack = hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: "ctx_test_pack".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
        },
        suggested_next_reads: vec![],
    };

    let path = persist_context_pack_v0(&scope, &context_pack).expect("persist context pack");
    assert!(path.ends_with("default/context_pack.json"));
    let latest = temp.path().join("default/context_pack.json");
    let history = temp.path().join("default/context_packs/ctx_test_pack.json");
    assert!(latest.exists());
    assert!(history.exists());
    let decoded: hyprduck_engine_types::ContextPackV0 =
        serde_json::from_str(&fs::read_to_string(latest).expect("latest context pack"))
            .expect("context pack json");
    assert_eq!(decoded.pack_id, "ctx_test_pack");
}

#[test]
fn read_context_pack_reads_latest_and_history_without_path_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };
    let context_pack = hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: "ctx_test_pack".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
        },
        suggested_next_reads: vec![],
    };
    persist_context_pack_v0(&scope, &context_pack).expect("persist context pack");

    let latest = handle_read_context_pack(ReadContextPackRequest {
        scope: scope.clone(),
        pack_id: None,
    })
    .expect("latest context pack");
    assert_eq!(latest.context_pack.pack_id, "ctx_test_pack");

    let history = handle_read_context_pack(ReadContextPackRequest {
        scope: scope.clone(),
        pack_id: Some("ctx_test_pack".into()),
    })
    .expect("history context pack");
    assert_eq!(history.context_pack.pack_id, "ctx_test_pack");

    let error = handle_read_context_pack(ReadContextPackRequest {
        scope,
        pack_id: Some("../ctx_test_pack".into()),
    })
    .expect_err("packId path escape rejected");
    assert!(error.to_string().contains("invalid packId"));
}

#[test]
#[cfg(unix)]
fn read_context_pack_rejects_symlink_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let outside_root = temp.path().join("outside");
    fs::create_dir_all(workspace_root.join("context_packs")).expect("context packs dir");
    fs::create_dir_all(&outside_root).expect("outside dir");
    let context_pack = hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: "ctx_escape".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
        },
        suggested_next_reads: vec![],
    };
    let outside_latest = outside_root.join("context_pack.json");
    let outside_history = outside_root.join("ctx_escape.json");
    fs::write(
        &outside_latest,
        serde_json::to_string(&context_pack).expect("context pack json"),
    )
    .expect("outside latest");
    fs::write(
        &outside_history,
        serde_json::to_string(&context_pack).expect("context pack json"),
    )
    .expect("outside history");
    std::os::unix::fs::symlink(&outside_latest, workspace_root.join("context_pack.json"))
        .expect("latest symlink");
    std::os::unix::fs::symlink(
        &outside_history,
        workspace_root.join("context_packs/ctx_escape.json"),
    )
    .expect("history symlink");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };

    let latest_error = handle_read_context_pack(ReadContextPackRequest {
        scope: scope.clone(),
        pack_id: None,
    })
    .expect_err("latest symlink escape rejected");
    assert_eq!(
        latest_error.to_string(),
        "persisted context pack could not be read or decoded"
    );
    assert!(!latest_error
        .to_string()
        .contains(temp.path().display().to_string().as_str()));

    let history_error = handle_read_context_pack(ReadContextPackRequest {
        scope,
        pack_id: Some("ctx_escape".into()),
    })
    .expect_err("history symlink escape rejected");
    assert_eq!(
        history_error.to_string(),
        "persisted context pack could not be read or decoded"
    );
    assert!(!history_error
        .to_string()
        .contains(temp.path().display().to_string().as_str()));
}

#[test]
fn read_context_pack_rejects_schema_and_requested_pack_id_mismatch() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("context_packs")).expect("context packs dir");
    let mut context_pack = hyprduck_engine_types::ContextPackV0 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: "ctx_expected".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV0 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
        },
        suggested_next_reads: vec![],
    };
    context_pack.schema_version = "hyprduck.context_pack.future".into();
    fs::write(
        workspace_root.join("context_pack.json"),
        serde_json::to_string(&context_pack).expect("invalid schema pack json"),
    )
    .expect("invalid schema context pack");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };
    let schema_error = handle_read_context_pack(ReadContextPackRequest {
        scope: scope.clone(),
        pack_id: None,
    })
    .expect_err("schema mismatch rejected");
    assert!(schema_error.to_string().contains("schemaVersion"));

    context_pack.schema_version = hyprduck_engine_types::CONTEXT_PACK_V0_SCHEMA_VERSION.into();
    context_pack.pack_id = "ctx_other".into();
    fs::write(
        workspace_root.join("context_packs/ctx_expected.json"),
        serde_json::to_string(&context_pack).expect("pack mismatch json"),
    )
    .expect("pack mismatch context pack");
    let pack_id_error = handle_read_context_pack(ReadContextPackRequest {
        scope,
        pack_id: Some("ctx_expected".into()),
    })
    .expect_err("packId mismatch rejected");
    assert!(pack_id_error
        .to_string()
        .contains("does not match requested packId"));
}

#[test]
fn resolve_brain_workspace_root_rejects_workspace_path_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: "../outside".into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let error = resolve_brain_workspace_root(&scope).expect_err("workspace escape rejected");
    assert!(error.to_string().contains("invalid workspaceId"));
}

#[test]
#[cfg(unix)]
fn resolve_brain_workspace_root_rejects_symlink_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root).expect("root");
    fs::create_dir_all(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("default")).expect("symlink");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(root.display().to_string()),
    };

    let error = resolve_brain_workspace_root(&scope).expect_err("symlink escape rejected");
    assert!(error.to_string().contains("escapes allowed root"));
}

#[test]
#[cfg(unix)]
fn brain_reader_rejects_wiki_artifact_symlink_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root/default");
    let outside = temp.path().join("outside.md");
    fs::create_dir_all(root.join("events")).expect("events");
    fs::create_dir_all(root.join("graph")).expect("graph");
    fs::create_dir_all(root.join("memory")).expect("memory");
    fs::create_dir_all(root.join("wiki")).expect("wiki");
    fs::write(&outside, "# Outside\n").expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("wiki/index.md")).expect("wiki symlink");
    fs::write(
        root.join("brain-manifest.json"),
        r#"{"workspaceId":"default","generatedAt":1,"sources":[],"nodes":[],"relations":[],"evidence":[],"memories":[],"wikiPages":[{"pageId":"wiki-index","workspaceId":"default","path":"wiki/index.md","title":"Index","body":"","nodeRefs":[],"sourceRefs":[],"evidenceRefs":[],"updatedAt":1}],"entities":[],"claims":[],"extractions":[],"events":[]}"#,
    )
    .expect("manifest");
    fs::write(root.join("graph/nodes.json"), "[]").expect("nodes");
    fs::write(root.join("graph/edges.json"), "[]").expect("edges");
    fs::write(root.join("graph/evidence.json"), "[]").expect("evidence");
    fs::write(root.join("memory/records.json"), "[]").expect("memories");
    fs::write(root.join("events/brain_events.jsonl"), "").expect("events");

    let reader = BrainReader::open(&BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().join("root").display().to_string()),
    })
    .expect("reader");
    let error = reader
        .read_wiki_page("wiki/index.md")
        .expect_err("wiki symlink escape rejected");
    assert!(error.to_string().contains("escapes workspace root"));
}

#[test]
fn structured_extraction_artifact_tracks_claims_relations_and_provenance() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Source import\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\nEvidence Map links page images to markdown snippets.\n\n## Page 2\n\nShared Context Layer turns imported documents into agent-ready knowledge.\n";
    let mut sections = extract_page_sections(markdown);
    let mut manifest = sample_manifest(&temp);
    manifest.pages = vec![
        PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: Some(
                temp.path()
                    .join("default/artifacts/source-test/images/page-1.png")
                    .display()
                    .to_string(),
            ),
            markdown_path: Some(
                temp.path()
                    .join("default/artifacts/source-test/pages/page-1.md")
                    .display()
                    .to_string(),
            ),
            plain_text_path: None,
            error_message: None,
        },
        PageArtifact {
            index: 1,
            label: "Page 2".into(),
            image_path: Some(
                temp.path()
                    .join("default/artifacts/source-test/images/page-2.png")
                    .display()
                    .to_string(),
            ),
            markdown_path: Some(
                temp.path()
                    .join("default/artifacts/source-test/pages/page-2.md")
                    .display()
                    .to_string(),
            ),
            plain_text_path: None,
            error_message: None,
        },
    ];
    attach_page_artifacts_to_sections(&mut sections, Some(&manifest));

    let artifact = build_extraction_artifact(
        &sections,
        markdown,
        &manifest.source_path,
        Some(manifest.source_id.as_str()),
        &[],
        &[],
    );

    assert!(artifact.concepts.len() >= 2);
    assert!(artifact.claims.len() >= 2);
    assert!(!artifact.relations.is_empty());
    assert!(artifact
        .relations
        .iter()
        .all(|relation| !relation.evidence_ids.is_empty()));
    let evidence = artifact
        .evidence_refs
        .values()
        .find(|evidence| evidence.page_label == "Page 1")
        .expect("page 1 evidence");
    assert_eq!(evidence.page_index, 0);
    assert!(evidence
        .markdown_path
        .as_deref()
        .is_some_and(|path| path.ends_with("page-1.md")));
    assert!(evidence
        .image_path
        .as_deref()
        .is_some_and(|path| path.ends_with("page-1.png")));
    assert!(evidence.provenance.contains("Page 1"));

    let collected = collected_concepts_from_artifact(&artifact);
    assert!(collected
        .page_concepts
        .iter()
        .any(|page| page.page_label == "Page 1" && page.concept_ids.len() >= 2));
}

#[test]
fn structured_extraction_uses_unique_evidence_ids_across_pages() {
    let markdown = "# Source import\n\n## Page 1\n\nAlpha Planning Notes stay local.\nShared Context Layer keeps agents grounded.\n\n## Page 2\n\nShared Context Layer keeps agents grounded.\nEvidence Map links page images to markdown snippets.\n";
    let sections = extract_page_sections(markdown);
    let artifact = build_extraction_artifact(
        &sections,
        markdown,
        "/tmp/source.pdf",
        Some("source-test"),
        &[],
        &[],
    );
    let shared = artifact
        .concepts
        .iter()
        .find(|concept| {
            normalize_key(&concept.label) == "shared-context-layer-keeps-agents-grounded"
        })
        .expect("shared context concept");

    assert_eq!(shared.evidence_ids.len(), 2);
    assert_eq!(
        shared.evidence_ids.iter().collect::<BTreeSet<_>>().len(),
        shared.evidence_ids.len()
    );
    for evidence_id in &shared.evidence_ids {
        assert!(artifact.evidence_refs.contains_key(evidence_id));
    }
    assert_eq!(
        artifact
            .evidence_refs
            .values()
            .filter(|evidence| evidence
                .provenance
                .contains("Shared Context Layer keeps agents"))
            .count(),
        2
    );
}

#[test]
fn source_backed_project_id_uses_manifest_identity() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let shared_original = temp
        .path()
        .join("shared-original.pdf")
        .display()
        .to_string();
    let markdown_a = "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n";
    let markdown_b =
        "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n";
    let markdown_path_a = temp.path().join("source-a.md");
    let markdown_path_b = temp.path().join("source-b.md");
    fs::write(&markdown_path_a, markdown_a).expect("write source a markdown");
    fs::write(&markdown_path_b, markdown_b).expect("write source b markdown");

    let mut manifest_a = sample_manifest_with_source(&temp, "source-a", "alpha", 10);
    let mut manifest_b = sample_manifest_with_source(&temp, "source-b", "beta", 11);
    manifest_a.original_path = shared_original.clone();
    manifest_b.original_path = shared_original.clone();
    let request_a = CompileProjectRequest {
        source_markdown_path: markdown_path_a.display().to_string(),
        source_document_path: Some(shared_original.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: markdown_path_b.display().to_string(),
        source_document_path: Some(shared_original),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };

    let project_a = compile_knowledge_project(&request_a, markdown_a, Some(&manifest_a));
    let project_b = compile_knowledge_project(&request_b, markdown_b, Some(&manifest_b));
    assert_ne!(project_a.summary.project_id, project_b.summary.project_id);
    assert_eq!(
        project_a.summary.project_id,
        build_source_backed_project_id(DEFAULT_WORKSPACE_ID, "source-a")
    );
    assert_eq!(
        project_b.summary.project_id,
        build_source_backed_project_id(DEFAULT_WORKSPACE_ID, "source-b")
    );

    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    assert!(aggregate.details_by_node_id.values().any(|detail| {
        detail
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-a"))
    }));
    assert!(aggregate.details_by_node_id.values().any(|detail| {
        detail
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some("source-b"))
    }));
}

#[test]
fn source_rows_round_trip_paths_with_pipe_characters() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let markdown = "# Source A\n\n## Page 1\n\nPipe path evidence stays readable.\n";
    let markdown_path = temp.path().join("pipe-source.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let mut manifest = sample_manifest_with_source(&temp, "source-pipe", "alpha", 10);
    manifest.original_path = temp.path().join("a|b.pdf").display().to_string();
    manifest.source_path = temp
        .path()
        .join("default/sources/source-pipe/a|b.pdf")
        .display()
        .to_string();
    manifest.markdown_path = temp
        .path()
        .join("default/artifacts/source-pipe/a|b.md")
        .display()
        .to_string();
    manifest.manifest_path = temp
        .path()
        .join("default/artifacts/source-pipe/source|manifest.json")
        .display()
        .to_string();
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save pipe path source");

    let sources = store
        .load_sources(DEFAULT_WORKSPACE_ID)
        .expect("load sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].original_path, manifest.original_path);
    assert_eq!(sources[0].source_path, manifest.source_path);
    assert_eq!(sources[0].markdown_path, manifest.markdown_path);

    let rows = store
        .load_source_rows(DEFAULT_WORKSPACE_ID)
        .expect("load source rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].manifest_path, manifest.manifest_path);
    assert_eq!(rows[0].project_id, project.summary.project_id);
}

#[test]
fn source_ui_projection_caps_visible_nodes_and_edges() {
    let project = synthetic_projection_project("project-source-ui", 1, 150, 180);
    let original_relation_count = project.edges.len();
    let projected = source_ui_graph_projection(project);
    let visible_source_count = projected
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Source)
        .count();
    let visible_concept_count = projected
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();

    assert_eq!(visible_source_count, 1);
    assert!(visible_concept_count <= SOURCE_UI_VISIBLE_CONCEPT_LIMIT);
    assert!(projected.edges.len() <= SOURCE_UI_VISIBLE_RELATION_LIMIT);
    assert_eq!(visible_source_count + visible_concept_count, 17);
    assert_eq!(projected.summary.hidden_concept_count, 134);
    assert_eq!(
        projected.summary.hidden_relation_count,
        original_relation_count - projected.edges.len()
    );
    assert_projection_has_no_dangling_edges(&projected);
    assert_projection_details_match_visible_graph(&projected);
}

#[test]
fn workspace_ui_projection_caps_visible_nodes_and_edges() {
    let project = synthetic_projection_project("workspace:projection-ui", 2, 75, 120);
    let original_relation_count = project.edges.len();
    let projected = workspace_ui_graph_projection(project);
    let visible_source_count = projected
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Source)
        .count();
    let visible_concept_count = projected
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .count();

    assert_eq!(visible_source_count, 2);
    assert!(visible_concept_count <= WORKSPACE_UI_VISIBLE_CONCEPT_LIMIT);
    assert!(projected.edges.len() <= WORKSPACE_UI_VISIBLE_RELATION_LIMIT);
    assert_eq!(projected.summary.hidden_concept_count, 15);
    assert_eq!(
        projected.summary.hidden_relation_count,
        original_relation_count - projected.edges.len()
    );
    assert!(projected
        .summary
        .summary
        .contains("Default projection shows 60 visible concept nodes"));
    assert_projection_has_no_dangling_edges(&projected);
    assert_projection_details_match_visible_graph(&projected);
}

#[test]
fn workspace_ui_projection_handles_large_graph_under_target_time() {
    let project = synthetic_projection_project("workspace:projection-ui-large", 4, 1_000, 1_400);
    let start = std::time::Instant::now();
    let projected = workspace_ui_graph_projection(project);
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "UI projection took {:?}",
        elapsed
    );
    assert!(
        projected
            .nodes
            .iter()
            .filter(|node| node.kind == GraphNodeKind::Concept)
            .count()
            <= WORKSPACE_UI_VISIBLE_CONCEPT_LIMIT
    );
    assert!(projected.edges.len() <= WORKSPACE_UI_VISIBLE_RELATION_LIMIT);
    assert_projection_has_no_dangling_edges(&projected);
}

#[test]
fn context_pack_caps_selected_evidence_and_graph_facts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let snapshot = synthetic_context_pack_snapshot(20);
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write synthetic context pack graph");
    let reader = BrainReader::open_workspace_root(workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("open brain reader");

    let start = std::time::Instant::now();
    let default_pack = reader
        .context_pack("alpha projection", 8_000)
        .expect("default context pack");
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "context pack assembly took {:?}",
        elapsed
    );
    assert!(default_pack.evidence.len() <= 15);
    assert!(default_pack.claims.len() + default_pack.relations.len() <= 12);
    assert!(default_pack.nodes.len() <= 24);

    let small_pack = reader
        .context_pack("alpha projection", 1_500)
        .expect("small context pack");
    assert!(small_pack.evidence.len() <= 8);
    assert!(small_pack.claims.len() + small_pack.relations.len() <= 5);
    assert!(small_pack.nodes.len() <= 10);
}

#[test]
fn context_pack_selected_node_bias_respects_budget_without_full_graph_export() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let snapshot = synthetic_context_pack_snapshot(20);
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write synthetic context pack graph");
    let reader = BrainReader::open_workspace_root(workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("open brain reader");

    let pack = reader
        .context_pack_with_selection("unmatched query", 1_500, Some("concept-alpha-019"))
        .expect("selected node context pack");

    assert!(pack
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-alpha-019"));
    assert!(pack
        .evidence
        .iter()
        .any(|evidence| evidence.id == "ev-alpha-019"));
    assert!(pack.evidence.len() <= 8);
    assert!(pack.claims.len() + pack.relations.len() <= 5);
    assert!(pack.nodes.len() <= 10);
    assert!(pack.nodes.len() < 20);
}

#[test]
fn context_pack_falls_back_to_evidence_when_graph_has_no_match() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = synthetic_context_pack_snapshot(3);
    snapshot.nodes.clear();
    snapshot.relations.clear();
    snapshot.claims.clear();
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write source-only context pack graph");
    let reader = BrainReader::open_workspace_root(workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("open brain reader");

    let pack = reader
        .context_pack("projection evidence 1", 8_000)
        .expect("source-only context pack");

    assert!(pack.nodes.is_empty());
    assert!(pack.relations.is_empty());
    assert!(!pack.sources.is_empty());
    assert!(pack.evidence.iter().any(|evidence| {
        evidence.source_id.as_deref() == Some("source-alpha")
            && evidence.snippet.contains("Alpha projection evidence")
    }));
}

#[test]
fn context_pack_excludes_ui_graph_state_and_raw_candidate_paths() {
    let pack = BrainContextPack {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "alpha projection".into(),
        token_budget: 8_000,
        summary: "Synthetic pack.".into(),
        wiki_pages: Vec::new(),
        nodes: vec![BrainNodeRecord {
            node_id: "concept-alpha".into(),
            kind: BrainNodeKind::Concept,
            label: "Alpha Projection".into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec!["ev-alpha".into()],
            source_ids: vec!["source-alpha".into()],
            confidence: Some(0.9),
            updated_at: 1,
        }],
        sources: vec![SourceRecord {
            source_id: "source-alpha".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/Users/example/private/source.pdf".into(),
            source_path: "/Users/example/private/source.pdf".into(),
            markdown_path:
                "/Users/example/private/artifacts/source-alpha/provider-graph-candidates/raw.json"
                    .into(),
            format: "pdf".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: Vec::new(),
        entities: Vec::new(),
        claims: vec![ClaimRecord {
            claim_id: "claim-alpha".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            statement: "Alpha projection is evidence backed.".into(),
            topic_refs: vec!["concept-alpha".into()],
            source_refs: vec!["source-alpha".into()],
            evidence_refs: vec!["ev-alpha".into()],
            status: "supported".into(),
            updated_at: 1,
        }],
        relations: Vec::new(),
        evidence: vec![EvidenceRef {
            id: "ev-alpha".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Alpha projection evidence.".into(),
            source_path: Some("/Users/example/private/source.pdf".into()),
            source_id: Some("source-alpha".into()),
            markdown_path: Some(
                "/Users/example/private/artifacts/source-alpha/provider-graph-source-raw-merged.json"
                    .into(),
            ),
            image_path: None,
            provenance: Some("synthetic context pack".into()),
        }],
        recent_events: Vec::new(),
        warnings: Vec::new(),
    };
    let mut artifact_metadata = ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([(
        "source-alpha".into(),
        ContextPackSourceMetadataV0 {
            content_hash: "fnv64:alpha".into(),
            provider_route: "ollama:local".into(),
            local_only: true,
        },
    )]));
    artifact_metadata.evidence.insert(
        "source-alpha".into(),
        BTreeMap::from([(
            "ev-alpha".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "source-alpha".into(),
                page: 1,
                region: Some("page:Page 1".into()),
                span: Some("line:1".into()),
                quoted_text: "Alpha projection evidence.".into(),
                parse_confidence: hyprduck_engine_types::ContextPackParseConfidence::High,
                content_hash: "fnv64:alpha".into(),
                markdown_path: Some(
                    "/Users/example/private/artifacts/source-alpha/provider-graph-candidates/raw.json"
                        .into(),
                ),
                image_path: None,
            },
        )]),
    );

    let external = hyprduck_engine_types::ContextPackV0::from_brain_context_pack(
        &pack,
        "ctx-alpha",
        "2026-05-20T00:00:00Z",
        &artifact_metadata,
    );
    let json = serde_json::to_string(&external).expect("context pack json");

    assert!(!json.contains("\"x\""));
    assert!(!json.contains("\"y\""));
    assert!(!json.contains("layout"));
    assert!(!json.contains("canvas"));
    assert!(!json.contains("selectedNodePosition"));
    assert!(!json.contains("provider-graph-candidates"));
    assert!(!json.contains("provider-graph-source-raw-merged"));
    assert!(!json.contains("/Users/example"));
}

#[test]
fn read_graph_snapshot_includes_materialization_report_counts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output_root = temp.path().join("HyprDuck");
    let workspace_root = output_root.join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 10;
    snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-alpha".into(),
        kind: BrainNodeKind::Concept,
        label: "Alpha".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: vec!["source-alpha".into()],
        confidence: Some(0.9),
        updated_at: 10,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized graph");
    let artifact_root = workspace_root.join("artifacts/source-alpha");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    write_json_pretty(
        &artifact_root.join("provider-graph-materialization.json"),
        &serde_json::json!({
            "sourceId": "source-alpha",
            "status": "linked",
            "stage": "linked",
            "progress": 1.0,
            "sourceGraphMaterialized": true,
            "workspaceLinkingMaterialized": true,
            "rawSourceGraphNodeCount": 151,
            "rawSourceGraphRelationCount": 150,
            "canonicalSourceGraphNodeCount": 32,
            "canonicalSourceGraphRelationCount": 48,
            "prunedSourceGraphNodeCount": 119,
            "prunedSourceGraphRelationCount": 102,
            "compactionStatus": "compacted"
        }),
    )
    .expect("write materialization report");

    let response = handle_read_graph_snapshot(hyprduck_engine_types::ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(output_root.display().to_string()),
        },
    })
    .expect("read graph snapshot");

    let report = response
        .graph_materialization_reports
        .first()
        .expect("materialization report");
    assert_eq!(report.source_id, "source-alpha");
    assert_eq!(report.status, "linked");
    assert_eq!(report.progress, 1.0);
    assert!(report.source_graph_materialized);
    assert_eq!(report.raw_source_graph_node_count, 151);
    assert_eq!(report.canonical_source_graph_node_count, 32);
    assert_eq!(report.canonical_source_graph_relation_count, 48);
}

#[test]
fn handle_load_project_defaults_to_workspace_graph_aggregate() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project, manifest) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\n",
        "source-a",
        "alpha",
        10,
    );
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project");

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    let response =
        handle_load_project(LoadProjectRequest::default()).expect("load project through handler");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }

    let project = response.project.expect("workspace aggregate project");
    assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
    assert_eq!(
        project.summary.project_id,
        workspace_project_id(DEFAULT_WORKSPACE_ID)
    );
    assert!(project
        .nodes
        .iter()
        .any(|node| node.id == "source:source-a"));
}

#[test]
fn default_load_project_falls_back_to_latest_legacy_project() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let project = compile_fixture_project(
        &temp,
        "# Legacy import\n\n## Page 1\n\nLegacy project snapshots remain visible.\n",
    );
    let request = CompileProjectRequest {
        source_markdown_path: temp.path().join("legacy.md").display().to_string(),
        source_document_path: None,
        source_manifest_path: None,
        workspace_id: None,
        source_id: None,
        skip_graph_generation: None,
    };
    store
        .save_project(&project, &request, None)
        .expect("save legacy project");

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    let response =
        handle_load_project(LoadProjectRequest::default()).expect("load default project");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }

    assert_eq!(response.sources.len(), 0);
    assert_eq!(
        response.project.expect("legacy project").summary.project_id,
        project.summary.project_id
    );
}

#[test]
fn workspace_delete_materialized_node_without_source_rows_returns_empty_project() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("HyprDuck").join("knowledge.sqlite3");
    let output_root = temp.path().join("HyprDuck");
    let workspace_root = output_root.join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 10;
    snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-last-materialized-node".into(),
        kind: BrainNodeKind::Concept,
        label: "Last Materialized Node".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: Vec::new(),
        confidence: None,
        updated_at: 10,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write materialized-only graph");

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output_root = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", &output_root);
    let response = handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: "concept-last-materialized-node".into(),
        kind: CorrectionKind::Delete,
        target_node_id: None,
        value: None,
    })
    .expect("delete last materialized node without source rows");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }
    match previous_output_root {
        Some(value) => std::env::set_var("HYPRDUCK_OUTPUT_DIR", value),
        None => std::env::remove_var("HYPRDUCK_OUTPUT_DIR"),
    }

    assert_eq!(response.project.summary.project_id, "workspace:default");
    assert!(response.project.nodes.is_empty());
    assert!(response.project.edges.is_empty());
    let nodes_after_delete: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json"))
            .expect("read nodes after delete");
    assert!(nodes_after_delete.is_empty());

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output_root = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", &output_root);
    let retry_response = handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: "concept-last-materialized-node".into(),
        kind: CorrectionKind::Delete,
        target_node_id: None,
        value: None,
    })
    .expect("retrying delete for already removed materialized node is a no-op");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }
    match previous_output_root {
        Some(value) => std::env::set_var("HYPRDUCK_OUTPUT_DIR", value),
        None => std::env::remove_var("HYPRDUCK_OUTPUT_DIR"),
    }
    assert!(retry_response.project.nodes.is_empty());
}

#[test]
fn deleting_source_replays_provider_graph_for_remaining_sources() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nThis source should keep its provider graph overlay.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nThis source will be deleted.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b");

    let rows = store
        .load_projects_for_workspace(DEFAULT_WORKSPACE_ID)
        .expect("load workspace rows");
    let workspace_root = workspace_root_for_rows(&rows).expect("workspace root");
    let mut provider_snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
            .expect("read base snapshot");
    provider_snapshot.generated_at = 100;
    provider_snapshot.evidence.push(EvidenceRef {
        id: "ev-provider-source-a".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Provider-only graph evidence for source A.".into(),
        source_path: Some(manifest_a.source_path.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        markdown_path: Some(manifest_a.markdown_path.clone()),
        image_path: None,
        provenance: Some("test provider overlay".into()),
    });
    provider_snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-provider-only-source-a".into(),
        kind: BrainNodeKind::Concept,
        label: "Provider Only Source A".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec!["ev-provider-source-a".into()],
        source_ids: vec![manifest_a.source_id.clone()],
        confidence: Some(0.91),
        updated_at: 100,
    });
    provider_snapshot.relations.push(BrainRelationRecord {
        relation_id: "edge-provider-source-a".into(),
        kind: BrainRelationKind::SourceOf,
        source_node_id: format!("source:{}", manifest_a.source_id),
        target_node_id: "concept-provider-only-source-a".into(),
        label: "Provider overlay".into(),
        evidence_ids: vec!["ev-provider-source-a".into()],
        confidence: Some(0.91),
        updated_at: 100,
    });
    provider_snapshot.claims.push(ClaimRecord {
        claim_id: "claim-provider-source-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Provider graph added a durable source A concept.".into(),
        topic_refs: vec!["concept-provider-only-source-a".into()],
        source_refs: vec![manifest_a.source_id.clone()],
        evidence_refs: vec!["ev-provider-source-a".into()],
        status: "supported".into(),
        updated_at: 100,
    });
    let provider_event = BrainEvent {
        event_id: "evt-provider-source-a-overlay".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("full_workspace_rebuild".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-graph-agent:full-workspace-rebuild".into(),
        },
        source_refs: provider_snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: provider_snapshot
            .sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
        node_refs: provider_snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: provider_snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: provider_snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: Vec::new(),
        target_node_ids: provider_snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: provider_snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: provider_snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: Vec::new(),
        evidence_refs: provider_snapshot
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: materialized_graph_event_payload_json(
            provider_snapshot.generated_at,
            &provider_snapshot.sources,
            &provider_snapshot.nodes,
            &provider_snapshot.relations,
            &provider_snapshot.evidence,
            &provider_snapshot.memories,
            &provider_snapshot.wiki_pages,
            &provider_snapshot.entities,
            &provider_snapshot.claims,
            &provider_snapshot.extractions,
        )
        .expect("provider graph payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: vec![manifest_a.source_id.clone(), manifest_b.source_id.clone()],
            snapshot_id: Some("snapshot-provider-source-a-overlay".into()),
            materialized_version: Some(100),
            ..Default::default()
        },
        confidence: Some("provider_full_workspace_rebuild".into()),
        policy_result: "materialized".into(),
        created_at: 100,
    };
    let mut events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events");
    events.push(provider_event);
    write_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"), &events)
        .expect("write provider event");

    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("replay provider overlay");
    let before_delete = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read overlay snapshot");
    assert!(before_delete
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-provider-only-source-a"));

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", temp.path());
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: format!("source:{}", manifest_b.source_id),
        kind: CorrectionKind::Delete,
        target_node_id: None,
        value: None,
    })
    .expect("delete source b");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }
    match previous_output {
        Some(value) => std::env::set_var("HYPRDUCK_OUTPUT_DIR", value),
        None => std::env::remove_var("HYPRDUCK_OUTPUT_DIR"),
    }

    let after_delete = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read after delete");
    assert!(after_delete
        .sources
        .iter()
        .all(|source| source.source_id != manifest_b.source_id));
    assert!(after_delete
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-provider-only-source-a"));
    assert!(after_delete.nodes.iter().all(|node| {
        !node.source_ids.contains(&manifest_b.source_id)
            && node.node_id != format!("source:{}", manifest_b.source_id)
    }));
}

#[test]
fn provider_overlay_replay_uses_latest_event_per_source_stage() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source = SourceRecord {
        source_id: "source-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/source-a.pdf".into(),
        source_path: "/tmp/source-a.pdf".into(),
        markdown_path: "/tmp/source-a.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence = EvidenceRef {
        id: "ev-source-a".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Source A evidence.".into(),
        source_path: Some(source.source_path.clone()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let source_node = BrainNodeRecord {
        node_id: "source:source-a".into(),
        kind: BrainNodeKind::Source,
        label: "source-a.pdf".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(1.0),
        updated_at: 1,
    };
    let concept_x = BrainNodeRecord {
        node_id: "concept-x".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept X".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let concept_y = BrainNodeRecord {
        node_id: "concept-y".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept Y".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let edge_x = BrainRelationRecord {
        relation_id: "edge-source-x".into(),
        kind: BrainRelationKind::SourceOf,
        source_node_id: source_node.node_id.clone(),
        target_node_id: concept_x.node_id.clone(),
        label: "source_of".into(),
        evidence_ids: vec![evidence.id.clone()],
        confidence: Some(1.0),
        updated_at: 100,
    };
    let edge_y = BrainRelationRecord {
        relation_id: "edge-source-y".into(),
        kind: BrainRelationKind::SourceOf,
        source_node_id: source_node.node_id.clone(),
        target_node_id: concept_y.node_id.clone(),
        label: "source_of".into(),
        evidence_ids: vec![evidence.id.clone()],
        confidence: Some(1.0),
        updated_at: 100,
    };
    let old_provider_evidence = EvidenceRef {
        id: "ev-provider-old".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Stale provider evidence.".into(),
        source_path: Some(source.source_path.clone()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(source.markdown_path.clone()),
        image_path: None,
        provenance: Some("provider_test".into()),
    };
    let old_entity = EntityRecord {
        entity_id: "entity-provider-old".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainNodeKind::Concept,
        name: "Old Entity".into(),
        aliases: Vec::new(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![old_provider_evidence.id.clone()],
        updated_at: 100,
    };
    let old_wiki_page = WikiPage {
        page_id: "wiki-provider-old".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/provider-old.md".into(),
        title: "Old Provider Wiki".into(),
        body: "Stale provider wiki.".into(),
        node_refs: vec![concept_y.node_id.clone()],
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![old_provider_evidence.id.clone()],
        updated_at: 100,
    };
    let old_extraction = StructuredExtractionArtifact {
        artifact_id: "extraction-provider-old".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: source.source_id.clone(),
        extractor: "provider_test".into(),
        extractor_model: Some("test-model".into()),
        source_refs: vec![source.source_id.clone()],
        page_refs: Vec::new(),
        entities: Vec::new(),
        topics: Vec::new(),
        claims: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        evidence_refs: vec![old_provider_evidence.clone()],
        confidence: Some(0.8),
        provenance: "provider_test".into(),
        created_at: 100,
    };
    let mut old_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-provider-old",
        operation_type: "source_graph_build",
        generated_at: 100,
        sources: std::slice::from_ref(&source),
        nodes: &[source_node.clone(), concept_x.clone(), concept_y.clone()],
        relations: &[edge_x.clone(), edge_y],
        evidence: &[evidence.clone(), old_provider_evidence.clone()],
        claims: &[],
    });
    old_event.payload_json = materialized_graph_event_payload_json(
        100,
        std::slice::from_ref(&source),
        &[source_node.clone(), concept_x.clone(), concept_y.clone()],
        std::slice::from_ref(&edge_x),
        &[evidence.clone(), old_provider_evidence.clone()],
        &[],
        std::slice::from_ref(&old_wiki_page),
        std::slice::from_ref(&old_entity),
        &[],
        std::slice::from_ref(&old_extraction),
    )
    .expect("provider graph payload with stale artifacts");
    let new_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-provider-new",
        operation_type: "source_graph_build",
        generated_at: 200,
        sources: std::slice::from_ref(&source),
        nodes: &[source_node.clone(), concept_x.clone()],
        relations: &[edge_x],
        evidence: std::slice::from_ref(&evidence),
        claims: &[],
    });
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![source];
    snapshot.evidence = vec![evidence, old_provider_evidence];
    snapshot.nodes = vec![source_node];
    snapshot.entities = vec![old_entity];
    snapshot.wiki_pages = vec![old_wiki_page];
    snapshot.extractions = vec![old_extraction];
    snapshot.events = vec![old_event, new_event];

    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write replayed graph");
    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read replayed graph");

    assert!(replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-x"));
    assert!(!replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-y"));
    assert!(!replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-source-y"));
    assert!(replayed
        .evidence
        .iter()
        .any(|evidence| evidence.id == "ev-source-a"));
    assert!(!replayed
        .evidence
        .iter()
        .any(|evidence| evidence.id == "ev-provider-old"));
    assert!(!replayed
        .entities
        .iter()
        .any(|entity| entity.entity_id == "entity-provider-old"));
    assert!(!replayed
        .wiki_pages
        .iter()
        .any(|page| page.page_id == "wiki-provider-old"));
    assert!(!replayed
        .extractions
        .iter()
        .any(|extraction| extraction.artifact_id == "extraction-provider-old"));
}

#[test]
fn full_workspace_rebuild_replay_supersedes_old_source_sets() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_a = SourceRecord {
        source_id: "source-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/source-a.pdf".into(),
        source_path: "/tmp/source-a.pdf".into(),
        markdown_path: "/tmp/source-a.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let source_b = SourceRecord {
        source_id: "source-b".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/source-b.pdf".into(),
        source_path: "/tmp/source-b.pdf".into(),
        markdown_path: "/tmp/source-b.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence_a = EvidenceRef {
        id: "ev-source-a".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Source A evidence.".into(),
        source_path: Some(source_a.source_path.clone()),
        source_id: Some(source_a.source_id.clone()),
        markdown_path: Some(source_a.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let source_node_a = BrainNodeRecord {
        node_id: "source:source-a".into(),
        kind: BrainNodeKind::Source,
        label: "source-a.pdf".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence_a.id.clone()],
        source_ids: vec![source_a.source_id.clone()],
        confidence: Some(1.0),
        updated_at: 1,
    };
    let stale_a_concept = BrainNodeRecord {
        node_id: "concept-stale-a".into(),
        kind: BrainNodeKind::Concept,
        label: "Stale A Concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence_a.id.clone()],
        source_ids: vec![source_a.source_id.clone()],
        confidence: Some(0.8),
        updated_at: 100,
    };
    let old_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-full-old",
        operation_type: "full_workspace_rebuild",
        generated_at: 100,
        sources: &[source_a.clone(), source_b],
        nodes: &[source_node_a.clone(), stale_a_concept],
        relations: &[],
        evidence: std::slice::from_ref(&evidence_a),
        claims: &[],
    });
    let new_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-full-new",
        operation_type: "full_workspace_rebuild",
        generated_at: 200,
        sources: std::slice::from_ref(&source_a),
        nodes: std::slice::from_ref(&source_node_a),
        relations: &[],
        evidence: std::slice::from_ref(&evidence_a),
        claims: &[],
    });
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![source_a];
    snapshot.evidence = vec![evidence_a];
    snapshot.nodes = vec![source_node_a];
    snapshot.events = vec![old_event, new_event];

    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write replayed graph");
    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read replayed graph");

    assert!(!replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-stale-a"));
}

#[test]
fn partial_linking_failure_state_keeps_source_graph_with_explicit_report() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source = SourceRecord {
        source_id: "source-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/source-a.pdf".into(),
        source_path: "/tmp/source-a.pdf".into(),
        markdown_path: "/tmp/source-a.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence = EvidenceRef {
        id: "ev-source-a".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Source A evidence.".into(),
        source_path: Some(source.source_path.clone()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let source_node = BrainNodeRecord {
        node_id: "source:source-a".into(),
        kind: BrainNodeKind::Source,
        label: "source-a.pdf".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(1.0),
        updated_at: 1,
    };
    let concept = BrainNodeRecord {
        node_id: "concept-source-a".into(),
        kind: BrainNodeKind::Concept,
        label: "Source A Concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let edge = BrainRelationRecord {
        relation_id: "edge-source-a-concept".into(),
        kind: BrainRelationKind::SourceOf,
        source_node_id: source_node.node_id.clone(),
        target_node_id: concept.node_id.clone(),
        label: "source_of".into(),
        evidence_ids: vec![evidence.id.clone()],
        confidence: Some(1.0),
        updated_at: 100,
    };
    let source_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-provider-source-a",
        operation_type: "source_graph_build",
        generated_at: 100,
        sources: std::slice::from_ref(&source),
        nodes: &[source_node.clone(), concept.clone()],
        relations: &[edge],
        evidence: std::slice::from_ref(&evidence),
        claims: &[],
    });
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![source.clone()];
    snapshot.evidence = vec![evidence];
    snapshot.nodes = vec![source_node];
    snapshot.events = vec![source_event];

    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write source graph");
    let artifact_root = workspace_root.join("artifacts").join(&source.source_id);
    write_json_pretty(
        &artifact_root.join("provider-graph-materialization.json"),
        &json!({
            "status": "source_graph_materialized_linking_failed",
            "provider": "openai",
            "model": "test-model",
            "sourceId": source.source_id,
            "sourceGraphNodeCount": 2,
            "sourceGraphRelationCount": 1,
            "workspaceLinkCount": 0,
            "materializedNodeCount": 2,
            "materializedRelationCount": 1,
            "materializedClaimCount": 0,
            "materializedMemoryCount": 0,
            "providerRunIds": ["provider-source-graph-test", "provider-workspace-linking-test"],
            "sourceGraphRunId": "provider-source-graph-test",
            "workspaceLinkingRunId": "provider-workspace-linking-test",
            "sourceGraphMaterialized": true,
            "workspaceLinkingMaterialized": false,
            "errorMessage": "workspace linking validation failed",
            "updatedAt": 101
        }),
    )
    .expect("write report");

    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read source graph");
    let report: Value =
        read_json_artifact(&artifact_root.join("provider-graph-materialization.json"))
            .expect("read materialization report");

    assert!(replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-source-a"));
    assert!(!replayed.events.iter().any(|event| {
        event.operation_type.as_deref() == Some("workspace_linking")
            && event.policy_result == "materialized"
    }));
    assert_eq!(report["status"], "source_graph_materialized_linking_failed");
    assert_eq!(report["sourceGraphMaterialized"], true);
    assert_eq!(report["workspaceLinkingMaterialized"], false);
    assert_eq!(report["materializedNodeCount"], 2);
    assert_eq!(report["materializedRelationCount"], 1);
    assert!(report.get("providerRunId").is_none());
    assert_eq!(
        report["providerRunIds"],
        json!([
            "provider-source-graph-test",
            "provider-workspace-linking-test"
        ])
    );
}

#[test]
fn provider_overlay_drops_null_source_evidence_after_source_deletion() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let active_source = SourceRecord {
        source_id: "source-active".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/active.pdf".into(),
        source_path: "/tmp/active.pdf".into(),
        markdown_path: "/tmp/active.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let deleted_source = SourceRecord {
        source_id: "source-deleted".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/deleted.pdf".into(),
        source_path: "/tmp/deleted.pdf".into(),
        markdown_path: "/tmp/deleted.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let null_source_evidence = EvidenceRef {
        id: "ev-null-source".into(),
        page_label: "Provider evidence".into(),
        page_index: None,
        snippet: "This evidence is not tied to an active source.".into(),
        source_path: None,
        source_id: None,
        markdown_path: None,
        image_path: None,
        provenance: Some("provider".into()),
    };
    let deleted_node = BrainNodeRecord {
        node_id: "concept-deleted-source".into(),
        kind: BrainNodeKind::Concept,
        label: "Deleted source concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![null_source_evidence.id.clone()],
        source_ids: vec![deleted_source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let deleted_claim = ClaimRecord {
        claim_id: "claim-deleted-source".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "A deleted source claim should not survive through null evidence.".into(),
        topic_refs: vec![deleted_node.node_id.clone()],
        source_refs: vec![deleted_source.source_id.clone()],
        evidence_refs: vec![null_source_evidence.id.clone()],
        status: "supported".into(),
        updated_at: 100,
    };
    let deleted_event = provider_test_event(ProviderTestEventInput {
        event_id: "evt-provider-deleted-source",
        operation_type: "source_graph_build",
        generated_at: 100,
        sources: &[deleted_source],
        nodes: &[deleted_node],
        relations: &[],
        evidence: &[null_source_evidence],
        claims: &[deleted_claim],
    });
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![active_source];
    snapshot.events = vec![deleted_event];

    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write replayed graph");
    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read replayed graph");

    assert!(replayed
        .evidence
        .iter()
        .all(|evidence| evidence.source_id.is_some()));
    assert!(!replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-deleted-source"));
    assert!(!replayed
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-deleted-source"));
}

#[test]
fn workspace_linking_artifacts_drop_after_source_deletion_breaks_cross_source_refs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let active_source = SourceRecord {
        source_id: "source-active".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/active.pdf".into(),
        source_path: "/tmp/active.pdf".into(),
        markdown_path: "/tmp/active.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let deleted_source = SourceRecord {
        source_id: "source-deleted".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/deleted.pdf".into(),
        source_path: "/tmp/deleted.pdf".into(),
        markdown_path: "/tmp/deleted.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let active_evidence = EvidenceRef {
        id: "ev-active".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Active source evidence.".into(),
        source_path: Some(active_source.source_path.clone()),
        source_id: Some(active_source.source_id.clone()),
        markdown_path: Some(active_source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let deleted_evidence = EvidenceRef {
        id: "ev-deleted".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Deleted source evidence.".into(),
        source_path: Some(deleted_source.source_path.clone()),
        source_id: Some(deleted_source.source_id.clone()),
        markdown_path: Some(deleted_source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let active_node = BrainNodeRecord {
        node_id: "concept-active".into(),
        kind: BrainNodeKind::Concept,
        label: "Current active concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![active_evidence.id.clone()],
        source_ids: vec![active_source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let mut stale_payload_active_node = active_node.clone();
    stale_payload_active_node.label = "Stale payload active concept".into();
    stale_payload_active_node.updated_at = 100;
    let active_peer_node = BrainNodeRecord {
        node_id: "concept-active-peer".into(),
        kind: BrainNodeKind::Concept,
        label: "Active peer concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![active_evidence.id.clone()],
        source_ids: vec![active_source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let deleted_node = BrainNodeRecord {
        node_id: "concept-deleted".into(),
        kind: BrainNodeKind::Concept,
        label: "Deleted concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![deleted_evidence.id.clone()],
        source_ids: vec![deleted_source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let cross_source_refs = vec![
        active_source.source_id.clone(),
        deleted_source.source_id.clone(),
    ];
    let cross_evidence_refs = vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    let linking_claim = ClaimRecord {
        claim_id: "claim-cross-source".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "This claim only has meaning while both sources exist.".into(),
        topic_refs: vec![active_node.node_id.clone(), deleted_node.node_id.clone()],
        source_refs: cross_source_refs.clone(),
        evidence_refs: cross_evidence_refs.clone(),
        status: "supported".into(),
        updated_at: 100,
    };
    let linking_memory = MemoryRecord {
        memory_id: "memory-cross-source".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Cross-source memory".into(),
        body: "This memory only has meaning while both sources exist.".into(),
        source_refs: cross_source_refs.clone(),
        evidence_refs: cross_evidence_refs.clone(),
        created_at: 100,
        updated_at: 100,
    };
    let linking_wiki_page = WikiPage {
        page_id: "wiki-cross-source".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/workspace/cross-source.md".into(),
        title: "Cross-source wiki".into(),
        body: "This wiki page only has meaning while both sources exist.".into(),
        node_refs: vec![active_node.node_id.clone(), deleted_node.node_id.clone()],
        source_refs: cross_source_refs,
        evidence_refs: cross_evidence_refs,
        updated_at: 100,
    };
    let linking_relation = BrainRelationRecord {
        relation_id: "relation-cross-source".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: active_node.node_id.clone(),
        target_node_id: active_peer_node.node_id.clone(),
        label: "cross-source relation".into(),
        evidence_ids: vec![active_evidence.id.clone(), deleted_evidence.id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let current_relation_collision = BrainRelationRecord {
        relation_id: "relation-collision".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: active_node.node_id.clone(),
        target_node_id: active_peer_node.node_id.clone(),
        label: "current source relation".into(),
        evidence_ids: vec![active_evidence.id.clone()],
        confidence: Some(0.8),
        updated_at: 1,
    };
    let mut stale_payload_relation_collision = current_relation_collision.clone();
    stale_payload_relation_collision.label = "stale workspace relation".into();
    stale_payload_relation_collision.evidence_ids =
        vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    stale_payload_relation_collision.updated_at = 100;
    let current_exact_relation_collision = BrainRelationRecord {
        relation_id: "relation-exact-source-owned".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: active_node.node_id.clone(),
        target_node_id: active_peer_node.node_id.clone(),
        label: "exact source-owned relation".into(),
        evidence_ids: vec![active_evidence.id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let stale_active_only_relation = BrainRelationRecord {
        relation_id: "relation-active-only-stale".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: active_node.node_id.clone(),
        target_node_id: active_peer_node.node_id.clone(),
        label: "stale active-only relation".into(),
        evidence_ids: vec![active_evidence.id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let current_claim_collision = ClaimRecord {
        claim_id: "claim-collision".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "current source claim".into(),
        topic_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        status: "supported".into(),
        updated_at: 1,
    };
    let mut stale_payload_claim_collision = current_claim_collision.clone();
    stale_payload_claim_collision.statement = "stale workspace claim".into();
    stale_payload_claim_collision.source_refs = vec![
        active_source.source_id.clone(),
        deleted_source.source_id.clone(),
    ];
    stale_payload_claim_collision.evidence_refs =
        vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    stale_payload_claim_collision.updated_at = 100;
    let current_exact_claim_collision = ClaimRecord {
        claim_id: "claim-exact-source-owned".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "exact source-owned claim".into(),
        topic_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        status: "supported".into(),
        updated_at: 100,
    };
    let stale_active_only_claim = ClaimRecord {
        claim_id: "claim-active-only-stale".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "stale active-only claim".into(),
        topic_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        status: "supported".into(),
        updated_at: 100,
    };
    let current_memory_collision = MemoryRecord {
        memory_id: "memory-collision".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "current source memory".into(),
        body: "current source memory".into(),
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        created_at: 1,
        updated_at: 1,
    };
    let mut stale_payload_memory_collision = current_memory_collision.clone();
    stale_payload_memory_collision.title = "stale workspace memory".into();
    stale_payload_memory_collision.body = "stale workspace memory".into();
    stale_payload_memory_collision.source_refs = vec![
        active_source.source_id.clone(),
        deleted_source.source_id.clone(),
    ];
    stale_payload_memory_collision.evidence_refs =
        vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    stale_payload_memory_collision.updated_at = 100;
    let current_exact_memory_collision = MemoryRecord {
        memory_id: "memory-exact-source-owned".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "exact source-owned memory".into(),
        body: "exact source-owned memory".into(),
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        created_at: 100,
        updated_at: 100,
    };
    let stale_active_only_memory = MemoryRecord {
        memory_id: "memory-active-only-stale".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "stale active-only memory".into(),
        body: "stale active-only memory".into(),
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        created_at: 100,
        updated_at: 100,
    };
    let source_owned_subset_memory = MemoryRecord {
        memory_id: "memory-source-owned-subset".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "source owned subset memory".into(),
        body: "source owned subset memory".into(),
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        created_at: 100,
        updated_at: 100,
    };
    let mut raw_payload_source_owned_subset_memory = source_owned_subset_memory.clone();
    raw_payload_source_owned_subset_memory.source_refs = vec![
        active_source.source_id.clone(),
        deleted_source.source_id.clone(),
    ];
    raw_payload_source_owned_subset_memory.evidence_refs =
        vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    let current_wiki_collision = WikiPage {
        page_id: "wiki-collision".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/workspace/collision.md".into(),
        title: "Current source wiki".into(),
        body: "current source wiki".into(),
        node_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        updated_at: 1,
    };
    let mut stale_payload_wiki_collision = current_wiki_collision.clone();
    stale_payload_wiki_collision.title = "Stale workspace wiki".into();
    stale_payload_wiki_collision.body = "stale workspace wiki".into();
    stale_payload_wiki_collision.source_refs = vec![
        active_source.source_id.clone(),
        deleted_source.source_id.clone(),
    ];
    stale_payload_wiki_collision.evidence_refs =
        vec![active_evidence.id.clone(), deleted_evidence.id.clone()];
    stale_payload_wiki_collision.updated_at = 100;
    let current_exact_wiki_collision = WikiPage {
        page_id: "wiki-exact-source-owned".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/workspace/exact-source-owned.md".into(),
        title: "Exact source-owned wiki".into(),
        body: "exact source-owned wiki".into(),
        node_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        updated_at: 100,
    };
    let stale_active_only_wiki = WikiPage {
        page_id: "wiki-active-only-stale".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/workspace/active-only-stale.md".into(),
        title: "Stale active-only wiki".into(),
        body: "stale active-only wiki".into(),
        node_refs: vec![active_node.node_id.clone()],
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        updated_at: 100,
    };
    let current_entity = EntityRecord {
        entity_id: "entity-active".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainNodeKind::Concept,
        name: "Current active entity".into(),
        aliases: Vec::new(),
        source_refs: vec![active_source.source_id.clone()],
        evidence_refs: vec![active_evidence.id.clone()],
        updated_at: 1,
    };
    let stale_payload_entity = EntityRecord {
        entity_id: "entity-active".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainNodeKind::Concept,
        name: "Stale payload active entity".into(),
        aliases: Vec::new(),
        source_refs: vec![
            active_source.source_id.clone(),
            deleted_source.source_id.clone(),
        ],
        evidence_refs: vec![active_evidence.id.clone(), deleted_evidence.id.clone()],
        updated_at: 100,
    };
    let linking_event = BrainEvent {
        event_id: "evt-workspace-linking-cross-source".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("workspace_linking".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-workspace-linking-agent:test".into(),
        },
        source_refs: vec![
            active_source.source_id.clone(),
            deleted_source.source_id.clone(),
        ],
        source_markdown_refs: vec![
            active_source.markdown_path.clone(),
            deleted_source.markdown_path.clone(),
        ],
        node_refs: vec![
            active_node.node_id.clone(),
            active_peer_node.node_id.clone(),
            deleted_node.node_id.clone(),
        ],
        relation_refs: vec![linking_relation.relation_id.clone()],
        claim_refs: vec![linking_claim.claim_id.clone()],
        memory_refs: vec![linking_memory.memory_id.clone()],
        target_node_ids: vec![active_node.node_id.clone(), deleted_node.node_id.clone()],
        target_edge_ids: vec![
            linking_relation.relation_id.clone(),
            current_relation_collision.relation_id.clone(),
        ],
        target_claim_ids: vec![
            linking_claim.claim_id.clone(),
            current_claim_collision.claim_id.clone(),
        ],
        target_memory_ids: vec![
            linking_memory.memory_id.clone(),
            current_memory_collision.memory_id.clone(),
        ],
        evidence_refs: vec![active_evidence.id.clone(), deleted_evidence.id.clone()],
        payload_json: materialized_graph_event_payload_json(
            100,
            &[],
            &[
                stale_payload_active_node,
                active_peer_node.clone(),
                deleted_node.clone(),
            ],
            &[
                linking_relation.clone(),
                stale_payload_relation_collision,
                current_exact_relation_collision.clone(),
            ],
            &[active_evidence.clone(), deleted_evidence],
            &[
                linking_memory.clone(),
                stale_payload_memory_collision,
                raw_payload_source_owned_subset_memory,
                current_exact_memory_collision.clone(),
            ],
            &[
                linking_wiki_page.clone(),
                stale_payload_wiki_collision,
                current_exact_wiki_collision.clone(),
            ],
            std::slice::from_ref(&stale_payload_entity),
            &[
                linking_claim.clone(),
                stale_payload_claim_collision,
                current_exact_claim_collision.clone(),
            ],
            &[],
        )
        .expect("workspace linking payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: vec![active_source.source_id.clone()],
            snapshot_id: Some("snapshot-workspace-linking-cross-source".into()),
            materialized_version: Some(100),
            ..Default::default()
        },
        confidence: Some("provider_test".into()),
        policy_result: "materialized".into(),
        created_at: 100,
    };
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![active_source];
    snapshot.evidence = vec![active_evidence];
    snapshot.nodes = vec![active_node, active_peer_node];
    snapshot.relations = vec![
        linking_relation,
        current_relation_collision,
        current_exact_relation_collision,
        stale_active_only_relation,
    ];
    snapshot.claims = vec![
        linking_claim,
        current_claim_collision,
        current_exact_claim_collision,
        stale_active_only_claim,
    ];
    snapshot.memories = vec![
        linking_memory,
        current_memory_collision,
        current_exact_memory_collision.clone(),
        source_owned_subset_memory,
        stale_active_only_memory,
    ];
    snapshot.entities = vec![current_entity];
    snapshot.wiki_pages = vec![
        linking_wiki_page.clone(),
        current_wiki_collision,
        current_exact_wiki_collision,
        stale_active_only_wiki,
    ];
    snapshot.events = vec![linking_event];
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure workspace dirs");
    write_json_pretty(
        &workspace_root.join("state/materialized-record-origins.json"),
        &serde_json::json!({
            "schemaVersion": BRAIN_EVENT_SCHEMA_VERSION,
            "relations": {
                "relation-active-only-stale": {
                    "eventId": "evt-workspace-linking-active-only-projection",
                    "operationType": "workspace_linking"
                }
            },
            "claims": {
                "claim-active-only-stale": {
                    "eventId": "evt-workspace-linking-active-only-projection",
                    "operationType": "workspace_linking"
                }
            },
            "memories": {
                "memory-active-only-stale": {
                    "eventId": "evt-workspace-linking-active-only-projection",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesById": {
                "wiki-active-only-stale": {
                    "eventId": "evt-workspace-linking-active-only-projection",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesByPath": {
                "wiki/workspace/active-only-stale.md": {
                    "eventId": "evt-workspace-linking-active-only-projection",
                    "operationType": "workspace_linking"
                }
            }
        }),
    )
    .expect("write previous materialized record origins");
    let mut persisted_stale_memory_collision = current_exact_memory_collision.clone();
    persisted_stale_memory_collision.source_refs =
        vec!["source-active".to_string(), "source-deleted".to_string()];
    persisted_stale_memory_collision.evidence_refs =
        vec!["ev-active".to_string(), "ev-deleted".to_string()];
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &[persisted_stale_memory_collision],
    )
    .expect("write stale persisted memory collision");
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write previous manifest with stale wiki page");
    let stale_wiki_path = workspace_root.join(&linking_wiki_page.path);
    write_file_atomic(&stale_wiki_path, b"stale cross-source wiki page")
        .expect("write stale wiki file");
    let orphan_wiki_path = workspace_root.join("wiki/workspace/orphan-cross-source.md");
    write_file_atomic(&orphan_wiki_path, b"orphaned stale cross-source wiki page")
        .expect("write orphan stale wiki file");

    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write replayed workspace graph");
    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read replayed workspace graph");

    assert!(!replayed
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-cross-source"));
    assert!(!replayed
        .memories
        .iter()
        .any(|memory| memory.memory_id == "memory-cross-source"));
    assert!(!replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "relation-cross-source"));
    assert!(!replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "relation-active-only-stale"));
    assert_eq!(
        replayed
            .relations
            .iter()
            .find(|relation| relation.relation_id == "relation-collision")
            .map(|relation| relation.label.as_str()),
        Some("current source relation")
    );
    assert!(replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "relation-exact-source-owned"));
    assert_eq!(
        replayed
            .nodes
            .iter()
            .find(|node| node.node_id == "concept-active")
            .map(|node| node.label.as_str()),
        Some("Current active concept")
    );
    assert!(replayed
        .evidence
        .iter()
        .any(|evidence| evidence.id == "ev-active"));
    assert_eq!(
        replayed
            .entities
            .iter()
            .find(|entity| entity.entity_id == "entity-active")
            .map(|entity| entity.name.as_str()),
        Some("Current active entity")
    );
    assert!(!replayed
        .wiki_pages
        .iter()
        .any(|page| page.page_id == "wiki-cross-source"));
    assert!(!replayed
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-active-only-stale"));
    assert!(!replayed
        .memories
        .iter()
        .any(|memory| memory.memory_id == "memory-active-only-stale"));
    assert!(!replayed
        .wiki_pages
        .iter()
        .any(|page| page.page_id == "wiki-active-only-stale"));
    assert_eq!(
        replayed
            .claims
            .iter()
            .find(|claim| claim.claim_id == "claim-collision")
            .map(|claim| claim.statement.as_str()),
        Some("current source claim")
    );
    assert!(replayed
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-exact-source-owned"));
    assert_eq!(
        replayed
            .memories
            .iter()
            .find(|memory| memory.memory_id == "memory-collision")
            .map(|memory| memory.body.as_str()),
        Some("current source memory")
    );
    assert!(replayed.memories.iter().any(|memory| {
        memory.memory_id == "memory-exact-source-owned"
            && memory.source_refs == vec!["source-active".to_string()]
            && memory.evidence_refs == vec!["ev-active".to_string()]
    }));
    assert!(replayed.memories.iter().any(|memory| {
        memory.memory_id == "memory-source-owned-subset"
            && memory.source_refs == vec!["source-active".to_string()]
            && memory.evidence_refs == vec!["ev-active".to_string()]
    }));
    assert_eq!(
        replayed
            .wiki_pages
            .iter()
            .find(|page| page.page_id == "wiki-collision")
            .map(|page| page.body.as_str()),
        Some("current source wiki")
    );
    assert!(replayed
        .wiki_pages
        .iter()
        .any(|page| page.page_id == "wiki-exact-source-owned"));
    assert!(
        !stale_wiki_path.exists(),
        "stale workspace-linking wiki markdown should be removed"
    );
    assert!(
        !orphan_wiki_path.exists(),
        "orphaned workspace-linking wiki markdown should be removed"
    );
}

#[test]
fn workspace_linking_legacy_top_level_refs_do_not_delete_current_records() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source = SourceRecord {
        source_id: "source-active".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/active.pdf".into(),
        source_path: "/tmp/active.pdf".into(),
        markdown_path: "/tmp/active.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence = EvidenceRef {
        id: "ev-active".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Active source evidence.".into(),
        source_path: Some(source.source_path.clone()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let node_a = BrainNodeRecord {
        node_id: "concept-a".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept A".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let node_b = BrainNodeRecord {
        node_id: "concept-b".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept B".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let relation = BrainRelationRecord {
        relation_id: "relation-stale".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: node_a.node_id.clone(),
        target_node_id: node_b.node_id.clone(),
        label: "stale relation".into(),
        evidence_ids: vec![evidence.id.clone()],
        confidence: Some(0.9),
        updated_at: 10,
    };
    let claim = ClaimRecord {
        claim_id: "claim-stale".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "stale claim".into(),
        topic_refs: vec![node_a.node_id.clone()],
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        status: "supported".into(),
        updated_at: 10,
    };
    let memory = MemoryRecord {
        memory_id: "memory-stale".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "stale memory".into(),
        body: "stale memory".into(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        created_at: 10,
        updated_at: 10,
    };
    let event = BrainEvent {
        event_id: "evt-workspace-linking-legacy-shape".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("workspace_linking".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-workspace-linking-agent:test".into(),
        },
        source_refs: vec![source.source_id.clone()],
        source_markdown_refs: vec![source.markdown_path.clone()],
        node_refs: vec![node_a.node_id.clone(), node_b.node_id.clone()],
        relation_refs: vec![relation.relation_id.clone()],
        claim_refs: vec![claim.claim_id.clone()],
        memory_refs: vec![memory.memory_id.clone()],
        target_node_ids: vec![node_a.node_id.clone(), node_b.node_id.clone()],
        target_edge_ids: vec![relation.relation_id.clone()],
        target_claim_ids: vec![claim.claim_id.clone()],
        target_memory_ids: vec![memory.memory_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        payload_json: "{}".into(),
        causality: BrainEventCausality {
            caused_by_source_ids: vec![source.source_id.clone()],
            snapshot_id: Some("snapshot-workspace-linking-legacy-shape".into()),
            materialized_version: Some(10),
            ..Default::default()
        },
        confidence: Some("provider_test".into()),
        policy_result: "materialized".into(),
        created_at: 10,
    };
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    snapshot.sources = vec![source];
    snapshot.evidence = vec![evidence];
    snapshot.nodes = vec![node_a, node_b];
    snapshot.relations = vec![relation];
    snapshot.claims = vec![claim];
    snapshot.memories = vec![memory];
    snapshot.events = vec![event];

    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write replayed workspace graph");
    let replayed = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read replayed workspace graph");

    assert!(replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "relation-stale"));
    assert!(replayed
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-stale"));
    assert!(replayed
        .memories
        .iter()
        .any(|memory| memory.memory_id == "memory-stale"));
    assert!(replayed
        .evidence
        .iter()
        .any(|existing| existing.id == "ev-active"));
    assert_eq!(replayed.nodes.len(), 2);
}

#[test]
fn valid_workspace_linking_records_carry_forward_previous_origins() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_a = SourceRecord {
        source_id: "source-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/a.pdf".into(),
        source_path: "/tmp/a.pdf".into(),
        markdown_path: "/tmp/a.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let source_b = SourceRecord {
        source_id: "source-b".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/b.pdf".into(),
        source_path: "/tmp/b.pdf".into(),
        markdown_path: "/tmp/b.md".into(),
        format: "pdf".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence_a = EvidenceRef {
        id: "ev-a".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Evidence A.".into(),
        source_path: Some(source_a.source_path.clone()),
        source_id: Some(source_a.source_id.clone()),
        markdown_path: Some(source_a.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let evidence_b = EvidenceRef {
        id: "ev-b".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Evidence B.".into(),
        source_path: Some(source_b.source_path.clone()),
        source_id: Some(source_b.source_id.clone()),
        markdown_path: Some(source_b.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let node_a = BrainNodeRecord {
        node_id: "concept-a".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept A".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence_a.id.clone()],
        source_ids: vec![source_a.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let node_b = BrainNodeRecord {
        node_id: "concept-b".into(),
        kind: BrainNodeKind::Concept,
        label: "Concept B".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence_b.id.clone()],
        source_ids: vec![source_b.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 1,
    };
    let relation = BrainRelationRecord {
        relation_id: "relation-valid-linking".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: node_a.node_id.clone(),
        target_node_id: node_b.node_id.clone(),
        label: "valid linking relation".into(),
        evidence_ids: vec![evidence_a.id.clone(), evidence_b.id.clone()],
        confidence: Some(0.9),
        updated_at: 10,
    };
    let claim = ClaimRecord {
        claim_id: "claim-valid-linking".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "valid linking claim".into(),
        topic_refs: vec![node_a.node_id.clone(), node_b.node_id.clone()],
        source_refs: vec![source_a.source_id.clone(), source_b.source_id.clone()],
        evidence_refs: vec![evidence_a.id.clone(), evidence_b.id.clone()],
        status: "supported".into(),
        updated_at: 10,
    };
    let memory = MemoryRecord {
        memory_id: "memory-valid-linking".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "valid linking memory".into(),
        body: "valid linking memory".into(),
        source_refs: vec![source_a.source_id.clone(), source_b.source_id.clone()],
        evidence_refs: vec![evidence_a.id.clone(), evidence_b.id.clone()],
        created_at: 10,
        updated_at: 10,
    };
    let wiki_page = WikiPage {
        page_id: "wiki-valid-linking".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/workspace/valid-linking.md".into(),
        title: "Valid linking wiki".into(),
        body: "valid linking wiki".into(),
        node_refs: vec![node_a.node_id.clone(), node_b.node_id.clone()],
        source_refs: vec![source_a.source_id.clone(), source_b.source_id.clone()],
        evidence_refs: vec![evidence_a.id.clone(), evidence_b.id.clone()],
        updated_at: 10,
    };
    let event = BrainEvent {
        event_id: "evt-valid-linking-origin".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("workspace_linking".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-workspace-linking-agent:test".into(),
        },
        source_refs: vec![source_a.source_id.clone(), source_b.source_id.clone()],
        source_markdown_refs: vec![
            source_a.markdown_path.clone(),
            source_b.markdown_path.clone(),
        ],
        node_refs: vec![node_a.node_id.clone(), node_b.node_id.clone()],
        relation_refs: vec![relation.relation_id.clone()],
        claim_refs: vec![claim.claim_id.clone()],
        memory_refs: vec![memory.memory_id.clone()],
        target_node_ids: Vec::new(),
        target_edge_ids: vec![relation.relation_id.clone()],
        target_claim_ids: vec![claim.claim_id.clone()],
        target_memory_ids: vec![memory.memory_id.clone()],
        evidence_refs: vec![evidence_a.id.clone(), evidence_b.id.clone()],
        payload_json: materialized_graph_event_payload_json(
            10,
            &[],
            &[],
            std::slice::from_ref(&relation),
            &[],
            std::slice::from_ref(&memory),
            std::slice::from_ref(&wiki_page),
            &[],
            std::slice::from_ref(&claim),
            &[],
        )
        .expect("workspace linking payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: vec![source_a.source_id.clone(), source_b.source_id.clone()],
            snapshot_id: Some("snapshot-valid-linking-origin".into()),
            materialized_version: Some(10),
            ..Default::default()
        },
        confidence: Some("provider_test".into()),
        policy_result: "materialized".into(),
        created_at: 10,
    };
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 10;
    snapshot.sources = vec![source_a, source_b];
    snapshot.evidence = vec![evidence_a, evidence_b];
    snapshot.nodes = vec![node_a, node_b];
    snapshot.relations = vec![relation];
    snapshot.claims = vec![claim];
    snapshot.memories = vec![memory];
    snapshot.wiki_pages = vec![wiki_page];
    snapshot.events = vec![event];
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure workspace dirs");
    write_json_pretty(
        &workspace_root.join("state/materialized-record-origins.json"),
        &serde_json::json!({
            "schemaVersion": BRAIN_EVENT_SCHEMA_VERSION,
            "relations": {
                "relation-valid-linking": {
                    "eventId": "evt-valid-linking-origin",
                    "operationType": "workspace_linking"
                }
            },
            "claims": {
                "claim-valid-linking": {
                    "eventId": "evt-valid-linking-origin",
                    "operationType": "workspace_linking"
                }
            },
            "memories": {
                "memory-valid-linking": {
                    "eventId": "evt-valid-linking-origin",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesById": {
                "wiki-valid-linking": {
                    "eventId": "evt-valid-linking-origin",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesByPath": {
                "wiki/workspace/valid-linking.md": {
                    "eventId": "evt-valid-linking-origin",
                    "operationType": "workspace_linking"
                }
            }
        }),
    )
    .expect("write previous materialized record origins");

    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write replayed workspace graph");
    let origins: serde_json::Value =
        read_json_artifact(&workspace_root.join("state/materialized-record-origins.json"))
            .expect("read materialized record origins");

    assert_eq!(
        origins["relations"]["relation-valid-linking"]["operationType"],
        "workspace_linking"
    );
    assert_eq!(
        origins["claims"]["claim-valid-linking"]["operationType"],
        "workspace_linking"
    );
    assert_eq!(
        origins["memories"]["memory-valid-linking"]["operationType"],
        "workspace_linking"
    );
    assert_eq!(
        origins["wikiPagesById"]["wiki-valid-linking"]["operationType"],
        "workspace_linking"
    );
    assert_eq!(
        origins["wikiPagesByPath"]["wiki/workspace/valid-linking.md"]["operationType"],
        "workspace_linking"
    );
}

#[test]
fn persist_effective_brain_snapshot_recomputes_record_origins() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 1;
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure workspace dirs");
    write_json_pretty(
        &workspace_root.join("state/materialized-record-origins.json"),
        &serde_json::json!({
            "schemaVersion": BRAIN_EVENT_SCHEMA_VERSION,
            "relations": {
                "relation-stale-origin": {
                    "eventId": "evt-stale-origin",
                    "operationType": "workspace_linking"
                }
            },
            "claims": {
                "claim-stale-origin": {
                    "eventId": "evt-stale-origin",
                    "operationType": "workspace_linking"
                }
            },
            "memories": {
                "memory-stale-origin": {
                    "eventId": "evt-stale-origin",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesById": {
                "wiki-stale-origin": {
                    "eventId": "evt-stale-origin",
                    "operationType": "workspace_linking"
                }
            },
            "wikiPagesByPath": {
                "wiki/workspace/stale-origin.md": {
                    "eventId": "evt-stale-origin",
                    "operationType": "workspace_linking"
                }
            }
        }),
    )
    .expect("write stale materialized record origins");

    persist_effective_brain_snapshot(&workspace_root, &snapshot)
        .expect("persist effective snapshot");
    let origins: serde_json::Value =
        read_json_artifact(&workspace_root.join("state/materialized-record-origins.json"))
            .expect("read recomputed materialized record origins");

    assert_eq!(origins["relations"].as_object().unwrap().len(), 0);
    assert_eq!(origins["claims"].as_object().unwrap().len(), 0);
    assert_eq!(origins["memories"].as_object().unwrap().len(), 0);
    assert_eq!(origins["wikiPagesById"].as_object().unwrap().len(), 0);
    assert_eq!(origins["wikiPagesByPath"].as_object().unwrap().len(), 0);
}

struct ProviderTestEventInput<'a> {
    event_id: &'a str,
    operation_type: &'a str,
    generated_at: u64,
    sources: &'a [SourceRecord],
    nodes: &'a [BrainNodeRecord],
    relations: &'a [BrainRelationRecord],
    evidence: &'a [EvidenceRef],
    claims: &'a [ClaimRecord],
}

fn provider_test_event(input: ProviderTestEventInput<'_>) -> BrainEvent {
    BrainEvent {
        event_id: input.event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some(input.operation_type.into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-graph-agent:test".into(),
        },
        source_refs: input
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: input
            .sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
        node_refs: input
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: input
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: input
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: Vec::new(),
        target_node_ids: input
            .nodes
            .iter()
            .filter(|node| node.kind != BrainNodeKind::Source)
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: input
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: input
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: Vec::new(),
        evidence_refs: input
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: materialized_graph_event_payload_json(
            input.generated_at,
            input.sources,
            input.nodes,
            input.relations,
            input.evidence,
            &[],
            &[],
            &[],
            input.claims,
            &[],
        )
        .expect("provider graph payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: input
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(format!("snapshot-{}", input.event_id)),
            materialized_version: Some(input.generated_at),
            ..Default::default()
        },
        confidence: Some("provider_test".into()),
        policy_result: "materialized".into(),
        created_at: input.generated_at,
    }
}

#[test]
fn exact_project_load_uses_project_workspace_sources() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nWorkspace A evidence stays separate.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, mut manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nWorkspace B evidence is newer.\n",
        "source-b",
        "beta",
        99,
    );
    manifest_b.workspace_id = "workspace-b".into();
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save workspace a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save workspace b project");

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    let response = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: None,
    })
    .expect("load exact project");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }

    assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
    assert_eq!(response.sources.len(), 1);
    assert_eq!(response.sources[0].source_id, "source-a");
    assert_eq!(
        response.project.expect("exact project").summary.project_id,
        project_a.summary.project_id
    );

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    let error = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: Some("workspace-b".into()),
    })
    .expect_err("stale workspace should not hydrate exact project");
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }
    assert!(error
        .to_string()
        .contains("belongs to workspace default, not workspace-b"));
}

#[test]
fn answer_project_supports_workspace_project_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project, manifest) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\n",
        "source-a",
        "alpha",
        10,
    );
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What does the shared context layer say?".into(),
        },
    )
    .expect("answer workspace project");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
}

#[test]
fn answer_empty_workspace_project_blocks_instead_of_error() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load empty workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What remains in the graph?".into(),
        },
    )
    .expect("answer empty workspace");

    assert_eq!(aggregate.summary.project_id, "workspace:default");
    assert_eq!(answer.status, AnswerStatus::Blocked);
    assert!(answer.text.is_none());
    assert!(answer.explanation.contains("No graph nodes"));
}

#[test]
fn workspace_answer_without_node_uses_matching_source_evidence() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: None,
            question: "What does the beta architecture context say?".into(),
        },
    )
    .expect("answer workspace project");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
    assert!(!answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
}

#[test]
fn materialized_workspace_rag_treats_selected_node_as_bias_only() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context says the release checklist owns quality gates.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context says the retry worker owns recovery.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    assert!(temp.path().join("default/brain-manifest.json").exists());
    assert!(temp.path().join("default/graph/evidence.json").exists());

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    let selected_bias_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("source:source-b".into()),
        question: "What does the architecture context say?".into(),
    })
    .expect("answer with selected source bias")
    .answer;
    let irrelevant_selection_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("source:source-b".into()),
        question: "What does the alpha planning context say?".into(),
    })
    .expect("answer across workspace despite irrelevant selected source")
    .answer;
    let missing_selection_answer = handle_answer_project(AnswerProjectRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: Some("concept-stale-selection".into()),
        question: "What does the beta retry worker own?".into(),
    })
    .expect("answer across workspace despite missing selected node")
    .answer;
    match previous_store {
        Some(value) => std::env::set_var("HYPRDUCK_PROJECT_STORE", value),
        None => std::env::remove_var("HYPRDUCK_PROJECT_STORE"),
    }

    assert_eq!(selected_bias_answer.status, AnswerStatus::Grounded);
    let selected_text = selected_bias_answer
        .text
        .as_deref()
        .expect("selected answer text");
    assert!(selected_text.contains("- Beta architecture context says"));
    assert!(!selected_text.contains("Best support:"));
    assert!(!selected_text.contains("strongest workspace match"));
    assert_eq!(
        selected_bias_answer
            .citations
            .first()
            .and_then(|citation| citation.source_id.as_deref()),
        Some("source-b")
    );
    assert!(irrelevant_selection_answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-a")));
    assert_eq!(
        irrelevant_selection_answer
            .citations
            .first()
            .and_then(|citation| citation.source_id.as_deref()),
        Some("source-a")
    );
    assert_ne!(missing_selection_answer.status, AnswerStatus::Blocked);
    assert!(missing_selection_answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
}

#[test]
fn workspace_answer_with_missing_selected_node_falls_back_to_question_match() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha planning context stays evidence backed.\n",
        "source-a",
        "alpha",
        10,
    );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBeta architecture context stays evidence backed.\n",
        "source-b",
        "beta",
        11,
    );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
        skip_graph_generation: None,
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
        skip_graph_generation: None,
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save source a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save source b project");
    let aggregate = load_answerable_project(&store, &workspace_project_id(DEFAULT_WORKSPACE_ID))
        .expect("load workspace answerable project");
    let answer = answer_project(
        &aggregate,
        &AnswerProjectRequest {
            project_id: aggregate.summary.project_id.clone(),
            node_id: Some("concept-stale-selection".into()),
            question: "What does the beta architecture context say?".into(),
        },
    )
    .expect("answer workspace project despite stale selected node");

    assert_ne!(answer.status, AnswerStatus::Blocked);
    assert!(answer
        .citations
        .iter()
        .any(|citation| citation.source_id.as_deref() == Some("source-b")));
}

#[test]
fn compile_project_rejects_request_ids_that_conflict_with_manifest() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some("/tmp/source.pdf".into()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some("different-workspace".into()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };

    let error = resolved_source_ids(&request, Some(&manifest)).expect_err("id mismatch");
    assert!(error
        .to_string()
        .contains("does not match source manifest workspace_id"));
}

#[test]
fn output_packaging_falls_back_to_next_root_when_primary_root_is_unwritable() {
    let temp = tempfile::tempdir().expect("temp dir");
    let blocked_root = temp.path().join("blocked-root");
    fs::write(&blocked_root, "not a directory").expect("blocked root file");
    let fallback_root = temp.path().join("fallback-root");
    let request = sample_parse_request(&temp);

    let manifest = write_output_package_with_fallback(
        &[blocked_root.clone(), fallback_root.clone()],
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
        &sample_engine_config(),
    )
    .expect("fallback output manifest");

    assert!(Path::new(&manifest.markdown_path).starts_with(&fallback_root));
    assert!(Path::new(&manifest.markdown_path).exists());
    assert!(Path::new(&manifest.source_path).exists());
    assert!(Path::new(&manifest.manifest_path).exists());
    assert!(manifest.artifact_root.contains("/default/artifacts/"));
    assert!(manifest.source_path.contains("/default/sources/"));
    assert_eq!(manifest.pages[0].label, "Page 1");
    assert!(manifest.pages[0]
        .markdown_path
        .as_deref()
        .is_some_and(|path| Path::new(path).exists()));
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    assert!(source_pack_path.exists());
    assert!(evidence_index_path.exists());

    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    assert_eq!(
        source_pack.schema_version,
        hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
    );
    assert_eq!(source_pack.source_id, manifest.source_id);
    assert_eq!(source_pack.page_count, 1);
    assert!(source_pack.content_hash.starts_with("fnv64:"));

    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");
    assert_eq!(
        evidence_index.schema_version,
        hyprduck_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
    );
    assert_eq!(evidence_index.source_id, manifest.source_id);
    assert_eq!(evidence_index.content_hash, source_pack.content_hash);
    assert_eq!(evidence_index.evidence.len(), 1);
    assert_eq!(evidence_index.evidence[0].page, 1);
    assert!(evidence_index.evidence[0]
        .quoted_text
        .contains("Grounded evidence"));
}

#[test]
fn output_packaging_uses_requested_workspace_and_source_ids() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let mut request = sample_parse_request(&temp);
    request.output = Some(hyprduck_engine_types::ParseOutputTarget {
        root_dir: Some(fallback_root.display().to_string()),
        name: Some("sample-import".into()),
        workspace_id: Some("workspace-alpha".into()),
        source_id: Some("source-alpha".into()),
    });

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
        &sample_engine_config(),
    )
    .expect("output manifest");

    assert_eq!(manifest.workspace_id, "workspace-alpha");
    assert_eq!(manifest.source_id, "source-alpha");
    assert!(manifest
        .artifact_root
        .contains("/workspace-alpha/artifacts/source-alpha"));
    assert!(manifest
        .source_path
        .contains("/workspace-alpha/sources/source-alpha"));
}

#[test]
fn output_packaging_records_partial_import_warnings_in_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.pages.push(ParsedPage {
        index: 1,
        markdown: None,
        plain_text: None,
        svg: None,
        image_asset_path: Some("images/page_2.png".into()),
        error_message: Some("provider unavailable".into()),
    });
    result.failed_count = 1;

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("partial output manifest");

    assert_eq!(manifest.status, IngestStatus::Partial);
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.warnings.len(), 1);
    assert_eq!(source_pack.warnings[0].warning_type, "page_parse_failed");
    assert_eq!(source_pack.warnings[0].page, Some(2));
    assert_eq!(evidence_index.warnings, source_pack.warnings);
    assert_eq!(evidence_index.evidence.len(), 1);
}

#[test]
fn retry_failed_page_updates_artifacts_and_regenerates_evidence_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.pages.push(ParsedPage {
        index: 1,
        markdown: None,
        plain_text: None,
        svg: None,
        image_asset_path: None,
        error_message: Some("provider unavailable".into()),
    });
    result.failed_count = 1;

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("partial output manifest");
    let first_page_path = manifest.pages[0]
        .markdown_path
        .as_ref()
        .expect("first page markdown")
        .clone();
    let first_page_before = fs::read_to_string(&first_page_path).expect("first page before");

    let response = retry_failed_page_artifacts(
        &RetryFailedPagesRequest {
            source_manifest_path: manifest.manifest_path.clone(),
            pages: vec![RetryPageArtifactUpdate {
                page_index: 1,
                markdown: Some("Recovered retry evidence for page two.".into()),
                plain_text: Some("Recovered retry evidence for page two.".into()),
                image_asset_path: None,
                error_message: None,
            }],
        },
        &sample_engine_config(),
    )
    .expect("retry failed page");

    assert_eq!(response.retried_page_count, 1);
    assert_eq!(response.remaining_failed_count, 0);
    assert_eq!(response.warnings_before, 1);
    assert_eq!(response.warnings_after, 0);
    assert_eq!(response.source_manifest.status, IngestStatus::Ingested);
    assert_eq!(
        fs::read_to_string(&first_page_path).expect("first page after"),
        first_page_before
    );

    let source_pack: hyprduck_engine_types::SourcePackV0 = serde_json::from_str(
        &fs::read_to_string(&response.source_pack_path).expect("source pack json"),
    )
    .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(&response.evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert!(source_pack.warnings.is_empty());
    assert_eq!(source_pack.ingestion_status, IngestStatus::Ingested);
    assert_eq!(source_pack.pages[1].page, 2);
    assert!(source_pack.pages[1].error_message.is_none());
    assert_eq!(evidence_index.warnings, source_pack.warnings);
    assert_eq!(evidence_index.evidence.len(), 2);
    assert!(evidence_index
        .evidence
        .iter()
        .any(|evidence| evidence.page == 2
            && evidence.quoted_text.contains("Recovered retry evidence")));
}

#[test]
fn retry_failed_page_failure_preserves_existing_partial_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.pages.push(ParsedPage {
        index: 1,
        markdown: None,
        plain_text: None,
        svg: None,
        image_asset_path: None,
        error_message: Some("provider unavailable".into()),
    });
    result.failed_count = 1;

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("partial output manifest");
    let first_page_path = manifest.pages[0]
        .markdown_path
        .as_ref()
        .expect("first page markdown")
        .clone();
    let first_page_before = fs::read_to_string(&first_page_path).expect("first page before");

    let response = retry_failed_page_artifacts(
        &RetryFailedPagesRequest {
            source_manifest_path: manifest.manifest_path.clone(),
            pages: vec![RetryPageArtifactUpdate {
                page_index: 1,
                markdown: None,
                plain_text: None,
                image_asset_path: None,
                error_message: Some("provider timeout on retry".into()),
            }],
        },
        &sample_engine_config(),
    )
    .expect("retry failure result");

    assert_eq!(response.retried_page_count, 0);
    assert_eq!(response.remaining_failed_count, 1);
    assert_eq!(response.warnings_before, 1);
    assert_eq!(response.warnings_after, 1);
    assert_eq!(response.source_manifest.status, IngestStatus::Partial);
    assert_eq!(
        fs::read_to_string(&first_page_path).expect("first page after"),
        first_page_before
    );

    let source_pack: hyprduck_engine_types::SourcePackV0 = serde_json::from_str(
        &fs::read_to_string(&response.source_pack_path).expect("source pack json"),
    )
    .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(&response.evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.warnings.len(), 1);
    assert_eq!(source_pack.warnings[0].page, Some(2));
    assert_eq!(source_pack.warnings[0].message, "provider timeout on retry");
    assert_eq!(evidence_index.warnings, source_pack.warnings);
    assert_eq!(evidence_index.evidence.len(), 1);
}

#[test]
fn output_packaging_records_all_page_failure_status() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.pages = vec![ParsedPage {
        index: 0,
        markdown: None,
        plain_text: None,
        svg: None,
        image_asset_path: Some("images/page_1.png".into()),
        error_message: Some("provider unavailable".into()),
    }];
    result.success_count = 0;
    result.failed_count = 1;

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("failed output manifest");

    assert_eq!(manifest.status, IngestStatus::Failed);
    assert_eq!(manifest.pages.len(), 1);
    assert_eq!(
        manifest.pages[0].error_message.as_deref(),
        Some("provider unavailable")
    );
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.warnings.len(), 1);
    assert_eq!(source_pack.warnings[0].warning_type, "page_parse_failed");
    assert_eq!(evidence_index.warnings, source_pack.warnings);
    assert!(evidence_index.evidence.is_empty());
}

#[test]
fn output_packaging_records_ollama_as_local_provider() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = EngineConfig {
        provider: ProviderKind::Ollama,
        model_id: "qwen3-vl:8b".into(),
        api_key: String::new(),
        base_url: None,
        prompt_template: "General".into(),
    };

    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
        &config,
    )
    .expect("output manifest");
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.provider_route, "ollama");
    assert!(source_pack.local_only);
    assert_eq!(evidence_index.provider_route, "ollama");
    assert!(evidence_index.local_only);
}

#[test]
fn output_packaging_marks_remote_ollama_as_not_local_only() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = EngineConfig {
        provider: ProviderKind::Ollama,
        model_id: "qwen3-vl:8b".into(),
        api_key: String::new(),
        base_url: Some("http://192.168.1.10:11434".into()),
        prompt_template: "General".into(),
    };
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
        &config,
    )
    .expect("output manifest");
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: hyprduck_engine_types::EvidenceIndexV0 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.provider_route, "ollama");
    assert!(!source_pack.local_only);
    assert_eq!(evidence_index.provider_route, "ollama");
    assert!(!evidence_index.local_only);
}

#[test]
fn output_packaging_does_not_treat_loopback_prefix_domains_as_local() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = EngineConfig {
        provider: ProviderKind::Ollama,
        model_id: "qwen3-vl:8b".into(),
        api_key: String::new(),
        base_url: Some("http://localhost.example.com:11434".into()),
        prompt_template: "General".into(),
    };
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
        &config,
    )
    .expect("output manifest");
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let source_pack: hyprduck_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");

    assert_eq!(source_pack.provider_route, "ollama");
    assert!(!source_pack.local_only);
}

#[test]
fn answer_project_blocks_empty_question() {
    let temp = tempfile::tempdir().expect("temp dir");
    let project = compile_fixture_project(
        &temp,
        "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\n",
    );

    let answer = answer_project(
        &project,
        &AnswerProjectRequest {
            project_id: project.summary.project_id.clone(),
            node_id: None,
            question: "   ".into(),
        },
    )
    .expect("answer project");

    assert_eq!(answer.status, AnswerStatus::Blocked);
    assert!(answer.citations.is_empty());
}

#[test]
fn ollama_models_endpoint_normalizes_common_base_urls() {
    let mut config = EngineConfig {
        provider: ProviderKind::Ollama,
        model_id: "qwen3-vl:8b".into(),
        api_key: String::new(),
        base_url: Some("http://127.0.0.1:11434".into()),
        prompt_template: "General".into(),
    };

    assert_eq!(
        ollama_models_endpoint(&config),
        "http://127.0.0.1:11434/v1/models"
    );

    config.base_url = Some("http://127.0.0.1:11434/api/generate".into());
    assert_eq!(
        ollama_models_endpoint(&config),
        "http://127.0.0.1:11434/api/tags"
    );

    config.base_url = Some("http://127.0.0.1:11434/v1/chat/completions".into());
    assert_eq!(
        ollama_models_endpoint(&config),
        "http://127.0.0.1:11434/v1/models"
    );
}

#[test]
fn resolve_binary_prefers_path_before_common_locations() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    let binary_path = bin_dir.join("hyprduck-test-bin");
    fs::write(&binary_path, "").expect("test bin");

    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", &bin_dir);
    let resolved = resolve_binary("hyprduck-test-bin", &["/definitely/missing"]);
    match old_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }

    assert_eq!(resolved, binary_path);
}
