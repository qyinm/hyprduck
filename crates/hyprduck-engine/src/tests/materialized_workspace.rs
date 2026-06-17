use super::common::*;
use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn seed_agent_patch_workspace() -> (tempfile::TempDir, PathBuf, BrainReadScope) {
    let temp = tempfile::tempdir().expect("temp dir");
    let output_root = temp.path().join("HyprDuck");
    let workspace_root = output_root.join(DEFAULT_WORKSPACE_ID);
    let mut seed = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    seed.generated_at = 10;
    seed.sources.push(SourceRecord {
        source_id: "source-agent".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "[redacted-local-path]".into(),
        source_path: "[redacted-local-path]".into(),
        markdown_path: "[redacted-local-path]".into(),
        format: hyprduck_engine_types::SourceFormat::markdown(),
        status: SourceStatus::ingested(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 10,
    });
    seed.evidence.push(EvidenceRef {
        id: "ev-agent-1".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Agents can create graph patches with existing evidence.".into(),
        source_path: Some("[redacted-local-path]".into()),
        source_id: Some("source-agent".into()),
        markdown_path: Some("[redacted-local-path]".into()),
        image_path: None,
        provenance: Some("test".into()),
    });
    write_materialized_brain_repo(&workspace_root, &seed).expect("write seed repo");
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root))
        .expect("open knowledge store");
    store
        .persist_graph_snapshot(&seed)
        .expect("persist source/evidence rows");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(output_root.display().to_string()),
    };
    (temp, workspace_root, scope)
}

fn agent_graph_patch() -> hyprduck_engine_types::GraphPatch {
    hyprduck_engine_types::GraphPatch {
        schema_version: hyprduck_engine_types::GRAPH_PATCH_SCHEMA_VERSION.into(),
        source_ids: vec!["source-agent".into()],
        evidence_refs: vec!["ev-agent-1".into()],
        nodes: vec![hyprduck_engine_types::GraphPatchNode {
            node_id: "concept-agent-graph-patch".into(),
            kind: BrainNodeKind::Concept,
            label: "Agent graph patch".into(),
            scope: None,
            aliases: Vec::new(),
            source_ids: vec!["source-agent".into()],
            evidence_ids: vec!["ev-agent-1".into()],
        }],
        relations: vec![hyprduck_engine_types::GraphPatchRelation {
            relation_id: "rel-source-agent-graph-patch".into(),
            kind: BrainRelationKind::Mentions,
            source_node_id: "source:source-agent".into(),
            target_node_id: "concept-agent-graph-patch".into(),
            label: "mentions".into(),
            evidence_ids: vec!["ev-agent-1".into()],
        }],
        claims: vec![hyprduck_engine_types::GraphPatchClaim {
            claim_id: "claim-agent-graph-patch".into(),
            statement: "Agent-created graph patches are evidence-backed.".into(),
            topic_refs: vec!["concept-agent-graph-patch".into()],
            source_refs: vec!["source-agent".into()],
            evidence_refs: vec!["ev-agent-1".into()],
            status: "agent_generated".into(),
        }],
        wiki_pages: vec![hyprduck_engine_types::GraphPatchWikiPage {
            page_id: "wiki-agent-graph-patch".into(),
            path: "wiki/agent-graph-patch.md".into(),
            title: "Agent graph patch".into(),
            body: "Evidence-backed graph patch page.".into(),
            node_refs: vec!["concept-agent-graph-patch".into()],
            source_refs: vec!["source-agent".into()],
            evidence_refs: vec!["ev-agent-1".into()],
        }],
        agent_metadata: BTreeMap::new(),
    }
}

#[test]
fn agent_graph_patch_applies_evidence_backed_nodes_and_relations() {
    let (_temp, workspace_root, scope) = seed_agent_patch_workspace();

    let response = handle_apply_graph_patch(hyprduck_engine_types::ApplyGraphPatchRequest {
        scope: scope.clone(),
        agent_id: Some("codex".into()),
        graph_patch: agent_graph_patch(),
    })
    .expect("apply graph patch");

    assert_eq!(response.status, "applied");
    assert!(response.graph_ready);
    assert_eq!(response.changed_node_ids, vec!["concept-agent-graph-patch"]);

    let node = handle_read_node(hyprduck_engine_types::ReadNodeRequest {
        scope: scope.clone(),
        node_id: "concept-agent-graph-patch".into(),
    })
    .expect("read patched node");
    assert_eq!(node.node.label, "Agent graph patch");
    assert_eq!(node.evidence[0].id, "ev-agent-1");

    let snapshot = handle_read_graph_snapshot(hyprduck_engine_types::ReadGraphSnapshotRequest {
        scope,
        include_local_paths: false,
    })
    .expect("read graph snapshot");
    assert_eq!(snapshot.materialized_at, response.applied_at);
    assert_eq!(
        snapshot.snapshot_id,
        format!("snapshot-{}-{}", DEFAULT_WORKSPACE_ID, response.applied_at)
    );
    assert!(workspace_root.join("graph/entities.json").exists());
    assert!(workspace_root.join("wiki/agent-graph-patch.md").exists());
    assert!(workspace_root.join(LATEST_READABLE_SNAPSHOT_PATH).exists());
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-agent-graph-patch"));
    assert!(snapshot
        .edges
        .iter()
        .any(|edge| edge.relation_id == "rel-source-agent-graph-patch"));
}

