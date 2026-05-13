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
    };

    let markdown = fs::read_to_string(&markdown_path).expect("read markdown");
    compile_knowledge_project(&request, &markdown, None)
}

fn compile_manifest_fixture_project(
    temp: &tempfile::TempDir,
    markdown: &str,
) -> (KnowledgeProject, SourceArtifactManifest) {
    compile_manifest_fixture_project_with_source(temp, markdown, "source-test", "source", 2)
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
    };

    (
        compile_knowledge_project(&request, markdown, Some(&manifest)),
        manifest,
    )
}

#[test]
fn provider_graph_parser_accepts_fenced_payloads() {
    let raw = r#"```json
{
  "proposals": [
    {
      "changeType": "new_node",
      "node": {
        "label": "HyprDuck",
        "kind": "project",
        "sourcePath": "/tmp/source.md",
        "nodeId": "node-provider-hyprduck",
        "sourceRefs": ["source-a"],
        "evidenceRefs": ["ev-a"],
        "reason": "Project identity is explicit."
      }
    },
    {
      "changeType": "new_edge",
      "edge": {
        "sourceNodeId": "node-provider-hyprduck",
        "targetNodeId": "node-provider-agent-brain",
        "kind": "related_to",
        "label": "relates to",
        "sourcePath": "/tmp/source.md",
        "sourceRefs": ["source-a"],
        "evidenceRefs": ["ev-a"]
      }
    }
  ]
}
```"#;

    let payloads = parse_provider_graph_proposal_payloads(raw).expect("parse provider JSON");

    assert_eq!(payloads.len(), 2);
    assert!(matches!(
        &payloads[0],
        AgentGraphProposalPayload::NewNode { node }
            if node.label == "HyprDuck" && node.kind == BrainNodeKind::Project
    ));
    assert!(matches!(
        &payloads[1],
        AgentGraphProposalPayload::NewEdge { edge }
            if edge.kind == BrainRelationKind::RelatedTo
                && edge.source_node_id == "node-provider-hyprduck"
    ));
}

#[test]
fn provider_graph_payload_normalization_adds_source_and_evidence_refs() {
    let temp = tempdir().expect("tempdir");
    let manifest = sample_manifest_with_source(&temp, "source-agent", "source", 1);
    let mut payload = AgentGraphProposalPayload::NewClaim {
        claim: AgentNewClaimPayload {
            statement: "HyprDuck keeps graph updates source-backed.".into(),
            source_path: String::new(),
            claim_id: None,
            topic_refs: vec!["node-provider-hyprduck".into()],
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            reason: None,
        },
    };

    normalize_provider_graph_proposal_payload(&mut payload, &manifest, &["ev-agent".into()]);

    let AgentGraphProposalPayload::NewClaim { claim } = payload else {
        panic!("expected claim payload");
    };
    assert_eq!(claim.source_path, manifest.markdown_path);
    assert_eq!(claim.source_refs, vec!["source-agent"]);
    assert_eq!(claim.evidence_refs, vec!["ev-agent"]);
    assert!(claim
        .claim_id
        .as_deref()
        .is_some_and(|id| id.starts_with("claim-provider-")));
}

#[test]
fn provider_graph_payload_proposal_is_auto_applied_by_queue_worker() {
    let temp = tempdir().expect("tempdir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&workspace_root).expect("create workspace");
    write_json_pretty(
        &workspace_root.join("brain-manifest.json"),
        &empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID),
    )
    .expect("write baseline manifest");
    let payload = AgentGraphProposalPayload::NewNode {
        node: AgentNewNodePayload {
            label: "Agent maintained graph".into(),
            kind: BrainNodeKind::Concept,
            source_path: "/tmp/source.md".into(),
            node_id: Some("node-provider-agent-maintained-graph".into()),
            aliases: Vec::new(),
            source_refs: vec!["source-agent".into()],
            evidence_refs: vec!["ev-agent".into()],
            reason: Some("The source defines the durable graph behavior.".into()),
        },
    };
    let mut proposal =
        provider_graph_payload_to_proposal(DEFAULT_WORKSPACE_ID, PROVIDER_GRAPH_AGENT_ID, payload);
    enrich_agent_graph_proposal_refs(&mut proposal);
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("open writer");
    writer.write_proposal(&proposal).expect("write proposal");
    writer
        .append_event(&brain_event_for_proposal(&proposal).expect("proposal event"))
        .expect("append event");
    drop(writer);

    let result = run_queued_agent_proposal_apply_worker(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("apply provider proposal");
    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");

    assert_eq!(result.applied, vec![proposal.proposal_id]);
    assert!(snapshot.nodes.iter().any(|node| {
        node.node_id == "node-provider-agent-maintained-graph"
            && node.label == "Agent maintained graph"
    }));
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

fn multi_source_fixture_pages(temp: &tempfile::TempDir, source_id: &str) -> Vec<PageArtifact> {
    (0..2)
        .map(|index| PageArtifact {
            index,
            label: format!("Page {}", index + 1),
            image_path: Some(
                temp.path()
                    .join(format!(
                        "default/artifacts/{source_id}/images/page_{}.png",
                        index + 1
                    ))
                    .display()
                    .to_string(),
            ),
            markdown_path: Some(
                temp.path()
                    .join(format!(
                        "default/artifacts/{source_id}/pages/page_{}.md",
                        index + 1
                    ))
                    .display()
                    .to_string(),
            ),
            plain_text_path: None,
            error_message: None,
        })
        .collect()
}

fn rename_first_concept_for_test(
    project: &mut KnowledgeProject,
    canonical_name: &str,
    aliases: &[&str],
) {
    let concept_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    rename_concept_for_test(project, &concept_id, canonical_name, aliases);
}

fn rename_concept_for_test(
    project: &mut KnowledgeProject,
    concept_id: &str,
    canonical_name: &str,
    aliases: &[&str],
) {
    let node = project
        .nodes
        .iter_mut()
        .find(|node| node.id == concept_id)
        .expect("mutable concept node");
    node.label = canonical_name.into();
    let detail = project
        .details_by_node_id
        .get_mut(concept_id)
        .expect("concept detail");
    detail.canonical_name = canonical_name.into();
    detail.aliases = aliases.iter().map(|alias| (*alias).to_string()).collect();
    detail.node.label = canonical_name.into();
}

fn assert_materialized_brain_has_no_dangling_refs(workspace_root: &Path) {
    let snapshot: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read brain manifest");
    let nodes: Vec<BrainNodeRecord> = read_json_artifact(&workspace_root.join("graph/nodes.json"))
        .expect("read materialized nodes");
    let edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json"))
            .expect("read materialized edges");
    let claims: Vec<ClaimRecord> = read_json_artifact(&workspace_root.join("graph/claims.json"))
        .expect("read materialized claims");
    let memories: Vec<MemoryRecord> =
        read_json_artifact(&workspace_root.join("memory/records.json"))
            .expect("read materialized memories");

    let node_ids = nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<BTreeSet<_>>();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect::<BTreeSet<_>>();
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<BTreeSet<_>>();
    let wiki_paths = snapshot
        .wiki_pages
        .iter()
        .map(|page| page.path.as_str())
        .collect::<BTreeSet<_>>();

    for edge in &edges {
        assert!(
            node_ids.contains(edge.source_node_id.as_str()),
            "edge {} has dangling source node {}",
            edge.relation_id,
            edge.source_node_id
        );
        assert!(
            node_ids.contains(edge.target_node_id.as_str()),
            "edge {} has dangling target node {}",
            edge.relation_id,
            edge.target_node_id
        );
        for evidence_id in &edge.evidence_ids {
            assert!(
                evidence_ids.contains(evidence_id.as_str()),
                "edge {} has dangling evidence {}",
                edge.relation_id,
                evidence_id
            );
        }
    }

    for claim in &claims {
        for node_id in &claim.topic_refs {
            assert!(
                node_ids.contains(node_id.as_str()),
                "claim {} has dangling topic {}",
                claim.claim_id,
                node_id
            );
        }
        for source_id in &claim.source_refs {
            assert!(
                source_ids.contains(source_id.as_str()),
                "claim {} has dangling source {}",
                claim.claim_id,
                source_id
            );
        }
        for evidence_id in &claim.evidence_refs {
            assert!(
                evidence_ids.contains(evidence_id.as_str()),
                "claim {} has dangling evidence {}",
                claim.claim_id,
                evidence_id
            );
        }
    }

    for memory in &memories {
        for source_id in &memory.source_refs {
            assert!(
                source_ids.contains(source_id.as_str()),
                "memory {} has dangling source {}",
                memory.memory_id,
                source_id
            );
        }
        for evidence_id in &memory.evidence_refs {
            assert!(
                evidence_ids.contains(evidence_id.as_str()),
                "memory {} has dangling evidence {}",
                memory.memory_id,
                evidence_id
            );
        }
    }

    for page in &snapshot.wiki_pages {
        assert!(
            workspace_root.join(&page.path).exists(),
            "wiki page {} is listed but missing on disk",
            page.path
        );
        for node_id in &page.node_refs {
            assert!(
                node_ids.contains(node_id.as_str()),
                "wiki page {} has dangling node {}",
                page.path,
                node_id
            );
        }
        for source_id in &page.source_refs {
            assert!(
                source_ids.contains(source_id.as_str()),
                "wiki page {} has dangling source {}",
                page.path,
                source_id
            );
        }
        for evidence_id in &page.evidence_refs {
            assert!(
                evidence_ids.contains(evidence_id.as_str()),
                "wiki page {} has dangling evidence {}",
                page.path,
                evidence_id
            );
        }

        let body =
            fs::read_to_string(workspace_root.join(&page.path)).expect("read wiki page body");
        for linked_topic in markdown_topic_links(&body) {
            let linked_path = format!("wiki/topics/{linked_topic}");
            assert!(
                wiki_paths.contains(linked_path.as_str()),
                "wiki page {} links to missing topic page {}",
                page.path,
                linked_path
            );
            assert!(
                workspace_root.join(&linked_path).exists(),
                "wiki page {} links to topic page {} missing on disk",
                page.path,
                linked_path
            );
        }
    }
}

fn markdown_topic_links(body: &str) -> Vec<String> {
    body.match_indices("](topics/")
        .filter_map(|(start, marker)| {
            let path_start = start + marker.len();
            let rest = &body[path_start..];
            rest.find(')').map(|end| rest[..end].to_string())
        })
        .collect()
}

#[test]
fn compile_and_store_project_round_trip() {
    let temp = tempfile::tempdir().expect("temp dir");
    let project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nHyprDuck compile path keeps evidence visible for every concept.\nExplainable graph view grounds answers in visible snippets.\n\n## Page 2\n\nEvidence inspector helps people trust the graph.\n",
        );
    let markdown_path = temp.path().join("sample.md");
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some("/tmp/source.pdf".into()),
        source_manifest_path: None,
        workspace_id: None,
        source_id: None,
    };
    assert_eq!(project.summary.status, ProjectStatus::Ready);
    assert!(project
        .nodes
        .iter()
        .any(|node| node.kind == GraphNodeKind::Concept));
    assert!(!project.edges.is_empty());
    assert!(project
        .edges
        .iter()
        .any(|edge| edge.kind == RelationKind::RelatedTo));

    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path);
    store
        .save_project(&project, &request, None)
        .expect("save project to sqlite");

    let loaded = store
        .load_project(Some(&project.summary.project_id))
        .expect("load project")
        .expect("stored project");
    assert_eq!(loaded.summary.project_id, project.summary.project_id);
    assert_eq!(loaded.summary.title, "Sample import");
    assert_eq!(loaded.nodes.len(), project.nodes.len());
    assert_eq!(loaded.edges.len(), project.edges.len());
    assert!(loaded.details_by_node_id.contains_key("document"));
    assert!(!loaded.edge_details_by_id.is_empty());
}

#[test]
fn project_store_persists_workspace_source_manifest_summary() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Sample import\n\n## Page 1\n\nSource evidence belongs to the workspace.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some("/tmp/source.pdf".into()),
        source_manifest_path: Some(sample_manifest(&temp).manifest_path),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some("source-test".into()),
    };
    let manifest = sample_manifest(&temp);
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));

    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project with source");

    let workspace_id = store
        .load_latest_workspace_id()
        .expect("load latest workspace")
        .expect("workspace id");
    let sources = store.load_sources(&workspace_id).expect("load sources");
    assert_eq!(workspace_id, DEFAULT_WORKSPACE_ID);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].source_id, "source-test");
    assert_eq!(sources[0].page_count, 1);
    assert_eq!(sources[0].status, IngestStatus::Ingested);
    assert_eq!(sources[0].format, DocumentFormat::Pdf);
    assert_eq!(sources[0].success_count, 1);
    assert_eq!(sources[0].failed_count, 0);
}

#[test]
fn markdown_ingest_paths_resolve_configured_source_and_wiki_dirs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let inbox = root.join("incoming-markdown");
    fs::create_dir_all(&inbox).expect("source dir");
    fs::create_dir_all(root.join("custom-wiki")).expect("wiki dir");
    fs::write(
        root.join("brain-config.json"),
        serde_json::json!({
            "markdownSourcesDir": "incoming-markdown",
            "wikiDir": "custom-wiki"
        })
        .to_string(),
    )
    .expect("write brain config");

    let paths = resolve_markdown_ingest_paths(&BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    })
    .expect("resolve ingest paths");

    assert_eq!(paths.workspace_root, root);
    assert_eq!(paths.source_dir, inbox);
    assert_eq!(paths.wiki_dir, paths.workspace_root.join("custom-wiki"));
    assert!(paths.source_dir.exists());
    assert!(paths.wiki_dir.exists());
}

#[test]
fn markdown_ingest_paths_default_to_workspace_sources_and_wiki() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);

    let paths = resolve_markdown_ingest_paths(&BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    })
    .expect("resolve default ingest paths");

    assert_eq!(paths.workspace_root, workspace_root);
    assert_eq!(paths.source_dir, paths.workspace_root.join("sources"));
    assert_eq!(paths.wiki_dir, paths.workspace_root.join("wiki"));
    assert!(paths.source_dir.exists());
    assert!(paths.wiki_dir.exists());
}

#[test]
fn markdown_ingest_paths_reject_missing_configured_source_dir() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(&root).expect("workspace root");
    fs::write(
        root.join("brain-config.json"),
        serde_json::json!({
            "markdownSourcesDir": "missing-inbox",
            "wikiDir": "wiki"
        })
        .to_string(),
    )
    .expect("write brain config");

    let error = resolve_markdown_ingest_paths(&BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    })
    .expect_err("missing configured source dir should fail");

    assert!(format!("{error:#}").contains("configured markdown source directory"));
}

#[test]
fn markdown_ingest_scan_finds_new_markdown_sources_only() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(source_dir.join("nested")).expect("source dirs");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let alpha = source_dir.join("alpha.md");
    let beta = source_dir.join("nested/beta.markdown");
    let old = source_dir.join("old.md");
    let ignored = source_dir.join("notes.txt");
    fs::write(&alpha, "# Alpha\n").expect("alpha");
    fs::write(&beta, "# Beta\n").expect("beta");
    fs::write(&old, "# Old\n").expect("old");
    fs::write(&ignored, "ignore").expect("ignored");

    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![SourceRecord {
            source_id: "source-old".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: old.display().to_string(),
            source_path: old.display().to_string(),
            markdown_path: old.display().to_string(),
            format: "markdown".into(),
            status: "ingested".into(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
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
    let paths = MarkdownIngestPaths {
        workspace_root,
        source_dir,
        wiki_dir,
    };

    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan markdown sources");

    assert_eq!(scan.new_sources.len(), 2);
    assert_eq!(scan.new_sources[0].source_path, alpha);
    assert_eq!(scan.new_sources[0].relative_path, PathBuf::from("alpha.md"));
    assert_eq!(scan.new_sources[1].source_path, beta);
    assert_eq!(
        scan.new_sources[1].relative_path,
        PathBuf::from("nested/beta.markdown")
    );
    assert_eq!(scan.current_state.sources.len(), 3);
}

#[test]
fn markdown_ingest_scan_uses_persisted_state_to_avoid_repeat_reports() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let source = source_dir.join("agent-loop.md");
    fs::write(&source, "# Agent loop\n\nEvents are the source of truth.\n").expect("source");

    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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
    let paths = MarkdownIngestPaths {
        workspace_root,
        source_dir,
        wiki_dir,
    };

    let first_scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    assert_eq!(first_scan.new_sources.len(), 1);
    write_markdown_source_state(&paths, &first_scan.current_state).expect("persist state");

    let persisted = read_markdown_source_state(&paths).expect("read persisted state");
    let second_scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &persisted,
        &MarkdownIngestQueueFile::default(),
    )
    .expect("second scan");

    assert!(second_scan.new_sources.is_empty());
    assert_eq!(second_scan.current_state.sources.len(), 1);
    assert_eq!(
        second_scan.current_state.sources[0].relative_path,
        "agent-loop.md"
    );
}

#[test]
fn markdown_ingest_enqueue_appends_one_event_and_queue_record() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let source = source_dir.join("agent-maintained-graph.md");
    fs::write(
        &source,
        "# Agent-maintained graph\n\nEvents are the source of truth.\n",
    )
    .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let mut snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");

    let first_scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan new source");
    let first_enqueue = enqueue_markdown_sources(
        &writer,
        &paths,
        &MarkdownIngestQueueFile::default(),
        &first_scan,
    )
    .expect("enqueue new source");

    assert_eq!(first_enqueue.enqueued.len(), 1);
    let queue = read_markdown_ingest_queue(&paths).expect("read queue");
    assert_eq!(queue.records.len(), 1);
    assert_eq!(queue.records[0].status, "queued");
    assert_eq!(queue.records[0].trigger_status, "accepted");
    assert_eq!(queue.records[0].trigger_error_message, None);
    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == BrainEventKind::SourceIngestQueued)
            .count(),
        1
    );
    let trigger_event = events
        .iter()
        .find(|event| event.event_type == BrainEventKind::SourceIngestQueued)
        .expect("trigger event");
    let trigger_payload: MarkdownIngestQueueRecord =
        serde_json::from_str(&trigger_event.payload_json).expect("trigger payload");
    assert_eq!(trigger_payload.relative_path, "agent-maintained-graph.md");
    assert_eq!(trigger_payload.status, "queued");
    assert_eq!(trigger_payload.trigger_status, "accepted");
    assert_eq!(trigger_payload.trigger_error_message, None);

    snapshot.events = events;
    let second_scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("rescan source");
    let second_enqueue =
        enqueue_markdown_sources(&writer, &paths, &queue, &second_scan).expect("re-enqueue");

    assert!(second_scan.new_sources.is_empty());
    assert!(second_enqueue.enqueued.is_empty());
    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read final events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == BrainEventKind::SourceIngestQueued)
            .count(),
        1
    );
    assert_eq!(
        read_markdown_ingest_queue(&paths)
            .expect("read final queue")
            .records
            .len(),
        1
    );
}

#[test]
fn markdown_ingest_worker_starts_for_queued_sources_and_materializes_graph() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let source = source_dir.join("agent-maintained-graph.md");
    fs::write(
            &source,
            "# Agent-maintained graph\n\nEvents JSONL is the source of truth.\nThe worker updates graph nodes, claims, memory candidates, and wiki pages.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let nodes_path = workspace_root.join("graph/nodes.json");
    fs::write(
        workspace_root.join("wiki/index.md"),
        "# Stale Brain Index\n",
    )
    .expect("stale wiki index");
    assert!(!nodes_path.exists());
    let queue = read_markdown_ingest_queue(&paths).expect("read queue");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let mut completed_ingests = Vec::<CompletedMarkdownIngest>::new();
    let result = run_markdown_ingest_worker_with_post_ingest_hook(
        &paths,
        &queue,
        &store,
        &mut |completed| {
            completed_ingests.push(completed.clone());
            Ok(())
        },
    )
    .expect("run ingest worker");

    assert!(result.started);
    assert_eq!(result.processed, 1);
    assert_eq!(result.failed, 0);
    assert_eq!(completed_ingests.len(), 1);
    assert_eq!(
        completed_ingests[0].source_metadata.relative_path,
        "agent-maintained-graph.md"
    );
    assert_eq!(
        completed_ingests[0].source_metadata.source_path,
        source.display().to_string()
    );
    assert_eq!(
        completed_ingests[0].source_metadata.content_hash,
        queue.records[0].content_hash
    );
    assert_eq!(
        completed_ingests[0].source_metadata.source_id,
        completed_ingests[0].manifest.source_id
    );
    assert_eq!(
        completed_ingests[0].record.queue_id,
        queue.records[0].queue_id
    );
    assert_eq!(completed_ingests[0].record.status, "ingested");
    assert!(completed_ingests[0].source_metadata.started_at.is_some());
    assert!(completed_ingests[0].source_metadata.completed_at.is_some());
    let queue = read_markdown_ingest_queue(&paths).expect("read processed queue");
    assert_eq!(queue.records.len(), 1);
    assert_eq!(queue.records[0].status, "ingested");
    assert!(queue.records[0].started_at.is_some());
    assert!(queue.records[0].completed_at.is_some());
    assert!(workspace_root.join("brain-manifest.json").exists());
    assert!(nodes_path.exists());
    assert!(workspace_root.join("graph/edges.json").exists());
    assert!(workspace_root.join("graph/claims.json").exists());
    assert!(workspace_root.join("memory/records.json").exists());
    assert!(workspace_root.join("wiki/index.md").exists());

    let manifest: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json")).expect("manifest");
    assert_eq!(manifest.sources.len(), 1);
    assert_eq!(manifest.sources[0].format, "markdown");
    assert!(!manifest.nodes.is_empty());
    assert!(!manifest.claims.is_empty());
    let materialized_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&nodes_path).expect("read materialized nodes");
    assert_eq!(materialized_nodes, manifest.nodes);
    assert!(materialized_nodes.iter().any(|node| {
        node.label == "Agent-maintained graph"
            && node.source_ids == vec![manifest.sources[0].source_id.clone()]
            && !node.evidence_ids.is_empty()
    }));
    let candidates_path = workspace_root
        .join("artifacts")
        .join(&manifest.sources[0].source_id)
        .join("node-candidates.json");
    let candidates: Vec<MarkdownNodeCandidate> =
        read_json_artifact(&candidates_path).expect("node candidates");
    assert!(candidates
        .iter()
        .any(|candidate| candidate.label == "Agent-maintained graph"));
    assert!(manifest
        .nodes
        .iter()
        .any(|node| node.label == "Agent-maintained graph"));
    let wiki_index =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki index");
    assert!(!wiki_index.contains("Stale Brain Index"));
    assert!(wiki_index.contains(&format!(
        "[{}](sources/{}.md)",
        manifest.sources[0].source_id,
        sanitize_name(&manifest.sources[0].source_id)
    )));
    assert!(
        wiki_index.contains("[Agent-maintained graph](topics/concept-agent-maintained-graph.md)")
    );
    let topic_path = "wiki/topics/concept-agent-maintained-graph.md";
    let source_wiki_path = format!(
        "wiki/sources/{}.md",
        sanitize_name(&manifest.sources[0].source_id)
    );
    assert!(manifest
        .wiki_pages
        .iter()
        .any(|page| page.path == topic_path
            && page.node_refs == vec!["concept-agent-maintained-graph".to_string()]
            && page.source_refs == vec![manifest.sources[0].source_id.clone()]));
    assert!(manifest
        .wiki_pages
        .iter()
        .any(|page| page.path == source_wiki_path
            && page.source_refs == vec![manifest.sources[0].source_id.clone()]));
    let topic_body =
        fs::read_to_string(workspace_root.join(topic_path)).expect("read topic wiki page");
    assert!(topic_body.contains("# Agent-maintained graph"));
    assert!(topic_body.contains("- Node: `concept-agent-maintained-graph`"));
    assert!(topic_body.contains(&format!("- Sources: {}", manifest.sources[0].source_id)));
    assert!(topic_body.contains("## Source References"));
    assert!(topic_body.contains(&format!(
        "[{}](../sources/{}.md)",
        manifest.sources[0].source_id,
        sanitize_name(&manifest.sources[0].source_id)
    )));
    assert!(topic_body.contains(&source.display().to_string()));
    assert!(topic_body.contains(&manifest.sources[0].markdown_path));
    let source_page_body =
        fs::read_to_string(workspace_root.join(&source_wiki_path)).expect("read source wiki page");
    assert!(source_page_body.contains(&format!("# {}", manifest.sources[0].source_id)));
    assert!(source_page_body.contains("agent-maintained-graph.md"));
    let persisted_snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
            .expect("read persisted snapshot");
    assert!(persisted_snapshot
        .wiki_pages
        .iter()
        .any(|page| page.path == topic_path));
    let read_topic = handle_read_wiki_page(ReadWikiPageRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        path: topic_path.into(),
    })
    .expect("read topic through wiki read path");
    assert_eq!(read_topic.page.path, topic_path);
    assert!(read_topic.page.body.contains("# Agent-maintained graph"));
    let events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl")).expect("events");
    assert!(events
        .iter()
        .any(|event| event.event_type == BrainEventKind::SourceIngestQueued));
    assert!(events
        .iter()
        .any(|event| event.event_type == BrainEventKind::SourceCompiled));
    assert!(events
        .iter()
        .any(|event| event.event_type == BrainEventKind::GraphMaterialized));
    let read_snapshot = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("read latest graph/wiki snapshot after ingest");
    assert_eq!(
        read_snapshot.latest_readable_snapshot_path,
        "state/latest-readable-snapshot.json"
    );
    assert_eq!(
        read_snapshot.snapshot_id,
        format!("snapshot-default-{}", manifest.generated_at)
    );
    assert_eq!(read_snapshot.nodes, manifest.nodes);
    assert_eq!(read_snapshot.edges, manifest.relations);
    assert_eq!(read_snapshot.claims, manifest.claims);
    assert_eq!(
        read_snapshot.memory_refs,
        manifest
            .memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect::<Vec<_>>()
    );
    assert!(read_snapshot
        .materialized_paths
        .iter()
        .any(|path| path == "graph/nodes.json"));
    assert!(read_snapshot
        .materialized_paths
        .iter()
        .any(|path| path == "wiki/index.md"));
    assert!(workspace_root
        .join("state/latest-readable-snapshot.json")
        .exists());

    let second_result =
        run_markdown_ingest_worker(&paths, &queue, &store).expect("rerun ingest worker");
    assert!(!second_result.started);
    assert_eq!(second_result.processed, 0);
}

#[test]
fn replay_state_applies_node_edge_claim_and_memory_events_in_memory() {
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-replay".into(),
    };
    let source_ref = "source-agent-graph-loop".to_string();
    let source_path = "sources/agent-graph-loop.md".to_string();
    let node_a_id = "concept-agent-graph-loop".to_string();
    let node_b_id = "concept-materialized-wiki".to_string();

    let node_a = BrainUpdateProposal {
        proposal_id: "proposal-replay-node-a".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Node,
        status: BrainProposalStatus::Accepted,
        actor: actor.clone(),
        scope: BrainScope::Project,
        title: "Agent graph loop".into(),
        body: "Agent graph loop is maintained from events.".into(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_refs: vec![source_ref.clone()],
        node_refs: Vec::new(),
        evidence_refs: vec!["ev-node-a".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Agent graph loop".into(),
                kind: BrainNodeKind::Concept,
                source_path: source_path.clone(),
                node_id: Some(node_a_id.clone()),
                aliases: vec!["Autonomous graph loop".into()],
                source_refs: vec![source_ref.clone()],
                evidence_refs: vec!["ev-node-a".into()],
                reason: Some("source introduced the graph loop".into()),
            },
        }),
        created_at: 100,
    };
    let node_b = BrainUpdateProposal {
        proposal_id: "proposal-replay-node-b".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Node,
        status: BrainProposalStatus::Accepted,
        actor: actor.clone(),
        scope: BrainScope::Project,
        title: "Materialized wiki".into(),
        body: "Materialized wiki is rebuilt from graph state.".into(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_refs: vec![source_ref.clone()],
        node_refs: Vec::new(),
        evidence_refs: vec!["ev-node-b".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Materialized wiki".into(),
                kind: BrainNodeKind::Concept,
                source_path: source_path.clone(),
                node_id: Some(node_b_id.clone()),
                aliases: Vec::new(),
                source_refs: vec![source_ref.clone()],
                evidence_refs: vec!["ev-node-b".into()],
                reason: Some("source introduced the wiki read model".into()),
            },
        }),
        created_at: 110,
    };
    let edge = BrainUpdateProposal {
        proposal_id: "proposal-replay-edge".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Link,
        status: BrainProposalStatus::Accepted,
        actor: actor.clone(),
        scope: BrainScope::Project,
        title: "materializes".into(),
        body: "Agent graph loop materializes the wiki.".into(),
        target_node_id: Some(node_b_id.clone()),
        target_source_id: None,
        relation_kind: Some(BrainRelationKind::Supports),
        source_refs: vec![source_ref.clone()],
        node_refs: vec![node_a_id.clone(), node_b_id.clone()],
        evidence_refs: vec!["ev-edge".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewEdge {
            edge: AgentNewEdgePayload {
                source_node_id: node_a_id.clone(),
                target_node_id: node_b_id.clone(),
                kind: BrainRelationKind::Supports,
                label: "materializes".into(),
                source_path: source_path.clone(),
                edge_id: Some("edge-agent-loop-materializes-wiki".into()),
                source_refs: vec![source_ref.clone()],
                evidence_refs: vec!["ev-edge".into()],
                reason: Some("source links the loop to wiki output".into()),
            },
        }),
        created_at: 120,
    };
    let claim = BrainUpdateProposal {
        proposal_id: "proposal-replay-claim".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Claim,
        status: BrainProposalStatus::Accepted,
        actor: actor.clone(),
        scope: BrainScope::Project,
        title: "Events drive materialized graph state".into(),
        body: "Events JSONL drives the materialized graph and wiki.".into(),
        target_node_id: Some(node_a_id.clone()),
        target_source_id: None,
        relation_kind: None,
        source_refs: vec![source_ref.clone()],
        node_refs: vec![node_a_id.clone()],
        evidence_refs: vec!["ev-claim".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Events JSONL drives the materialized graph and wiki.".into(),
                source_path: source_path.clone(),
                claim_id: Some("claim-events-drive-materialized-state".into()),
                topic_refs: vec![node_a_id.clone()],
                source_refs: vec![source_ref.clone()],
                evidence_refs: vec!["ev-claim".into()],
                reason: Some("source states the replay contract".into()),
            },
        }),
        created_at: 130,
    };
    let memory = BrainUpdateProposal {
            proposal_id: "proposal-replay-memory".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            kind: BrainProposalKind::Memory,
            status: BrainProposalStatus::Accepted,
            actor,
            scope: BrainScope::Project,
            title: "Replay engine contract".into(),
            body: "Replay keeps node, edge, claim, and memory state in memory before writing read models.".into(),
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_refs: vec![source_ref.clone()],
            node_refs: vec![node_a_id.clone()],
            evidence_refs: vec!["ev-memory".into()],
            proposal_payload: Some(AgentGraphProposalPayload::NewMemory {
                memory: AgentNewMemoryPayload {
                    title: "Replay engine contract".into(),
                    body: "Replay keeps node, edge, claim, and memory state in memory before writing read models.".into(),
                    source_path,
                    memory_id: Some("memory-replay-engine-contract".into()),
                    source_refs: vec![source_ref.clone()],
                    evidence_refs: vec!["ev-memory".into()],
                    reason: Some("source should become durable memory".into()),
                },
            }),
            created_at: 140,
        };
    let events = [&node_a, &node_b, &edge, &claim, &memory]
        .into_iter()
        .map(|proposal| brain_graph_mutation_applied_event(proposal).expect("mutation event"))
        .collect::<Vec<_>>();

    let mut replay_state = BrainReplayState::new(DEFAULT_WORKSPACE_ID);
    for event in &events {
        replay_state.apply_event(event).expect("apply replay event");
    }
    let snapshot = replay_state.into_snapshot();

    assert_eq!(snapshot.nodes.len(), 2);
    assert!(snapshot.nodes.iter().any(|node| {
        node.node_id == node_a_id
            && node.aliases == vec!["Autonomous graph loop".to_string()]
            && node.source_ids == vec![source_ref.clone()]
            && node.evidence_ids == vec!["ev-node-a".to_string()]
    }));
    assert!(snapshot.relations.iter().any(|relation| {
        relation.relation_id == "edge-agent-loop-materializes-wiki"
            && relation.source_node_id == node_a_id
            && relation.target_node_id == node_b_id
            && relation.kind == BrainRelationKind::Supports
    }));
    assert!(snapshot.claims.iter().any(|claim| {
        claim.claim_id == "claim-events-drive-materialized-state"
            && claim.topic_refs == vec![node_a_id.clone()]
            && claim.status == "supported"
    }));
    assert!(snapshot.memories.iter().any(|memory| {
        memory.memory_id == "memory-replay-engine-contract"
            && memory.source_refs == vec![source_ref.clone()]
            && memory.evidence_refs == vec!["ev-memory".to_string()]
    }));
}

#[test]
fn reconstruct_brain_replays_persisted_events_to_timestamp_and_version() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");

    let source = SourceRecord {
        source_id: "source-replay".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "sources/replay.md".into(),
        source_path: "sources/replay.md".into(),
        markdown_path: "sources/replay.md".into(),
        format: "markdown".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 100,
    };
    let base_node = BrainNodeRecord {
        node_id: "concept-event-ledger".into(),
        kind: BrainNodeKind::Concept,
        label: "Event ledger".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec!["ev-ledger".into()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let base_page = WikiPage {
        page_id: "topic-concept-event-ledger".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/topics/concept-event-ledger.md".into(),
        title: "Event ledger".into(),
        body: "# Event ledger\n\nReplay starts from this materialized graph event.\n".into(),
        node_refs: vec![base_node.node_id.clone()],
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec!["ev-ledger".into()],
        updated_at: 100,
    };
    let extraction = StructuredExtractionArtifact {
        artifact_id: "extraction-source-replay".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: source.source_id.clone(),
        extractor: "agent-replay".into(),
        extractor_model: Some("duckdocs-agent".into()),
        source_refs: vec![source.source_id.clone()],
        page_refs: Vec::new(),
        entities: Vec::new(),
        topics: Vec::new(),
        claims: Vec::new(),
        relations: Vec::new(),
        memories: Vec::new(),
        evidence_refs: Vec::new(),
        confidence: Some(0.8),
        provenance: "Replay fixture extraction must materialize in the existing output layout."
            .into(),
        created_at: 100,
    };
    let base_payload = materialized_graph_event_payload_json(
        100,
        std::slice::from_ref(&source),
        std::slice::from_ref(&base_node),
        &[],
        &[],
        &[],
        std::slice::from_ref(&base_page),
        &[],
        &[],
        std::slice::from_ref(&extraction),
    )
    .expect("base payload");
    let base_event = BrainEvent {
        event_id: "evt-base-materialized".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "duckdocs-engine".into(),
        },
        source_refs: vec![source.source_id.clone()],
        source_markdown_refs: vec![source.markdown_path.clone()],
        node_refs: vec![base_node.node_id.clone()],
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: vec![base_node.node_id.clone()],
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: vec!["ev-ledger".into()],
        payload_json: base_payload,
        causality: BrainEventCausality {
            snapshot_id: Some("snapshot-default-100".into()),
            materialized_version: Some(100),
            ..Default::default()
        },
        confidence: None,
        policy_result: "materialized".into(),
        created_at: 100,
    };
    let proposal = BrainUpdateProposal {
        proposal_id: "proposal-replay-node".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Node,
        status: BrainProposalStatus::Accepted,
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        scope: BrainScope::Project,
        title: "Replay node".into(),
        body: "Replay node should appear only after its mutation event.".into(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_refs: vec![source.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: vec!["ev-replay".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Replay node".into(),
                kind: BrainNodeKind::Concept,
                source_path: source.markdown_path.clone(),
                node_id: Some("concept-replay-node".into()),
                aliases: Vec::new(),
                source_refs: vec![source.source_id.clone()],
                evidence_refs: vec!["ev-replay".into()],
                reason: Some("test mutation".into()),
            },
        }),
        created_at: 200,
    };
    let mutation_event = brain_graph_mutation_applied_event(&proposal).expect("mutation event");
    write_brain_events_jsonl(
        &workspace_root.join("events/brain_events.jsonl"),
        &[base_event.clone(), mutation_event.clone()],
    )
    .expect("write events");

    let historical = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        up_to_timestamp: Some(150),
        up_to_materialized_version: None,
        up_to_event_id: None,
        output_root: Some(temp.path().join("replay-150").display().to_string()),
        write_materialized: false,
    })
    .expect("reconstruct timestamp");
    assert_eq!(historical.replayed_event_count, 1);
    assert!(historical
        .snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-event-ledger"));
    assert!(!historical
        .snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-replay-node"));
    assert!(Path::new(&historical.output_root)
        .join("graph/nodes.json")
        .exists());
    assert!(Path::new(&historical.output_root)
        .join("wiki/index.md")
        .exists());
    assert!(Path::new(&historical.output_root)
        .join("artifacts/source-replay/extraction.json")
        .exists());
    assert!(Path::new(&historical.output_root)
        .join("reviews/proposed-updates/.gitkeep")
        .exists());
    assert!(Path::new(&historical.output_root)
        .join("reviews/lint-reports/.gitkeep")
        .exists());
    let historical_manifest: BrainRepoSnapshot =
        read_json_artifact(&Path::new(&historical.output_root).join("brain-manifest.json"))
            .expect("read replayed manifest");
    assert_eq!(historical_manifest.extractions, vec![extraction]);

    let reconstructed = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        up_to_timestamp: None,
        up_to_materialized_version: Some(200),
        up_to_event_id: None,
        output_root: Some(temp.path().join("replay-200").display().to_string()),
        write_materialized: false,
    })
    .expect("reconstruct version");
    assert_eq!(reconstructed.replayed_event_count, 2);
    assert_eq!(
        reconstructed.selected_event_id,
        Some(mutation_event.event_id.clone())
    );
    assert!(reconstructed
        .snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-replay-node"
            && node.source_ids == vec![source.source_id.clone()]
            && node.evidence_ids == vec!["ev-replay".to_string()]));
    let replayed_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&Path::new(&reconstructed.output_root).join("graph/nodes.json"))
            .expect("read replayed nodes");
    assert!(replayed_nodes
        .iter()
        .any(|node| node.node_id == "concept-replay-node"));

    let invalid_target = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: Some("evt-missing-rollback-target".into()),
        output_root: Some(
            temp.path()
                .join("replay-invalid-target")
                .display()
                .to_string(),
        ),
        write_materialized: true,
    })
    .expect_err("invalid replay target should not roll back to the latest event");
    assert!(format!("{invalid_target:#}").contains("evt-missing-rollback-target"));
    assert!(!temp.path().join("replay-invalid-target").exists());
    let unchanged_events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
            .expect("events remain unchanged after invalid target");
    assert_eq!(unchanged_events, vec![base_event, mutation_event]);
}

#[test]
fn reconstruct_brain_uses_materialized_version_order_for_incremental_replay() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");

    let older_timestamp_node = BrainNodeRecord {
        node_id: "concept-appended-later".into(),
        kind: BrainNodeKind::Concept,
        label: "Appended later".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: Vec::new(),
        confidence: None,
        updated_at: 100,
    };
    let newer_timestamp_node = BrainNodeRecord {
        node_id: "concept-appended-first".into(),
        kind: BrainNodeKind::Concept,
        label: "Appended first".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: Vec::new(),
        confidence: None,
        updated_at: 200,
    };
    let first_event = BrainEvent {
        event_id: "evt-appended-first".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "duckdocs-engine".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: vec![newer_timestamp_node.node_id.clone()],
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: vec![newer_timestamp_node.node_id.clone()],
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: materialized_graph_event_payload_json(
            200,
            &[],
            std::slice::from_ref(&newer_timestamp_node),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .expect("first payload"),
        causality: BrainEventCausality {
            snapshot_id: Some("snapshot-default-200".into()),
            materialized_version: Some(200),
            ..Default::default()
        },
        confidence: None,
        policy_result: "materialized".into(),
        created_at: 200,
    };
    let appended_later_event = BrainEvent {
        event_id: "evt-appended-later".into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::System,
            actor_id: "duckdocs-engine".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: vec![older_timestamp_node.node_id.clone()],
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: vec![older_timestamp_node.node_id.clone()],
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: Vec::new(),
        payload_json: materialized_graph_event_payload_json(
            100,
            &[],
            std::slice::from_ref(&older_timestamp_node),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .expect("later payload"),
        causality: BrainEventCausality {
            snapshot_id: Some("snapshot-default-100".into()),
            materialized_version: Some(100),
            previous_snapshot_id: Some("snapshot-default-200".into()),
            ..Default::default()
        },
        confidence: None,
        policy_result: "materialized".into(),
        created_at: 100,
    };
    write_brain_events_jsonl(
        &workspace_root.join("events/brain_events.jsonl"),
        &[first_event.clone(), appended_later_event],
    )
    .expect("write out-of-time-order events");

    let replay = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: None,
        output_root: Some(
            temp.path()
                .join("version-order-replay")
                .display()
                .to_string(),
        ),
        write_materialized: false,
    })
    .expect("replay version-ordered events");

    assert_eq!(replay.replayed_event_count, 2);
    assert_eq!(replay.selected_event_id, Some(first_event.event_id.clone()));
    assert_eq!(replay.snapshot.nodes, vec![newer_timestamp_node]);
    assert_eq!(
        replay.selected_event_id.as_deref(),
        Some("evt-appended-first")
    );
}

#[test]
fn markdown_reingest_identical_source_preserves_unchanged_node_revisions() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
        source_dir.join("agent-maintained-graph.md"),
        "# Agent-maintained graph\n\nEvents JSONL is the source of truth.\n",
    )
    .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    let nodes_path = workspace_root.join("graph/nodes.json");
    let mut nodes_before: Vec<BrainNodeRecord> =
        read_json_artifact(&nodes_path).expect("read nodes before");
    for node in &mut nodes_before {
        node.updated_at = 123;
    }
    write_json_pretty(&nodes_path, &nodes_before).expect("seed stable node revisions");

    let mut requeue = read_markdown_ingest_queue(&paths).expect("processed queue");
    requeue.records[0].status = "queued".into();
    requeue.records[0].started_at = None;
    requeue.records[0].completed_at = None;
    requeue.records[0].error_message = None;
    write_markdown_ingest_queue(&paths, &requeue).expect("requeue identical source");

    let reingest_result = run_markdown_ingest_worker(&paths, &requeue, &store).expect("reingest");
    assert!(reingest_result.started);
    assert_eq!(reingest_result.processed, 1);

    let nodes_after: Vec<BrainNodeRecord> =
        read_json_artifact(&nodes_path).expect("read nodes after");
    assert_eq!(nodes_after, nodes_before);
    assert_eq!(
        nodes_after
            .iter()
            .filter(|node| node.node_id == "concept-agent-maintained-graph")
            .count(),
        1
    );
}

#[test]
fn markdown_reingest_identical_source_preserves_edges_without_repeated_relation_changes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
            source_dir.join("agent-maintained-graph.md"),
            "# Agent-maintained graph\n\n## Event ledger\n\nAgent-maintained graph depends on Event ledger for replayable changes.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    let edges_path = workspace_root.join("graph/edges.json");
    let mut edges_before: Vec<BrainRelationRecord> =
        read_json_artifact(&edges_path).expect("read edges before");
    assert!(edges_before.iter().any(|edge| {
        edge.source_node_id.contains("agent-maintained-graph")
            && edge.target_node_id.contains("event-ledger")
            && edge.kind == BrainRelationKind::DependsOn
            && edge.label == "Depends on"
    }));
    for edge in &mut edges_before {
        edge.updated_at = 456;
    }
    write_json_pretty(&edges_path, &edges_before).expect("seed stable edge revisions");

    let mut requeue = read_markdown_ingest_queue(&paths).expect("processed queue");
    requeue.records[0].status = "queued".into();
    requeue.records[0].started_at = None;
    requeue.records[0].completed_at = None;
    requeue.records[0].error_message = None;
    write_markdown_ingest_queue(&paths, &requeue).expect("requeue identical source");

    let reingest_result = run_markdown_ingest_worker(&paths, &requeue, &store).expect("reingest");
    assert!(reingest_result.started);
    assert_eq!(reingest_result.processed, 1);

    let edges_after: Vec<BrainRelationRecord> =
        read_json_artifact(&edges_path).expect("read edges after");
    assert_eq!(edges_after, edges_before);
    let edge_ids = edges_after
        .iter()
        .map(|edge| edge.relation_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(edge_ids.len(), edges_after.len());

    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events");
    let materialized_relation_events = events
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event
                    .relation_refs
                    .iter()
                    .any(|relation_id| relation_id.contains("agent-maintained-graph"))
        })
        .count();
    assert_eq!(materialized_relation_events, 1);
}

#[test]
fn markdown_reingest_identical_source_reuses_existing_wiki_entry() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
            source_dir.join("agent-maintained-graph.md"),
            "# Agent-maintained graph\n\nEvents JSONL is the source of truth.\nThe wiki entry stays stable across identical re-ingest runs.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    let snapshot_before: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read manifest before");
    let source = snapshot_before
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("agent-maintained-graph.md"))
        .expect("source record before")
        .clone();
    let source_page_path = format!("wiki/sources/{}.md", sanitize_name(&source.source_id));
    assert_eq!(
        snapshot_before
            .wiki_pages
            .iter()
            .filter(|page| page.path == source_page_path)
            .count(),
        1
    );
    let source_page_body_before =
        fs::read_to_string(workspace_root.join(&source_page_path)).expect("source wiki before");
    let index_before =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("index before");
    let source_link = format!(
        "[{}](sources/{}.md)",
        source.source_id,
        sanitize_name(&source.source_id)
    );
    assert_eq!(index_before.matches(&source_link).count(), 1);

    let mut requeue = read_markdown_ingest_queue(&paths).expect("processed queue");
    requeue.records[0].status = "queued".into();
    requeue.records[0].started_at = None;
    requeue.records[0].completed_at = None;
    requeue.records[0].error_message = None;
    write_markdown_ingest_queue(&paths, &requeue).expect("requeue identical source");

    let reingest_result = run_markdown_ingest_worker(&paths, &requeue, &store).expect("reingest");
    assert!(reingest_result.started);
    assert_eq!(reingest_result.processed, 1);

    let snapshot_after: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read manifest after");
    assert_eq!(
        snapshot_after
            .sources
            .iter()
            .filter(|record| record.source_id == source.source_id)
            .count(),
        1
    );
    assert_eq!(
        snapshot_after
            .wiki_pages
            .iter()
            .filter(|page| page.path == source_page_path)
            .count(),
        1
    );
    assert_eq!(
        snapshot_after
            .wiki_pages
            .iter()
            .filter(|page| {
                page.path.starts_with("wiki/sources/")
                    && page.source_refs == vec![source.source_id.clone()]
            })
            .count(),
        1
    );
    let index_after =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("index after");
    assert_eq!(index_after.matches(&source_link).count(), 1);
    let source_page_body_after =
        fs::read_to_string(workspace_root.join(&source_page_path)).expect("source wiki after");
    assert!(source_page_body_after.starts_with(source_page_body_before.trim_end()));
    let source_page_files = fs::read_dir(workspace_root.join("wiki/sources"))
        .expect("source wiki dir")
        .collect::<Result<Vec<_>, _>>()
        .expect("source wiki entries");
    assert_eq!(source_page_files.len(), 1);
}

#[test]
fn markdown_reingest_identical_source_keeps_claim_memory_and_wiki_counts_stable() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
            source_dir.join("agent-maintained-graph.md"),
            "# Agent-maintained graph\n\nEvents JSONL remains the source of truth for graph replay.\nThe wiki entry stays stable across identical re-ingest runs.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    let snapshot_before: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read manifest before");
    let source = snapshot_before
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("agent-maintained-graph.md"))
        .expect("source record before")
        .clone();
    let source_page_path = format!("wiki/sources/{}.md", sanitize_name(&source.source_id));
    let source_link = format!(
        "[{}](sources/{}.md)",
        source.source_id,
        sanitize_name(&source.source_id)
    );
    let claim_count_before = snapshot_before.claims.len();
    let wiki_page_count_before = snapshot_before.wiki_pages.len();
    let memory_count_before = read_memory_records(&workspace_root)
        .expect("read memories before")
        .len();
    let wiki_source_file_count_before = fs::read_dir(workspace_root.join("wiki/sources"))
        .expect("source wiki dir before")
        .count();
    let index_source_link_count_before = fs::read_to_string(workspace_root.join("wiki/index.md"))
        .expect("index before")
        .matches(&source_link)
        .count();
    assert!(claim_count_before > 0);
    assert!(memory_count_before > 0);
    assert!(wiki_page_count_before > 0);
    assert_eq!(index_source_link_count_before, 1);
    assert!(workspace_root.join(&source_page_path).exists());
    let events_before = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events before reingest");
    let graph_mutation_count_before = events_before
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event.policy_result == "auto_applied"
        })
        .count();
    let stable_materialized_paths = [
        "brain-manifest.json".to_string(),
        "graph/nodes.json".to_string(),
        "graph/edges.json".to_string(),
        "graph/claims.json".to_string(),
        "graph/evidence.json".to_string(),
        "graph/entities.json".to_string(),
        "memory/records.json".to_string(),
        "wiki/index.md".to_string(),
        source_page_path.clone(),
    ];
    let stable_materialized_before = stable_materialized_paths
        .iter()
        .map(|path| {
            (
                path.clone(),
                fs::read(workspace_root.join(path)).expect("read stable materialized file"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut requeue = read_markdown_ingest_queue(&paths).expect("processed queue");
    requeue.records[0].status = "queued".into();
    requeue.records[0].started_at = None;
    requeue.records[0].completed_at = None;
    requeue.records[0].error_message = None;
    write_markdown_ingest_queue(&paths, &requeue).expect("requeue identical source");

    let reingest_result = run_markdown_ingest_worker(&paths, &requeue, &store).expect("reingest");
    assert!(reingest_result.started);
    assert_eq!(reingest_result.processed, 1);

    let snapshot_after: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read manifest after");
    let memory_count_after = read_memory_records(&workspace_root)
        .expect("read memories after")
        .len();
    let wiki_source_file_count_after = fs::read_dir(workspace_root.join("wiki/sources"))
        .expect("source wiki dir after")
        .count();
    let index_source_link_count_after = fs::read_to_string(workspace_root.join("wiki/index.md"))
        .expect("index after")
        .matches(&source_link)
        .count();

    assert_eq!(snapshot_after.claims.len(), claim_count_before);
    assert_eq!(memory_count_after, memory_count_before);
    assert_eq!(snapshot_after.wiki_pages.len(), wiki_page_count_before);
    assert_eq!(wiki_source_file_count_after, wiki_source_file_count_before);
    assert_eq!(
        index_source_link_count_after,
        index_source_link_count_before
    );
    assert_eq!(
        snapshot_after
            .wiki_pages
            .iter()
            .filter(|page| page.path == source_page_path)
            .count(),
        1
    );
    let stable_materialized_after = stable_materialized_paths
        .iter()
        .map(|path| {
            (
                path.clone(),
                fs::read(workspace_root.join(path)).expect("read stable materialized file"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(stable_materialized_after, stable_materialized_before);

    let events_after = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events after reingest");
    let graph_mutation_count_after = events_after
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event.policy_result == "auto_applied"
        })
        .count();
    assert_eq!(graph_mutation_count_after, graph_mutation_count_before);
    let noop_events = events_after
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event.operation_type.as_deref() == Some("graph_materialize_noop")
                && event.policy_result == "idempotent_noop"
                && event.source_refs == vec![source.source_id.clone()]
                && event.payload_json.contains("\"changedFiles\":[]")
        })
        .collect::<Vec<_>>();
    assert_eq!(noop_events.len(), 1);
}

#[test]
fn markdown_ingest_worker_records_source_errors_in_queue_and_event_payload() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let source = source_dir.join("missing-after-trigger.md");
    fs::write(&source, "# Missing after trigger\n").expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);
    fs::remove_file(&source).expect("remove source");

    let queue = read_markdown_ingest_queue(&paths).expect("read queue");
    assert_eq!(queue.records[0].trigger_status, "accepted");
    assert_eq!(queue.records[0].trigger_error_message, None);
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let result = run_markdown_ingest_worker(&paths, &queue, &store).expect("run ingest worker");

    assert!(result.started);
    assert_eq!(result.processed, 0);
    assert_eq!(result.failed, 1);
    let queue = read_markdown_ingest_queue(&paths).expect("read failed queue");
    assert_eq!(queue.records[0].status, "failed");
    assert_eq!(queue.records[0].trigger_status, "accepted");
    let error = queue.records[0]
        .error_message
        .as_deref()
        .expect("queue error");
    assert!(error.contains("failed reading queued markdown source"));

    let events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl")).expect("events");
    let failed_event = events
        .iter()
        .find(|event| {
            event.event_type == BrainEventKind::SourceCompiled && event.policy_result == "failed"
        })
        .expect("failed event");
    let payload: MarkdownIngestQueueRecord =
        serde_json::from_str(&failed_event.payload_json).expect("failed payload");
    assert_eq!(payload.status, "failed");
    assert_eq!(payload.trigger_status, "accepted");
    assert!(payload
        .error_message
        .as_deref()
        .unwrap_or_default()
        .contains("failed reading queued markdown source"));
}

#[test]
fn markdown_ingest_keeps_derived_graph_agent_owned() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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

    fs::write(
        source_dir.join("first.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth.\n",
    )
    .expect("first source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    fs::write(
            source_dir.join("second.md"),
            "# Agent maintained graph\n\nThe autonomous agent keeps the graph fresh.\n\n## Autonomous memory loop\n\nThe agent records durable memories from source evidence.\n",
        )
        .expect("second source");
    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    let enqueue = enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("enqueue");
    drop(writer);
    assert_eq!(enqueue.enqueued.len(), 1);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == BrainNodeKind::Concept)
            .count(),
        0
    );
    let second_source = snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("second.md"))
        .expect("second source record");
    let candidates_path = workspace_root
        .join("artifacts")
        .join(&second_source.source_id)
        .join("node-candidates.json");
    let candidates: Vec<MarkdownNodeCandidate> =
        read_json_artifact(&candidates_path).expect("second node candidates");
    assert!(
        candidates.is_empty(),
        "markdown ingest should not create heuristic node candidates before agent proposals"
    );
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.kind == BrainNodeKind::Source
            && node.source_ids == vec![second_source.source_id.clone()]));
}

#[test]
fn markdown_ingest_extracts_reviewable_matching_signals() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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

    fs::write(
        source_dir.join("first.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth.\n",
    )
    .expect("first source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    fs::write(
            source_dir.join("signals.md"),
            "---\ntitle: Agent maintained graph\n---\n\n# Agent-maintained graph\n\n## Event ledger\n\nAgent-maintained graph depends on [[Event ledger]] and [Replay log](./replay.md) for rollback.\nAutonomous graph memory keeps durable source evidence for agent workflows.\n",
        )
        .expect("signals source");
    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("second enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("updated snapshot");
    let source = snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("signals.md"))
        .expect("signals source record");
    let signals_path = workspace_root
        .join("artifacts")
        .join(&source.source_id)
        .join("markdown-signals.json");
    let signals: MarkdownSignalArtifact =
        read_json_artifact(&signals_path).expect("markdown signals");

    assert_eq!(signals.title.as_deref(), Some("Agent maintained graph"));
    assert!(signals
        .headings
        .iter()
        .any(|heading| heading.text == "Agent-maintained graph" && heading.level == 1));
    assert!(signals
        .headings
        .iter()
        .any(|heading| heading.text == "Event ledger" && heading.level == 2));
    assert!(signals.links.iter().any(|link| {
        link.label == "Event ledger" && link.target == "Event ledger" && link.kind == "wiki"
    }));
    assert!(signals.links.iter().any(|link| {
        link.label == "Replay log" && link.target == "./replay.md" && link.kind == "markdown"
    }));
    assert!(signals.entities.iter().any(|entity| {
        entity.label == "Agent maintained graph"
            && entity.matched_node_label.as_deref() == Some("Agent-maintained graph")
    }));
    assert!(signals
        .entities
        .iter()
        .any(|entity| entity.label == "Event ledger"));
    assert!(signals
        .keywords
        .iter()
        .any(|keyword| keyword.term == "agent"));
    assert!(signals
        .keywords
        .iter()
        .any(|keyword| keyword.term == "graph"));
    assert_eq!(signals.source_refs, vec![source.source_id.clone()]);
}

#[test]
fn markdown_ingest_ranks_related_wiki_pages_from_existing_page_metadata_and_content() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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

    fs::write(
            source_dir.join("existing.md"),
            "# Agent Graph Loop\n\nThe agent graph loop keeps event logs replayable and rollback-ready.\n",
        )
        .expect("existing source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    fs::write(
            source_dir.join("new.md"),
            "# Graph Replay\n\nGraph Replay depends on Agent Graph Loop event logs for rollback and replay.\n",
        )
        .expect("new source");
    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("second enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("updated snapshot");
    let new_source = snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("new.md"))
        .expect("new source record");
    let signals: MarkdownSignalArtifact = read_json_artifact(
        &workspace_root
            .join("artifacts")
            .join(&new_source.source_id)
            .join("markdown-signals.json"),
    )
    .expect("markdown signals");

    let top_related = signals.related_pages.first().expect("ranked related page");
    assert!(top_related
        .path
        .starts_with("wiki/topics/concept-agent-graph-loop"));
    assert!(top_related.score > 0);
    assert!(top_related.matched_terms.iter().any(|term| term == "graph"));
    assert!(top_related.matched_terms.iter().any(|term| term == "agent"));
    assert!(top_related.reason.contains("metadata"));
    assert!(top_related.reason.contains("content"));

    let source_wiki = fs::read_to_string(
        workspace_root
            .join("wiki/sources")
            .join(format!("{}.md", sanitize_name(&new_source.source_id))),
    )
    .expect("source wiki page");
    assert!(source_wiki.contains("## Related Wiki Pages"));
    assert!(source_wiki.contains("topics/concept-agent-graph-loop.md"));
    assert!(source_wiki.contains("score:"));
    assert!(source_wiki.contains("reason:"));
}

#[test]
fn markdown_ingest_maps_changed_entities_to_existing_wiki_sections() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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

    fs::write(
        source_dir.join("existing.md"),
        "# Agent Graph Loop\n\nAgent Graph Loop keeps event logs replayable and rollback-ready.\n",
    )
    .expect("existing source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    fs::write(
            source_dir.join("new.md"),
            "# Graph Replay\n\nGraph Replay depends on Agent Graph Loop for replayable event logs.\nAgent Graph Loop keeps graph mutations auditable.\n",
        )
        .expect("new source");
    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("second enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("updated snapshot");
    let new_source = snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("new.md"))
        .expect("new source record");
    let targets: Vec<serde_json::Value> = read_json_artifact(
        &workspace_root
            .join("artifacts")
            .join(&new_source.source_id)
            .join("wiki-update-targets.json"),
    )
    .expect("wiki update target map");

    assert!(targets.iter().any(|target| {
        target["entityType"] == "node"
            && target["entityId"] == "concept-agent-graph-loop"
            && target["path"] == "wiki/topics/concept-agent-graph-loop.md"
            && target["targetSection"] == "## Evidence"
    }));
    assert!(targets.iter().any(|target| {
        target["entityType"] == "edge"
            && target["path"] == "wiki/topics/concept-agent-graph-loop.md"
            && target["targetSection"] == "## Relations"
    }));
    assert!(targets.iter().any(|target| {
        target["entityType"] == "claim"
            && target["path"] == "wiki/topics/concept-agent-graph-loop.md"
            && target["targetSection"] == "## Claims"
    }));
}

#[test]
fn reingesting_identical_markdown_reuses_existing_graph_nodes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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
    let markdown = "# Agent-maintained graph\n\nEvents JSONL remains the source of truth.\n";

    fs::write(source_dir.join("first.md"), markdown).expect("first source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");
    let first_snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("first snapshot");
    let first_node = first_snapshot
        .nodes
        .iter()
        .find(|node| {
            node.kind == BrainNodeKind::Concept
                && normalize_key(&node.label) == "agent-maintained-graph"
        })
        .expect("first graph node")
        .clone();

    fs::write(source_dir.join("second.md"), markdown).expect("second source");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &first_snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    let enqueue = enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("enqueue");
    drop(writer);
    assert_eq!(enqueue.enqueued.len(), 1);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let second_snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("second snapshot");
    let matching_nodes = second_snapshot
        .nodes
        .iter()
        .filter(|node| {
            node.kind == BrainNodeKind::Concept
                && node
                    .aliases
                    .iter()
                    .chain(std::iter::once(&node.label))
                    .any(|label| normalize_key(label) == "agent-maintained-graph")
        })
        .collect::<Vec<_>>();
    assert_eq!(matching_nodes.len(), 1);
    assert_eq!(matching_nodes[0].node_id, first_node.node_id);
    assert_eq!(matching_nodes[0].source_ids.len(), 2);

    let second_source = second_snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("second.md"))
        .expect("second source record");
    let candidates_path = workspace_root
        .join("artifacts")
        .join(&second_source.source_id)
        .join("node-candidates.json");
    let candidates: Vec<MarkdownNodeCandidate> =
        read_json_artifact(&candidates_path).expect("second node candidates");
    let duplicate_candidate = candidates
        .iter()
        .find(|candidate| candidate.label == "Agent-maintained graph")
        .expect("duplicate candidate");
    assert_eq!(
        duplicate_candidate.matched_node_id.as_deref(),
        Some(first_node.node_id.as_str())
    );
    assert_eq!(
        duplicate_candidate.matched_node_label.as_deref(),
        Some(first_node.label.as_str())
    );
}

#[test]
fn markdown_ingest_extracts_relationship_evidence_for_node_links() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
            source_dir.join("links.md"),
            "# Agent-maintained graph\n\n## Event ledger\n\nAgent-maintained graph depends on Event ledger for replayable changes.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    run_markdown_ingest_worker(&paths, &queue, &store).expect("ingest");

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let source = snapshot.sources.first().expect("source");
    let materialized_edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json"))
            .expect("read materialized edges");
    let relationship_path = workspace_root
        .join("artifacts")
        .join(&source.source_id)
        .join("edge-candidates.json");
    let relationship_evidence: Vec<MarkdownRelationshipEvidence> =
        read_json_artifact(&relationship_path).expect("edge candidates");
    let candidate = relationship_evidence
        .iter()
        .find(|evidence| {
            evidence.source_label == "Agent-maintained graph"
                && evidence.target_label == "Event ledger"
                && evidence.relation_kind == BrainRelationKind::DependsOn
                && evidence.snippet.contains("depends on Event ledger")
        })
        .expect("depends-on edge candidate");
    assert!(candidate.candidate_id.starts_with("edge-candidate-"));
    assert_eq!(candidate.relation_label, "Depends on");
    assert_eq!(
        candidate.source_id.as_deref(),
        Some(source.source_id.as_str())
    );
    assert_eq!(candidate.source_refs, vec![source.source_id.clone()]);
    assert!(snapshot.relations.iter().any(|relation| {
        relation.source_node_id.contains("agent-maintained-graph")
            && relation.target_node_id.contains("event-ledger")
            && relation.kind == BrainRelationKind::DependsOn
            && relation.label == "Depends on"
            && relation.confidence.unwrap_or_default() >= 0.82
    }));
    assert!(materialized_edges.iter().any(|edge| {
        edge.source_node_id.contains("agent-maintained-graph")
            && edge.target_node_id.contains("event-ledger")
            && edge.kind == BrainRelationKind::DependsOn
            && edge.label == "Depends on"
            && !edge.evidence_ids.is_empty()
    }));
    assert!(snapshot
        .evidence
        .iter()
        .any(|evidence| evidence.snippet.contains("depends on Event ledger")));
}

#[test]
fn markdown_ingest_extracts_claim_candidates_from_source_lines() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
            source_dir.join("claims.md"),
            "# Agent-maintained graph\n\nEvents JSONL remains the source of truth for graph replay.\nAgent-maintained graph depends on Events JSONL for rollback.\n",
        )
        .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    run_markdown_ingest_worker(&paths, &queue, &store).expect("ingest");

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let source = snapshot.sources.first().expect("source");
    let claim_candidates_path = workspace_root
        .join("artifacts")
        .join(&source.source_id)
        .join("claim-candidates.json");
    let candidates: Vec<MarkdownClaimCandidate> =
        read_json_artifact(&claim_candidates_path).expect("claim candidates");
    let source_truth_claim = candidates
        .iter()
        .find(|candidate| {
            candidate
                .statement
                .contains("Events JSONL remains the source of truth")
        })
        .expect("source truth claim");
    assert!(source_truth_claim
        .candidate_id
        .starts_with("claim-candidate-"));
    assert_eq!(
        source_truth_claim.classification,
        MarkdownClaimClassification::Decision
    );
    assert!(source_truth_claim.durable);
    assert!(source_truth_claim.memory_candidate);
    assert_eq!(
        source_truth_claim.source_id.as_deref(),
        Some(source.source_id.as_str())
    );
    assert_eq!(
        source_truth_claim.source_refs,
        vec![source.source_id.clone()]
    );
    assert_eq!(source_truth_claim.line_start, 3);
    assert_eq!(source_truth_claim.line_end, 3);
    assert_eq!(source_truth_claim.char_start, 0);
    assert!(source_truth_claim.char_end > source_truth_claim.char_start);
    assert_eq!(source_truth_claim.evidence_span.line_start, 3);
    assert_eq!(source_truth_claim.evidence_span.line_end, 3);
    assert_eq!(
        source_truth_claim.evidence_span.source_path,
        source_truth_claim.source_path
    );
    assert_eq!(
        source_truth_claim.evidence_span.source_id.as_deref(),
        Some(source.source_id.as_str())
    );
    assert!(source_truth_claim
        .evidence_span
        .snippet
        .contains("source of truth"));
    assert!(source_truth_claim.confidence >= 0.64);
    let rollback_claim = candidates
        .iter()
        .find(|candidate| {
            candidate
                .statement
                .contains("Agent-maintained graph depends on Events JSONL")
        })
        .expect("durable fact claim");
    assert_eq!(
        rollback_claim.classification,
        MarkdownClaimClassification::DurableFact
    );
    assert!(rollback_claim.durable);
    assert!(!rollback_claim.memory_candidate);
    assert!(snapshot.claims.iter().any(|claim| {
        claim
            .statement
            .contains("Events JSONL remains the source of truth")
            && claim.status == "candidate"
            && claim.source_refs == vec![source.source_id.clone()]
            && claim
                .evidence_refs
                .iter()
                .any(|evidence_id| evidence_id.starts_with("ev-claim-"))
    }));
    let extraction = snapshot.extractions.first().expect("extraction");
    assert!(extraction.claims.iter().any(|claim| {
        claim
            .statement
            .contains("Agent-maintained graph depends on Events JSONL")
            && claim.status == "candidate"
            && !claim.evidence_refs.is_empty()
            && !claim.page_refs.is_empty()
    }));
    assert!(extraction.evidence_refs.iter().any(|evidence| {
        evidence.id == source_truth_claim.evidence_id
            && evidence.snippet.contains("source of truth")
    }));
    let memory_candidate = extraction
        .memories
        .iter()
        .find(|memory| {
            memory
                .body
                .contains("Events JSONL remains the source of truth")
        })
        .expect("structured memory candidate");
    assert_eq!(memory_candidate.kind, "decision");
    assert_eq!(memory_candidate.status, "auto_apply_candidate");
    assert!(memory_candidate.title.starts_with("Decision:"));
    assert_eq!(memory_candidate.source_refs, vec![source.source_id.clone()]);
    assert_eq!(
        memory_candidate.evidence_refs,
        vec![source_truth_claim.evidence_id.clone()]
    );
    assert!(!memory_candidate.page_refs.is_empty());
    assert!(memory_candidate
        .provenance
        .contains("Autonomous markdown ingest promoted"));
    let memory_records: Vec<MemoryRecord> =
        read_json_artifact(&workspace_root.join("memory/records.json")).expect("memories");
    assert!(memory_records.iter().any(|memory| {
        memory.memory_id == memory_candidate.memory_id
            && memory.title.starts_with("Decision:")
            && memory
                .body
                .contains("Events JSONL remains the source of truth")
            && memory.source_refs == vec![source.source_id.clone()]
            && memory.evidence_refs == vec![source_truth_claim.evidence_id.clone()]
    }));
    let memory_event = snapshot
        .events
        .iter()
        .find(|event| {
            event.event_type == BrainEventKind::MemoryAccepted
                && event.policy_result == "auto_applied"
                && event
                    .payload_json
                    .contains("Events JSONL remains the source of truth")
        })
        .expect("auto memory event");
    assert_eq!(memory_event.actor.actor_id, "duckdocs-agent-ingest");
}

#[test]
fn markdown_ingest_merges_matching_claim_candidates_without_duplicates() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
        source_dir.join("first.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth for graph replay.\n",
    )
    .expect("first source");
    fs::write(
        source_dir.join("second.md"),
        "# Graph replay\n\nEvents JSONL remains the source of truth for graph replay.\n",
    )
    .expect("second source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    run_markdown_ingest_worker(&paths, &queue, &store).expect("ingest");

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let matching_claims = snapshot
        .claims
        .iter()
        .filter(|claim| {
            claim.statement == "Events JSONL remains the source of truth for graph replay."
                && claim.status == "candidate"
        })
        .collect::<Vec<_>>();
    assert_eq!(matching_claims.len(), 1);
    let claim = matching_claims[0];
    assert_eq!(claim.source_refs.len(), 2);
    assert_eq!(claim.evidence_refs.len(), 2);
    assert_eq!(claim.source_refs.iter().collect::<BTreeSet<_>>().len(), 2);
    assert_eq!(claim.evidence_refs.iter().collect::<BTreeSet<_>>().len(), 2);
}

#[test]
fn markdown_ingest_updates_matching_existing_memory_candidate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let existing_memory = MemoryRecord {
        memory_id: "memory-existing-source-truth".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Decision: Events JSONL remains source truth".into(),
        body: "Events JSONL remains the source of truth.".into(),
        source_refs: vec!["source-legacy".into()],
        evidence_refs: vec!["ev-legacy".into()],
        created_at: 10,
        updated_at: 10,
    };
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &vec![existing_memory],
    )
    .expect("seed existing memory");
    fs::write(
        source_dir.join("source.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth for graph replay.\n",
    )
    .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    run_markdown_ingest_worker(&paths, &queue, &store).expect("ingest");

    let memories = read_memory_records(&workspace_root).expect("memories");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].memory_id, "memory-existing-source-truth");
    assert!(memories[0]
        .body
        .contains("Events JSONL remains the source of truth for graph replay"));
    assert!(memories[0].source_refs.contains(&"source-legacy".into()));
    assert!(memories[0].evidence_refs.contains(&"ev-legacy".into()));
    assert!(memories[0]
        .source_refs
        .iter()
        .any(|source| source != "source-legacy"));
    assert!(memories[0]
        .evidence_refs
        .iter()
        .any(|evidence| evidence != "ev-legacy"));

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    assert!(snapshot.events.iter().any(|event| {
        event.event_type == BrainEventKind::MemoryAccepted
            && event.policy_result == "auto_applied"
            && event.payload_json.contains("memory-existing-source-truth")
            && event.source_refs.contains(&"source-legacy".into())
            && event.evidence_refs.contains(&"ev-legacy".into())
            && event
                .source_refs
                .iter()
                .any(|source| source != "source-legacy")
            && event
                .evidence_refs
                .iter()
                .any(|evidence| evidence != "ev-legacy")
    }));
}

#[test]
fn markdown_reingest_identical_source_reuses_existing_memory_record() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    fs::write(
        source_dir.join("source.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth for graph replay.\n",
    )
    .expect("source");

    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir,
        wiki_dir,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("enqueue");
    drop(writer);

    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let queue = read_markdown_ingest_queue(&paths).expect("queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");

    let memory_path = workspace_root.join("memory/records.json");
    let mut memories_before: Vec<MemoryRecord> =
        read_json_artifact(&memory_path).expect("read memories before");
    assert_eq!(memories_before.len(), 1);
    memories_before[0].created_at = 11;
    memories_before[0].updated_at = 22;
    write_json_pretty(&memory_path, &memories_before).expect("seed stable memory revision");

    let mut requeue = read_markdown_ingest_queue(&paths).expect("processed queue");
    requeue.records[0].status = "queued".into();
    requeue.records[0].started_at = None;
    requeue.records[0].completed_at = None;
    requeue.records[0].error_message = None;
    write_markdown_ingest_queue(&paths, &requeue).expect("requeue identical source");

    let reingest_result = run_markdown_ingest_worker(&paths, &requeue, &store).expect("reingest");
    assert!(reingest_result.started);
    assert_eq!(reingest_result.processed, 1);

    let memories_after: Vec<MemoryRecord> =
        read_json_artifact(&memory_path).expect("read memories after");
    assert_eq!(memories_after, memories_before);
    assert_eq!(
        memories_after
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect::<BTreeSet<_>>()
            .len(),
        memories_after.len()
    );

    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events");
    let memory_events = events
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::MemoryAccepted
                && event.target_memory_ids == vec![memories_before[0].memory_id.clone()]
        })
        .collect::<Vec<_>>();
    assert_eq!(memory_events.len(), 1);
}

#[test]
fn markdown_relationship_endpoints_resolve_existing_and_new_nodes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_dir = workspace_root.join("sources");
    let wiki_dir = workspace_root.join("wiki");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&wiki_dir).expect("wiki dir");
    let paths = MarkdownIngestPaths {
        workspace_root: workspace_root.clone(),
        source_dir: source_dir.clone(),
        wiki_dir,
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let empty_snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
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

    fs::write(
        source_dir.join("first.md"),
        "# Agent-maintained graph\n\nEvents JSONL remains the source of truth.\n",
    )
    .expect("first source");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &empty_snapshot,
        &MarkdownSourceStateFile::default(),
        &MarkdownIngestQueueFile::default(),
    )
    .expect("first scan");
    enqueue_markdown_sources(&writer, &paths, &MarkdownIngestQueueFile::default(), &scan)
        .expect("first enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("first queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("first ingest");
    let first_snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("first snapshot");
    let existing_node_id = first_snapshot
        .nodes
        .iter()
        .find(|node| normalize_key(&node.label) == "agent-maintained-graph")
        .expect("existing graph node")
        .node_id
        .clone();

    fs::write(
            source_dir.join("second.md"),
            "# Agent maintained graph\n\n## Event ledger\n\nAgent maintained graph depends on Event ledger for replayable changes.\n",
        )
        .expect("second source");
    let queue = read_markdown_ingest_queue(&paths).expect("processed queue");
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    let scan = scan_new_markdown_sources(
        &paths,
        &first_snapshot,
        &MarkdownSourceStateFile::default(),
        &queue,
    )
    .expect("second scan");
    enqueue_markdown_sources(&writer, &paths, &queue, &scan).expect("second enqueue");
    drop(writer);
    let queue = read_markdown_ingest_queue(&paths).expect("second queue");
    run_markdown_ingest_worker(&paths, &queue, &store).expect("second ingest");

    let snapshot =
        read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID).expect("snapshot");
    let second_source = snapshot
        .sources
        .iter()
        .find(|source| source.original_path.ends_with("second.md"))
        .expect("second source");
    let relationship_path = workspace_root
        .join("artifacts")
        .join(&second_source.source_id)
        .join("edge-candidates.json");
    let relationship_evidence: Vec<MarkdownRelationshipEvidence> =
        read_json_artifact(&relationship_path).expect("edge candidates");
    let resolved = relationship_evidence
        .iter()
        .find(|evidence| {
            evidence.source_label == "Agent maintained graph"
                && evidence.target_label == "Event ledger"
        })
        .expect("resolved relationship evidence");
    assert_eq!(
        resolved.resolved_source_node_id.as_deref(),
        Some(existing_node_id.as_str())
    );
    assert_eq!(
        resolved.resolved_target_node_id.as_deref(),
        Some("concept-event-ledger")
    );
    assert!(resolved.endpoint_resolution.contains("existing_node"));
    assert!(resolved.endpoint_resolution.contains("proposed_node"));
    assert!(snapshot.relations.iter().any(|relation| {
        relation.source_node_id == existing_node_id
            && relation.target_node_id == "concept-event-ledger"
            && relation.kind == BrainRelationKind::DependsOn
            && !relation.evidence_ids.is_empty()
    }));
}

#[test]
fn project_store_materializes_brain_repo_artifacts() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));

    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");

    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    assert!(workspace_root.join("brain-manifest.json").exists());
    assert!(workspace_root.join("graph/nodes.json").exists());
    assert!(workspace_root.join("graph/edges.json").exists());
    assert!(workspace_root.join("graph/evidence.json").exists());
    assert!(workspace_root.join("graph/entities.json").exists());
    assert!(workspace_root.join("graph/claims.json").exists());
    assert!(workspace_root
        .join("artifacts/source-test/extraction.json")
        .exists());
    assert!(workspace_root.join("events/brain_events.jsonl").exists());
    assert!(workspace_root.join("wiki/index.md").exists());
    assert!(workspace_root.join("wiki/log.md").exists());
    assert!(workspace_root.join("wiki/overview.md").exists());
    assert!(workspace_root
        .join("reviews/proposed-updates/.gitkeep")
        .exists());
    assert!(workspace_root
        .join("reviews/lint-reports/.gitkeep")
        .exists());

    let manifest_json =
        fs::read_to_string(workspace_root.join("brain-manifest.json")).expect("brain manifest");
    let snapshot: BrainRepoSnapshot =
        serde_json::from_str(&manifest_json).expect("decode brain manifest");
    assert_eq!(snapshot.workspace_id, DEFAULT_WORKSPACE_ID);
    assert_eq!(snapshot.sources.len(), 1);
    assert_eq!(snapshot.extractions.len(), 1);
    assert_eq!(snapshot.extractions[0].extractor, "heuristic");
    assert_eq!(snapshot.extractions[0].source_refs, vec!["source-test"]);
    assert!(!snapshot.extractions[0].page_refs.is_empty());
    assert!(snapshot.extractions[0]
        .entities
        .iter()
        .all(|entity| !entity.evidence_refs.is_empty()
            && !entity.source_refs.is_empty()
            && !entity.page_refs.is_empty()
            && !entity.provenance.is_empty()));
    assert!(snapshot.extractions[0]
        .claims
        .iter()
        .all(|claim| !claim.evidence_refs.is_empty()
            && !claim.source_refs.is_empty()
            && !claim.page_refs.is_empty()
            && !claim.provenance.is_empty()));
    assert!(snapshot.extractions[0]
        .relations
        .iter()
        .all(|relation| !relation.evidence_refs.is_empty()
            && !relation.source_refs.is_empty()
            && !relation.page_refs.is_empty()
            && !relation.provenance.is_empty()));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.kind == BrainNodeKind::Concept));
    assert!(snapshot
        .entities
        .iter()
        .any(|entity| entity.kind == BrainNodeKind::Concept
            && !entity.evidence_refs.is_empty()
            && !entity.source_refs.is_empty()));
    assert!(snapshot.claims.iter().any(|claim| {
        claim.status == "supported"
            && claim.statement.contains("Agent brain")
            && !claim.topic_refs.is_empty()
            && !claim.evidence_refs.is_empty()
            && !claim.source_refs.is_empty()
    }));
    assert!(snapshot
        .relations
        .iter()
        .all(|relation| !relation.evidence_ids.is_empty()));
    assert!(snapshot
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::GraphMaterialized));
    let extraction_json =
        fs::read_to_string(workspace_root.join("artifacts/source-test/extraction.json"))
            .expect("read extraction artifact");
    let extraction: duckdocs_engine_types::StructuredExtractionArtifact =
        serde_json::from_str(&extraction_json).expect("decode extraction artifact");
    assert_eq!(extraction.extractor, "heuristic");
    assert_eq!(extraction.source_id, "source-test");
    assert!(extraction.created_at > 0);
    assert!(!extraction.entities.is_empty());
    assert!(!extraction.claims.is_empty());
    assert!(extraction
        .claims
        .iter()
        .all(|claim| !claim.evidence_refs.is_empty()));
    let index = fs::read_to_string(workspace_root.join("wiki/index.md")).expect("wiki index");
    assert!(index.contains("## Sources"));
    assert!(index.contains("## Topics"));
    let concept_node = snapshot
        .nodes
        .iter()
        .find(|node| {
            node.kind == BrainNodeKind::Concept
                && node.label.contains("Agent brain")
                && !node.evidence_ids.is_empty()
        })
        .expect("agent brain topic node");
    let topic_page = fs::read_to_string(workspace_root.join(format!(
        "wiki/topics/{}.md",
        sanitize_name(&concept_node.node_id)
    )))
    .expect("read agent brain topic page");
    assert!(topic_page.contains("## Node Description"));
    assert!(topic_page.contains("Agent brain context stays source backed."));
    assert!(topic_page.contains("## Claims"));
    assert!(topic_page.contains("_Source-backed claims linked to materialized evidence._"));
    assert!(topic_page.contains("Agent brain context stays source backed"));
    assert!(topic_page.contains("source-test"));
}

#[test]
fn structured_extraction_does_not_trust_claims_or_relations_without_evidence() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (mut project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nAgent brain context stays source backed.\nShared graph context keeps evidence visible.\n",
        );

    for detail in project.details_by_node_id.values_mut() {
        if detail.node.kind == GraphNodeKind::Concept {
            detail.evidence.clear();
        }
    }
    for detail in project.edge_details_by_id.values_mut() {
        detail.evidence.clear();
    }

    let row = StoredSourceRow {
        summary: source_summary_from_manifest(&manifest),
        project_id: project.summary.project_id.clone(),
        manifest_path: manifest.manifest_path.clone(),
    };
    let rows = vec![(row, Some(project.clone()))];
    let snapshot =
        build_brain_repo_snapshot(DEFAULT_WORKSPACE_ID, &rows, &project, &[], &[], &[], &[]);

    assert!(snapshot.claims.is_empty());
    assert!(snapshot.relations.is_empty());
    assert_eq!(snapshot.extractions.len(), 1);
    assert!(snapshot.extractions[0].claims.is_empty());
    assert!(snapshot.extractions[0].relations.is_empty());
}

#[test]
fn read_only_brain_api_reads_materialized_repo() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let search = handle_search_brain(SearchBrainRequest {
        scope: scope.clone(),
        query: "agent brain".into(),
        limit: Some(20),
    })
    .expect("search brain");
    assert!(!search.results.is_empty());
    assert!(search
        .results
        .iter()
        .any(|result| result.kind == BrainSearchResultKind::Entity));
    assert!(search
        .results
        .iter()
        .any(|result| result.kind == BrainSearchResultKind::Claim));

    let source = handle_read_source(ReadSourceRequest {
        scope: scope.clone(),
        source_id: manifest.source_id.clone(),
    })
    .expect("read source");
    assert_eq!(source.source.source_id, manifest.source_id);
    assert!(!source.evidence.is_empty());

    let wiki = handle_read_wiki_page(ReadWikiPageRequest {
        scope: scope.clone(),
        path: "index.md".into(),
    })
    .expect("read wiki page");
    assert_eq!(wiki.page.path, "wiki/index.md");
    assert!(wiki.page.body.contains("Brain Index"));

    let node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let node = handle_read_node(ReadNodeRequest {
        scope: scope.clone(),
        node_id,
    })
    .expect("read node");
    assert_eq!(node.node.kind, BrainNodeKind::Concept);
    assert!(!node.evidence.is_empty());

    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope: scope.clone(),
        limit: Some(2),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read recent events");
    assert!(!events.events.is_empty());

    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &vec![MemoryRecord {
            memory_id: "mem-read-snapshot".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            scope: BrainScope::Project,
            title: "Read snapshot memory".into(),
            body: "Snapshot reads should expose durable memory refs.".into(),
            source_refs: vec![manifest.source_id.clone()],
            evidence_refs: Vec::new(),
            created_at: 100,
            updated_at: 100,
        }],
    )
    .expect("write materialized memory");

    let snapshot = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: scope.clone(),
    })
    .expect("read graph snapshot");
    assert_eq!(snapshot.workspace_id, DEFAULT_WORKSPACE_ID);
    assert!(!snapshot.snapshot_id.is_empty());
    assert!(!snapshot.source_ingest_id.is_empty());
    assert!(snapshot.materialized_at >= snapshot.created_at);
    assert_eq!(snapshot.nodes.len(), project.nodes.len());
    assert_eq!(snapshot.edges.len(), project.edges.len());
    assert!(!snapshot.claims.is_empty());
    assert_eq!(snapshot.memory_refs, vec!["mem-read-snapshot"]);
    assert!(snapshot
        .wiki_pages
        .iter()
        .any(|page| page.path == "wiki/index.md" && page.body.contains("Brain Index")));
    assert!(snapshot
        .source_paths
        .iter()
        .any(|path| path.ends_with("source.md")));

    let context_pack = handle_get_context_pack(GetContextPackRequest {
        scope,
        query: "agent brain".into(),
        budget: Some(4000),
    })
    .expect("context pack")
    .context_pack;
    assert_eq!(context_pack.workspace_id, DEFAULT_WORKSPACE_ID);
    assert!(!context_pack.wiki_pages.is_empty());
    assert!(!context_pack.entities.is_empty());
    assert!(!context_pack.claims.is_empty());
    assert!(!context_pack.relations.is_empty());
    assert!(!context_pack.recent_events.is_empty());
}

#[test]
fn event_history_reader_filters_graph_loop_events_by_change_refs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source = SourceRecord {
        source_id: "source-event-history".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/event-history.md".into(),
        source_path: "/tmp/event-history.md".into(),
        markdown_path: "artifacts/source-event-history/event-history.md".into(),
        format: "markdown".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 100,
    };
    let node = BrainNodeRecord {
        node_id: "concept-event-history".into(),
        kind: BrainNodeKind::Concept,
        label: "Event history".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec!["ev-event-history".into()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let relation = BrainRelationRecord {
        relation_id: "edge-event-history-source".into(),
        kind: BrainRelationKind::DerivedFrom,
        source_node_id: node.node_id.clone(),
        target_node_id: "source:source-event-history".into(),
        label: "Derived from source".into(),
        evidence_ids: vec!["ev-event-history".into()],
        confidence: Some(0.8),
        updated_at: 100,
    };
    let claim = ClaimRecord {
        claim_id: "claim-event-history-filterable".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Graph loop events are filterable by durable refs.".into(),
        topic_refs: vec![node.node_id.clone()],
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec!["ev-event-history".into()],
        status: "supported".into(),
        updated_at: 100,
    };
    let memory = MemoryRecord {
        memory_id: "mem-event-history-filterable".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Event history reader".into(),
        body: "Display graph loop events through source-of-truth JSONL.".into(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec!["ev-event-history".into()],
        created_at: 100,
        updated_at: 100,
    };
    let events = vec![
        test_brain_event(
            "evt-run",
            BrainEventKind::SourceIngestQueued,
            Some("source_ingest_queued"),
            vec![source.source_id.clone()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            101,
        ),
        test_brain_event(
            "evt-node",
            BrainEventKind::GraphMaterialized,
            Some("new_node"),
            vec![source.source_id.clone()],
            vec![node.node_id.clone()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            102,
        ),
        test_brain_event(
            "evt-edge",
            BrainEventKind::GraphMaterialized,
            Some("new_edge"),
            vec![source.markdown_path.clone()],
            Vec::new(),
            vec![relation.relation_id.clone()],
            Vec::new(),
            Vec::new(),
            103,
        ),
        test_brain_event(
            "evt-claim",
            BrainEventKind::ClaimProposed,
            Some("new_claim"),
            vec![source.source_id.clone()],
            Vec::new(),
            Vec::new(),
            vec![claim.claim_id.clone()],
            Vec::new(),
            104,
        ),
        test_brain_event(
            "evt-memory",
            BrainEventKind::MemoryAccepted,
            Some("new_memory"),
            vec![source.source_id.clone()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![memory.memory_id.clone()],
            105,
        ),
    ];
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 105,
        sources: vec![source.clone()],
        nodes: vec![node.clone()],
        relations: vec![relation.clone()],
        evidence: vec![EvidenceRef {
            id: "ev-event-history".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Graph loop events are filterable.".into(),
            source_path: Some(source.source_path.clone()),
            source_id: Some(source.source_id.clone()),
            markdown_path: Some(source.markdown_path.clone()),
            image_path: None,
            provenance: Some("test".into()),
        }],
        memories: vec![memory.clone()],
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: vec![claim.clone()],
        extractions: Vec::new(),
        events,
    };
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized brain");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    assert_eq!(
        filtered_event_ids(&scope, |request| request.run_id =
            Some("run-evt-run".into())),
        vec!["evt-run".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| {
            request.source_ref = Some(source.markdown_path.clone())
        }),
        vec!["evt-edge".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| request.node_id =
            Some(node.node_id.clone())),
        vec!["evt-node".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| request.edge_id =
            Some(relation.relation_id)),
        vec!["evt-edge".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| request.claim_id = Some(claim.claim_id)),
        vec!["evt-claim".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| request.memory_id = Some(memory.memory_id)),
        vec!["evt-memory".to_string()]
    );
    assert_eq!(
        filtered_event_ids(&scope, |request| request.change_type =
            Some("new_edge".into())),
        vec!["evt-edge".to_string()]
    );
}

#[test]
fn graph_history_reader_lists_materialized_states_with_audit_locations() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut older_event = test_brain_event(
        "evt-graph-state-old",
        BrainEventKind::GraphMaterialized,
        Some("new_node"),
        vec!["source-old".into()],
        vec!["node-old".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        100,
    );
    older_event.causality.snapshot_id = Some("snapshot-old".into());
    older_event.causality.materialized_version = Some(110);
    older_event.payload_json =
        r#"{"nodeCount":1,"relationCount":0,"claimCount":0,"memoryCount":0,"wikiPageCount":1}"#
            .into();
    let mut latest_event = test_brain_event(
        "evt-graph-state-latest",
        BrainEventKind::GraphMaterialized,
        Some("new_memory"),
        vec![
            "source-latest".into(),
            "artifacts/source-latest/source.md".into(),
        ],
        vec!["node-latest".into()],
        vec!["edge-latest".into()],
        vec!["claim-latest".into()],
        vec!["memory-latest".into()],
        120,
    );
    latest_event.causality.snapshot_id = Some("snapshot-latest".into());
    latest_event.causality.materialized_version = Some(130);
    latest_event.payload_json =
        r#"{"nodeCount":2,"relationCount":1,"claimCount":1,"memoryCount":1,"wikiPageCount":2}"#
            .into();
    let mut failed_event = test_brain_event(
        "evt-graph-state-failed",
        BrainEventKind::GraphMaterialized,
        Some("new_claim"),
        vec!["source-failed".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        125,
    );
    failed_event.causality.snapshot_id = Some("snapshot-failed".into());
    failed_event.causality.materialized_version = Some(125);
    failed_event.policy_result = "failed".into();
    let mut in_progress_event = test_brain_event(
        "evt-graph-state-in-progress",
        BrainEventKind::GraphMaterialized,
        Some("new_edge"),
        vec!["source-in-progress".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        126,
    );
    in_progress_event.causality.snapshot_id = Some("snapshot-in-progress".into());
    in_progress_event.causality.materialized_version = Some(126);
    in_progress_event.policy_result = "in_progress".into();
    let mut missing_version_event = test_brain_event(
        "evt-graph-state-missing-version",
        BrainEventKind::GraphMaterialized,
        Some("new_node"),
        vec!["source-missing-version".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        127,
    );
    missing_version_event.causality.snapshot_id = Some("snapshot-missing-version".into());
    missing_version_event.policy_result = "materialized".into();
    let non_materialized_event = test_brain_event(
        "evt-not-a-state",
        BrainEventKind::MemoryAccepted,
        Some("new_memory"),
        vec!["source-latest".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec!["memory-latest".into()],
        140,
    );
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 130,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![
            older_event,
            latest_event,
            failed_event,
            in_progress_event,
            missing_version_event,
            non_materialized_event,
        ],
    };
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized brain");
    fs::create_dir_all(
        workspace_root
            .join("snapshots")
            .join("snapshot-latest")
            .join("files"),
    )
    .expect("snapshot files dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let states = handle_read_graph_history(ReadGraphHistoryRequest {
        scope,
        limit: Some(10),
    })
    .expect("read graph history")
    .states;

    assert_eq!(
        states
            .iter()
            .map(|state| state.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["evt-graph-state-latest", "evt-graph-state-old"]
    );
    let latest = states.first().expect("latest graph state");
    assert_eq!(latest.snapshot_id, "snapshot-latest");
    assert_eq!(latest.materialized_at, 130);
    assert_eq!(latest.rollback_target.snapshot_id, "snapshot-latest");
    assert_eq!(latest.rollback_target.event_id, "evt-graph-state-latest");
    assert_eq!(latest.rollback_target.materialized_version, 130);
    assert_eq!(
        latest.rollback_target.replay_selector,
        "--event evt-graph-state-latest"
    );
    assert_eq!(latest.operation_type.as_deref(), Some("new_memory"));
    assert_eq!(latest.node_count, 2);
    assert_eq!(latest.edge_count, 1);
    assert_eq!(latest.claim_count, 1);
    assert_eq!(latest.memory_count, 1);
    assert_eq!(latest.wiki_page_count, 2);
    assert!(latest
        .source_run_ids
        .iter()
        .any(|source_run_id| source_run_id == "source-latest"));
    assert!(latest
        .source_markdown_refs
        .iter()
        .any(|source_ref| source_ref == "artifacts/source-latest/source.md"));
    assert!(latest
        .storage_locations
        .iter()
        .any(|location| location == "events/brain_events.jsonl#evt-graph-state-latest"));
    assert!(latest
        .storage_locations
        .iter()
        .any(|location| location == "snapshots/snapshot-latest/files"));
    assert!(latest
        .storage_locations
        .iter()
        .any(|location| location == "graph/nodes.json"));
    assert!(latest
        .storage_locations
        .iter()
        .any(|location| location == "wiki/index.md"));
}

#[test]
fn graph_snapshot_reader_uses_latest_readable_marker_after_materialization() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut materialized_event = test_brain_event(
        "evt-readable-materialized",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-readable".into()],
        vec!["node-readable".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        100,
    );
    materialized_event.causality.snapshot_id = Some("snapshot-readable".into());
    materialized_event.causality.materialized_version = Some(100);
    materialized_event.payload_json =
        r#"{"nodeCount":1,"relationCount":0,"claimCount":0,"memoryCount":0,"wikiPageCount":1}"#
            .into();
    let node = BrainNodeRecord {
        node_id: "node-readable".into(),
        kind: BrainNodeKind::Concept,
        label: "Readable snapshot".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: vec!["source-readable".into()],
        confidence: None,
        updated_at: 100,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 100,
        sources: Vec::new(),
        nodes: vec![node],
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-readable".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            path: "wiki/index.md".into(),
            title: "Readable snapshot".into(),
            body: "# Readable snapshot\n".into(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: 100,
        }],
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![materialized_event.clone()],
    };
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write readable materialized brain");

    let marker_path = workspace_root.join("state/latest-readable-snapshot.json");
    let marker: serde_json::Value =
        read_json_artifact(&marker_path).expect("read latest readable marker");
    assert_eq!(marker["snapshotId"], serde_json::json!("snapshot-readable"));
    assert_eq!(
        marker["eventId"],
        serde_json::json!("evt-readable-materialized")
    );

    let mut unmarked_event = materialized_event.clone();
    unmarked_event.event_id = "evt-unmarked-materialized".into();
    unmarked_event.causality.snapshot_id = Some("snapshot-unmarked".into());
    unmarked_event.causality.materialized_version = Some(200);
    unmarked_event.created_at = 200;
    let mut events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read materialized events");
    events.push(unmarked_event);
    write_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"), &events)
        .expect("append unmarked event");

    let read = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("read latest readable snapshot");

    assert_eq!(read.snapshot_id, "snapshot-readable");
    assert_eq!(read.materialized_at, 100);
    assert_eq!(read.source_ingest_id, "source-readable");
    assert_eq!(read.source_of_truth_path, "events/brain_events.jsonl");
    assert_eq!(
        read.latest_readable_snapshot_path,
        "state/latest-readable-snapshot.json"
    );
    assert!(read
        .materialized_paths
        .iter()
        .any(|path| path == "graph/nodes.json"));
    assert!(read
        .materialized_paths
        .iter()
        .any(|path| path == "wiki/index.md"));

    let mut bad_marker = marker;
    bad_marker["workspaceId"] = serde_json::json!("other-workspace");
    bad_marker["eventId"] = serde_json::json!("evt-missing-readable");
    write_json_pretty(&marker_path, &bad_marker).expect("write stale marker");

    let fallback = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("ignore unresolved latest readable marker");

    assert_eq!(fallback.snapshot_id, "snapshot-unmarked");
    assert_eq!(fallback.materialized_at, 200);
    assert_eq!(fallback.source_ingest_id, "source-readable");
    assert!(fallback
        .materialized_paths
        .iter()
        .any(|path| path == "graph/nodes.json"));
}

#[test]
fn graph_snapshot_reader_uses_latest_completed_snapshot_without_marker() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut older_event = test_brain_event(
        "evt-completed-old",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-old".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        100,
    );
    older_event.causality.snapshot_id = Some("snapshot-old".into());
    older_event.causality.materialized_version = Some(100);
    let mut latest_event = test_brain_event(
        "evt-completed-latest",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-latest".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        150,
    );
    latest_event.causality.snapshot_id = Some("snapshot-latest-completed".into());
    latest_event.causality.materialized_version = Some(150);
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 150,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-latest-completed".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            path: "wiki/index.md".into(),
            title: "Latest completed".into(),
            body: "# Latest completed\n".into(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: 150,
        }],
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![older_event, latest_event],
    };
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write completed materialized brain");
    fs::remove_file(workspace_root.join("state/latest-readable-snapshot.json"))
        .expect("remove marker");

    let read = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("read markerless latest completed snapshot");

    assert_eq!(read.snapshot_id, "snapshot-latest-completed");
    assert_eq!(read.materialized_at, 150);
    assert_eq!(read.created_at, 150);
    assert_eq!(read.source_ingest_id, "source-latest");
}

#[test]
fn graph_snapshot_reader_falls_back_when_no_completed_snapshot_exists() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 77,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-no-completed".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            path: "wiki/index.md".into(),
            title: "No completed snapshot".into(),
            body: "# No completed snapshot\n".into(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: 77,
        }],
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write materialized brain without completed event");

    let read = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("read fallback snapshot");

    assert_eq!(read.snapshot_id, "snapshot-default-77");
    assert_eq!(read.source_ingest_id, "materialized://default");
    assert_eq!(read.created_at, 77);
    assert_eq!(read.materialized_at, 77);
    assert!(!workspace_root
        .join("state/latest-readable-snapshot.json")
        .exists());
}

#[test]
fn graph_snapshot_reader_returns_empty_state_for_fresh_workspace() {
    let temp = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(temp.path().join(DEFAULT_WORKSPACE_ID)).expect("create workspace root");

    let read = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("fresh workspace reads as empty snapshot");

    assert_eq!(read.workspace_id, DEFAULT_WORKSPACE_ID);
    assert_eq!(read.snapshot_id, "snapshot-default-0");
    assert_eq!(read.source_ingest_id, "materialized://default");
    assert!(read.nodes.is_empty());
    assert!(read.edges.is_empty());
    assert!(read.source_paths.is_empty());
    assert!(read.wiki_pages.is_empty());
}

#[test]
fn graph_snapshot_reader_excludes_stale_and_in_progress_ingests() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut completed_event = test_brain_event(
        "evt-completed-readable",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-completed".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        100,
    );
    completed_event.causality.snapshot_id = Some("snapshot-completed-readable".into());
    completed_event.causality.materialized_version = Some(100);
    completed_event.policy_result = "materialized".into();
    let mut stale_event = test_brain_event(
        "evt-stale-graph",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-stale".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        200,
    );
    stale_event.causality.snapshot_id = Some("snapshot-stale".into());
    stale_event.causality.materialized_version = Some(200);
    stale_event.policy_result = "stale".into();
    let mut in_progress_event = test_brain_event(
        "evt-in-progress-graph",
        BrainEventKind::GraphMaterialized,
        Some("graph_materialized"),
        vec!["source-in-progress".into()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        300,
    );
    in_progress_event.causality.snapshot_id = Some("snapshot-in-progress".into());
    in_progress_event.causality.materialized_version = Some(300);
    in_progress_event.policy_result = "in_progress".into();
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 100,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: Vec::new(),
        memories: Vec::new(),
        wiki_pages: vec![WikiPage {
            page_id: "wiki-completed-readable".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            path: "wiki/index.md".into(),
            title: "Completed readable".into(),
            body: "# Completed readable\n".into(),
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: Vec::new(),
            updated_at: 100,
        }],
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![completed_event, stale_event, in_progress_event],
    };
    write_materialized_brain_repo(&workspace_root, &snapshot)
        .expect("write materialized brain with unfinished events");
    fs::remove_file(workspace_root.join("state/latest-readable-snapshot.json"))
        .expect("remove marker");

    let read = handle_read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
    })
    .expect("read completed snapshot only");

    assert_eq!(read.snapshot_id, "snapshot-completed-readable");
    assert_eq!(read.materialized_at, 100);
    assert_eq!(read.source_ingest_id, "source-completed");
}

#[test]
fn replay_orders_incremental_events_and_rebuilds_from_empty_materialized_state() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");

    let source = SourceRecord {
        source_id: "source-replay-order".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "/tmp/replay-order.md".into(),
        source_path: "/tmp/replay-order.md".into(),
        markdown_path: "artifacts/source-replay-order/replay-order.md".into(),
        format: "markdown".into(),
        status: "ingested".into(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 100,
    };
    let evidence = EvidenceRef {
        id: "ev-replay-order".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Replay events rebuild graph, claims, memories, and wiki.".into(),
        source_path: Some(source.source_path.clone()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(source.markdown_path.clone()),
        image_path: None,
        provenance: Some("test".into()),
    };
    let node = BrainNodeRecord {
        node_id: "concept-replay-order".into(),
        kind: BrainNodeKind::Concept,
        label: "Replay Order".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec![evidence.id.clone()],
        source_ids: vec![source.source_id.clone()],
        confidence: Some(0.9),
        updated_at: 100,
    };
    let relation = BrainRelationRecord {
        relation_id: "relation-replay-order-self".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: node.node_id.clone(),
        target_node_id: node.node_id.clone(),
        label: "Replays before".into(),
        evidence_ids: vec![evidence.id.clone()],
        confidence: Some(0.8),
        updated_at: 300,
    };
    let claim = ClaimRecord {
        claim_id: "claim-replay-order".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Replay ordering must rebuild the latest materialized graph.".into(),
        topic_refs: vec![node.node_id.clone()],
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        status: "supported".into(),
        updated_at: 300,
    };
    let memory = MemoryRecord {
        memory_id: "memory-replay-order".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Replay ordering is deterministic".into(),
        body: "Full replay uses event materialized versions, then rebuilds current graph files."
            .into(),
        source_refs: vec![source.source_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        created_at: 300,
        updated_at: 300,
    };
    let node_only_page = WikiPage {
        page_id: "topic-concept-replay-order".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/topics/concept-replay-order.md".into(),
        title: "Replay Order".into(),
        body: "# Replay Order\n\nSource node only.\n".into(),
        source_refs: vec![source.source_id.clone()],
        node_refs: vec![node.node_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        updated_at: 100,
    };
    let full_page = WikiPage {
        page_id: "topic-concept-replay-order".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/topics/concept-replay-order.md".into(),
        title: "Replay Order".into(),
        body: "# Replay Order\n\nReplay ordering must rebuild the latest materialized graph.\n"
            .into(),
        source_refs: vec![source.source_id.clone()],
        node_refs: vec![node.node_id.clone()],
        evidence_refs: vec![evidence.id.clone()],
        updated_at: 300,
    };

    let mut node_event = test_brain_event(
        "evt-replay-node",
        BrainEventKind::GraphMaterialized,
        Some("new_node"),
        vec![source.source_id.clone()],
        vec![node.node_id.clone()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        100,
    );
    node_event.causality.materialized_version = Some(100);
    node_event.payload_json = materialized_graph_event_payload_json(
        100,
        std::slice::from_ref(&source),
        std::slice::from_ref(&node),
        &[],
        std::slice::from_ref(&evidence),
        &[],
        std::slice::from_ref(&node_only_page),
        &[],
        &[],
        &[],
    )
    .expect("node materialized payload");

    let mut full_event = test_brain_event(
        "evt-replay-full",
        BrainEventKind::GraphMaterialized,
        Some("new_memory"),
        vec![source.source_id.clone()],
        vec![node.node_id.clone()],
        vec![relation.relation_id.clone()],
        vec![claim.claim_id.clone()],
        vec![memory.memory_id.clone()],
        300,
    );
    full_event.causality.materialized_version = Some(300);
    full_event.payload_json = materialized_graph_event_payload_json(
        300,
        std::slice::from_ref(&source),
        std::slice::from_ref(&node),
        std::slice::from_ref(&relation),
        std::slice::from_ref(&evidence),
        std::slice::from_ref(&memory),
        std::slice::from_ref(&full_page),
        &[],
        std::slice::from_ref(&claim),
        &[],
    )
    .expect("full materialized payload");

    write_brain_events_jsonl(
        &workspace_root.join("events/brain_events.jsonl"),
        &[full_event, node_event],
    )
    .expect("write intentionally unordered events");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let incremental = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: scope.clone(),
        up_to_timestamp: None,
        up_to_materialized_version: Some(100),
        up_to_event_id: None,
        output_root: Some(temp.path().join("incremental-replay").display().to_string()),
        write_materialized: false,
    })
    .expect("incremental replay");
    assert_eq!(
        incremental.selected_event_id.as_deref(),
        Some("evt-replay-node")
    );
    assert_eq!(incremental.snapshot.nodes, vec![node.clone()]);
    assert!(incremental.snapshot.relations.is_empty());
    assert!(incremental.snapshot.claims.is_empty());
    assert!(incremental.snapshot.memories.is_empty());

    let rebuilt = handle_reconstruct_brain(ReconstructBrainRequest {
        scope,
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: None,
        output_root: Some(workspace_root.display().to_string()),
        write_materialized: false,
    })
    .expect("rebuild materialized state from events only");
    assert_eq!(
        rebuilt.selected_event_id.as_deref(),
        Some("evt-replay-full")
    );
    assert_eq!(rebuilt.snapshot.relations, vec![relation]);
    assert_eq!(rebuilt.snapshot.claims, vec![claim]);
    assert_eq!(rebuilt.snapshot.memories, vec![memory]);
    assert!(fs::read_to_string(workspace_root.join("graph/nodes.json"))
        .expect("rebuilt nodes")
        .contains("concept-replay-order"));
    assert!(fs::read_to_string(workspace_root.join("graph/edges.json"))
        .expect("rebuilt edges")
        .contains("relation-replay-order-self"));
    assert!(fs::read_to_string(workspace_root.join("graph/claims.json"))
        .expect("rebuilt claims")
        .contains("claim-replay-order"));
    assert!(
        fs::read_to_string(workspace_root.join("memory/records.json"))
            .expect("rebuilt memories")
            .contains("memory-replay-order")
    );
    assert!(fs::read_to_string(workspace_root.join("wiki/index.md"))
        .expect("rebuilt wiki index")
        .contains("[Replay Order](topics/concept-replay-order.md)"));
    assert!(
        fs::read_to_string(workspace_root.join("wiki/topics/concept-replay-order.md"))
            .expect("rebuilt wiki topic")
            .contains("Replay ordering must rebuild the latest materialized graph.")
    );
}

#[test]
fn graph_history_readers_handle_missing_and_corrupt_event_ledgers() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 10,
        sources: Vec::new(),
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
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized brain");
    let events_path = workspace_root.join("events/brain_events.jsonl");
    fs::remove_file(&events_path).expect("remove event ledger");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let recent_events = handle_read_recent_events(ReadRecentEventsRequest {
        scope: scope.clone(),
        limit: Some(10),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("missing event ledger reads as empty");
    assert!(recent_events.events.is_empty());

    let states = handle_read_graph_history(ReadGraphHistoryRequest {
        scope: scope.clone(),
        limit: Some(10),
    })
    .expect("missing event ledger yields no graph states");
    assert!(states.states.is_empty());

    fs::write(&events_path, "{not-json}\n").expect("write corrupt event ledger");
    let err = handle_read_recent_events(ReadRecentEventsRequest {
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
    .expect_err("corrupt event ledger must fail");
    assert!(format!("{err:#}").contains("failed decoding brain event JSONL row"));
}

fn filtered_event_ids<F>(scope: &BrainReadScope, configure: F) -> Vec<String>
where
    F: FnOnce(&mut ReadRecentEventsRequest),
{
    let mut request = ReadRecentEventsRequest {
        scope: scope.clone(),
        limit: Some(10),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    };
    configure(&mut request);
    handle_read_recent_events(request)
        .expect("read filtered events")
        .events
        .into_iter()
        .map(|event| event.event_id)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn test_brain_event(
    event_id: &str,
    event_type: BrainEventKind,
    operation_type: Option<&str>,
    refs: Vec<String>,
    node_refs: Vec<String>,
    relation_refs: Vec<String>,
    claim_refs: Vec<String>,
    memory_refs: Vec<String>,
    created_at: u64,
) -> BrainEvent {
    BrainEvent {
        event_id: event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        event_type,
        operation_type: operation_type.map(ToOwned::to_owned),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent".into(),
        },
        source_refs: refs.clone(),
        source_markdown_refs: refs
            .iter()
            .filter(|value| value.ends_with(".md"))
            .cloned()
            .collect(),
        node_refs: node_refs.clone(),
        relation_refs: relation_refs.clone(),
        claim_refs: claim_refs.clone(),
        memory_refs: memory_refs.clone(),
        target_node_ids: node_refs,
        target_edge_ids: relation_refs,
        target_claim_ids: claim_refs,
        target_memory_ids: memory_refs,
        evidence_refs: vec!["ev-event-history".into()],
        payload_json: format!(
            "{{\"runId\":\"run-{event_id}\",\"changeType\":\"{}\"}}",
            operation_type.unwrap_or("event")
        ),
        causality: BrainEventCausality {
            caused_by_event_ids: vec![format!("run-{event_id}")],
            caused_by_source_ids: refs,
            ..Default::default()
        },
        confidence: None,
        policy_result: "applied".into(),
        created_at,
    }
}

#[test]
fn brain_search_uses_tokenized_retrieval_and_context_expansion() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Planning memo\n\n## Page 1\n\nAgent brain context stays source backed.\nProject retrieval packs cite evidence for the graph.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let search = handle_search_brain(SearchBrainRequest {
        scope: scope.clone(),
        query: "agents".into(),
        limit: Some(10),
    })
    .expect("search plural query");
    assert!(search.results.iter().any(|result| {
        result.kind == BrainSearchResultKind::Entity && result.title.contains("Agent brain")
    }));
    assert!(search
        .results
        .iter()
        .any(|result| result.kind == BrainSearchResultKind::Evidence));
    assert!(search.results.iter().any(|result| {
        result.kind == BrainSearchResultKind::Claim && result.snippet.contains("evidence:")
    }));

    let context_pack = handle_get_context_pack(GetContextPackRequest {
        scope,
        query: "agents".into(),
        budget: Some(4000),
    })
    .expect("context pack")
    .context_pack;
    assert!(context_pack
        .entities
        .iter()
        .any(|entity| entity.name.contains("Agent brain")));
    assert!(!context_pack.claims.is_empty());
    assert!(!context_pack.relations.is_empty());
    assert!(!context_pack.sources.is_empty());
    assert!(!context_pack.evidence.is_empty());
}

#[test]
fn safe_brain_update_proposals_auto_apply_memory_records_without_mutating_graph_or_wiki() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    write_source_manifest(&manifest).expect("write source manifest");
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let nodes_before =
        fs::read_to_string(workspace_root.join("graph/nodes.json")).expect("read nodes before");
    let edges_before =
        fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges before");
    let wiki_before =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki before");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "claude-code".into(),
    };
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let proposal_specs = [
        (
            BrainProposalKind::Memory,
            "Remember parser wedge",
            "HyprDuck starts from source-backed document parsing.",
            None,
            None,
            None,
            Vec::new(),
        ),
        (
            BrainProposalKind::Claim,
            "Claim source backing",
            "Every durable brain fact should point back to source evidence.",
            Some(concept_node_id.clone()),
            None,
            None,
            vec![concept_node_id.clone()],
        ),
        (
            BrainProposalKind::Link,
            "Relate document and concept",
            "The concept is derived from the imported document.",
            Some(concept_node_id.clone()),
            None,
            Some(BrainRelationKind::RelatedTo),
            vec!["document".into()],
        ),
        (
            BrainProposalKind::Observation,
            "Append session observation",
            "The agent saw the user prioritize a review queue.",
            None,
            None,
            None,
            Vec::new(),
        ),
        (
            BrainProposalKind::SourceNote,
            "Annotate source",
            "This source should seed the local brain.",
            None,
            Some(manifest.source_id.clone()),
            None,
            Vec::new(),
        ),
    ];

    let mut proposal_paths = Vec::new();
    for (kind, title, body, target_node_id, target_source_id, relation_kind, node_refs) in
        proposal_specs
    {
        let response = handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind,
            title: title.into(),
            body: body.into(),
            actor: actor.clone(),
            target_node_id,
            target_source_id,
            relation_kind,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs,
            evidence_refs: Vec::new(),
            proposal_payload: None,
        })
        .expect("propose brain update");
        if matches!(
            response.proposal.kind,
            BrainProposalKind::Memory
                | BrainProposalKind::Observation
                | BrainProposalKind::SourceNote
        ) {
            assert_eq!(response.proposal.status, BrainProposalStatus::Accepted);
            assert_eq!(response.event.policy_result, "auto_applied");
        } else {
            assert_eq!(response.proposal.status, BrainProposalStatus::PendingReview);
            assert_eq!(response.event.policy_result, "needs_review");
        }
        proposal_paths.push(PathBuf::from(response.proposal_path));
    }

    let mut accepted_count = 0usize;
    let mut pending_count = 0usize;
    for path in &proposal_paths {
        assert!(path.exists(), "missing proposal {}", path.display());
        let proposal_json = fs::read_to_string(path).expect("read proposal");
        let proposal: BrainUpdateProposal =
            serde_json::from_str(&proposal_json).expect("decode proposal");
        match proposal.status {
            BrainProposalStatus::Accepted => accepted_count += 1,
            BrainProposalStatus::PendingReview => pending_count += 1,
            BrainProposalStatus::Rejected => panic!("proposal should not be rejected"),
        }
    }
    assert_eq!(accepted_count, 3);
    assert_eq!(pending_count, 2);

    let memory_records: Vec<MemoryRecord> =
        read_json_artifact(&workspace_root.join("memory/records.json"))
            .expect("read memory records");
    assert_eq!(memory_records.len(), 2);
    assert!(memory_records
        .iter()
        .any(|memory| memory.title == "Remember parser wedge"));
    assert!(memory_records
        .iter()
        .any(|memory| memory.title == "Append session observation"));
    assert!(!memory_records
        .iter()
        .any(|memory| memory.title == "Annotate source"));
    let updated_manifest: SourceArtifactManifest =
        read_json_artifact(&PathBuf::from(&manifest.manifest_path))
            .expect("read updated source manifest");
    assert_eq!(
        updated_manifest.description,
        "This source should seed the local brain."
    );
    let stored_sources = store
        .load_sources(DEFAULT_WORKSPACE_ID)
        .expect("load source summaries after source note");
    assert_eq!(
        stored_sources[0].description,
        "This source should seed the local brain."
    );
    let read_source = handle_read_source(ReadSourceRequest {
        scope: scope.clone(),
        source_id: manifest.source_id.clone(),
    })
    .expect("read source after source note");
    assert_eq!(
        read_source.source.description,
        "This source should seed the local brain."
    );
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::SourceNote,
        title: "Update source metadata fields".into(),
        body: "Fallback source note body.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: Some(manifest.source_id.clone()),
        relation_kind: None,
        source_description: Some("Reader guide".into()),
        source_user_context: Some("Imported for agent planning.".into()),
        source_ingest_instruction: Some("Extract decisions and open questions.".into()),
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: Vec::new(),
        proposal_payload: None,
    })
    .expect("propose explicit source metadata");
    let explicit_manifest: SourceArtifactManifest =
        read_json_artifact(&PathBuf::from(&manifest.manifest_path))
            .expect("read explicit source manifest");
    assert_eq!(explicit_manifest.description, "Reader guide");
    assert_eq!(
        explicit_manifest.user_context,
        "Imported for agent planning."
    );
    assert_eq!(
        explicit_manifest.ingest_instruction,
        "Extract decisions and open questions."
    );

    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope: scope.clone(),
        limit: Some(20),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read proposal events");
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::MemoryProposed));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::ClaimProposed));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::LinkProposed));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::ObservationAppended));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::SourceNoteProposed));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::MemoryAccepted));

    let search = handle_search_brain(SearchBrainRequest {
        scope: scope.clone(),
        query: "review queue".into(),
        limit: Some(10),
    })
    .expect("search brain memories");
    assert!(search.results.iter().any(|result| {
        result.kind == BrainSearchResultKind::Memory && result.title == "Append session observation"
    }));

    let context_pack = handle_get_context_pack(GetContextPackRequest {
        scope: scope.clone(),
        query: "source backed document parsing".into(),
        budget: Some(4000),
    })
    .expect("context pack")
    .context_pack;
    assert!(context_pack
        .memories
        .iter()
        .any(|memory| memory.title == "Remember parser wedge"));

    assert_eq!(
        fs::read_to_string(workspace_root.join("graph/nodes.json")).expect("read nodes after"),
        nodes_before
    );
    assert_eq!(
        fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges after"),
        edges_before
    );
    assert_eq!(
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki after"),
        wiki_before
    );

    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("rematerialize workspace brain repo");
    let rematerialized_memory_records: Vec<MemoryRecord> =
        read_json_artifact(&workspace_root.join("memory/records.json"))
            .expect("read rematerialized memory records");
    assert_eq!(rematerialized_memory_records.len(), 2);
    let rematerialized_events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
            .expect("read rematerialized events");
    let accepted_event_ids = events
        .events
        .iter()
        .filter(|event| event.event_type == BrainEventKind::MemoryAccepted)
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    let claim_event_ids = events
        .events
        .iter()
        .filter(|event| event.event_type == BrainEventKind::ClaimProposed)
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    let rematerialized_event_ids = rematerialized_events
        .iter()
        .map(|event| event.event_id.clone())
        .collect::<BTreeSet<_>>();
    assert!(accepted_event_ids
        .iter()
        .all(|event_id| rematerialized_event_ids.contains(event_id)));
    assert!(claim_event_ids
        .iter()
        .all(|event_id| rematerialized_event_ids.contains(event_id)));
}

#[test]
fn graph_mutation_event_schema_tracks_targets_payload_and_causality() {
    let proposal = BrainUpdateProposal {
        proposal_id: "proposal-schema-claim".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        kind: BrainProposalKind::Claim,
        status: BrainProposalStatus::Accepted,
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        scope: BrainScope::Project,
        title: "Schema claim".into(),
        body: "Typed graph mutation events stay replayable.".into(),
        target_node_id: Some("concept-agent-maintained-graph".into()),
        target_source_id: None,
        relation_kind: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: vec!["concept-agent-maintained-graph".into()],
        evidence_refs: vec!["ev-agent-loop-1".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Events JSONL is the source of truth for graph replay.".into(),
                source_path: "sources/agent-loop.md".into(),
                claim_id: Some("claim-events-jsonl-source-of-truth".into()),
                topic_refs: vec!["concept-agent-maintained-graph".into()],
                source_refs: vec!["source-agent-loop".into()],
                evidence_refs: vec!["ev-agent-loop-1".into()],
                reason: Some("The source describes replayable graph mutations.".into()),
            },
        }),
        created_at: 42,
    };

    let event = brain_graph_mutation_applied_event(&proposal).expect("build graph mutation event");
    assert_eq!(event.schema_version, BRAIN_EVENT_SCHEMA_VERSION);
    assert_eq!(event.operation_type.as_deref(), Some("new_claim"));
    assert_eq!(event.actor.actor_type, BrainActorType::Agent);
    assert_eq!(event.actor.actor_id, "duckdocs-agent-ingest");
    assert_eq!(event.created_at, 42);
    assert_eq!(event.source_refs, vec!["source-agent-loop".to_string()]);
    assert_eq!(
        event.source_markdown_refs,
        vec!["sources/agent-loop.md".to_string()]
    );
    assert_eq!(
        event.target_node_ids,
        vec!["concept-agent-maintained-graph".to_string()]
    );
    assert_eq!(
        event.target_claim_ids,
        vec!["claim-events-jsonl-source-of-truth".to_string()]
    );
    assert_eq!(
        event.claim_refs,
        vec!["claim-events-jsonl-source-of-truth".to_string()]
    );
    assert!(event.target_edge_ids.is_empty());
    assert!(event.target_memory_ids.is_empty());
    assert_eq!(
        event.causality.caused_by_proposal_id.as_deref(),
        Some("proposal-schema-claim")
    );
    assert_eq!(
        event.causality.caused_by_source_ids,
        vec!["source-agent-loop".to_string()]
    );
    assert_eq!(event.causality.schema_version, BRAIN_EVENT_SCHEMA_VERSION);
    assert_eq!(event.causality.materialized_version, Some(42));

    let encoded = serde_json::to_value(&event).expect("encode event");
    assert_eq!(encoded["operationType"], "new_claim");
    assert_eq!(encoded["sourceMarkdownRefs"][0], "sources/agent-loop.md");
    assert_eq!(
        encoded["targetClaimIds"][0],
        "claim-events-jsonl-source-of-truth"
    );
    assert_eq!(
        encoded["payloadJson"]
            .as_str()
            .unwrap()
            .contains("proposal-schema-claim"),
        true
    );
    assert_eq!(
        encoded["causality"]["causedByProposalId"],
        "proposal-schema-claim"
    );
}

#[test]
fn queued_agent_proposal_runner_applies_valid_changes_transactionally() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown =
        "# Sample import\n\n## Page 1\n\nQueued agent graph changes stay source backed.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let evidence_ids = project
        .details_by_node_id
        .get(&concept_node_id)
        .expect("concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };
    let queued = vec![
            BrainUpdateProposal {
                proposal_id: "proposal-queued-node".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                kind: BrainProposalKind::Node,
                status: BrainProposalStatus::PendingReview,
                actor: actor.clone(),
                scope: BrainScope::Project,
                title: "Create queued node".into(),
                body: "Queued proposals can create graph nodes.".into(),
                target_node_id: None,
                target_source_id: None,
                relation_kind: None,
                source_refs: Vec::new(),
                node_refs: Vec::new(),
                evidence_refs: Vec::new(),
                proposal_payload: Some(AgentGraphProposalPayload::NewNode {
                    node: AgentNewNodePayload {
                        label: "Queued Graph Runner".into(),
                        kind: BrainNodeKind::Concept,
                        source_path: manifest.markdown_path.clone(),
                        node_id: Some("concept-queued-graph-runner".into()),
                        aliases: Vec::new(),
                        source_refs: vec![manifest.source_id.clone()],
                        evidence_refs: evidence_ids.clone(),
                        reason: Some("The queued source describes graph changes.".into()),
                    },
                }),
                created_at: 10,
            },
            BrainUpdateProposal {
                proposal_id: "proposal-queued-edge".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                kind: BrainProposalKind::Link,
                status: BrainProposalStatus::PendingReview,
                actor: actor.clone(),
                scope: BrainScope::Project,
                title: "Connect queued graph runner".into(),
                body: "Queued proposals can create graph edges.".into(),
                target_node_id: None,
                target_source_id: None,
                relation_kind: None,
                source_refs: Vec::new(),
                node_refs: Vec::new(),
                evidence_refs: Vec::new(),
                proposal_payload: Some(AgentGraphProposalPayload::NewEdge {
                    edge: AgentNewEdgePayload {
                        source_node_id: concept_node_id.clone(),
                        target_node_id: "concept-queued-graph-runner".into(),
                        kind: BrainRelationKind::RelatedTo,
                        label: "Related queued change".into(),
                        source_path: manifest.markdown_path.clone(),
                        edge_id: Some("relation-queued-graph-runner".into()),
                        source_refs: vec![manifest.source_id.clone()],
                        evidence_refs: evidence_ids.clone(),
                        reason: Some("The queued node is derived from the source.".into()),
                    },
                }),
                created_at: 11,
            },
            BrainUpdateProposal {
                proposal_id: "proposal-queued-claim".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                kind: BrainProposalKind::Claim,
                status: BrainProposalStatus::PendingReview,
                actor: actor.clone(),
                scope: BrainScope::Project,
                title: "Attach queued claim".into(),
                body: "Queued proposals should become source-backed claims.".into(),
                target_node_id: None,
                target_source_id: None,
                relation_kind: None,
                source_refs: Vec::new(),
                node_refs: Vec::new(),
                evidence_refs: Vec::new(),
                proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
                    claim: AgentNewClaimPayload {
                        statement:
                            "Queued agent graph changes are applied without a human gate.".into(),
                        source_path: manifest.markdown_path.clone(),
                        claim_id: Some("claim-queued-agent-runner".into()),
                        topic_refs: vec!["concept-queued-graph-runner".into()],
                        source_refs: vec![manifest.source_id.clone()],
                        evidence_refs: evidence_ids.clone(),
                        reason: Some("The runner auto-applies valid queued changes.".into()),
                    },
                }),
                created_at: 12,
            },
            BrainUpdateProposal {
                proposal_id: "proposal-queued-memory".into(),
                workspace_id: DEFAULT_WORKSPACE_ID.into(),
                kind: BrainProposalKind::Memory,
                status: BrainProposalStatus::PendingReview,
                actor,
                scope: BrainScope::Project,
                title: "Remember queued graph runner".into(),
                body: "Queued graph runner changes remain auditable.".into(),
                target_node_id: None,
                target_source_id: None,
                relation_kind: None,
                source_refs: Vec::new(),
                node_refs: Vec::new(),
                evidence_refs: Vec::new(),
                proposal_payload: Some(AgentGraphProposalPayload::NewMemory {
                    memory: AgentNewMemoryPayload {
                        title: "Queued graph runner is autonomous".into(),
                        body: "Valid queued graph proposals are automatically applied with snapshot audit artifacts.".into(),
                        source_path: manifest.markdown_path.clone(),
                        memory_id: Some("memory-queued-graph-runner".into()),
                        source_refs: vec![manifest.source_id.clone()],
                        evidence_refs: evidence_ids.clone(),
                        reason: Some("The runner has no human approval gate.".into()),
                    },
                }),
                created_at: 13,
            },
        ];
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    for proposal in &queued {
        writer
            .write_proposal(proposal)
            .expect("write queued proposal");
    }
    drop(writer);

    let result = run_queued_agent_proposal_apply_worker(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("apply queued proposals");

    assert_eq!(result.applied.len(), 4);
    assert!(result.failed.is_empty());
    let nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("nodes");
    assert!(nodes
        .iter()
        .any(|node| node.node_id == "concept-queued-graph-runner"));
    let edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json")).expect("edges");
    assert!(edges
        .iter()
        .any(|edge| edge.relation_id == "relation-queued-graph-runner"));
    let claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json")).expect("claims");
    assert!(claims
        .iter()
        .any(|claim| claim.claim_id == "claim-queued-agent-runner"));
    let memories = read_memory_records(&workspace_root).expect("memories");
    assert!(memories
        .iter()
        .any(|memory| memory.memory_id == "memory-queued-graph-runner"));
    assert!(workspace_root.join("snapshots").exists());
    assert!(workspace_root.join("reviews/applied-runs").exists());
    let topic =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-queued-graph-runner.md"))
            .expect("topic");
    assert!(topic.contains("## Claims"));
    assert!(topic.contains("## Relations"));

    let events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl")).expect("events");
    assert!(events.iter().any(|event| {
        event.event_type == BrainEventKind::ReviewResolved
            && event.policy_result == "auto_applied"
            && event.payload_json.contains("snapshotId")
    }));
    let audits_dir = workspace_root.join("reviews/applied-runs");
    let audit_paths = fs::read_dir(&audits_dir)
        .expect("read applied run audits")
        .map(|entry| entry.expect("audit entry").path())
        .collect::<Vec<_>>();
    assert_eq!(audit_paths.len(), 4);
    let audits = audit_paths
        .iter()
        .map(|path| read_json_artifact::<AgentProposalApplyAudit>(path).expect("read audit"))
        .collect::<Vec<_>>();
    assert!(audits.iter().all(|audit| {
        let run_root = workspace_root.join("runs").join(&audit.run_id);
        audit.status == "applied"
            && audit.error_code.is_none()
            && audit.error_message.is_none()
            && audit.rollback_hint.contains("snapshots/<snapshotId>/files")
            && workspace_root
                .join("snapshots")
                .join(&audit.snapshot_id)
                .join("manifest.json")
                .exists()
            && run_root.join("before.json").exists()
            && run_root.join("before").exists()
            && run_root.join("after.json").exists()
            && run_root.join("after").exists()
            && run_root.join("graph-diff.json").exists()
            && run_root.join("provider-response.json").exists()
            && run_root.join("validation-report.json").exists()
    }));
    assert!(audits.iter().any(|audit| {
        audit.proposal_id == "proposal-queued-node"
            && audit
                .changed_files
                .iter()
                .any(|path| path == "graph/nodes.json")
            && audit
                .changed_files
                .iter()
                .any(|path| path == "wiki/index.md")
            && audit
                .changed_files
                .iter()
                .any(|path| path == "wiki/topics/concept-queued-graph-runner.md")
    }));
    assert!(audits.iter().any(|audit| {
        audit.proposal_id == "proposal-queued-claim"
            && audit
                .changed_files
                .iter()
                .any(|path| path == "graph/claims.json")
            && audit
                .changed_files
                .iter()
                .any(|path| path == "wiki/topics/concept-queued-graph-runner.md")
    }));
    for proposal_id in [
        "proposal-queued-node",
        "proposal-queued-edge",
        "proposal-queued-claim",
        "proposal-queued-memory",
    ] {
        let proposal: BrainUpdateProposal = read_json_artifact(
            &workspace_root
                .join("reviews/proposed-updates")
                .join(format!("{proposal_id}.json")),
        )
        .expect("read applied proposal");
        assert_eq!(proposal.status, BrainProposalStatus::Accepted);
    }

    let rerun = run_queued_agent_proposal_apply_worker(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("rerun queued proposal worker");
    assert!(rerun.applied.is_empty());
    assert!(rerun.failed.is_empty());
}

#[test]
fn queued_agent_proposal_runner_reports_failures_and_continues_queue() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown =
            "# Queued graph failures\n\n## Page 1\n\nQueued graph failures should be audited without stopping valid changes.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let evidence_ids = project
        .details_by_node_id
        .get(&concept_node_id)
        .expect("concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("writer");
    writer
        .write_proposal(&BrainUpdateProposal {
            proposal_id: "proposal-invalid-node".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            kind: BrainProposalKind::Node,
            status: BrainProposalStatus::PendingReview,
            actor: actor.clone(),
            scope: BrainScope::Project,
            title: "Create invalid node".into(),
            body: "This proposal is missing evidence refs and must be rejected.".into(),
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_refs: Vec::new(),
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
            proposal_payload: Some(AgentGraphProposalPayload::NewNode {
                node: AgentNewNodePayload {
                    label: "Invalid Queued Node".into(),
                    kind: BrainNodeKind::Concept,
                    source_path: manifest.markdown_path.clone(),
                    node_id: Some("concept-invalid-queued-node".into()),
                    aliases: Vec::new(),
                    source_refs: vec![manifest.source_id.clone()],
                    evidence_refs: Vec::new(),
                    reason: Some("Failure path coverage.".into()),
                },
            }),
            created_at: 10,
        })
        .expect("write invalid queued proposal");
    writer
        .write_proposal(&BrainUpdateProposal {
            proposal_id: "proposal-valid-node-after-failure".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            kind: BrainProposalKind::Node,
            status: BrainProposalStatus::PendingReview,
            actor,
            scope: BrainScope::Project,
            title: "Create valid node after failure".into(),
            body: "Valid proposals behind failed proposals should still apply.".into(),
            target_node_id: None,
            target_source_id: None,
            relation_kind: None,
            source_refs: Vec::new(),
            node_refs: Vec::new(),
            evidence_refs: Vec::new(),
            proposal_payload: Some(AgentGraphProposalPayload::NewNode {
                node: AgentNewNodePayload {
                    label: "Valid Queued Node".into(),
                    kind: BrainNodeKind::Concept,
                    source_path: manifest.markdown_path.clone(),
                    node_id: Some("concept-valid-queued-node".into()),
                    aliases: Vec::new(),
                    source_refs: vec![manifest.source_id.clone()],
                    evidence_refs: evidence_ids.clone(),
                    reason: Some("The queue should continue after failures.".into()),
                },
            }),
            created_at: 11,
        })
        .expect("write valid queued proposal");
    drop(writer);

    let result = run_queued_agent_proposal_apply_worker(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("run proposal worker");

    assert_eq!(result.applied, vec!["proposal-valid-node-after-failure"]);
    assert_eq!(result.failed.len(), 1);
    let failure = &result.failed[0];
    assert_eq!(failure.proposal_id, "proposal-invalid-node");
    assert_eq!(failure.error_code, "invalid_agent_graph_proposal");
    assert!(failure
        .validation_issues
        .iter()
        .any(|issue| issue.code == AgentGraphProposalValidationCode::MissingEvidenceRefs));
    let failure_audit_path = PathBuf::from(&failure.audit_path);
    assert!(failure_audit_path.exists());
    let failure_audit: AgentProposalApplyAudit =
        read_json_artifact(&failure_audit_path).expect("read failure audit");
    assert_eq!(failure_audit.status, "failed");
    assert_eq!(failure_audit.proposal_id, "proposal-invalid-node");
    assert_eq!(
        failure_audit.error_code.as_deref(),
        Some("invalid_agent_graph_proposal")
    );
    assert!(failure_audit.changed_files.is_empty());
    assert!(failure_audit
        .rollback_hint
        .contains("pre-apply snapshot was restored"));
    assert!(workspace_root
        .join("snapshots")
        .join(&failure.snapshot_id)
        .join("manifest.json")
        .exists());

    let invalid: BrainUpdateProposal = read_json_artifact(
        &workspace_root.join("reviews/proposed-updates/proposal-invalid-node.json"),
    )
    .expect("read invalid proposal");
    assert_eq!(invalid.status, BrainProposalStatus::Rejected);
    let valid: BrainUpdateProposal = read_json_artifact(
        &workspace_root.join("reviews/proposed-updates/proposal-valid-node-after-failure.json"),
    )
    .expect("read valid proposal");
    assert_eq!(valid.status, BrainProposalStatus::Accepted);
    let nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("nodes");
    assert!(nodes
        .iter()
        .any(|node| node.node_id == "concept-valid-queued-node"));
    assert!(!nodes
        .iter()
        .any(|node| node.node_id == "concept-invalid-queued-node"));

    let events =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl")).expect("events");
    assert!(events.iter().any(|event| {
        event.event_type == BrainEventKind::ReviewResolved
            && event.policy_result == "auto_rejected"
            && event
                .payload_json
                .contains("\"errorCode\":\"invalid_agent_graph_proposal\"")
            && event
                .payload_json
                .contains(&format!("\"snapshotId\":\"{}\"", failure.snapshot_id))
            && event.payload_json.contains("\"validationIssues\"")
    }));

    let rerun = run_queued_agent_proposal_apply_worker(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("rerun proposal worker");
    assert!(rerun.applied.is_empty());
    assert!(rerun.failed.is_empty());
    let events_after_rerun =
        read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
            .expect("events after rerun");
    assert_eq!(events_after_rerun.len(), events.len());
    let nodes_after_rerun: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("nodes after rerun");
    assert_eq!(
        nodes_after_rerun
            .iter()
            .filter(|node| node.node_id == "concept-valid-queued-node")
            .count(),
        1
    );
    assert!(!nodes_after_rerun
        .iter()
        .any(|node| node.node_id == "concept-invalid-queued-node"));
}

#[test]
fn agent_proposal_payloads_parse_and_validate_new_node_and_new_claim() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };

    let node_payload_json = json!({
        "changeType": "new_node",
        "node": {
            "label": "Agent Maintained Knowledge Graph",
            "kind": "concept",
            "sourcePath": "sources/agent-loop.md",
            "sourceRefs": ["source-agent-loop"],
            "evidenceRefs": ["ev-agent-loop-1"],
            "aliases": ["AMKG"],
            "reason": "Source heading introduces the concept."
        }
    });
    let node_payload: AgentGraphProposalPayload =
        serde_json::from_value(node_payload_json).expect("parse new_node payload");
    let node_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Node,
        title: "Create graph concept".into(),
        body: "Create a concept node from source evidence.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: Vec::new(),
        evidence_refs: vec!["ev-agent-loop-1".into()],
        proposal_payload: Some(node_payload),
    })
    .expect("propose new node payload");
    assert_eq!(node_response.proposal.status, BrainProposalStatus::Accepted);
    assert_eq!(node_response.event.policy_result, "auto_applied");
    assert_eq!(node_response.event.event_type, BrainEventKind::NodeProposed);
    assert!(node_response
        .event
        .payload_json
        .contains("\"changeType\":\"new_node\""));
    assert!(node_response
        .proposal
        .proposal_payload
        .as_ref()
        .is_some_and(|payload| matches!(payload, AgentGraphProposalPayload::NewNode { .. })));

    let claim_payload = AgentGraphProposalPayload::NewClaim {
        claim: AgentNewClaimPayload {
            statement: "Events JSONL is the source of truth for replay.".into(),
            source_path: "sources/agent-loop.md".into(),
            claim_id: Some("claim-events-source-truth".into()),
            topic_refs: vec!["concept-agent-maintained-knowledge-graph".into()],
            source_refs: vec!["source-agent-loop".into()],
            evidence_refs: vec!["ev-agent-loop-2".into()],
            reason: Some("The source states the replay contract directly.".into()),
        },
    };
    let claim_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Create source-backed claim".into(),
        body: "Events JSONL is the source of truth for replay.".into(),
        actor,
        target_node_id: Some("concept-agent-maintained-knowledge-graph".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: vec!["concept-agent-maintained-knowledge-graph".into()],
        evidence_refs: vec!["ev-agent-loop-2".into()],
        proposal_payload: Some(claim_payload),
    })
    .expect("propose new claim payload");
    assert_eq!(
        claim_response.event.event_type,
        BrainEventKind::ClaimProposed
    );
    assert!(claim_response
        .event
        .payload_json
        .contains("\"changeType\":\"new_claim\""));

    let memory_payload_json = json!({
        "changeType": "new_memory",
        "memory": {
            "title": "Events are replayable",
            "body": "Append-only events are the source of truth for graph replay.",
            "sourcePath": "sources/agent-loop.md",
            "memoryId": "memory-events-replayable",
            "sourceRefs": ["source-agent-loop"],
            "evidenceRefs": ["ev-agent-loop-3"],
            "reason": "The source defines events JSONL as the replay log."
        }
    });
    let memory_payload: AgentGraphProposalPayload =
        serde_json::from_value(memory_payload_json).expect("parse new_memory payload");
    let memory_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Memory,
        title: "Create replay memory".into(),
        body: "Append-only events are the source of truth for graph replay.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
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
        proposal_payload: Some(memory_payload),
    })
    .expect("propose new memory payload");
    assert_eq!(
        memory_response.proposal.status,
        BrainProposalStatus::Accepted
    );
    assert_eq!(memory_response.event.policy_result, "auto_applied");
    assert!(memory_response
        .event
        .payload_json
        .contains("\"changeType\":\"new_memory\""));
    let memories: Vec<MemoryRecord> = read_json_artifact(
        &temp
            .path()
            .join(DEFAULT_WORKSPACE_ID)
            .join("memory/records.json"),
    )
    .expect("read materialized memories");
    assert!(memories.iter().any(|memory| {
        memory.memory_id == "memory-events-replayable"
            && memory.title == "Events are replayable"
            && memory.source_refs == vec!["source-agent-loop".to_string()]
            && memory.evidence_refs == vec!["ev-agent-loop-3".to_string()]
    }));

    let invalid = validate_brain_update_proposal(&ProposeBrainUpdateRequest {
        scope,
        kind: BrainProposalKind::Claim,
        title: "Invalid claim".into(),
        body: "Missing source path should fail.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: vec!["concept-agent-maintained-knowledge-graph".into()],
        evidence_refs: vec!["ev-agent-loop-2".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Events JSONL is the source of truth for replay.".into(),
                source_path: " ".into(),
                claim_id: None,
                topic_refs: vec!["concept-agent-maintained-knowledge-graph".into()],
                source_refs: vec!["source-agent-loop".into()],
                evidence_refs: vec!["ev-agent-loop-2".into()],
                reason: None,
            },
        }),
    });
    assert!(invalid
        .expect_err("invalid payload should fail")
        .to_string()
        .contains("sourcePath"));
}

#[test]
fn accepted_agent_node_and_claim_proposals_materialize_graph_and_wiki() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Agent graph loop\n\n## Page 1\n\nAutonomous graph proposals create durable source-backed graph state.\n",
        );
    let markdown_path = temp.path().join("sample.md");
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let concept_evidence_ids = project
        .details_by_node_id
        .get(&concept_node_id)
        .expect("concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };

    let node_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Node,
        title: "Create autonomous graph loop node".into(),
        body: "Agent proposals should materialize graph nodes without a review gate.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Autonomous Graph Loop".into(),
                kind: BrainNodeKind::Concept,
                source_path: manifest.markdown_path.clone(),
                node_id: Some("concept-autonomous-graph-loop".into()),
                aliases: vec!["Agent graph loop".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some("The source describes autonomous graph proposals.".into()),
            },
        }),
    })
    .expect("auto-apply node proposal");
    assert_eq!(node_response.proposal.status, BrainProposalStatus::Accepted);
    assert_eq!(node_response.event.policy_result, "auto_applied");

    let nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("read nodes");
    assert!(nodes.iter().any(|node| {
        node.node_id == "concept-autonomous-graph-loop"
            && node.label == "Autonomous Graph Loop"
            && node.source_ids == vec![manifest.source_id.clone()]
            && node.evidence_ids == concept_evidence_ids
    }));
    let index = fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read index");
    assert!(index.contains("[Autonomous Graph Loop](topics/concept-autonomous-graph-loop.md)"));
    assert!(workspace_root
        .join("wiki/topics/concept-autonomous-graph-loop.md")
        .exists());

    let edge_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Link,
        title: "Connect source concept to agent-maintained node".into(),
        body: "The autonomous ingest agent can apply validated new edge changes.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: Vec::new(),
        proposal_payload: Some(AgentGraphProposalPayload::NewEdge {
            edge: AgentNewEdgePayload {
                source_node_id: concept_node_id.clone(),
                target_node_id: "concept-autonomous-graph-loop".into(),
                kind: BrainRelationKind::RelatedTo,
                label: "Related to".into(),
                source_path: manifest.markdown_path.clone(),
                edge_id: Some("relation-source-autonomous-graph-loop".into()),
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some("Both nodes are grounded in the same source evidence.".into()),
            },
        }),
    })
    .expect("auto-apply edge proposal");
    assert_eq!(edge_response.proposal.status, BrainProposalStatus::Accepted);
    assert_eq!(edge_response.event.policy_result, "auto_applied");
    assert_eq!(edge_response.event.event_type, BrainEventKind::LinkProposed);
    assert_eq!(
        edge_response.event.relation_refs,
        vec!["relation-source-autonomous-graph-loop".to_string()]
    );
    assert!(edge_response
        .event
        .payload_json
        .contains("\"changeType\":\"new_edge\""));

    let edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json")).expect("read edges");
    assert!(edges.iter().any(|edge| {
        edge.relation_id == "relation-source-autonomous-graph-loop"
            && edge.source_node_id == concept_node_id
            && edge.target_node_id == "concept-autonomous-graph-loop"
            && edge.kind == BrainRelationKind::RelatedTo
            && edge.label == "Related to"
            && edge.evidence_ids == concept_evidence_ids
    }));
    let topic_after_edge =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-autonomous-graph-loop.md"))
            .expect("read topic page after edge");
    assert!(topic_after_edge.contains("## Relations"));
    assert!(topic_after_edge.contains("relation-source-autonomous-graph-loop"));
    assert!(topic_after_edge.contains("Autonomous Graph Loop"));
    assert!(topic_after_edge.contains("sources: source-test"));
    assert!(topic_after_edge.contains(&format!("]({}.md)", sanitize_name(&concept_node_id))));
    let source_topic_after_edge = fs::read_to_string(workspace_root.join(format!(
        "wiki/topics/{}.md",
        sanitize_name(&concept_node_id)
    )))
    .expect("read source topic page after edge");
    assert!(source_topic_after_edge.contains("## Relations"));
    assert!(source_topic_after_edge
        .contains("[Autonomous Graph Loop](concept-autonomous-graph-loop.md)"));

    let claim_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Attach source-backed graph claim".into(),
        body: "The typed claim payload should become the materialized claim.".into(),
        actor,
        target_node_id: Some("concept-autonomous-graph-loop".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec!["concept-autonomous-graph-loop".into()],
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Autonomous graph proposals create durable source-backed graph state."
                    .into(),
                source_path: manifest.markdown_path.clone(),
                claim_id: Some("claim-autonomous-graph-loop-state".into()),
                topic_refs: vec!["concept-autonomous-graph-loop".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some("The source states the durable graph behavior directly.".into()),
            },
        }),
    })
    .expect("auto-apply claim proposal");
    assert_eq!(
        claim_response.proposal.status,
        BrainProposalStatus::Accepted
    );
    assert_eq!(claim_response.event.policy_result, "auto_applied");

    let claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json")).expect("read claims");
    assert!(claims.iter().any(|claim| {
        claim.claim_id == "claim-autonomous-graph-loop-state"
            && claim.statement
                == "Autonomous graph proposals create durable source-backed graph state."
            && claim.topic_refs == vec!["concept-autonomous-graph-loop".to_string()]
            && claim.source_refs == vec![manifest.source_id.clone()]
            && claim.evidence_refs == concept_evidence_ids
            && claim.status == "supported"
    }));
    let duplicate_claim_response = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Re-ingest existing source-backed graph claim".into(),
        body: "The duplicate typed claim payload should reuse the materialized claim.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: Some("concept-autonomous-graph-loop".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec!["concept-autonomous-graph-loop".into()],
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Autonomous graph proposals create durable source-backed graph state."
                    .into(),
                source_path: manifest.markdown_path.clone(),
                claim_id: Some("claim-autonomous-graph-loop-state-rerun".into()),
                topic_refs: vec!["concept-autonomous-graph-loop".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some("A repeated markdown ingest saw the same claim again.".into()),
            },
        }),
    })
    .expect("auto-apply duplicate claim proposal");
    assert_eq!(
        duplicate_claim_response.proposal.status,
        BrainProposalStatus::Accepted
    );
    let claims_after_duplicate: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read claims after duplicate");
    let duplicate_statement_claims = claims_after_duplicate
        .iter()
        .filter(|claim| {
            claim.statement
                == "Autonomous graph proposals create durable source-backed graph state."
                && claim.topic_refs == vec!["concept-autonomous-graph-loop".to_string()]
        })
        .collect::<Vec<_>>();
    assert_eq!(duplicate_statement_claims.len(), 1);
    assert_eq!(
        duplicate_statement_claims[0].claim_id,
        "claim-autonomous-graph-loop-state"
    );
    assert!(!claims_after_duplicate
        .iter()
        .any(|claim| claim.claim_id == "claim-autonomous-graph-loop-state-rerun"));
    let topic =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-autonomous-graph-loop.md"))
            .expect("read topic page");
    assert!(topic.contains("## Claims"));
    assert!(topic.contains("## Relations"));
    assert!(topic.contains("relation-source-autonomous-graph-loop"));
    assert!(topic.contains("Autonomous graph proposals create durable source-backed graph state."));

    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read graph mutation events");
    let applied_graph_mutations = events
        .iter()
        .filter(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event.policy_result == "auto_applied"
                && event.actor.actor_id == "duckdocs-agent-ingest"
        })
        .collect::<Vec<_>>();
    assert!(applied_graph_mutations.len() >= 3);
    assert!(applied_graph_mutations.iter().any(|event| {
        event.payload_json.contains("\"mutationType\":\"new_node\"")
            && event
                .node_refs
                .contains(&"concept-autonomous-graph-loop".to_string())
    }));
    assert!(applied_graph_mutations.iter().any(|event| {
        event.payload_json.contains("\"mutationType\":\"new_edge\"")
            && event
                .relation_refs
                .contains(&"relation-source-autonomous-graph-loop".to_string())
    }));
    assert!(applied_graph_mutations.iter().any(|event| {
        event
            .payload_json
            .contains("\"mutationType\":\"new_claim\"")
            && event
                .node_refs
                .contains(&"concept-autonomous-graph-loop".to_string())
    }));

    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("replay accepted graph proposals");
    let replayed_edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json")).expect("read replayed edges");
    assert_eq!(
        replayed_edges
            .iter()
            .filter(|edge| edge.relation_id == "relation-source-autonomous-graph-loop")
            .count(),
        1
    );
    let replayed_topic =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-autonomous-graph-loop.md"))
            .expect("read replayed topic page");
    assert!(replayed_topic.contains("## Relations"));
    assert!(replayed_topic.contains("relation-source-autonomous-graph-loop"));
    assert!(replayed_topic.contains("sources: source-test"));
    assert!(replayed_topic.contains(&format!("]({}.md)", sanitize_name(&concept_node_id))));
}

#[test]
fn accepted_wiki_page_proposal_preserves_existing_user_authored_page() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (project, manifest) = compile_manifest_fixture_project(
        &temp,
        "# Agent graph loop\n\n## Page 1\n\nAgent save-back wiki pages remain recoverable.\n",
    );
    let request = CompileProjectRequest {
        source_markdown_path: temp.path().join("sample.md").display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let existing_page = workspace_root.join("wiki/save-back/agent-save-back.md");
    fs::create_dir_all(existing_page.parent().expect("save-back parent"))
        .expect("create save-back dir");
    fs::write(
        &existing_page,
        "# Agent save-back\n\nUser-authored page must stay intact.\n",
    )
    .expect("write user-authored page");
    let existing_before =
        fs::read_to_string(&existing_page).expect("read user-authored page before");
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let concept_evidence_ids = project
        .details_by_node_id
        .get(&concept_node_id)
        .expect("concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let wiki = handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::WikiPage,
            title: "Agent save-back".into(),
            body: "Agent-authored wiki page content should persist without overwriting user-authored markdown.".into(),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "duckdocs-agent-ingest".into(),
            },
            target_node_id: Some(concept_node_id.clone()),
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![concept_node_id],
            evidence_refs: concept_evidence_ids,
            proposal_payload: None,
        })
        .expect("propose wiki page");

    handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
        scope,
        proposal_id: wiki.proposal.proposal_id.clone(),
        decision: BrainReviewDecision::Accept,
        actor: BrainActor {
            actor_type: BrainActorType::User,
            actor_id: "local-user".into(),
        },
        reason: Some("Accept agent save-back page.".into()),
    })
    .expect("accept wiki page");

    assert_eq!(
        fs::read_to_string(&existing_page).expect("read user-authored page after"),
        existing_before
    );
    let manifest_snapshot: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read brain manifest");
    let saved_page = manifest_snapshot
        .wiki_pages
        .iter()
        .find(|page| {
            page.title == "Agent save-back" && page.path != "wiki/save-back/agent-save-back.md"
        })
        .expect("collision-safe saved page");
    let saved_body =
        fs::read_to_string(workspace_root.join(&saved_page.path)).expect("read saved page");
    assert!(saved_body.contains(
            "Agent-authored wiki page content should persist without overwriting user-authored markdown."
        ));

    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("rematerialize workspace brain repo");
    assert_eq!(
        fs::read_to_string(&existing_page).expect("read user-authored page after replay"),
        existing_before
    );
    assert!(fs::read_to_string(workspace_root.join(&saved_page.path))
            .expect("read saved page after replay")
            .contains(
                "Agent-authored wiki page content should persist without overwriting user-authored markdown."
            ));
}

#[test]
fn persisted_mutation_events_reconstruct_current_and_prior_graph_states() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Replayable graph\n\n## Page 1\n\nReplayable graph events rebuild nodes edges claims and memories.\n",
        );
    let request = CompileProjectRequest {
        source_markdown_path: temp.path().join("sample.md").display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("source concept node")
        .id
        .clone();
    let evidence_ids = project
        .details_by_node_id
        .get(&source_node_id)
        .expect("source concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };

    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Node,
        title: "Create replay node".into(),
        body: "Replay should rebuild this node from persisted events.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Replayable Graph State".into(),
                kind: BrainNodeKind::Concept,
                source_path: manifest.markdown_path.clone(),
                node_id: Some("concept-replayable-graph-state".into()),
                aliases: Vec::new(),
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: evidence_ids.clone(),
                reason: Some("The fixture source describes replayable graph state.".into()),
            },
        }),
    })
    .expect("auto-apply replay node");
    std::thread::sleep(std::time::Duration::from_secs(1));
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Link,
        title: "Connect replay node".into(),
        body: "Replay should rebuild this edge only after its event.".into(),
        actor: actor.clone(),
        target_node_id: Some("concept-replayable-graph-state".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec!["concept-replayable-graph-state".into()],
        evidence_refs: Vec::new(),
        proposal_payload: Some(AgentGraphProposalPayload::NewEdge {
            edge: AgentNewEdgePayload {
                source_node_id: "concept-replayable-graph-state".into(),
                target_node_id: "concept-replayable-graph-state".into(),
                kind: BrainRelationKind::RelatedTo,
                label: "Replays to".into(),
                source_path: manifest.markdown_path.clone(),
                edge_id: Some("relation-replayable-graph-state".into()),
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: evidence_ids.clone(),
                reason: Some("Replay has to preserve relation endpoints.".into()),
            },
        }),
    })
    .expect("auto-apply replay edge");
    std::thread::sleep(std::time::Duration::from_secs(1));
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Attach replay claim".into(),
        body: "Replay should rebuild this claim only after its event.".into(),
        actor: actor.clone(),
        target_node_id: Some("concept-replayable-graph-state".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec!["concept-replayable-graph-state".into()],
        evidence_refs: evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Persisted graph mutation events reconstruct prior graph states.".into(),
                source_path: manifest.markdown_path.clone(),
                claim_id: Some("claim-replayable-graph-state".into()),
                topic_refs: vec!["concept-replayable-graph-state".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: evidence_ids.clone(),
                reason: Some("The replay fixture proves prior-state reconstruction.".into()),
            },
        }),
    })
    .expect("auto-apply replay claim");
    std::thread::sleep(std::time::Duration::from_secs(1));
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Memory,
        title: "Remember replay state".into(),
        body: "Replayable graph events can restore materialized memories.".into(),
        actor,
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewMemory {
            memory: AgentNewMemoryPayload {
                title: "Replay state is restorable".into(),
                body: "Persisted mutation events restore graph memories during replay.".into(),
                source_path: manifest.markdown_path.clone(),
                memory_id: Some("memory-replayable-graph-state".into()),
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: evidence_ids.clone(),
                reason: Some("Memory replay is part of the materialized state.".into()),
            },
        }),
    })
    .expect("auto-apply replay memory");

    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read persisted events");
    let node_materialized_event = events
        .iter()
        .find(|event| {
            event.event_type == BrainEventKind::GraphMaterialized
                && event.operation_type.as_deref() == Some("new_node")
                && event
                    .target_node_ids
                    .contains(&"concept-replayable-graph-state".to_string())
        })
        .expect("node materialized event");
    let prior_output = temp.path().join("replay-prior");
    let prior = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: scope.clone(),
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: Some(node_materialized_event.event_id.clone()),
        output_root: Some(prior_output.display().to_string()),
        write_materialized: false,
    })
    .expect("reconstruct prior state");
    assert_eq!(
        prior.selected_event_id.as_deref(),
        Some(node_materialized_event.event_id.as_str())
    );
    assert!(prior.snapshot.nodes.iter().any(|node| {
        node.node_id == "concept-replayable-graph-state" && node.label == "Replayable Graph State"
    }));
    assert!(!prior
        .snapshot
        .relations
        .iter()
        .any(|edge| edge.relation_id == "relation-replayable-graph-state"));
    assert!(!prior
        .snapshot
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-replayable-graph-state"));
    assert!(!prior
        .snapshot
        .memories
        .iter()
        .any(|memory| memory.memory_id == "memory-replayable-graph-state"));
    assert!(fs::read_to_string(prior_output.join("wiki/index.md"))
        .expect("read prior replay wiki index")
        .contains("[Replayable Graph State](topics/concept-replayable-graph-state.md)"));
    assert!(!fs::read_to_string(
        prior_output.join("wiki/topics/concept-replayable-graph-state.md")
    )
    .expect("read prior replay topic")
    .contains("Persisted graph mutation events reconstruct prior graph states."));

    let full_output = temp.path().join("replay-current");
    let full = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: scope.clone(),
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: None,
        output_root: Some(full_output.display().to_string()),
        write_materialized: false,
    })
    .expect("reconstruct full state");
    let current_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("current nodes");
    let current_edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json")).expect("current edges");
    let current_claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json")).expect("current claims");
    let current_memories = read_memory_records(&workspace_root).expect("current memories");
    assert_eq!(full.snapshot.nodes, current_nodes);
    assert_eq!(full.snapshot.relations, current_edges);
    assert_eq!(full.snapshot.claims, current_claims);
    assert_eq!(full.snapshot.memories, current_memories);
    assert!(full
        .changed_files
        .iter()
        .any(|path| path == "events/brain_events.jsonl"));
    assert!(fs::read_to_string(full_output.join("graph/nodes.json"))
        .expect("read replayed nodes")
        .contains("concept-replayable-graph-state"));
    assert!(
        fs::read_to_string(full_output.join("wiki/topics/concept-replayable-graph-state.md"))
            .expect("read replayed topic")
            .contains("Persisted graph mutation events reconstruct prior graph states.")
    );
    assert_eq!(
        read_brain_events_jsonl(&full_output.join("events/brain_events.jsonl"))
            .expect("read replayed event log")
            .len(),
        full.replayed_event_count
    );

    let rollback = handle_reconstruct_brain(ReconstructBrainRequest {
        scope: scope.clone(),
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: Some(node_materialized_event.event_id.clone()),
        output_root: Some(temp.path().join("rollback-preview").display().to_string()),
        write_materialized: true,
    })
    .expect("apply rollback to prior graph state");
    assert!(rollback
        .changed_files
        .iter()
        .any(|path| path == "events/brain_events.jsonl"));
    assert!(rollback
        .changed_files
        .iter()
        .any(|path| path == "graph/edges.json"));
    assert!(workspace_root.join("snapshots").exists());
    let rolled_back_edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json"))
            .expect("read rolled back edges");
    assert!(!rolled_back_edges
        .iter()
        .any(|edge| edge.relation_id == "relation-replayable-graph-state"));
    let rolled_back_claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read rolled back claims");
    assert!(!rolled_back_claims
        .iter()
        .any(|claim| claim.claim_id == "claim-replayable-graph-state"));
    let rolled_back_memories = read_memory_records(&workspace_root).expect("rolled memories");
    assert!(!rolled_back_memories
        .iter()
        .any(|memory| memory.memory_id == "memory-replayable-graph-state"));
    let rolled_events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read rollback events");
    assert_eq!(rolled_events.len(), events.len() + 1);
    assert_eq!(&rolled_events[..events.len()], events.as_slice());
    let rollback_event = rolled_events.last().expect("rollback event");
    assert_eq!(rollback_event.event_type, BrainEventKind::GraphMaterialized);
    assert_eq!(
        rollback_event.operation_type.as_deref(),
        Some("graph_rollback")
    );
    assert_eq!(rollback_event.policy_result, "rollback_applied");
    assert!(rollback_event
        .causality
        .caused_by_event_ids
        .contains(&node_materialized_event.event_id));
    let rollback_payload: serde_json::Value =
        serde_json::from_str(&rollback_event.payload_json).expect("rollback payload json");
    assert_eq!(
        rollback_payload["rollback"]["restoredSnapshotId"],
        rollback.snapshot_id
    );
    assert_eq!(
        rollback_payload["rollback"]["selectedEventId"],
        node_materialized_event.event_id
    );
    assert!(rollback_payload["rollback"]["preRollbackSnapshotId"]
        .as_str()
        .is_some_and(|snapshot_id| snapshot_id.starts_with("snapshot-pre-rollback-")));
    assert_eq!(
        rollback_payload["diff"]["removedEdgeIds"],
        serde_json::json!(["relation-replayable-graph-state"])
    );
    assert_eq!(
        rollback_payload["diff"]["removedClaimIds"],
        serde_json::json!(["claim-replayable-graph-state"])
    );
    assert_eq!(
        rollback_payload["diff"]["removedMemoryIds"],
        serde_json::json!(["memory-replayable-graph-state"])
    );
    assert_eq!(
        rollback_payload["rollback"]["sourceEventCount"],
        serde_json::json!(rollback.replayed_event_count)
    );
    let history = handle_read_graph_history(ReadGraphHistoryRequest {
        scope: scope.clone(),
        limit: Some(20),
    })
    .expect("read rollback history");
    assert_eq!(
        history
            .states
            .first()
            .expect("latest rollback history entry")
            .rollback_target
            .event_id,
        rollback_event.event_id
    );
    assert!(history.states.iter().any(|state| {
        state.rollback_target.event_id == node_materialized_event.event_id
            && state.source_run_ids.contains(&manifest.source_id)
    }));

    let replay_after_rollback = handle_reconstruct_brain(ReconstructBrainRequest {
        scope,
        up_to_timestamp: None,
        up_to_materialized_version: None,
        up_to_event_id: None,
        output_root: Some(
            temp.path()
                .join("replay-after-rollback")
                .display()
                .to_string(),
        ),
        write_materialized: false,
    })
    .expect("replay after rollback");
    assert!(!replay_after_rollback
        .snapshot
        .relations
        .iter()
        .any(|edge| edge.relation_id == "relation-replayable-graph-state"));
    assert!(!replay_after_rollback
        .snapshot
        .claims
        .iter()
        .any(|claim| claim.claim_id == "claim-replayable-graph-state"));
    assert!(!replay_after_rollback
        .snapshot
        .memories
        .iter()
        .any(|memory| memory.memory_id == "memory-replayable-graph-state"));
}

#[test]
fn invalid_agent_graph_proposal_is_rejected_before_writing_audit_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let error = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Node,
        title: "Invalid node".into(),
        body: "Missing evidence refs should reject the proposal before writes.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: Vec::new(),
        evidence_refs: Vec::new(),
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Invalid Agent Node".into(),
                kind: BrainNodeKind::Concept,
                source_path: "sources/agent-loop.md".into(),
                node_id: Some("concept-invalid-agent-node".into()),
                aliases: Vec::new(),
                source_refs: vec!["source-agent-loop".into()],
                evidence_refs: Vec::new(),
                reason: None,
            },
        }),
    })
    .expect_err("invalid node proposal should fail");

    assert!(format!("{error:#}").contains("evidenceRefs"));
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    assert!(!workspace_root.join("events/brain_events.jsonl").exists());
    assert!(!workspace_root.join("reviews/proposed-updates").exists());
    assert!(!workspace_root.join("graph/nodes.json").exists());

    let mismatch = validate_brain_update_proposal(&ProposeBrainUpdateRequest {
        scope,
        kind: BrainProposalKind::Claim,
        title: "Mismatched payload".into(),
        body: "A claim cannot carry a new node payload.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec!["source-agent-loop".into()],
        node_refs: vec!["concept-agent-loop".into()],
        evidence_refs: vec!["ev-agent-loop-1".into()],
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Wrong Payload".into(),
                kind: BrainNodeKind::Concept,
                source_path: "sources/agent-loop.md".into(),
                node_id: None,
                aliases: Vec::new(),
                source_refs: vec!["source-agent-loop".into()],
                evidence_refs: vec!["ev-agent-loop-1".into()],
                reason: None,
            },
        }),
    })
    .expect_err("mismatched payload should fail");
    assert!(format!("{mismatch:#}").contains("requires kind=node"));
    let validation_error = mismatch
        .downcast_ref::<AgentGraphProposalValidationError>()
        .expect("structured validation error");
    assert!(validation_error.issues.iter().any(|issue| {
        issue.code == AgentGraphProposalValidationCode::KindPayloadMismatch
            && issue.field == "proposalPayload.changeType"
    }));
}

#[test]
fn accepted_link_proposal_validates_node_ids_before_materializing_edge() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Sample import\n\n## Page 1\n\nAgent Graph Loop links source-backed claims.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let edges_before =
        fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges before");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let proposed = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Link,
        title: "Invalid relation edge".into(),
        body: "This relation should not materialize because its target node is missing.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: Some("missing-target-node".into()),
        target_source_id: None,
        relation_kind: Some(BrainRelationKind::RelatedTo),
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec![concept_node_id],
        evidence_refs: Vec::new(),
        proposal_payload: None,
    })
    .expect("write pending link proposal");
    assert_eq!(proposed.proposal.status, BrainProposalStatus::PendingReview);

    let error = handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
        scope: scope.clone(),
        proposal_id: proposed.proposal.proposal_id.clone(),
        decision: BrainReviewDecision::Accept,
        actor: BrainActor {
            actor_type: BrainActorType::User,
            actor_id: "local-user".into(),
        },
        reason: Some("Attempt to accept invalid edge.".into()),
    })
    .expect_err("invalid relation endpoint should block materialization");
    assert!(format!("{error:#}").contains("missing-target-node"));

    let persisted: BrainUpdateProposal =
        read_json_artifact(&PathBuf::from(&proposed.proposal_path)).expect("read pending proposal");
    assert_eq!(persisted.status, BrainProposalStatus::PendingReview);
    assert_eq!(
        fs::read_to_string(workspace_root.join("graph/edges.json")).expect("read edges after"),
        edges_before
    );
}

#[test]
fn brain_review_health_lists_pending_claims_and_links_only() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "claude-code".into(),
    };
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();

    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Memory,
        title: "Remember accepted memory".into(),
        body: "Safe memory proposals are auto-applied.".into(),
        actor: actor.clone(),
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
    })
    .expect("propose memory");
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Claim needs review".into(),
        body: "Durable claims require human attention before write-through.".into(),
        actor: actor.clone(),
        target_node_id: Some(concept_node_id.clone()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec![concept_node_id.clone()],
        evidence_refs: Vec::new(),
        proposal_payload: None,
    })
    .expect("propose claim");
    handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Link,
        title: "Link needs review".into(),
        body: "Typed graph links require review before graph mutation.".into(),
        actor,
        target_node_id: Some(concept_node_id.clone()),
        target_source_id: None,
        relation_kind: Some(BrainRelationKind::RelatedTo),
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id],
        node_refs: vec!["document".into()],
        evidence_refs: Vec::new(),
        proposal_payload: None,
    })
    .expect("propose link");

    let reviews = handle_list_brain_review_items(ListBrainReviewItemsRequest {
        scope: scope.clone(),
    })
    .expect("list reviews");
    assert_eq!(reviews.items.len(), 2);
    assert!(reviews
        .items
        .iter()
        .all(|item| item.status == BrainProposalStatus::PendingReview));
    assert!(reviews
        .items
        .iter()
        .any(|item| item.kind == BrainProposalKind::Claim));
    assert!(reviews
        .items
        .iter()
        .any(|item| item.kind == BrainProposalKind::Link));
    assert!(!reviews
        .items
        .iter()
        .any(|item| item.kind == BrainProposalKind::Memory));

    let health =
        handle_get_brain_health(GetBrainHealthRequest { scope }).expect("get brain health");
    assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
    assert_eq!(health.attention_count, 2);
    assert_eq!(health.review_items.len(), 2);
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
fn brain_maintenance_repairs_generated_index_log_and_structured_artifacts() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::write(workspace_root.join("wiki/index.md"), "# Broken\n").expect("break index");
    fs::write(workspace_root.join("wiki/log.md"), "# Broken log\n").expect("break log");
    fs::remove_file(workspace_root.join("graph/nodes.json")).expect("remove nodes");
    fs::remove_file(workspace_root.join("graph/evidence.json")).expect("remove evidence");
    fs::remove_file(workspace_root.join("graph/claims.json")).expect("remove claims");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let health =
        handle_get_brain_health(GetBrainHealthRequest { scope }).expect("health runs maintenance");
    assert_eq!(health.status, BrainHealthStatus::Clean);
    assert!(fs::read_to_string(workspace_root.join("wiki/index.md"))
        .expect("read repaired index")
        .contains("Brain Index"));
    assert!(fs::read_to_string(workspace_root.join("wiki/log.md"))
        .expect("read repaired log")
        .contains("graph-materialized"));
    let claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json")).expect("read claims");
    assert!(!claims.is_empty());
    let nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("read nodes");
    assert!(!nodes.is_empty());
    let evidence: Vec<EvidenceRef> =
        read_json_artifact(&workspace_root.join("graph/evidence.json")).expect("read evidence");
    assert!(!evidence.is_empty());
    let report: BrainMaintenanceReport =
        read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
            .expect("read lint report");
    assert!(report.repair_count >= 5);
    assert!(report.repairs.contains(&"wiki/index.md".into()));
    assert!(report.repairs.contains(&"wiki/log.md".into()));
    assert!(report.repairs.contains(&"graph/nodes.json".into()));
    assert!(report.repairs.contains(&"graph/evidence.json".into()));
    assert!(report.repairs.contains(&"graph/claims.json".into()));
}

#[test]
fn brain_maintenance_promotes_risky_lint_findings_to_health_review() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let claims_path = workspace_root.join("graph/claims.json");
    let mut claims: Vec<ClaimRecord> =
        read_json_artifact(&claims_path).expect("read materialized claims");
    claims.first_mut().expect("claim").evidence_refs.clear();
    write_json_pretty(&claims_path, &claims).expect("write broken claims");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let health = handle_get_brain_health(GetBrainHealthRequest {
        scope: scope.clone(),
    })
    .expect("health runs maintenance");
    assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
    assert!(health
        .review_items
        .iter()
        .any(|item| item.title.contains("Claim needs evidence")));
    let report: BrainMaintenanceReport =
        read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
            .expect("read lint report");
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.kind == "missing_evidence"));
    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope,
        limit: Some(20),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read maintenance events");
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::ReviewCreated));
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::BrainMaintenanceRun));
}

#[test]
fn brain_maintenance_repairs_graph_and_memory_refs_to_missing_wiki_pages() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let snapshot: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json")).expect("snapshot");
    let missing_wiki_path = snapshot
        .wiki_pages
        .iter()
        .find(|page| page.path.starts_with("wiki/topics/"))
        .expect("topic page")
        .path
        .clone();
    let missing_wiki_file = workspace_root.join(&missing_wiki_path);
    if missing_wiki_file.exists() {
        fs::remove_file(&missing_wiki_file).expect("remove topic page");
    }
    let mut memories = read_memory_records(&workspace_root).expect("read memories");
    memories.push(MemoryRecord {
        memory_id: "memory-missing-wiki-ref".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Missing wiki ref".into(),
        body: "Memory changes should not point at absent materialized wiki pages.".into(),
        source_refs: vec![missing_wiki_path.clone()],
        evidence_refs: Vec::new(),
        created_at: 10,
        updated_at: 10,
    });
    write_json_pretty(&workspace_root.join("memory/records.json"), &memories)
        .expect("write memory ref");

    let broken_snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read broken snapshot");
    let missing_issues = lint_missing_materialized_wiki_refs(&workspace_root, &broken_snapshot);
    assert!(missing_issues.iter().any(|issue| {
        issue.kind == "missing_wiki_page" && issue.source_refs.contains(&missing_wiki_path)
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
    assert!(report.repairs.contains(&missing_wiki_path));
    assert!(!report
        .issues
        .iter()
        .any(|issue| issue.kind == "missing_wiki_page"));
    assert!(!health
        .review_items
        .iter()
        .any(|item| item.title.contains("Wiki page is not materialized")));
    let recovered_page =
        fs::read_to_string(workspace_root.join(&missing_wiki_path)).expect("read stub");
    assert!(recovered_page.contains("Markdown-derived node"));
    assert!(recovered_page.contains("wiki pages materialized"));
    assert!(recovered_page.contains("## Origin Context"));
    assert!(recovered_page.contains("memory-missing-wiki-ref"));
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
fn brain_maintenance_detects_orphan_conflict_and_stale_cases() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);

    let manifest_path = workspace_root.join("brain-manifest.json");
    let mut snapshot: BrainRepoSnapshot =
        read_json_artifact(&manifest_path).expect("read brain manifest");
    snapshot.sources[0].status = "stale".into();
    snapshot.sources[0].updated_at = snapshot.generated_at + 1;
    write_json_pretty(&manifest_path, &snapshot).expect("write stale manifest");

    let nodes_path = workspace_root.join("graph/nodes.json");
    let mut nodes: Vec<BrainNodeRecord> = read_json_artifact(&nodes_path).expect("read nodes");
    nodes.push(BrainNodeRecord {
        node_id: "orphan-concept".into(),
        kind: BrainNodeKind::Concept,
        label: "Orphan Concept".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: Vec::new(),
        confidence: None,
        updated_at: snapshot.generated_at,
    });
    write_json_pretty(&nodes_path, &nodes).expect("write orphan node");

    let claims_path = workspace_root.join("graph/claims.json");
    let mut claims: Vec<ClaimRecord> = read_json_artifact(&claims_path).expect("read claims");
    let base_claim = claims.first().expect("base claim").clone();
    claims.push(ClaimRecord {
        claim_id: "claim-positive-agent-context".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Agent brain context is source backed".into(),
        topic_refs: base_claim.topic_refs.clone(),
        source_refs: base_claim.source_refs.clone(),
        evidence_refs: base_claim.evidence_refs.clone(),
        status: "supported".into(),
        updated_at: snapshot.generated_at,
    });
    claims.push(ClaimRecord {
        claim_id: "claim-negative-agent-context".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        statement: "Agent brain context is not source backed".into(),
        topic_refs: base_claim.topic_refs,
        source_refs: base_claim.source_refs,
        evidence_refs: base_claim.evidence_refs,
        status: "supported".into(),
        updated_at: snapshot.generated_at,
    });
    write_json_pretty(&claims_path, &claims).expect("write conflicting claims");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let health =
        handle_get_brain_health(GetBrainHealthRequest { scope }).expect("health runs maintenance");
    assert_eq!(health.status, BrainHealthStatus::AttentionNeeded);
    let report: BrainMaintenanceReport =
        read_json_artifact(&workspace_root.join("reviews/lint-reports/latest.json"))
            .expect("read lint report");
    assert!(report.issues.iter().any(|issue| issue.kind == "orphan"));
    assert!(report.issues.iter().any(|issue| issue.kind == "conflict"));
    assert!(report.issues.iter().any(|issue| issue.kind == "stale"));
    assert!(health
        .review_items
        .iter()
        .any(|item| item.kind == BrainProposalKind::Link));
    assert!(health
        .review_items
        .iter()
        .any(|item| item.title.contains("Claims may conflict")));
    assert!(health
        .review_items
        .iter()
        .any(|item| item.title.contains("Source may need recompilation")));
}

#[test]
fn resolving_brain_review_applies_save_back_artifacts() {
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
    };
    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    write_source_manifest(&manifest).expect("write source manifest");
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source-backed project");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let source_manifest_before =
        fs::read_to_string(&manifest.manifest_path).expect("read source manifest before");
    let memory_before = fs::read_to_string(workspace_root.join("memory/records.json"))
        .unwrap_or_else(|_| "[]".into());

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "claude-code".into(),
    };
    let concept_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let concept_evidence_ids = project
        .details_by_node_id
        .get(&concept_node_id)
        .expect("concept detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let node_proposal = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Node,
        title: "Create agent graph loop node".into(),
        body: "Autonomous ingest can add validated graph nodes.".into(),
        actor: actor.clone(),
        target_node_id: None,
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: Vec::new(),
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewNode {
            node: AgentNewNodePayload {
                label: "Agent Graph Loop".into(),
                kind: BrainNodeKind::Concept,
                source_path: manifest.markdown_path.clone(),
                node_id: Some("concept-agent-graph-loop".into()),
                aliases: vec!["Autonomous graph loop".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some("The parsed source introduces autonomous graph updates.".into()),
            },
        }),
    })
    .expect("auto-apply node proposal");
    assert_eq!(node_proposal.proposal.status, BrainProposalStatus::Accepted);
    assert_eq!(node_proposal.event.policy_result, "auto_applied");
    let nodes_after_node: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json"))
            .expect("read nodes after node proposal");
    assert!(nodes_after_node.iter().any(|node| {
        node.node_id == "concept-agent-graph-loop"
            && node.label == "Agent Graph Loop"
            && node.source_ids == vec![manifest.source_id.clone()]
            && node.evidence_ids == concept_evidence_ids
    }));
    let index_after_node =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki index");
    assert!(index_after_node.contains("[Agent Graph Loop](topics/concept-agent-graph-loop.md)"));
    assert!(workspace_root
        .join("wiki/topics/concept-agent-graph-loop.md")
        .exists());
    let auto_claim = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Attach claim to agent graph loop".into(),
        body: "This body should not override the typed claim payload.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "duckdocs-agent-ingest".into(),
        },
        target_node_id: Some("concept-agent-graph-loop".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec!["concept-agent-graph-loop".into()],
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
            claim: AgentNewClaimPayload {
                statement: "Validated new claim changes attach to referenced graph nodes.".into(),
                source_path: manifest.markdown_path.clone(),
                claim_id: Some("claim-agent-graph-loop-attachment".into()),
                topic_refs: vec!["concept-agent-graph-loop".into()],
                source_refs: vec![manifest.source_id.clone()],
                evidence_refs: concept_evidence_ids.clone(),
                reason: Some(
                    "The autonomous ingest agent validated the source-backed claim.".into(),
                ),
            },
        }),
    })
    .expect("auto-apply typed claim proposal");
    assert_eq!(auto_claim.proposal.status, BrainProposalStatus::Accepted);
    assert_eq!(auto_claim.event.policy_result, "auto_applied");
    assert_eq!(auto_claim.event.event_type, BrainEventKind::ClaimProposed);
    assert!(auto_claim
        .event
        .node_refs
        .contains(&"concept-agent-graph-loop".into()));
    let claims_after_auto_claim: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read claims after auto claim");
    assert!(claims_after_auto_claim.iter().any(|claim| {
        claim.claim_id == "claim-agent-graph-loop-attachment"
            && claim.statement == "Validated new claim changes attach to referenced graph nodes."
            && claim.topic_refs == vec!["concept-agent-graph-loop".to_string()]
            && claim.source_refs == vec![manifest.source_id.clone()]
            && claim.evidence_refs == concept_evidence_ids
            && claim.status == "supported"
    }));
    let topic_after_claim =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-agent-graph-loop.md"))
            .expect("read topic page after auto claim");
    assert!(topic_after_claim.contains("## Claims"));
    assert!(
        topic_after_claim.contains("Validated new claim changes attach to referenced graph nodes.")
    );
    let proposed = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::Claim,
        title: "Accepted source-backed claim".into(),
        body: "Accepted claim proposals become durable claim records.".into(),
        actor,
        target_node_id: Some(concept_node_id.clone()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec![concept_node_id.clone()],
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: None,
    })
    .expect("propose claim");
    assert_eq!(proposed.proposal.status, BrainProposalStatus::PendingReview);

    let human = BrainActor {
        actor_type: BrainActorType::User,
        actor_id: "local-user".into(),
    };
    let resolved_claim = handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
        scope: scope.clone(),
        proposal_id: proposed.proposal.proposal_id.clone(),
        decision: BrainReviewDecision::Accept,
        actor: human.clone(),
        reason: Some("Evidence is enough for the proposal ledger.".into()),
    })
    .expect("resolve claim review");
    assert_eq!(
        resolved_claim.proposal.status,
        BrainProposalStatus::Accepted
    );
    assert_eq!(
        resolved_claim.event.event_type,
        BrainEventKind::ReviewResolved
    );
    assert_eq!(resolved_claim.event.policy_result, "accept");

    let persisted: BrainUpdateProposal =
        read_json_artifact(&PathBuf::from(&resolved_claim.proposal_path)).expect("read proposal");
    assert_eq!(persisted.status, BrainProposalStatus::Accepted);
    let claims_after_accept: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read claims after accept");
    assert!(claims_after_accept.iter().any(|claim| {
        claim.statement == "Accepted claim proposals become durable claim records."
            && claim.topic_refs.contains(&concept_node_id)
            && claim.evidence_refs == concept_evidence_ids
    }));

    let wiki = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::WikiPage,
        title: "Accepted answer page".into(),
        body: "Accepted answer text becomes a durable wiki page.".into(),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "claude-code".into(),
        },
        target_node_id: Some(concept_node_id.clone()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec![concept_node_id.clone()],
        evidence_refs: concept_evidence_ids.clone(),
        proposal_payload: None,
    })
    .expect("propose wiki page");
    let resolved_wiki = handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
        scope: scope.clone(),
        proposal_id: wiki.proposal.proposal_id.clone(),
        decision: BrainReviewDecision::Accept,
        actor: human,
        reason: Some("Save answer as durable wiki page.".into()),
    })
    .expect("resolve wiki review");
    assert_eq!(resolved_wiki.proposal.status, BrainProposalStatus::Accepted);
    let saved_page = workspace_root.join("wiki/save-back/accepted-answer-page.md");
    assert!(saved_page.exists());
    assert!(fs::read_to_string(&saved_page)
        .expect("read saved wiki page")
        .contains("Accepted answer text becomes a durable wiki page."));

    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("rematerialize workspace brain repo");
    let rematerialized_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json"))
            .expect("read rematerialized nodes");
    assert!(rematerialized_nodes
        .iter()
        .any(|node| node.node_id == "concept-agent-graph-loop"));
    let rematerialized_claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read rematerialized claims");
    assert!(rematerialized_claims.iter().any(|claim| {
        claim.statement == "Accepted claim proposals become durable claim records."
    }));
    assert!(saved_page.exists());
    assert!(workspace_root
        .join("wiki/topics/concept-agent-graph-loop.md")
        .exists());

    let health = handle_get_brain_health(GetBrainHealthRequest {
        scope: scope.clone(),
    })
    .expect("get health after resolve");
    assert_eq!(health.status, BrainHealthStatus::Clean);
    assert_eq!(health.attention_count, 0);

    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope,
        limit: Some(20),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read events");
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::ReviewResolved));
    assert_eq!(
        fs::read_to_string(workspace_root.join("memory/records.json"))
            .unwrap_or_else(|_| "[]".into()),
        memory_before
    );
    assert_eq!(
        fs::read_to_string(&manifest.manifest_path).expect("read source manifest after"),
        source_manifest_before
    );
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
fn compile_project_uses_source_manifest_as_graph_node_backing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let markdown = "# Sample import\n\n## Page 1\n\nSource evidence belongs to the graph.\n";
    let markdown_path = temp.path().join("sample.md");
    fs::write(&markdown_path, markdown).expect("write markdown");
    let manifest = sample_manifest(&temp);
    let request = CompileProjectRequest {
        source_markdown_path: markdown_path.display().to_string(),
        source_document_path: Some("/tmp/source.pdf".into()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEFAULT_WORKSPACE_ID.into()),
        source_id: Some(manifest.source_id.clone()),
    };

    let project = compile_knowledge_project(&request, markdown, Some(&manifest));
    let source_node_id = source_node_id(&manifest.source_id);
    let source_detail = project
        .details_by_node_id
        .get(&source_node_id)
        .expect("source node detail");

    assert_eq!(source_detail.node.kind, GraphNodeKind::Source);
    assert_eq!(
        source_detail
            .source
            .as_ref()
            .map(|source| source.source_id.as_str()),
        Some("source-test")
    );
    assert!(source_detail
        .evidence
        .iter()
        .all(|evidence| evidence.source_id.as_deref() == Some("source-test")));
    assert!(project
        .edges
        .iter()
        .any(|edge| edge.source_node_id == source_node_id));
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
fn load_project_defaults_to_workspace_graph_aggregate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (project_a, manifest_a) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nShared Context Layer keeps agents grounded.\nAlpha Planning Notes mention source specific work.\n",
            "source-a",
            "alpha",
            10,
        );
    let (project_b, manifest_b) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source B\n\n## Page 1\n\nShared context layer keeps agents grounded.\nBeta Review Notes mention separate work.\n",
            "source-b",
            "beta",
            20,
        );
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save project a");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save project b");

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load workspace aggregate")
        .expect("workspace aggregate");
    assert_eq!(
        aggregate.summary.project_id,
        workspace_project_id(DEFAULT_WORKSPACE_ID)
    );
    assert_eq!(aggregate.summary.document_count, 2);
    assert!(aggregate
        .nodes
        .iter()
        .any(|node| node.id == "source:source-a"));
    assert!(aggregate
        .nodes
        .iter()
        .any(|node| node.id == "source:source-b"));

    let shared = aggregate
        .details_by_node_id
        .values()
        .find(|detail| {
            normalize_key(&detail.canonical_name) == "shared-context-layer-keeps-agents-grounded"
        })
        .expect("shared aggregate concept");
    assert!(shared
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-a")));
    assert!(shared
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-b")));
    assert!(aggregate.edges.iter().any(|edge| {
        edge.kind == RelationKind::SourceDocument
            && edge.source_node_id == "source:source-a"
            && edge.target_node_id == shared.node.id
    }));
    assert!(aggregate.edges.iter().any(|edge| {
        edge.kind == RelationKind::SourceDocument
            && edge.source_node_id == "source:source-b"
            && edge.target_node_id == shared.node.id
    }));

    let loaded_source_project = store
        .load_project(Some(&project_a.summary.project_id))
        .expect("load exact project")
        .expect("exact source project");
    assert_eq!(
        loaded_source_project.summary.project_id,
        project_a.summary.project_id
    );
}

#[test]
fn workspace_aggregate_smoke_uses_real_multi_source_markdown_fixtures() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("multisource");
    let fixture_a_path = fixture_root.join("agent-context.md");
    let fixture_b_path = fixture_root.join("review-notes.md");
    let markdown_a = fs::read_to_string(&fixture_a_path).expect("read agent context fixture");
    let markdown_b = fs::read_to_string(&fixture_b_path).expect("read review notes fixture");
    let mut manifest_a =
        sample_manifest_with_source(&temp, "source-agent-context", "agent-context", 10);
    let mut manifest_b =
        sample_manifest_with_source(&temp, "source-review-notes", "review-notes", 20);
    manifest_a.markdown_path = fixture_a_path.display().to_string();
    manifest_b.markdown_path = fixture_b_path.display().to_string();
    manifest_a.pages = multi_source_fixture_pages(&temp, "source-agent-context");
    manifest_b.pages = multi_source_fixture_pages(&temp, "source-review-notes");

    for (markdown, fixture_path, manifest) in [
        (&markdown_a, &fixture_a_path, &manifest_a),
        (&markdown_b, &fixture_b_path, &manifest_b),
    ] {
        let request = CompileProjectRequest {
            source_markdown_path: fixture_path.display().to_string(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        let project = compile_knowledge_project(&request, markdown, Some(manifest));
        store
            .save_project(&project, &request, Some(manifest))
            .expect("save source-backed fixture project");
    }

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");

    assert_eq!(aggregate.summary.document_count, 2);
    assert!(aggregate
        .nodes
        .iter()
        .any(|node| node.id == "source:source-agent-context"));
    assert!(aggregate
        .nodes
        .iter()
        .any(|node| node.id == "source:source-review-notes"));

    let shared = aggregate
        .details_by_node_id
        .values()
        .find(|detail| {
            normalize_key(&detail.canonical_name) == "shared-team-context-layer-keeps-agents"
        })
        .expect("shared team context layer concept");
    for source_id in ["source-agent-context", "source-review-notes"] {
        assert!(shared
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some(source_id)));
    }
    assert!(shared.evidence.iter().any(|evidence| {
        evidence
            .markdown_path
            .as_deref()
            .is_some_and(|path| path.ends_with("page_1.md"))
            && evidence
                .image_path
                .as_deref()
                .is_some_and(|path| path.ends_with("page_1.png"))
    }));
    assert!(aggregate.edges.iter().any(|edge| {
        edge.kind == RelationKind::RelatedTo
            && (edge.source_node_id == shared.node.id || edge.target_node_id == shared.node.id)
            && edge.evidence_count > 0
    }));
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
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: markdown_path_b.display().to_string(),
        source_document_path: Some(shared_original),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
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
fn workspace_aggregate_merges_concepts_by_alias_identity() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nFoo keeps project knowledge grounded.\n",
        "source-a",
        "alpha",
        20,
    );
    let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nBar keeps project knowledge grounded.\n",
        "source-b",
        "beta",
        10,
    );
    let source_a_concept_id = project_a
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("source a concept")
        .id
        .clone();
    let source_a_node = project_a
        .nodes
        .iter_mut()
        .find(|node| node.id == source_a_concept_id)
        .expect("source a concept node");
    source_a_node.label = "Foo".into();
    let source_a_detail = project_a
        .details_by_node_id
        .get_mut(&source_a_concept_id)
        .expect("source a concept detail");
    source_a_detail.canonical_name = "Foo".into();
    source_a_detail.aliases = vec!["Bar".into()];
    source_a_detail.node.label = "Foo".into();
    let source_b_concept_id = project_b
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("source b concept")
        .id
        .clone();
    let source_b_node = project_b
        .nodes
        .iter_mut()
        .find(|node| node.id == source_b_concept_id)
        .expect("source b concept node");
    source_b_node.label = "Bar".into();
    let source_b_detail = project_b
        .details_by_node_id
        .get_mut(&source_b_concept_id)
        .expect("source b concept detail");
    source_b_detail.canonical_name = "Bar".into();
    source_b_detail.node.label = "Bar".into();
    let request_a = CompileProjectRequest {
        source_markdown_path: manifest_a.markdown_path.clone(),
        source_document_path: Some(manifest_a.source_path.clone()),
        source_manifest_path: Some(manifest_a.manifest_path.clone()),
        workspace_id: Some(manifest_a.workspace_id.clone()),
        source_id: Some(manifest_a.source_id.clone()),
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
    };
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
    let merged = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Foo")
        .expect("merged foo concept");
    assert!(merged.aliases.contains(&"Bar".into()));
    assert!(merged
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-a")));
    assert!(merged
        .evidence
        .iter()
        .any(|evidence| evidence.source_id.as_deref() == Some("source-b")));
    assert_eq!(
        aggregate
            .nodes
            .iter()
            .filter(|node| node.kind == GraphNodeKind::Concept)
            .count(),
        1
    );
}

#[test]
fn workspace_aggregate_merges_transitive_alias_groups() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("knowledge.sqlite3"));
    let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha concept keeps project knowledge grounded.\n",
        "source-a",
        "alpha",
        30,
    );
    let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nGamma concept keeps project knowledge grounded.\n",
        "source-b",
        "beta",
        20,
    );
    let (mut project_c, manifest_c) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source C\n\n## Page 1\n\nBeta concept bridges gamma evidence.\n",
        "source-c",
        "gamma",
        10,
    );

    rename_first_concept_for_test(&mut project_a, "Alpha", &["Beta"]);
    rename_first_concept_for_test(&mut project_b, "Gamma", &["Delta"]);
    rename_first_concept_for_test(&mut project_c, "Beta", &["Gamma"]);

    for (project, manifest) in [
        (&project_a, &manifest_a),
        (&project_b, &manifest_b),
        (&project_c, &manifest_c),
    ] {
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(project, &request, Some(manifest))
            .expect("save source project");
    }

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let concept_details = aggregate
        .details_by_node_id
        .values()
        .filter(|detail| detail.node.kind == GraphNodeKind::Concept)
        .collect::<Vec<_>>();
    assert_eq!(concept_details.len(), 1);
    let merged = concept_details[0];
    assert!(merged.aliases.contains(&"Beta".into()));
    assert!(merged.aliases.contains(&"Gamma".into()));
    for source_id in ["source-a", "source-b", "source-c"] {
        assert!(merged
            .evidence
            .iter()
            .any(|evidence| evidence.source_id.as_deref() == Some(source_id)));
    }
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
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project");

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    let response =
        handle_load_project(LoadProjectRequest::default()).expect("load project through handler");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
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
    };
    store
        .save_project(&project, &request, None)
        .expect("save legacy project");

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    let response =
        handle_load_project(LoadProjectRequest::default()).expect("load default project");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    assert_eq!(response.sources.len(), 0);
    assert_eq!(
        response.project.expect("legacy project").summary.project_id,
        project.summary.project_id
    );
}

#[test]
fn workspace_rename_correction_replays_to_source_snapshots_and_ledger() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (mut project_a, manifest_a) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nShared context layer keeps agents grounded.\n",
        "source-a",
        "alpha",
        10,
    );
    let (mut project_b, manifest_b) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source B\n\n## Page 1\n\nShared context layer keeps document evidence grounded.\n",
        "source-b",
        "beta",
        20,
    );
    rename_first_concept_for_test(&mut project_a, "Shared Context Layer", &["Agent Context"]);
    rename_first_concept_for_test(&mut project_b, "Shared Context Layer", &[]);

    for (project, manifest) in [(&project_a, &manifest_a), (&project_b, &manifest_b)] {
        let request = CompileProjectRequest {
            source_markdown_path: manifest.markdown_path.clone(),
            source_document_path: Some(manifest.source_path.clone()),
            source_manifest_path: Some(manifest.manifest_path.clone()),
            workspace_id: Some(manifest.workspace_id.clone()),
            source_id: Some(manifest.source_id.clone()),
        };
        store
            .save_project(project, &request, Some(manifest))
            .expect("save source project");
    }

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let aggregate_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Shared Context Layer")
        .expect("workspace concept")
        .clone();

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    let response = handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: aggregate_detail.node.id.clone(),
        kind: CorrectionKind::Rename,
        target_node_id: None,
        value: Some("Agent Brain Context".into()),
    })
    .expect("apply workspace rename correction");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    assert!(response
        .project
        .details_by_node_id
        .values()
        .any(|detail| detail.canonical_name == "Agent Brain Context"));
    assert!(response
        .project
        .details_by_node_id
        .contains_key("concept-agent-brain-context"));
    assert!(!response
        .project
        .details_by_node_id
        .contains_key(&aggregate_detail.node.id));
    assert!(response.project.edges.iter().all(|edge| {
        edge.source_node_id != aggregate_detail.node.id
            && edge.target_node_id != aggregate_detail.node.id
    }));
    for project_id in [&project_a.summary.project_id, &project_b.summary.project_id] {
        let source_project = store
            .load_project(Some(project_id))
            .expect("load source project")
            .expect("source project");
        assert!(source_project
            .details_by_node_id
            .contains_key("concept-agent-brain-context"));
        let renamed = source_project
            .details_by_node_id
            .values()
            .find(|detail| detail.canonical_name == "Agent Brain Context")
            .expect("renamed source concept");
        assert!(renamed.aliases.contains(&"Shared Context Layer".into()));
    }

    let corrections = store
        .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
        .expect("load workspace corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].aggregate_node_id, aggregate_detail.node.id);
    assert_eq!(corrections[0].kind, CorrectionKind::Rename);
    assert_eq!(corrections[0].source_node_ids.len(), 2);
    assert!(!corrections[0].evidence_ids.is_empty());

    let events_path = temp
        .path()
        .join(DEFAULT_WORKSPACE_ID)
        .join("events/brain_events.jsonl");
    let events = fs::read_to_string(events_path).expect("brain events");
    assert!(events.contains("\"eventType\":\"correction_applied\""));
    assert!(events.contains("\"policyResult\":\"applied\""));

    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let materialized_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json"))
            .expect("read materialized nodes");
    assert!(materialized_nodes.iter().any(|node| {
        node.node_id == "concept-agent-brain-context"
            && node.label == "Agent Brain Context"
            && node.aliases.contains(&"Shared Context Layer".into())
    }));
    assert!(!materialized_nodes
        .iter()
        .any(|node| node.node_id == aggregate_detail.node.id));
    let materialized_edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json"))
            .expect("read materialized edges");
    assert!(materialized_edges.iter().all(|edge| {
        edge.source_node_id != aggregate_detail.node.id
            && edge.target_node_id != aggregate_detail.node.id
    }));
    assert!(materialized_edges.iter().any(|edge| {
        edge.source_node_id == "concept-agent-brain-context"
            || edge.target_node_id == "concept-agent-brain-context"
    }));
    let materialized_claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read materialized claims");
    assert!(materialized_claims.iter().all(|claim| {
        !claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == &aggregate_detail.node.id)
    }));
    assert!(materialized_claims.iter().any(|claim| {
        claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == "concept-agent-brain-context")
    }));
    let wiki_index =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki index");
    assert!(wiki_index.contains("[Agent Brain Context](topics/concept-agent-brain-context.md)"));
    assert!(!wiki_index.contains(&format!("topics/{}.md", aggregate_detail.node.id)));
    let topic_page =
        fs::read_to_string(workspace_root.join("wiki/topics/concept-agent-brain-context.md"))
            .expect("read renamed topic page");
    assert!(topic_page.contains("# Agent Brain Context"));
    assert!(topic_page.contains("concept-agent-brain-context"));
    assert_materialized_brain_has_no_dangling_refs(&workspace_root);
}

#[test]
fn workspace_merge_correction_replays_to_source_snapshot_and_ledger() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (mut project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha planning context keeps agents grounded.\n\n## Page 2\n\nBeta review context keeps evidence visible.\n",
            "source-a",
            "alpha",
            10,
        );
    let concept_ids = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    assert!(concept_ids.len() >= 2);
    rename_concept_for_test(&mut project, &concept_ids[0], "Alpha Context", &[]);
    rename_concept_for_test(&mut project, &concept_ids[1], "Beta Context", &[]);
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source project");

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let source_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Alpha Context")
        .expect("workspace source concept")
        .clone();
    let target_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Beta Context")
        .expect("workspace target concept")
        .clone();
    let node_count_before = project.nodes.len();

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: source_detail.node.id.clone(),
        kind: CorrectionKind::Merge,
        target_node_id: Some(target_detail.node.id.clone()),
        value: None,
    })
    .expect("apply workspace merge correction");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    let source_project = store
        .load_project(Some(&project.summary.project_id))
        .expect("load source project")
        .expect("source project");
    assert_eq!(source_project.nodes.len(), node_count_before - 1);
    assert!(!source_project
        .details_by_node_id
        .values()
        .any(|detail| detail.canonical_name == "Alpha Context"));
    assert!(source_project
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Beta Context")
        .expect("merged target concept")
        .aliases
        .contains(&"Alpha Context".into()));

    let corrections = store
        .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
        .expect("load workspace corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].kind, CorrectionKind::Merge);
    assert_eq!(corrections[0].target_node_id, Some(target_detail.node.id));
    assert_eq!(corrections[0].source_node_ids.len(), 1);
}

#[test]
fn workspace_merge_remaps_and_deduplicates_preserved_agent_artifacts() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (mut project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAlpha planning context keeps agents grounded.\n\n## Page 2\n\nBeta review context keeps evidence visible.\n\n## Page 3\n\nGamma release context keeps wiki links durable.\n",
            "source-a",
            "alpha",
            10,
        );
    let concept_ids = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    assert!(concept_ids.len() >= 3);
    rename_concept_for_test(&mut project, &concept_ids[0], "Alpha Context", &[]);
    rename_concept_for_test(&mut project, &concept_ids[1], "Beta Context", &[]);
    rename_concept_for_test(&mut project, &concept_ids[2], "Gamma Context", &[]);
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source project");

    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let evidence_ids = project
        .details_by_node_id
        .get(&concept_ids[0])
        .expect("alpha detail")
        .evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<Vec<_>>();
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let actor = BrainActor {
        actor_type: BrainActorType::Agent,
        actor_id: "duckdocs-agent-ingest".into(),
    };

    for (source, target, edge_id) in [
        (
            "concept-alpha-context",
            "concept-gamma-context",
            "relation-alpha-gamma-duplicate",
        ),
        (
            "concept-beta-context",
            "concept-gamma-context",
            "relation-beta-gamma-duplicate",
        ),
    ] {
        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Link,
            title: "Related context".into(),
            body: "Duplicate relation should collapse after merge.".into(),
            actor: actor.clone(),
            target_node_id: Some(target.into()),
            target_source_id: None,
            relation_kind: Some(BrainRelationKind::RelatedTo),
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![source.into()],
            evidence_refs: evidence_ids.clone(),
            proposal_payload: Some(AgentGraphProposalPayload::NewEdge {
                edge: AgentNewEdgePayload {
                    source_node_id: source.into(),
                    target_node_id: target.into(),
                    kind: BrainRelationKind::RelatedTo,
                    label: "Related context".into(),
                    source_path: manifest.markdown_path.clone(),
                    edge_id: Some(edge_id.into()),
                    source_refs: vec![manifest.source_id.clone()],
                    evidence_refs: evidence_ids.clone(),
                    reason: Some("Agent linked related contexts.".into()),
                },
            }),
        })
        .expect("auto-apply edge proposal");
    }

    for (topic, claim_id) in [
        ("concept-alpha-context", "claim-alpha-shared-context"),
        ("concept-beta-context", "claim-beta-shared-context"),
    ] {
        handle_propose_brain_update(ProposeBrainUpdateRequest {
            scope: scope.clone(),
            kind: BrainProposalKind::Claim,
            title: "Shared claim".into(),
            body: "Shared claim should collapse after merge.".into(),
            actor: actor.clone(),
            target_node_id: Some(topic.into()),
            target_source_id: None,
            relation_kind: None,
            source_description: None,
            source_user_context: None,
            source_ingest_instruction: None,
            source_refs: vec![manifest.source_id.clone()],
            node_refs: vec![topic.into()],
            evidence_refs: evidence_ids.clone(),
            proposal_payload: Some(AgentGraphProposalPayload::NewClaim {
                claim: AgentNewClaimPayload {
                    statement: "Merged context keeps source-backed evidence reviewable.".into(),
                    source_path: manifest.markdown_path.clone(),
                    claim_id: Some(claim_id.into()),
                    topic_refs: vec![topic.into()],
                    source_refs: vec![manifest.source_id.clone()],
                    evidence_refs: evidence_ids.clone(),
                    reason: Some("Agent extracted a duplicate claim.".into()),
                },
            }),
        })
        .expect("auto-apply claim proposal");
    }

    let wiki = handle_propose_brain_update(ProposeBrainUpdateRequest {
        scope: scope.clone(),
        kind: BrainProposalKind::WikiPage,
        title: "Merge replay page".into(),
        body: "Saved wiki pages should preserve only surviving graph refs.".into(),
        actor: actor.clone(),
        target_node_id: Some("concept-alpha-context".into()),
        target_source_id: None,
        relation_kind: None,
        source_description: None,
        source_user_context: None,
        source_ingest_instruction: None,
        source_refs: vec![manifest.source_id.clone()],
        node_refs: vec![
            "concept-alpha-context".into(),
            "concept-beta-context".into(),
        ],
        evidence_refs: evidence_ids.clone(),
        proposal_payload: None,
    })
    .expect("propose wiki page");
    handle_resolve_brain_review_item(ResolveBrainReviewItemRequest {
        scope: scope.clone(),
        proposal_id: wiki.proposal.proposal_id.clone(),
        decision: BrainReviewDecision::Accept,
        actor: BrainActor {
            actor_type: BrainActorType::User,
            actor_id: "local-user".into(),
        },
        reason: Some("Accept saved page before merge.".into()),
    })
    .expect("accept wiki page");

    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let source_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Alpha Context")
        .expect("workspace source concept")
        .clone();
    let target_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Beta Context")
        .expect("workspace target concept")
        .clone();

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: source_detail.node.id.clone(),
        kind: CorrectionKind::Merge,
        target_node_id: Some(target_detail.node.id.clone()),
        value: None,
    })
    .expect("apply workspace merge correction");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    let nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json")).expect("read nodes");
    assert!(!nodes
        .iter()
        .any(|node| node.node_id == "concept-alpha-context"));
    assert!(nodes
        .iter()
        .any(|node| node.node_id == "concept-beta-context"));

    let edges: Vec<BrainRelationRecord> =
        read_json_artifact(&workspace_root.join("graph/edges.json")).expect("read edges");
    assert!(edges.iter().all(|edge| {
        edge.source_node_id != "concept-alpha-context"
            && edge.target_node_id != "concept-alpha-context"
    }));
    assert_eq!(
        edges
            .iter()
            .filter(|edge| {
                edge.source_node_id == "concept-beta-context"
                    && edge.target_node_id == "concept-gamma-context"
                    && edge.label == "Related context"
            })
            .count(),
        1
    );

    let claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json")).expect("read claims");
    let matching_claims = claims
        .iter()
        .filter(|claim| {
            claim.statement == "Merged context keeps source-backed evidence reviewable."
        })
        .collect::<Vec<_>>();
    assert_eq!(matching_claims.len(), 1);
    assert_eq!(
        matching_claims[0].topic_refs,
        vec!["concept-beta-context".to_string()]
    );

    let page: WikiPage = read_json_artifact(
        &workspace_root
            .join("reviews/proposed-updates")
            .join(format!("{}.json", wiki.proposal.proposal_id)),
    )
    .map(|proposal: BrainUpdateProposal| wiki_page_for_proposal(&proposal))
    .expect("read accepted wiki proposal");
    let manifest_snapshot: BrainRepoSnapshot =
        read_json_artifact(&workspace_root.join("brain-manifest.json"))
            .expect("read brain manifest");
    let saved_page = manifest_snapshot
        .wiki_pages
        .iter()
        .find(|candidate| candidate.path == page.path)
        .expect("saved page in manifest");
    assert_eq!(
        saved_page.node_refs,
        vec!["concept-beta-context".to_string()]
    );
    let saved_page_body =
        fs::read_to_string(workspace_root.join(&saved_page.path)).expect("read saved page");
    assert!(saved_page_body.contains("Nodes: concept-beta-context"));
    assert!(!saved_page_body.contains("concept-alpha-context"));
    assert_materialized_brain_has_no_dangling_refs(&workspace_root);
}

#[test]
fn workspace_keep_separate_correction_replays_to_source_snapshot_and_ledger() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (mut project, manifest) = compile_manifest_fixture_project_with_source(
        &temp,
        "# Source A\n\n## Page 1\n\nAlpha context keeps agents grounded.\n",
        "source-a",
        "alpha",
        10,
    );
    rename_first_concept_for_test(&mut project, "Alpha Context", &["Beta Context"]);
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source project");
    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let aggregate_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Alpha Context")
        .expect("workspace concept")
        .clone();
    assert!(aggregate_detail.aliases.contains(&"Beta Context".into()));
    let node_count_before = project.nodes.len();

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: aggregate_detail.node.id.clone(),
        kind: CorrectionKind::KeepSeparate,
        target_node_id: None,
        value: None,
    })
    .expect("apply workspace keep separate correction");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    let source_project = store
        .load_project(Some(&project.summary.project_id))
        .expect("load source project")
        .expect("source project");
    assert_eq!(source_project.nodes.len(), node_count_before + 1);
    assert!(source_project
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Alpha Context")
        .expect("kept concept")
        .aliases
        .is_empty());
    assert!(source_project
        .details_by_node_id
        .values()
        .any(|detail| detail.canonical_name == "Beta Context"));

    let corrections = store
        .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
        .expect("load workspace corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].kind, CorrectionKind::KeepSeparate);
    assert_eq!(corrections[0].source_node_ids.len(), 1);
}

#[test]
fn workspace_split_correction_materializes_replacements_claims_wiki_and_event() {
    static PROJECT_STORE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PROJECT_STORE_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (mut project, manifest) = compile_manifest_fixture_project_with_source(
            &temp,
            "# Source A\n\n## Page 1\n\nAgent context keeps graph changes auditable.\n\n## Page 2\n\nRuntime context keeps wiki references fresh.\n",
            "source-a",
            "alpha",
            10,
        );
    let split_node_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let mut evidence = project
        .details_by_node_id
        .get(&split_node_id)
        .expect("split detail")
        .evidence
        .clone();
    if evidence.len() == 1 {
        let mut extra = evidence[0].clone();
        extra.id = format!("{}-runtime", extra.id);
        extra.snippet = "Runtime context keeps wiki references fresh.".into();
        project
            .details_by_node_id
            .get_mut(&split_node_id)
            .expect("split detail")
            .evidence
            .push(extra.clone());
        evidence.push(extra);
    }
    rename_concept_for_test(&mut project, &split_node_id, "Agent Runtime Context", &[]);
    let request = CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
    };
    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save source project");
    let aggregate = store
        .load_workspace_project(DEFAULT_WORKSPACE_ID)
        .expect("load aggregate")
        .expect("workspace aggregate");
    let aggregate_detail = aggregate
        .details_by_node_id
        .values()
        .find(|detail| detail.canonical_name == "Agent Runtime Context")
        .expect("workspace split concept")
        .clone();
    let aggregate_evidence = aggregate_detail.evidence.clone();
    assert!(aggregate_evidence.len() >= 2);
    let split_value = json!([
        {
            "replacementNodeId": "concept-agent-context-split",
            "replacementLabel": "Agent Context",
            "evidenceIds": [aggregate_evidence[0].id.clone()]
        },
        {
            "replacementNodeId": "concept-runtime-context-split",
            "replacementLabel": "Runtime Context",
            "evidenceIds": [aggregate_evidence[1].id.clone()]
        }
    ])
    .to_string();

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: aggregate_detail.node.id.clone(),
        kind: CorrectionKind::Split,
        target_node_id: None,
        value: Some(split_value.clone()),
    })
    .expect("apply workspace split correction");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    let source_project = store
        .load_project(Some(&project.summary.project_id))
        .expect("load source project")
        .expect("source project");
    assert!(!source_project
        .details_by_node_id
        .contains_key(&aggregate_detail.node.id));
    assert!(source_project
        .details_by_node_id
        .contains_key("concept-agent-context-split"));
    assert!(source_project
        .details_by_node_id
        .contains_key("concept-runtime-context-split"));

    let corrections = store
        .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
        .expect("load workspace corrections");
    assert_eq!(corrections.len(), 1);
    assert_eq!(corrections[0].kind, CorrectionKind::Split);
    assert_eq!(corrections[0].value, Some(split_value));
    assert_eq!(corrections[0].source_node_ids.len(), 1);

    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let materialized_nodes: Vec<BrainNodeRecord> =
        read_json_artifact(&workspace_root.join("graph/nodes.json"))
            .expect("read materialized nodes");
    assert!(materialized_nodes
        .iter()
        .any(|node| node.node_id == "concept-agent-context"));
    assert!(materialized_nodes
        .iter()
        .any(|node| node.node_id == "concept-runtime-context"));
    assert!(!materialized_nodes
        .iter()
        .any(|node| node.node_id == aggregate_detail.node.id));
    let materialized_claims: Vec<ClaimRecord> =
        read_json_artifact(&workspace_root.join("graph/claims.json"))
            .expect("read materialized claims");
    assert!(materialized_claims.iter().any(|claim| {
        claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == "concept-agent-context")
    }));
    assert!(materialized_claims.iter().any(|claim| {
        claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == "concept-runtime-context")
    }));
    assert!(materialized_claims.iter().all(|claim| {
        !claim
            .topic_refs
            .iter()
            .any(|node_id| node_id == &aggregate_detail.node.id)
    }));
    let wiki_index =
        fs::read_to_string(workspace_root.join("wiki/index.md")).expect("read wiki index");
    assert!(wiki_index.contains("[Agent Context](topics/concept-agent-context.md)"));
    assert!(wiki_index.contains("[Runtime Context](topics/concept-runtime-context.md)"));
    assert!(!wiki_index.contains(&format!("topics/{}.md", aggregate_detail.node.id)));
    assert!(workspace_root
        .join("wiki/topics/concept-agent-context.md")
        .exists());
    assert!(workspace_root
        .join("wiki/topics/concept-runtime-context.md")
        .exists());
    let events = read_brain_events_jsonl(&workspace_root.join("events/brain_events.jsonl"))
        .expect("read events");
    assert!(events.iter().any(|event| {
        event.event_type == BrainEventKind::CorrectionApplied
            && event.payload_json.contains("\"kind\":\"split\"")
    }));
    assert_materialized_brain_has_no_dangling_refs(&workspace_root);
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
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
    };
    store
        .save_project(&project_a, &request_a, Some(&manifest_a))
        .expect("save workspace a project");
    store
        .save_project(&project_b, &request_b, Some(&manifest_b))
        .expect("save workspace b project");

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    let response = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: None,
    })
    .expect("load exact project");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
    }

    assert_eq!(response.workspace_id.as_deref(), Some(DEFAULT_WORKSPACE_ID));
    assert_eq!(response.sources.len(), 1);
    assert_eq!(response.sources[0].source_id, "source-a");
    assert_eq!(
        response.project.expect("exact project").summary.project_id,
        project_a.summary.project_id
    );

    let previous_store = std::env::var_os("DUCKDOCS_PROJECT_STORE");
    std::env::set_var("DUCKDOCS_PROJECT_STORE", &store_path);
    let error = handle_load_project(LoadProjectRequest {
        project_id: Some(project_a.summary.project_id.clone()),
        workspace_id: Some("workspace-b".into()),
    })
    .expect_err("stale workspace should not hydrate exact project");
    match previous_store {
        Some(value) => std::env::set_var("DUCKDOCS_PROJECT_STORE", value),
        None => std::env::remove_var("DUCKDOCS_PROJECT_STORE"),
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
    };
    let request_b = CompileProjectRequest {
        source_markdown_path: manifest_b.markdown_path.clone(),
        source_document_path: Some(manifest_b.source_path.clone()),
        source_manifest_path: Some(manifest_b.manifest_path.clone()),
        workspace_id: Some(manifest_b.workspace_id.clone()),
        source_id: Some(manifest_b.source_id.clone()),
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
    request.output = Some(duckdocs_engine_types::ParseOutputTarget {
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
fn rename_correction_updates_canonical_name_and_aliases() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n",
        );
    let concept_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let previous_name = project
        .details_by_node_id
        .get(&concept_id)
        .expect("concept detail")
        .canonical_name
        .clone();
    let project_id = project.summary.project_id.clone();

    apply_correction(
        &mut project,
        &ApplyCorrectionRequest {
            project_id,
            node_id: concept_id.clone(),
            kind: CorrectionKind::Rename,
            target_node_id: None,
            value: Some("Graph Evidence View".into()),
        },
    )
    .expect("apply rename correction");

    let renamed_id = "concept-graph-evidence-view";
    let detail = project
        .details_by_node_id
        .get(renamed_id)
        .expect("renamed detail");
    assert_eq!(detail.canonical_name, "Graph Evidence View");
    assert!(detail.aliases.contains(&previous_name));
    assert!(!project.details_by_node_id.contains_key(&concept_id));
    assert_eq!(
        project
            .nodes
            .iter()
            .find(|node| node.id == renamed_id)
            .expect("renamed node")
            .label,
        "Graph Evidence View"
    );
    assert!(project
        .edges
        .iter()
        .all(|edge| { edge.source_node_id != concept_id && edge.target_node_id != concept_id }));
}

#[test]
fn merge_correction_combines_concepts_and_redirects_edges() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n\n## Page 2\n\nEvidence inspector helps people trust the graph.\n",
        );
    let concept_ids = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let source_id = concept_ids[0].clone();
    let target_id = concept_ids[1].clone();
    let source_name = project
        .details_by_node_id
        .get(&source_id)
        .expect("source detail")
        .canonical_name
        .clone();
    let node_count_before = project.nodes.len();
    let project_id = project.summary.project_id.clone();

    apply_correction(
        &mut project,
        &ApplyCorrectionRequest {
            project_id,
            node_id: source_id.clone(),
            kind: CorrectionKind::Merge,
            target_node_id: Some(target_id.clone()),
            value: None,
        },
    )
    .expect("apply merge correction");

    assert_eq!(project.nodes.len(), node_count_before - 1);
    assert!(!project.nodes.iter().any(|node| node.id == source_id));
    assert!(project
        .edges
        .iter()
        .all(|edge| { edge.source_node_id != source_id && edge.target_node_id != source_id }));
    assert!(project
        .details_by_node_id
        .get(&target_id)
        .expect("target detail")
        .aliases
        .contains(&source_name));
}

#[test]
fn keep_separate_correction_splits_aliases_into_new_nodes() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded Graph View keeps answers cautious.\n\n## Page 2\n\nGrounded graph view keeps answers cautious.\n",
        );
    let concept_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    assert!(!project
        .details_by_node_id
        .get(&concept_id)
        .expect("detail")
        .aliases
        .is_empty());
    let node_count_before = project.nodes.len();
    let project_id = project.summary.project_id.clone();

    apply_correction(
        &mut project,
        &ApplyCorrectionRequest {
            project_id,
            node_id: concept_id.clone(),
            kind: CorrectionKind::KeepSeparate,
            target_node_id: None,
            value: None,
        },
    )
    .expect("apply keep separate correction");

    assert!(project.nodes.len() > node_count_before);
    assert!(project
        .details_by_node_id
        .get(&concept_id)
        .expect("detail")
        .aliases
        .is_empty());
    assert!(project
        .edges
        .iter()
        .any(|edge| edge.label == "Separated by correction"));
}

#[test]
fn corrections_preserve_manifest_source_document_edges() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (mut project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded Graph View keeps answers cautious.\n\n## Page 2\n\nGrounded graph view keeps answers cautious.\n",
        );
    let source_node_id = source_node_id(&manifest.source_id);
    let concept_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();
    let project_id = project.summary.project_id.clone();

    apply_correction(
        &mut project,
        &ApplyCorrectionRequest {
            project_id,
            node_id: concept_id.clone(),
            kind: CorrectionKind::KeepSeparate,
            target_node_id: None,
            value: None,
        },
    )
    .expect("apply keep separate correction");

    assert!(project.edges.iter().any(|edge| {
        edge.kind == RelationKind::SourceDocument
            && edge.source_node_id == source_node_id
            && edge.target_node_id == concept_id
    }));
    assert!(project.edges.iter().any(|edge| {
        edge.kind == RelationKind::SourceDocument
            && edge.source_node_id == source_node_id
            && edge.target_node_id != concept_id
    }));
}

#[test]
fn split_correction_creates_replacements_and_redistributes_edges() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (mut project, manifest) = compile_manifest_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nPlanning Context keeps graph changes auditable.\n\n## Page 2\n\nRuntime Context keeps wiki references fresh.\n",
        );
    let source_node_id = source_node_id(&manifest.source_id);
    let concept_ids = project
        .nodes
        .iter()
        .filter(|node| node.kind == GraphNodeKind::Concept)
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    assert!(concept_ids.len() >= 2);
    let split_node_id = concept_ids[0].clone();
    let neighbor_node_id = concept_ids[1].clone();
    let mut evidence = project
        .details_by_node_id
        .get(&split_node_id)
        .expect("split detail")
        .evidence
        .clone();
    if evidence.len() == 1 {
        let mut extra = evidence[0].clone();
        extra.id = format!("{}-runtime", extra.id);
        extra.snippet = "Runtime Context keeps wiki references fresh.".into();
        project
            .details_by_node_id
            .get_mut(&split_node_id)
            .expect("split detail")
            .evidence
            .push(extra.clone());
        evidence.push(extra);
    }
    assert!(evidence.len() >= 2);
    let split_edge_id =
        relation_edge_id(RelationKind::RelatedTo, &split_node_id, &neighbor_node_id);
    let relation_evidence = vec![evidence[1].clone()];
    project.edges.push(RelationEdgeSummary {
        id: split_edge_id.clone(),
        source_node_id: split_node_id.clone(),
        target_node_id: neighbor_node_id.clone(),
        kind: RelationKind::RelatedTo,
        label: "Depends on".into(),
        confidence: Some(0.77),
        evidence_count: relation_evidence.len(),
    });
    project.edge_details_by_id.insert(
        split_edge_id.clone(),
        RelationEdgeDetail {
            edge: project
                .edges
                .iter()
                .find(|edge| edge.id == split_edge_id)
                .expect("split edge")
                .clone(),
            explanation: String::new(),
            evidence: relation_evidence,
        },
    );
    let project_id = project.summary.project_id.clone();

    apply_correction(
        &mut project,
        &ApplyCorrectionRequest {
            project_id,
            node_id: split_node_id.clone(),
            kind: CorrectionKind::Split,
            target_node_id: None,
            value: Some(
                json!([
                    {
                        "replacementNodeId": "concept-planning-context-split",
                        "replacementLabel": "Planning Context",
                        "evidenceIds": [evidence[0].id.clone()]
                    },
                    {
                        "replacementNodeId": "concept-runtime-context-split",
                        "replacementLabel": "Runtime Context",
                        "evidenceIds": [evidence[1].id.clone()],
                        "edgeIds": [split_edge_id.clone()]
                    }
                ])
                .to_string(),
            ),
        },
    )
    .expect("apply split correction");

    assert!(!project.details_by_node_id.contains_key(&split_node_id));
    assert!(project
        .details_by_node_id
        .contains_key("concept-planning-context-split"));
    assert!(project
        .details_by_node_id
        .contains_key("concept-runtime-context-split"));
    assert!(project.edges.iter().all(|edge| {
        edge.source_node_id != split_node_id && edge.target_node_id != split_node_id
    }));
    assert!(project.edges.iter().any(|edge| {
        edge.kind == RelationKind::SourceDocument
            && edge.source_node_id == source_node_id
            && edge.target_node_id == "concept-planning-context-split"
    }));
    assert!(project.edges.iter().any(|edge| {
        edge.kind == RelationKind::RelatedTo
            && ((edge.source_node_id == "concept-runtime-context-split"
                && edge.target_node_id == neighbor_node_id)
                || (edge.source_node_id == neighbor_node_id
                    && edge.target_node_id == "concept-runtime-context-split"))
    }));
    assert_eq!(
        project
            .details_by_node_id
            .get("concept-planning-context-split")
            .expect("planning detail")
            .evidence
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>(),
        vec![evidence[0].id.clone()]
    );
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
fn answer_project_returns_grounded_citations_for_matching_question() {
    let temp = tempfile::tempdir().expect("temp dir");
    let project = compile_fixture_project(
            &temp,
            "# Sample import\n\n## Page 1\n\nGrounded graph view keeps evidence visible.\nExplainable graph answers stay tied to snippets.\n",
        );
    let concept_id = project
        .nodes
        .iter()
        .find(|node| node.kind == GraphNodeKind::Concept)
        .expect("concept node")
        .id
        .clone();

    let answer = answer_project(
        &project,
        &AnswerProjectRequest {
            project_id: project.summary.project_id.clone(),
            node_id: Some(concept_id),
            question: "What evidence keeps graph answers grounded?".into(),
        },
    )
    .expect("answer project");

    assert_eq!(answer.status, AnswerStatus::Grounded);
    assert!(!answer.citations.is_empty());
    assert!(answer
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("grounded"));
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
    let binary_path = bin_dir.join("duckdocs-test-bin");
    fs::write(&binary_path, "").expect("test bin");

    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", &bin_dir);
    let resolved = resolve_binary("duckdocs-test-bin", &["/definitely/missing"]);
    match old_path {
        Some(value) => std::env::set_var("PATH", value),
        None => std::env::remove_var("PATH"),
    }

    assert_eq!(resolved, binary_path);
}
