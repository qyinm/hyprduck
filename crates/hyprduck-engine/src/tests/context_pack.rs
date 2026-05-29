use super::*;

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
fn context_pack_artifact_metadata_reports_unsupported_evidence_index_schema() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-future-schema");
    let source_root = workspace_root.join("sources/src-future-schema");
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
            "sourceId": "src-future-schema",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:future-source",
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
            "schemaVersion": "hyprduck.evidence_index.v99",
            "evidence": [{"futureField": true}]
        })
        .to_string(),
    )
    .expect("future evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-future-schema".into(),
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

    assert!(metadata.warnings.iter().any(|warning| warning.warning_type
        == "evidence_index_schema_mismatch"
        && warning.message.contains("hyprduck.evidence_index.v99")));
    assert!(!metadata
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "evidence_index_unreadable"));
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
    assert_eq!(
        evidence.evidence_type,
        hyprduck_engine_types::EvidenceType::Text
    );
}

#[test]
fn context_pack_artifact_metadata_reads_v1_evidence_type() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    let artifact_root = workspace_root.join("artifacts/src-table");
    let source_root = workspace_root.join("sources/src-table");
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
            "sourceId": "src-table",
            "originalFilename": "source.md",
            "originalPath": "source.md",
            "sourcePath": source_path.display().to_string(),
            "markdownPath": markdown_path.display().to_string(),
            "artifactRoot": artifact_root.display().to_string(),
            "contentHash": "fnv64:table",
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
            "schemaVersion": "hyprduck.evidence_index.v1",
            "workspaceId": DEFAULT_WORKSPACE_ID,
            "sourceId": "src-table",
            "contentHash": "fnv64:table",
            "providerRoute": "local_demo",
            "localOnly": true,
            "evidence": [{
                "evidenceRef": "evidence-table",
                "sourceId": "src-table",
                "page": 1,
                "region": "page:Page 1",
                "span": "page",
                "quotedText": "| A | B |",
                "parseConfidence": "high",
                "contentHash": "fnv64:table",
                "markdownPath": markdown_path.display().to_string(),
                "imagePath": null,
                "evidenceType": "table"
            }],
            "warnings": [],
            "generatedAt": 42
        })
        .to_string(),
    )
    .expect("evidence index");

    let metadata = build_context_pack_artifact_metadata(
        &workspace_root,
        &[SourceRecord {
            source_id: "src-table".into(),
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

    let evidence = metadata
        .evidence
        .get("src-table")
        .and_then(|source_evidence| source_evidence.get("evidence-table"))
        .expect("v1 evidence metadata");
    assert_eq!(
        evidence.evidence_type,
        hyprduck_engine_types::EvidenceType::Table
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
fn context_pack_v1_persistence_writes_latest_and_history_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };
    let context_pack = hyprduck_engine_types::ContextPackV1 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
        pack_id: "ctx_test_pack".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV1 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
            evidence_type_trace: hyprduck_engine_types::ContextPackEvidenceTypeTraceV1::default(),
        },
        suggested_next_reads: vec![],
    };

    let path = persist_context_pack_v1(&scope, &context_pack).expect("persist context pack");
    assert!(path.ends_with("default/context_pack.json"));
    let latest = temp.path().join("default/context_pack.json");
    let history = temp.path().join("default/context_packs/ctx_test_pack.json");
    assert!(latest.exists());
    assert!(history.exists());
    let decoded: hyprduck_engine_types::ContextPackV1 =
        serde_json::from_str(&fs::read_to_string(latest).expect("latest context pack"))
            .expect("context pack json");
    assert_eq!(
        decoded.schema_version,
        hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION
    );
    assert_eq!(decoded.pack_id, "ctx_test_pack");
}

#[test]
fn read_context_pack_reads_latest_and_history_without_path_escape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().to_string_lossy().into_owned()),
    };
    let context_pack = hyprduck_engine_types::ContextPackV1 {
        schema_version: hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
        pack_id: "ctx_test_pack".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        query: "agent reuse".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![],
        selected_evidence: vec![],
        findings: vec![],
        warnings: vec![],
        retrieval_trace: hyprduck_engine_types::ContextPackRetrievalTraceV1 {
            strategy: "test".into(),
            chunks_considered: 0,
            chunks_selected: 0,
            budget_requested: 4000,
            budget_used: 0,
            evidence_type_trace: hyprduck_engine_types::ContextPackEvidenceTypeTraceV1::default(),
        },
        suggested_next_reads: vec![],
    };
    persist_context_pack_v1(&scope, &context_pack).expect("persist context pack");

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
