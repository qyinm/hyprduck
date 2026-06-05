use super::*;
use crate::provider::ProviderKind;

pub(super) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn compile_fixture_project(
    temp: &tempfile::TempDir,
    markdown: &str,
) -> KnowledgeProject {
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

pub(super) fn compile_manifest_fixture_project_with_source(
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

pub(super) fn synthetic_projection_project(
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

pub(super) fn assert_projection_has_no_dangling_edges(project: &KnowledgeProject) {
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

pub(super) fn assert_projection_details_match_visible_graph(project: &KnowledgeProject) {
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

pub(super) fn synthetic_context_pack_snapshot(record_count: usize) -> BrainRepoSnapshot {
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
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
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
            valid_from: 0,
            valid_to: None,
            superseded_by: None,
        });
    }
    snapshot
}

pub(super) fn sample_parse_result() -> ParseResult {
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

pub(super) fn sample_parse_request(temp: &tempfile::TempDir) -> ParseRequest {
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

pub(super) fn sample_engine_config() -> EngineConfig {
    EngineConfig {
        provider: ProviderKind::OpenRouter,
        model_id: "openai/gpt-4.1-mini".into(),
        api_key: String::new(),
        base_url: None,
        prompt_template: "General".into(),
    }
}

pub(super) fn sample_manifest(temp: &tempfile::TempDir) -> SourceArtifactManifest {
    sample_manifest_with_source(temp, "source-test", "source", 2)
}

pub(super) fn sample_manifest_with_source(
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