#[test]
fn agent_graph_patch_rejects_records_without_direct_evidence() {
    let (_temp, _workspace_root, scope) = seed_agent_patch_workspace();
    let mut patch = agent_graph_patch();
    patch.nodes[0].evidence_ids.clear();

    let error = handle_apply_graph_patch(hyprduck_engine_types::ApplyGraphPatchRequest {
        scope,
        agent_id: Some("codex".into()),
        graph_patch: patch,
    })
    .expect_err("reject graph patch node without evidence");

    assert!(error
        .to_string()
        .contains("graphPatch node concept-agent-graph-patch evidenceIds"));
}

#[test]
fn agent_graph_patch_rejects_relations_to_out_of_scope_existing_nodes() {
    let (_temp, workspace_root, scope) = seed_agent_patch_workspace();
    let mut snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read seed snapshot");
    snapshot.sources.push(SourceRecord {
        source_id: "source-other".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: "[redacted-local-path]".into(),
        source_path: "[redacted-local-path]".into(),
        markdown_path: "[redacted-local-path]".into(),
        format: hyprduck_engine_types::SourceFormat::markdown(),
        status: SourceStatus::ingested(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 10,
    });
    snapshot.evidence.push(EvidenceRef {
        id: "ev-other".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Out of scope evidence.".into(),
        source_path: Some("[redacted-local-path]".into()),
        source_id: Some("source-other".into()),
        markdown_path: Some("[redacted-local-path]".into()),
        image_path: None,
        provenance: Some("test".into()),
    });
    snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-out-of-scope".into(),
        kind: BrainNodeKind::Concept,
        label: "Out of scope".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: vec!["ev-other".into()],
        source_ids: vec!["source-other".into()],
        confidence: Some(0.9),
        updated_at: 10,
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write out-of-scope node");
    let mut patch = agent_graph_patch();
    patch.relations[0].target_node_id = "concept-out-of-scope".into();

    let error = handle_apply_graph_patch(hyprduck_engine_types::ApplyGraphPatchRequest {
        scope,
        agent_id: Some("codex".into()),
        graph_patch: patch,
    })
    .expect_err("reject relation to out-of-scope node");

    assert!(error
        .to_string()
        .contains("references unknown targetNodeId concept-out-of-scope"));
}

#[test]
fn agent_graph_patch_preserves_existing_record_refs_when_updating_in_scope_node() {
    let (_temp, workspace_root, scope) = seed_agent_patch_workspace();
    let mut snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read seed snapshot");
    snapshot.evidence.push(EvidenceRef {
        id: "ev-other".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Existing cross-source evidence.".into(),
        source_path: Some("[redacted-local-path]".into()),
        source_id: Some("source-other".into()),
        markdown_path: Some("[redacted-local-path]".into()),
        image_path: None,
        provenance: Some("test".into()),
    });
    snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-agent-graph-patch".into(),
        kind: BrainNodeKind::Concept,
        label: "Existing graph concept".into(),
        scope: BrainScope::Project,
        aliases: vec!["Existing alias".into()],
        evidence_ids: vec!["ev-agent-1".into(), "ev-other".into()],
        source_ids: vec!["source-agent".into(), "source-other".into()],
        confidence: Some(0.9),
        updated_at: 10,
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write existing node");

    let mut patch = agent_graph_patch();
    patch.nodes[0].aliases = vec!["Patched alias".into()];
    handle_apply_graph_patch(hyprduck_engine_types::ApplyGraphPatchRequest {
        scope: scope.clone(),
        agent_id: Some("codex".into()),
        graph_patch: patch,
    })
    .expect("apply graph patch");

    let node = handle_read_node(hyprduck_engine_types::ReadNodeRequest {
        scope,
        node_id: "concept-agent-graph-patch".into(),
    })
    .expect("read patched node");
    assert!(node.node.source_ids.contains(&"source-agent".into()));
    assert!(node.node.source_ids.contains(&"source-other".into()));
    assert!(node.node.evidence_ids.contains(&"ev-agent-1".into()));
    assert!(node.node.evidence_ids.contains(&"ev-other".into()));
    assert!(node.node.aliases.contains(&"Existing alias".into()));
    assert!(node.node.aliases.contains(&"Patched alias".into()));
}

#[test]
fn read_graph_snapshot_includes_materialization_report_counts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output_root = temp.path().join("HyprDuck");
    let workspace_root = output_root.join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = empty_replayed_brain_snapshot(DEFAULT_WORKSPACE_ID);
    snapshot.generated_at = 10;
    snapshot.sources.push(SourceRecord {
        source_id: "source-alpha".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: output_root
            .join("imports/source-alpha/source-fixture.pdf")
            .display()
            .to_string(),
        source_path: output_root
            .join("sources/source-alpha/source-fixture.pdf")
            .display()
            .to_string(),
        markdown_path: output_root
            .join("artifacts/source-alpha/source-fixture.md")
            .display()
            .to_string(),
        format: hyprduck_engine_types::SourceFormat::pdf(),
        status: SourceStatus::partial(),
        page_count: 3,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 10,
    });
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized graph");
    let mut db_snapshot = snapshot.clone();
    db_snapshot.nodes.push(BrainNodeRecord {
        node_id: "concept-beta".into(),
        kind: BrainNodeKind::Concept,
        label: "Beta".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: Vec::new(),
        confidence: Some(0.8),
        updated_at: 11,
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    db_snapshot.relations.push(BrainRelationRecord {
        relation_id: "rel-alpha-beta".into(),
        kind: BrainRelationKind::RelatedTo,
        source_node_id: "concept-alpha".into(),
        target_node_id: "concept-beta".into(),
        label: "relates".into(),
        evidence_ids: Vec::new(),
        confidence: Some(0.7),
        updated_at: 11,
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
    });
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root))
        .expect("open knowledge store");
    store
        .persist_graph_snapshot(&db_snapshot)
        .expect("persist DB graph projection");
    let graph = graphqlite::Graph::open(store.path().to_path_buf()).expect("open test graph DB");
    graph
        .connection()
        .sqlite_connection()
        .execute(
            "UPDATE sources SET success_count = 2, failed_count = 1 WHERE source_id = 'source-alpha'",
            (),
        )
        .expect("update source import counts");
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
        include_local_paths: false,
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
    assert!(response
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-beta"));
    assert!(response
        .edges
        .iter()
        .any(|edge| edge.relation_id == "rel-alpha-beta"));
    assert_eq!(
        response.source_paths,
        vec![
            "source-fixture.md".to_string(),
            "source-fixture.pdf".to_string()
        ]
    );
    let source = response.sources.first().expect("snapshot source");
    assert_eq!(source.source_path, "source-fixture.pdf");
    assert_eq!(source.markdown_path, "source-fixture.md");
    assert_eq!(source.page_count, 3);
    assert_eq!(source.success_count, 2);
    assert_eq!(source.failed_count, 1);

    let search_response = handle_search_brain(hyprduck_engine_types::SearchBrainRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(output_root.display().to_string()),
        },
        query: "Beta".into(),
        limit: Some(10),
    })
    .expect("search DB graph projection");
    assert!(search_response
        .results
        .iter()
        .any(|result| result.id == "concept-beta"));
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
    let graph_store =
        KnowledgeStore::open(store_path.clone()).expect("open canonical GraphQLite project store");
    let projection = graph_store
        .read_graph_canvas_projection_from_db(DEFAULT_WORKSPACE_ID)
        .expect("read GraphQLite workspace projection")
        .expect("workspace graph persisted to GraphQLite");
    assert!(projection
        .0
        .iter()
        .any(|node| node.node_id == "source:source-a"));
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
    let source_b_dir = PathBuf::from(&manifest_b.source_path)
        .parent()
        .expect("source b dir")
        .to_path_buf();
    let artifact_b_dir = PathBuf::from(&manifest_b.artifact_root);
    fs::create_dir_all(&source_b_dir).expect("create source b dir");
    fs::create_dir_all(&artifact_b_dir).expect("create artifact b dir");
    fs::write(&manifest_b.source_path, b"source b pdf").expect("write source b copy");
    fs::write(
        artifact_b_dir.join("provider-graph-materialization.json"),
        b"{}",
    )
    .expect("write source b artifact");
    upsert_source_chunks(
        &workspace_root,
        &manifest_a,
        &chunk_source_markdown(
            &manifest_a,
            "# Source A\n\n## Page 1\n\nActive source chunks stay searchable.\n",
        ),
    )
    .expect("write source a chunks");
    upsert_source_chunks(
        &workspace_root,
        &manifest_b,
        &chunk_source_markdown(
            &manifest_b,
            "# Source B\n\n## Page 1\n\nDeleted source chunks should be removed.\n",
        ),
    )
    .expect("write source b chunks");
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
    assert!(
        !source_b_dir.exists(),
        "source document copy directory should be removed"
    );
    assert!(
        !artifact_b_dir.exists(),
        "source artifact directory should be removed"
    );
    let chunks_after_delete =
        read_workspace_source_chunks(&workspace_root).expect("read chunks after delete");
    assert!(chunks_after_delete
        .iter()
        .any(|chunk| chunk.source_id == manifest_a.source_id));
    assert!(chunks_after_delete
        .iter()
        .all(|chunk| chunk.source_id != manifest_b.source_id));
}

