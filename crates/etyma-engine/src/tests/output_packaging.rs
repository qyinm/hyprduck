use super::common::*;
use super::*;

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

    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    assert_eq!(
        source_pack.schema_version,
        etyma_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
    );
    assert_eq!(source_pack.source_id, manifest.source_id);
    assert_eq!(source_pack.page_count, 1);
    assert!(source_pack.content_hash.starts_with("fnv64:"));

    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("parse evidence index v1");
    assert_eq!(
        evidence_index.schema_version,
        etyma_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION
    );
    assert_eq!(evidence_index.source_id, manifest.source_id);
    assert_eq!(evidence_index.content_hash, source_pack.content_hash);
    assert_eq!(evidence_index.evidence.len(), 1);
    assert_eq!(evidence_index.evidence[0].page, 1);
    assert_eq!(
        evidence_index.evidence[0].evidence_type,
        etyma_engine_types::EvidenceType::Text
    );
    assert!(evidence_index.evidence[0]
        .quoted_text
        .contains("Grounded evidence"));
}

#[test]
fn output_package_marks_markdown_tables_as_table_evidence() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.markdown =
        "# Sample import\n\n## Page 1\n\n| Metric | Value |\n| - | - |\n| Revenue | 42 |\n".into();
    result.pages[0].markdown = Some("| Metric | Value |\n| - | - |\n| Revenue | 42 |\n".into());
    result.pages[0].plain_text = Some("Metric Value Revenue 42".into());

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("output manifest");

    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("parse evidence index v1");
    assert_eq!(evidence_index.evidence.len(), 1);
    assert_eq!(
        evidence_index.evidence[0].evidence_type,
        etyma_engine_types::EvidenceType::Table
    );
}

#[test]
fn output_package_marks_image_only_pages_as_image_region_evidence() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.markdown = "# Sample import\n\n## Page 1\n\n![Page 1](images/page_1.png)\n".into();
    result.pages[0].markdown = None;
    result.pages[0].plain_text = None;
    result.pages[0].image_asset_path = Some("images/page_1.png".into());
    result.pages[0].error_message = None;

    let manifest = write_output_package_with_fallback(
        std::slice::from_ref(&fallback_root),
        "sample-import",
        "123",
        &request,
        &result,
        &sample_engine_config(),
    )
    .expect("output manifest");

    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("parse evidence index v1");
    assert_eq!(evidence_index.evidence.len(), 1);
    assert_eq!(
        evidence_index.evidence[0].evidence_type,
        etyma_engine_types::EvidenceType::ImageRegion
    );
    assert!(evidence_index.evidence[0]
        .quoted_text
        .contains("Image region evidence for Page 1"));
}

#[test]
fn output_packaging_uses_requested_workspace_and_source_ids() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let mut request = sample_parse_request(&temp);
    request.output = Some(etyma_engine_types::ParseOutputTarget {
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
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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
fn output_packaging_indexes_fallback_markdown_for_warned_pages() {
    let temp = tempfile::tempdir().expect("temp dir");
    let fallback_root = temp.path().join("output-root");
    let request = sample_parse_request(&temp);
    let mut result = sample_parse_result();
    result.pages = vec![ParsedPage {
        index: 0,
        markdown: Some(
            "_Etyma provider fallback: provider timeout_\n\nRecovered local markdown evidence."
                .into(),
        ),
        plain_text: Some("Recovered local markdown evidence.".into()),
        svg: None,
        image_asset_path: None,
        error_message: Some("provider timeout".into()),
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
    .expect("fallback output manifest");

    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
        &fs::read_to_string(evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");

    assert_eq!(source_pack.warnings.len(), 1);
    assert_eq!(source_pack.warnings[0].warning_type, "page_parse_failed");
    assert_eq!(evidence_index.warnings, source_pack.warnings);
    assert_eq!(evidence_index.evidence.len(), 1);
    assert!(evidence_index.evidence[0]
        .quoted_text
        .contains("Recovered local markdown evidence"));
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

    let source_pack: etyma_engine_types::SourcePackV0 = serde_json::from_str(
        &fs::read_to_string(&response.source_pack_path).expect("source pack json"),
    )
    .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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

    let source_pack: etyma_engine_types::SourcePackV0 = serde_json::from_str(
        &fs::read_to_string(&response.source_pack_path).expect("source pack json"),
    )
    .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");
    let evidence_index: etyma_engine_types::EvidenceIndexV1 = serde_json::from_str(
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
    let source_pack: etyma_engine_types::SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(source_pack_path).expect("source pack json"))
            .expect("source pack");

    assert_eq!(source_pack.provider_route, "ollama");
    assert!(!source_pack.local_only);
}
