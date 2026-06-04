use super::common::*;
use super::*;

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
    let governance = health.governance.as_ref().expect("governance report");
    assert_eq!(governance.storage_locality, "local_workspace");
    assert_eq!(governance.interaction_surface, "desktop_mcp");
    assert!(governance.evidence_governed);
    assert!(governance.mutating_tools_require_evidence);
    assert_eq!(governance.local_path_disclosure_default, "redacted");
    let store = health
        .knowledge_store
        .as_ref()
        .expect("knowledge store report");
    assert_eq!(store.canonical_storage, "sqlite+graphqlite");
    assert_eq!(store.primary_graph_store, "graphqlite");
    assert!(store.pure_sqlite_relational_graph_rejected);
    assert!(store.optional_graphqlite_acceleration_rejected);
    assert_eq!(store.graph_store_mode, "required_primary");
    assert_eq!(store.graph_native_query_surface, "graphqlite_cypher");
    assert_eq!(store.migration_mode, "single_db_first_release");
    assert!(store.long_dual_write_transition_rejected);
    assert!(store.graphqlite_loaded);
    assert!(store.graphqlite_transactional);
    assert_eq!(store.graphqlite_release_gate, "passed");
    assert!(store.release_blocked_without_graphqlite);
    assert_eq!(store.migration_blast_radius, "high");
    assert!(store.broad_verification_required);
    assert!(!store.json_artifacts_canonical);
    assert_eq!(store.json_artifact_role, "migration_export_debug_compat");
    assert!(!store.vector_search_enabled);
    assert_eq!(
        store.vector_search_policy,
        "defer_until_db_graphqlite_read_paths_stabilize"
    );
    assert!(!store.checkpoint_rollback_api_enabled);
    assert_eq!(
        store.checkpoint_rollback_policy,
        "defer_until_checkpoints_reliably_stored"
    );
    assert!(!store.graph_algorithms_enabled);
    assert_eq!(
        store.graph_algorithm_policy,
        "revisit_after_primary_graph_data_stabilizes"
    );
    assert_eq!(store.evidence_item_count, 0);
    assert_eq!(store.wiki_page_count, 0);
    assert_eq!(store.graph_node_count, 0);
    assert_eq!(store.graph_relation_count, 0);
    assert!(health.source_reports.is_empty());
    assert!(health.recent_events.is_empty());
}

#[test]
fn agent_session_write_rejects_unknown_evidence_ref() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
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

    let error = handle_write_propose(WriteProposeRequest {
        scope: BrainReadScope {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            root_dir: Some(temp.path().display().to_string()),
        },
        content_type: "memory".into(),
        title: "Rejected".into(),
        body: "Missing evidence should reject this proposal.".into(),
        evidence_refs: vec!["missing-ev".into()],
    })
    .expect_err("unknown evidence rejected");
    assert!(error.to_string().contains("missing-ev"));
}

#[test]
fn agent_session_write_rejects_path_traversal_proposal_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("memory")).expect("memory dir");
    write_json_pretty::<Vec<MemoryRecord>>(&workspace_root.join("memory/records.json"), &vec![])
        .expect("write memory records");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let error = handle_write_reject(WriteRejectRequest {
        scope,
        proposal_id: "../memory/records".into(),
    })
    .expect_err("path traversal proposal id rejected");

    assert!(error.to_string().contains("invalid proposalId"));
    assert!(workspace_root.join("memory/records.json").exists());
}

#[test]
fn agent_session_write_revalidates_evidence_on_commit() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");

    let evidence = EvidenceRef {
        id: "ev-agent-write-stale".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Agent-session write MCP validates stale evidence.".into(),
        source_path: Some("/private/docs/source.pdf".into()),
        source_id: Some("source-agent-write".into()),
        markdown_path: Some("artifacts/source-agent-write/pages/page_1.md".into()),
        image_path: None,
        provenance: Some("test fixture".into()),
    };
    let mut snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: vec![evidence.clone()],
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
    write_json_pretty(&workspace_root.join("graph/evidence.json"), &vec![evidence])
        .expect("write evidence");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let proposal = handle_write_propose(WriteProposeRequest {
        scope: scope.clone(),
        content_type: "memory".into(),
        title: "Stale evidence".into(),
        body: "This proposal should not commit after evidence changes.".into(),
        evidence_refs: vec!["ev-agent-write-stale".into()],
    })
    .expect("propose write");

    snapshot.evidence.clear();
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write stale manifest");
    write_json_pretty::<Vec<EvidenceRef>>(&workspace_root.join("graph/evidence.json"), &vec![])
        .expect("write stale evidence");

    let error = handle_write_commit(WriteCommitRequest {
        scope,
        proposal_id: proposal.proposal_id,
        user_approved: false,
    })
    .expect_err("stale evidence rejected");
    assert!(error
        .to_string()
        .contains("ev-agent-write-stale was not found"));
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

