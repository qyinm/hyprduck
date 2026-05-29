use super::*;
use crate::provider::{ollama_models_endpoint, ProviderKind};

mod brain_repo;
mod common;
mod context_pack;
mod materialized_workspace;
mod output_packaging;
mod provider_workspace;
mod workspace_answer;

use common::*;

fn count_table_rows(store: &KnowledgeProjectStore, table: &str) -> usize {
    store
        .run_sql(&format!("SELECT COUNT(*) FROM {table};"))
        .expect("count table rows")
        .trim()
        .parse()
        .expect("parse row count")
}

fn import_job_readiness(store: &KnowledgeProjectStore, source_id: &str) -> String {
    let source_id = source_id.replace('\'', "''");
    store
        .run_sql(&format!(
            "SELECT citation_ready || ':' || graph_ready FROM import_jobs WHERE source_id = '{source_id}';"
        ))
        .expect("read import job readiness")
        .trim()
        .to_string()
}

fn materialized_test_event(snapshot: &BrainRepoSnapshot, event_id: &str) -> BrainEvent {
    BrainEvent {
        event_id: event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "test-agent".into(),
        },
        source_refs: snapshot
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect(),
        source_markdown_refs: snapshot
            .sources
            .iter()
            .map(|source| source.markdown_path.clone())
            .collect(),
        node_refs: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        relation_refs: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        claim_refs: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        memory_refs: Vec::new(),
        target_node_ids: snapshot
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect(),
        target_edge_ids: snapshot
            .relations
            .iter()
            .map(|relation| relation.relation_id.clone())
            .collect(),
        target_claim_ids: snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect(),
        target_memory_ids: Vec::new(),
        evidence_refs: snapshot
            .evidence
            .iter()
            .map(|evidence| evidence.id.clone())
            .collect(),
        payload_json: materialized_graph_event_payload_json(
            snapshot.generated_at,
            &snapshot.sources,
            &snapshot.nodes,
            &snapshot.relations,
            &snapshot.evidence,
            &snapshot.memories,
            &snapshot.wiki_pages,
            &snapshot.entities,
            &snapshot.claims,
            &snapshot.extractions,
        )
        .expect("materialized graph payload"),
        causality: BrainEventCausality {
            caused_by_source_ids: snapshot
                .sources
                .iter()
                .map(|source| source.source_id.clone())
                .collect(),
            snapshot_id: Some(format!("snapshot-{event_id}")),
            materialized_version: Some(snapshot.generated_at),
            ..Default::default()
        },
        confidence: Some("test".into()),
        policy_result: "materialized".into(),
        created_at: snapshot.generated_at,
    }
}

#[test]
fn default_project_store_migrates_legacy_db_to_canonical_name() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store_dir = temp.path().join("HyprDuck");
    fs::create_dir_all(&store_dir).expect("create store dir");
    let legacy_path = store_dir.join("knowledge.sqlite3");
    fs::write(&legacy_path, b"legacy sqlite bytes").expect("write legacy store");

    let store = KnowledgeProjectStore::from_data_root(temp.path()).expect("default store");

    assert_eq!(store.path, store_dir.join("hyprduck.sqlite"));
    assert!(legacy_path.exists());
    assert_eq!(
        fs::read(store.path).expect("read canonical store"),
        b"legacy sqlite bytes"
    );
}