#[test]
fn workspace_delete_source_resolves_canvas_node_id_alias() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let store_path = temp.path().join("knowledge.sqlite3");
    let store = KnowledgeProjectStore::new(store_path.clone());
    let (project, manifest) = compile_manifest_fixture_project_with_source(
        &temp,
        "# System Design Interview\n\n## Page 1\n\nChapter 13 source node.\n",
        "source-ch13",
        "SystemDesignInterview-CH13",
        12,
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
        .expect("save source");
    store
        .materialize_workspace_brain_repo(DEFAULT_WORKSPACE_ID)
        .expect("materialize workspace");

    let rows = store
        .load_projects_for_workspace(DEFAULT_WORKSPACE_ID)
        .expect("load workspace rows");
    let workspace_root = workspace_root_for_rows(&rows).expect("workspace root");
    let mut snapshot = read_materialized_brain_snapshot(&workspace_root, DEFAULT_WORKSPACE_ID)
        .expect("read materialized snapshot");
    let canvas_alias_node_id = "SystemDesignInterview-CH13.pdf".to_string();
    snapshot.nodes.push(BrainNodeRecord {
        node_id: canvas_alias_node_id.clone(),
        kind: BrainNodeKind::Source,
        label: "SystemDesignInterview-CH13.pdf".into(),
        scope: BrainScope::Project,
        aliases: Vec::new(),
        evidence_ids: Vec::new(),
        source_ids: vec![manifest.source_id.clone()],
        confidence: Some(0.72),
        updated_at: 12,
        valid_from: 12,
        valid_to: None,
        superseded_by: None,
    });
    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write alias source node");
    KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root))
        .expect("open knowledge store")
        .persist_graph_snapshot(&snapshot)
        .expect("persist alias source node");

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", &store_path);
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", temp.path());
    handle_apply_correction(ApplyCorrectionRequest {
        project_id: workspace_project_id(DEFAULT_WORKSPACE_ID),
        node_id: canvas_alias_node_id,
        kind: CorrectionKind::Delete,
        target_node_id: None,
        value: None,
    })
    .expect("delete source via canvas alias node id");
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
        .all(|source| source.source_id != manifest.source_id));
    assert!(after_delete.nodes.iter().all(|node| {
        node.node_id != format!("source:{}", manifest.source_id)
            && node.node_id != "SystemDesignInterview-CH13.pdf"
    }));
    let canvas_projection = KnowledgeStore::open(KnowledgeStore::default_path_for_root(
        &workspace_root,
    ))
    .expect("open knowledge store")
    .read_graph_canvas_projection_from_db(DEFAULT_WORKSPACE_ID)
    .expect("read canvas projection")
    .expect("canvas projection present");
    assert!(canvas_projection.0.iter().all(|node| {
        node.node_id != format!("source:{}", manifest.source_id)
            && node.node_id != "SystemDesignInterview-CH13.pdf"
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        relations: &[edge_x.clone(), edge_y.clone()],
        evidence: &[evidence.clone(), old_provider_evidence.clone()],
        claims: &[],
    });
    old_event.payload_json = materialized_graph_event_payload_json(
        100,
        std::slice::from_ref(&source),
        &[source_node.clone(), concept_x.clone(), concept_y.clone()],
        &[edge_x.clone(), edge_y],
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
    assert!(replayed.nodes.iter().any(|node| node.node_id == "concept-y"
        && node.valid_to == Some(200)
        && node.superseded_by.as_deref() == Some("evt-provider-new")));
    assert!(replayed
        .relations
        .iter()
        .any(|relation| relation.relation_id == "edge-source-y"
            && relation.valid_to == Some(200)
            && relation.superseded_by.as_deref() == Some("evt-provider-new")));
    assert!(replayed
        .evidence
        .iter()
        .any(|evidence| evidence.id == "ev-source-a"));
    assert!(replayed
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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

    assert!(replayed
        .nodes
        .iter()
        .any(|node| node.node_id == "concept-stale-a"
            && node.valid_to == Some(200)
            && node.superseded_by.as_deref() == Some("evt-full-new")));
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
        valid_from: 0,
        valid_to: None,
        superseded_by: None,
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