#[test]
fn brain_health_accepts_fresh_v1_evidence_index() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace_root.join("events")).expect("events dir");
    fs::create_dir_all(workspace_root.join("artifacts/src-v1")).expect("artifact dir");

    write_health_test_snapshot(
        &workspace_root,
        SourceRecord {
            source_id: "src-v1".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            original_path: "/private/docs/v1.pdf".into(),
            source_path: "/private/hyprduck/sources/src-v1/v1.pdf".into(),
            markdown_path: "/private/hyprduck/artifacts/src-v1/v1.md".into(),
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
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: "src-v1".into(),
        original_filename: "v1.pdf".into(),
        original_path: "/private/docs/v1.pdf".into(),
        source_path: "/private/hyprduck/sources/src-v1/v1.pdf".into(),
        markdown_path: "/private/hyprduck/artifacts/src-v1/v1.md".into(),
        artifact_root: "/private/hyprduck/artifacts/src-v1".into(),
        content_hash: "sha256:v1".into(),
        format: DocumentFormat::Pdf,
        page_count: 1,
        ingestion_status: IngestStatus::Ingested,
        provider_route: "local_demo".into(),
        local_only: true,
        pages: Vec::new(),
        warnings: Vec::new(),
        created_at: 10,
        updated_at: 20,
    };
    let evidence_index = hyprduck_engine_types::EvidenceIndexV1 {
        schema_version: hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION.into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        source_id: "src-v1".into(),
        content_hash: "sha256:v1".into(),
        provider_route: "local_demo".into(),
        local_only: true,
        evidence: Vec::new(),
        warnings: Vec::new(),
        generated_at: 20,
    };
    write_json_pretty(
        &workspace_root.join("artifacts/src-v1/source_pack.json"),
        &source_pack,
    )
    .expect("write source pack");
    write_json_pretty(
        &workspace_root.join("artifacts/src-v1/evidence_index.json"),
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
    assert_eq!(report.content_hash_status, "current");
    assert!(!report
        .warnings
        .contains(&"evidence_index_schema_mismatch".into()));
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
        include_local_paths: false,
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
    assert_eq!(
        response.evidence[0].markdown_path.as_deref(),
        Some("page_1.md")
    );
    assert!(response.warnings.is_empty());
}

#[test]
fn legacy_read_source_and_page_evidence_redact_local_paths_by_default() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-legacy-redaction");
    let source_root = workspace_root.join("sources/src-legacy-redaction");
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    fs::create_dir_all(artifact_root.join("pages")).expect("pages dir");
    fs::create_dir_all(&source_root).expect("source dir");
    let source_path = source_root.join("source.pdf");
    let markdown_path = artifact_root.join("pages/page_1.md");
    fs::write(&source_path, b"source bytes").expect("write source");
    fs::write(&markdown_path, "# legacy page\n").expect("write markdown");
    let source = SourceRecord {
        source_id: "src-legacy-redaction".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        original_path: workspace_root.join("original.pdf").display().to_string(),
        source_path: source_path.display().to_string(),
        markdown_path: workspace_root
            .join("artifacts/src-legacy-redaction/source.md")
            .display()
            .to_string(),
        format: hyprduck_engine_types::SourceFormat::pdf(),
        status: hyprduck_engine_types::SourceStatus::ingested(),
        page_count: 1,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        updated_at: 1,
    };
    let evidence = EvidenceRef {
        id: "ev-legacy-redaction".into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: "Legacy fallback evidence.".into(),
        source_path: Some(source_path.display().to_string()),
        source_id: Some(source.source_id.clone()),
        markdown_path: Some(markdown_path.display().to_string()),
        image_path: None,
        provenance: None,
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: vec![source.clone()],
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: vec![evidence.clone()],
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
    write_json_pretty(&workspace_root.join("graph/evidence.json"), &vec![evidence])
        .expect("write evidence");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": source.source_id,
            "contentHash": "fnv64:legacy-redaction",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-legacy-redaction",
                "sourceId": "src-legacy-redaction",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "Legacy fallback evidence.",
                "parseConfidence": "high",
                "contentHash": "fnv64:legacy-redaction",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null
            }],
            "warnings": [],
            "generatedAt": 1
        })
        .to_string(),
    )
    .expect("evidence index");

    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let source_response = handle_read_source(ReadSourceRequest {
        scope: scope.clone(),
        source_id: source.source_id.clone(),
        include_local_paths: false,
    })
    .expect("read source");
    let source_json = serde_json::to_string(&source_response).expect("source json");
    assert!(!source_json.contains(workspace_root.to_string_lossy().as_ref()));
    assert_eq!(source_response.source.source_path, "source.pdf");
    assert_eq!(
        source_response.evidence[0].markdown_path.as_deref(),
        Some("page_1.md")
    );

    let page_response = handle_read_page_evidence(ReadPageEvidenceRequest {
        scope,
        source_id: source.source_id,
        page: Some(1),
        include_local_paths: false,
    })
    .expect("read page evidence");
    let page_json = serde_json::to_string(&page_response).expect("page json");
    assert!(!page_json.contains(workspace_root.to_string_lossy().as_ref()));
    assert_eq!(
        page_response.evidence[0].markdown_path.as_deref(),
        Some("page_1.md")
    );
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
        include_local_paths: false,
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