#[test]
fn default_project_store_preserves_legacy_project_source_and_correction_rows() {
    let temp = tempfile::tempdir().expect("temp dir");
    let legacy_path = temp.path().join("HyprDuck").join("knowledge.sqlite3");
    let legacy_store = KnowledgeProjectStore::new(legacy_path.clone());
    let markdown = "# Source import\n\n## Page 1\n\nLegacy project rows stay linked.\n";
    let (project, manifest) =
        compile_manifest_fixture_project_with_source(&temp, markdown, "source-legacy", "alpha", 10);
    let request = CompileProjectRequest {
        source_markdown_path: temp.path().join("sample.md").display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };
    legacy_store
        .save_project(&project, &request, Some(&manifest))
        .expect("save legacy project rows");
    legacy_store
        .append_workspace_correction(&WorkspaceCorrection {
            id: "correction-legacy".into(),
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            aggregate_node_id: "concept-legacy".into(),
            kind: CorrectionKind::Delete,
            target_node_id: None,
            value: None,
            evidence_ids: vec!["evidence-legacy".into()],
            source_node_ids: vec!["source-legacy:concept-legacy".into()],
            created_at: 11,
        })
        .expect("save legacy correction row");

    let canonical_store =
        KnowledgeProjectStore::from_data_root(temp.path()).expect("default store");

    assert_eq!(
        canonical_store.path,
        temp.path().join("HyprDuck").join("hyprduck.sqlite")
    );
    assert!(legacy_path.exists());
    assert_eq!(
        canonical_store
            .load_project(Some(&project.summary.project_id))
            .expect("load migrated project")
            .expect("migrated project")
            .summary
            .project_id,
        project.summary.project_id
    );
    assert_eq!(
        canonical_store
            .load_sources(DEFAULT_WORKSPACE_ID)
            .expect("load migrated sources")
            .len(),
        1
    );
    assert_eq!(
        canonical_store
            .load_workspace_corrections(DEFAULT_WORKSPACE_ID)
            .expect("load migrated corrections")
            .len(),
        1
    );
}

#[test]
fn source_import_persists_canonical_db_rows_and_fts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = KnowledgeProjectStore::new(temp.path().join("hyprduck.sqlite"));
    let markdown = "# Source import\n\n## Page 1\n\nCanonical source evidence persists.\n";
    let (project, manifest) =
        compile_manifest_fixture_project_with_source(&temp, markdown, "source-db", "alpha", 10);
    let request = CompileProjectRequest {
        source_markdown_path: temp.path().join("sample.md").display().to_string(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(manifest.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: None,
    };

    store
        .save_project(&project, &request, Some(&manifest))
        .expect("save project and canonical source rows");

    assert_eq!(count_table_rows(&store, "import_jobs"), 1);
    assert_eq!(import_job_readiness(&store, "source-db"), "1:0");
    assert_eq!(count_table_rows(&store, "sources"), 1);
    assert_eq!(count_table_rows(&store, "source_pages"), 1);
    assert!(count_table_rows(&store, "evidence_items") > 0);
    assert!(count_table_rows(&store, "evidence_fts") > 0);
    assert!(store
        .run_sql("SELECT manifest_json FROM sources WHERE source_id = 'source-db';")
        .expect("read manifest json")
        .contains("\"source_id\":\"source-db\""));
    assert_eq!(
        store
            .run_sql("SELECT source_path_redacted FROM sources WHERE source_id = 'source-db';")
            .expect("read redacted source path")
            .trim(),
        "alpha.pdf"
    );
}

#[test]
fn latest_readable_marker_classifies_json_artifacts_as_migration_input() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let mut snapshot = synthetic_context_pack_snapshot(1);
    snapshot.generated_at = 42;
    snapshot.events = vec![materialized_test_event(
        &snapshot,
        "event-materialized-json-role",
    )];

    write_materialized_brain_repo(&workspace_root, &snapshot).expect("write materialized state");

    let marker: Value = serde_json::from_str(
        &fs::read_to_string(workspace_root.join(LATEST_READABLE_SNAPSHOT_PATH))
            .expect("read latest readable marker"),
    )
    .expect("decode latest readable marker");
    assert_eq!(marker["artifactRole"], "migration_input");
    assert_eq!(marker["canonicalStateStore"], "hyprduck.sqlite+graphqlite");
    assert!(marker["materializedFiles"]
        .as_array()
        .expect("materialized files array")
        .iter()
        .any(|path| path == "brain-manifest.json"));
}

