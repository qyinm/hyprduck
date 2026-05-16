use super::*;
use crate::provider::{ollama_models_endpoint, ProviderKind};

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
            snippet: "Alpha source describes graph algorithms.".into(),
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
                    "nodeId": "concept-kruskal",
                    "kind": "concept",
                    "label": "Kruskal algorithm",
                    "scope": "project",
                    "evidenceIds": ["ev-alpha", "retrieved:stale:chunk"],
                    "sourceIds": ["source-alpha", "source-stale"],
                    "updatedAt": generated_at
                }
            ],
            "edges": [
                {
                    "relationId": "edge-alpha-kruskal",
                    "kind": "related_to",
                    "sourceNodeId": "source:source-alpha",
                    "targetNodeId": "concept-kruskal",
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
        .any(|node| node.node_id == "concept-kruskal"));
    assert!(snapshot
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-alpha-kruskal"));
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
                    "relationId": "edge-multisource",
                    "kind": "related_to",
                    "sourceNodeId": "concept-alpha",
                    "targetNodeId": "concept-shared",
                    "label": "already shared endpoint",
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
        .any(|relation| relation.relation_id == "edge-multisource"));

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
fn workspace_linking_prompt_uses_only_active_workspace_source_chunks() {
    let temp = tempdir().expect("tempdir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("workspace root");
    let valid_manifest = sample_manifest_with_source(&temp, "source-valid", "valid", 1);
    let stale_manifest = sample_manifest_with_source(&temp, "source-stale", "stale", 1);
    let valid_markdown = "# Valid\n\nCurrent workspace source text.";
    let stale_markdown = "# Stale\n\nStale source-index text must not enter rebuild prompt.";
    let valid_chunks = chunk_source_markdown(&valid_manifest, valid_markdown);
    let stale_chunks = chunk_source_markdown(&stale_manifest, stale_markdown);
    upsert_source_chunks(&workspace_root, &valid_manifest, &valid_chunks)
        .expect("write valid chunks");
    upsert_source_chunks(&workspace_root, &stale_manifest, &stale_chunks)
        .expect("write stale chunks");

    let baseline = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![SourceRecord {
            source_id: valid_manifest.source_id.clone(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: valid_manifest.original_path.clone(),
            source_path: valid_manifest.source_path.clone(),
            markdown_path: valid_manifest.markdown_path.clone(),
            format: "pdf".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
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
        &valid_manifest,
        valid_markdown,
        &baseline,
        &valid_chunks,
    )
    .expect("context");

    let prompt = build_workspace_linking_prompt(
        &workspace_root,
        DEFAULT_WORKSPACE_ID,
        &valid_manifest,
        valid_markdown,
        &baseline,
        &context,
    )
    .expect("prompt");

    assert!(prompt.contains("Current workspace source text."));
    assert!(!prompt.contains("Stale source-index text must not enter rebuild prompt."));
    assert!(!prompt.contains("source-stale"));
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
    assert!(health.review_items.is_empty());
    assert!(health.recent_events.is_empty());
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
    let health =
        handle_get_brain_health(GetBrainHealthRequest { scope }).expect("health runs maintenance");
    assert_eq!(health.status, BrainHealthStatus::Clean);
    let report: BrainMaintenanceReport =
        read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
            .expect("read lint report");
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

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Memory,
        title: "Remember bootstrap behavior".into(),
        body: "Missing memory and event files should be recreated safely.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        },
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: Vec::new(),
        node_refs: Vec::new(),
        evidence_refs: Vec::new(),
        proposal_payload: None,
    })
    .expect("propose memory with missing files");
    assert_eq!(response.proposal.status, BrainProposalStatus::Accepted);

    let memories = read_memory_records(&workspace_root).expect("read bootstrapped memories");
    assert_eq!(memories.len(), 1);
    assert!(workspace_root.join("events/brain_events.jsonl").exists());
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
    assert!(events.events.iter().any(|event| {
        event.event_type == BrainEventKind::GraphMaterialized
            && event.operation_type.as_deref() == Some("new_memory")
            && event.policy_result == "auto_applied"
            && event.target_memory_ids.contains(&memories[0].memory_id)
    }));
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
fn brain_writer_deduplicates_concurrent_safe_proposals_by_fingerprint() {
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
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let proposal_request = ProposeBrainUpdateRequest {
        scope,
        kind: BrainProposalKind::Memory,
        title: "Remember duplicate write".into(),
        body: "The same agent memory should collapse to one memory record.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        },
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: Vec::new(),
        proposal_payload: None,
    };

    let handles = (0..8)
        .map(|_| {
            let request = proposal_request.clone();
            std::thread::spawn(move || {
                handle_propose_brain_update(request).expect("concurrent safe proposal")
            })
        })
        .collect::<Vec<_>>();
    let responses = handles
        .into_iter()
        .map(|handle| handle.join().expect("join concurrent proposal"))
        .collect::<Vec<_>>();
    assert!(responses
        .iter()
        .all(|response| response.proposal.status == BrainProposalStatus::Accepted));

    let memories = read_memory_records(&workspace_root).expect("read memory records");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].title, "Remember duplicate write");
    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("events remain valid JSONL");
    assert!(events.len() >= 16);
    assert!(!workspace_root.join(BRAIN_LOCK_DIRECTORY_NAME).exists());
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
    assert!(aggregate.details_by_node_id.values().any(|detail| detail
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-a"))));
    assert!(aggregate.details_by_node_id.values().any(|detail| detail
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-b"))));
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
fn handle_load_project_defaults_to_workspace_graph_aggregate() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
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
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
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
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
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
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
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
    let mut old_event = provider_test_event(
        "evt-provider-old",
        "source_graph_build",
        100,
        &[source.clone()],
        &[source_node.clone(), concept_x.clone(), concept_y.clone()],
        &[edge_x.clone(), edge_y],
        &[evidence.clone(), old_provider_evidence.clone()],
        &[],
    );
    old_event.payload_json = materialized_graph_event_payload_json(
        100,
        &[source.clone()],
        &[source_node.clone(), concept_x.clone(), concept_y.clone()],
        &[edge_x.clone()],
        &[evidence.clone(), old_provider_evidence.clone()],
        &[],
        std::slice::from_ref(&old_wiki_page),
        std::slice::from_ref(&old_entity),
        &[],
        std::slice::from_ref(&old_extraction),
    )
    .expect("provider graph payload with stale artifacts");
    let new_event = provider_test_event(
        "evt-provider-new",
        "source_graph_build",
        200,
        &[source.clone()],
        &[source_node.clone(), concept_x.clone()],
        &[edge_x],
        &[evidence.clone()],
        &[],
    );
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
    let old_event = provider_test_event(
        "evt-full-old",
        "full_workspace_rebuild",
        100,
        &[source_a.clone(), source_b],
        &[source_node_a.clone(), stale_a_concept],
        &[],
        std::slice::from_ref(&evidence_a),
        &[],
    );
    let new_event = provider_test_event(
        "evt-full-new",
        "full_workspace_rebuild",
        200,
        std::slice::from_ref(&source_a),
        std::slice::from_ref(&source_node_a),
        &[],
        std::slice::from_ref(&evidence_a),
        &[],
    );
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
    let source_event = provider_test_event(
        "evt-provider-source-a",
        "source_graph_build",
        100,
        &[source.clone()],
        &[source_node.clone(), concept.clone()],
        &[edge],
        &[evidence.clone()],
        &[],
    );
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
    let deleted_event = provider_test_event(
        "evt-provider-deleted-source",
        "source_graph_build",
        100,
        &[deleted_source],
        &[deleted_node],
        &[],
        &[null_source_evidence],
        &[deleted_claim],
    );
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

fn provider_test_event(
    event_id: &str,
    operation_type: &str,
    generated_at: u64,
    sources: &[SourceRecord],
    nodes: &[BrainNodeRecord],
    relations: &[BrainRelationRecord],
    evidence: &[EvidenceRef],
    claims: &[ClaimRecord],
) -> BrainEvent {
    BrainEvent {
        event_id: event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some(operation_type.into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "hyprduck-provider-graph-agent:test".into(),
        },
        source_refs: sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
        node_refs: nodes.iter().map(|node| node.node_id.clone()).collect(),
        relation_refs: relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: claims.iter().map(|claim| claim.claim_id.clone()).collect(),
        memory_refs: Vec::new(),
        target_node_ids: nodes
            .iter()
            .filter(|node| node.kind != BrainNodeKind::Source)
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: claims.iter().map(|claim| claim.claim_id.clone()).collect(),
        target_memory_ids: Vec::new(),
        evidence_refs: evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: materialized_graph_event_payload_json(
            generated_at,
            sources,
            nodes,
            relations,
            evidence,
            &[],
            &[],
            &[],
            claims,
            &[],
        )
        .expect("provider graph payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(format!("snapshot-{event_id}")),
            materialized_version: Some(generated_at),
            ..Default::default()
        },
        confidence: Some("provider_test".into()),
        policy_result: "materialized".into(),
        created_at: generated_at,
    }
}

#[test]
fn exact_project_load_uses_project_workspace_sources() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
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
        &[fallback_root.clone()],
        "sample-import",
        "123",
        &request,
        &sample_parse_result(),
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