#[test]
fn brain_reader_open_migrates_existing_materialized_json_into_sqlite() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("materialized dirs");

    let mut snapshot = synthetic_context_pack_snapshot(1);
    snapshot.generated_at = 43;
    snapshot.wiki_pages = vec![WikiPage {
        page_id: "wiki-alpha".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        path: "wiki/topics/alpha.md".into(),
        title: "Alpha Migration".into(),
        body: "Alpha migration wiki body.".into(),
        node_refs: vec!["concept-alpha-000".into()],
        source_refs: vec!["source-alpha".into()],
        evidence_refs: vec!["ev-alpha-000".into()],
        updated_at: snapshot.generated_at,
    }];
    snapshot.events = vec![materialized_test_event(
        &snapshot,
        "event-migrate-json-artifacts",
    )];

    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write manifest");
    write_json_pretty(&workspace_root.join("graph/nodes.json"), &snapshot.nodes)
        .expect("write nodes");
    write_json_pretty(
        &workspace_root.join("graph/edges.json"),
        &snapshot.relations,
    )
    .expect("write edges");
    write_json_pretty(
        &workspace_root.join("graph/evidence.json"),
        &snapshot.evidence,
    )
    .expect("write evidence");
    write_json_pretty(
        &workspace_root.join("graph/entities.json"),
        &snapshot.entities,
    )
    .expect("write entities");
    write_json_pretty(&workspace_root.join("graph/claims.json"), &snapshot.claims)
        .expect("write claims");
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &snapshot.memories,
    )
    .expect("write memories");
    write_brain_events_jsonl(
        &workspace_root.join("events/brain_events.jsonl"),
        &snapshot.events,
    )
    .expect("write events");
    assert!(!workspace_root.join("hyprduck.sqlite").exists());

    let reader = BrainReader::open_workspace_root(workspace_root.clone(), DEFAULT_WORKSPACE_ID)
        .expect("open reader and migrate artifacts");
    assert_eq!(reader.snapshot.nodes, snapshot.nodes);

    let store = KnowledgeProjectStore::new(workspace_root.join("hyprduck.sqlite"));
    assert_eq!(count_table_rows(&store, "sources"), 1);
    assert_eq!(count_table_rows(&store, "source_pages"), 1);
    assert_eq!(count_table_rows(&store, "wiki_pages"), 1);
    assert_eq!(count_table_rows(&store, "brain_events"), 1);
    assert!(count_table_rows(&store, "evidence_items") > 0);
    assert!(count_table_rows(&store, "evidence_fts") > 0);
    let graph_store =
        KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root)).expect("open");
    assert!(
        graph_store
            .graph_snapshot_counts(DEFAULT_WORKSPACE_ID)
            .expect("graph counts")
            .node_count
            > 0
    );
}

#[test]
fn brain_reader_migration_preserves_artifact_metadata() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("materialized dirs");

    let mut snapshot = synthetic_context_pack_snapshot(1);
    snapshot.generated_at = 44;
    snapshot.events = vec![materialized_test_event(
        &snapshot,
        "event-preserve-artifact-metadata",
    )];

    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write manifest");
    write_json_pretty(&workspace_root.join("graph/nodes.json"), &snapshot.nodes)
        .expect("write nodes");
    write_json_pretty(
        &workspace_root.join("graph/edges.json"),
        &snapshot.relations,
    )
    .expect("write edges");
    write_json_pretty(
        &workspace_root.join("graph/evidence.json"),
        &snapshot.evidence,
    )
    .expect("write evidence");
    write_json_pretty(
        &workspace_root.join("graph/entities.json"),
        &snapshot.entities,
    )
    .expect("write entities");
    write_json_pretty(&workspace_root.join("graph/claims.json"), &snapshot.claims)
        .expect("write claims");
    write_json_pretty(
        &workspace_root.join("memory/records.json"),
        &snapshot.memories,
    )
    .expect("write memories");
    write_brain_events_jsonl(
        &workspace_root.join("events/brain_events.jsonl"),
        &snapshot.events,
    )
    .expect("write events");

    let artifact_root = workspace_root.join("artifacts/source-alpha");
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.source_pack.v0",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "source-alpha",
            "originalFilename": "alpha.pdf",
            "originalPath": "alpha.pdf",
            "sourcePath": snapshot.sources[0].source_path.as_str(),
            "markdownPath": snapshot.sources[0].markdown_path.as_str(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:preserved-alpha",
            "format": "pdf",
            "pageCount": 1,
            "ingestionStatus": "ingested",
            "providerRoute": "ollama:llama3.2",
            "localOnly": true,
            "pages": [],
            "warnings": [{
                "type": "ocr_low_confidence",
                "severity": "medium",
                "message": "Low OCR confidence on page 1.",
                "page": 1
            }],
            "createdAt": 1,
            "updatedAt": 1
        })
        .to_string(),
    )
    .expect("source pack");
    fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.evidence_index.v1",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "source-alpha",
            "contentHash": "fnv64:preserved-alpha",
            "providerRoute": "ollama:llama3.2",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "ev-alpha-000",
                "sourceId": "source-alpha",
                "page": 1,
                "region": "bbox:10,20,300,140",
                "span": "text:15..92",
                "quotedText": "Preserved table evidence text.",
                "parseConfidence": "high",
                "contentHash": "fnv64:preserved-alpha",
                "markdownPath": "/private/tmp/source-alpha/page-1.md",
                "imagePath": "/private/tmp/source-alpha/page-1.png",
                "evidenceType": "table"
            }],
            "warnings": [],
            "generatedAt": 44
        })
        .to_string(),
    )
    .expect("evidence index");

    fs::write(
        workspace_root.join("context_pack.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.context_pack.v1",
            "packId": "ctx_preserved",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "query": "preserve artifact metadata",
            "generatedAt": "2026-05-29T09:53:26Z"
        })
        .to_string(),
    )
    .expect("latest context pack");
    fs::create_dir_all(workspace_root.join("context_packs")).expect("context packs dir");
    fs::write(
        workspace_root.join("context_packs/ctx_preserved.json"),
        serde_json::json!({
            "schemaVersion": "hyprduck.context_pack.v1",
            "packId": "ctx_preserved",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "query": "preserve artifact metadata",
            "generatedAt": "2026-05-29T09:53:26Z"
        })
        .to_string(),
    )
    .expect("history context pack");

    BrainReader::open_workspace_root(workspace_root.clone(), DEFAULT_WORKSPACE_ID)
        .expect("open reader and preserve artifact metadata");

    let store = KnowledgeProjectStore::new(workspace_root.join("hyprduck.sqlite"));
    assert_eq!(
        store
            .run_sql("SELECT provider_route || '|' || provider_locality || '|' || content_hash FROM sources WHERE source_id = 'source-alpha';")
            .expect("read source metadata")
            .trim(),
        "ollama:llama3.2|local|fnv64:preserved-alpha"
    );
    let evidence_row = store
        .run_sql("SELECT evidence_type || '|' || snippet || '|' || span_json || '|' || region_json || '|' || markdown_path_redacted || '|' || image_path_redacted || '|' || confidence FROM evidence_items WHERE evidence_id = 'ev-alpha-000';")
        .expect("read evidence metadata");
    assert!(evidence_row.contains("table_evidence|Preserved table evidence text."));
    assert!(evidence_row.contains(r#"{"span":"text:15..92"}"#));
    assert!(evidence_row.contains(r#"{"region":"bbox:10,20,300,140"}"#));
    assert!(evidence_row.contains("page-1.md|page-1.png|1.0"));
    assert_eq!(
        store
            .run_sql("SELECT COUNT(*) FROM context_pack_exports WHERE pack_id = 'ctx_preserved';")
            .expect("read context pack export count")
            .trim(),
        "2"
    );
    assert_eq!(
        store
            .run_sql("SELECT export_path FROM context_pack_exports WHERE is_latest = 1 AND pack_id = 'ctx_preserved';")
            .expect("read latest context pack export")
            .trim(),
        "context_pack.json"
    );
}

#[test]
fn resolving_workspace_root_loads_graphqlite_store() {
    let _guard = TEST_ENV_LOCK.lock().expect("env lock");
    let temp = tempfile::tempdir().expect("temp dir");
    let previous_output_root = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", temp.path());

    let workspace_root = resolve_brain_workspace_root(&BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: None,
    })
    .expect("resolve workspace root");

    match previous_output_root {
        Some(value) => std::env::set_var("HYPRDUCK_OUTPUT_DIR", value),
        None => std::env::remove_var("HYPRDUCK_OUTPUT_DIR"),
    }

    let store =
        KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root)).expect("open");
    let health = store.health().expect("knowledge store health");
    assert!(health.graphqlite_loaded);
    assert!(health.graphqlite_transactional);
    assert!(workspace_root.join("hyprduck.sqlite").exists());
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
                evidence_type: hyprduck_engine_types::EvidenceType::Text,
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
