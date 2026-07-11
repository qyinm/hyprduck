use super::super::*;

#[test]
fn import_job_status_strings_use_etyma_lifecycle_names() {
    assert_eq!(ImportJobStatus::Imported.as_str(), "imported");
    assert_eq!(ImportJobStatus::Parsing.as_str(), "parsing");
    assert_eq!(ImportJobStatus::Packaging.as_str(), "packaging");
    assert_eq!(ImportJobStatus::CitationReady.as_str(), "citation_ready");
    assert_eq!(
        ImportJobStatus::CitationReadyGraphPending.as_str(),
        "citation_ready_graph_pending"
    );
    assert_eq!(
        ImportJobStatus::CitationReadyGraphSkipped.as_str(),
        "citation_ready_graph_skipped"
    );
    assert_eq!(
        ImportJobStatus::GraphRetryWaiting.as_str(),
        "graph_retry_waiting"
    );
    assert_eq!(ImportJobStatus::ContextReady.as_str(), "context_ready");
    assert_eq!(ImportJobStatus::Failed.as_str(), "failed");
    assert_eq!(ImportJobStatus::Cancelled.as_str(), "cancelled");
}

#[test]
fn queued_import_job_serializes_as_imported_state() {
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: None,
    };
    let job = ImportJobSnapshot::queued("import-test".into(), &scope);
    let value = job.to_value();

    assert_eq!(value["status"], json!("imported"));
    assert_eq!(value["phase"], json!("imported"));
    assert_eq!(value["citationReady"], json!(false));
    assert_eq!(value["graphReady"], json!(false));
    assert_eq!(value["progressPercent"], json!(0));
}

#[test]
fn import_job_terminal_states_are_context_ready_failed_or_cancelled() {
    assert!(!ImportJobStatus::Imported.is_terminal());
    assert!(!ImportJobStatus::Parsing.is_terminal());
    assert!(!ImportJobStatus::Packaging.is_terminal());
    assert!(!ImportJobStatus::CitationReady.is_terminal());
    assert!(ImportJobStatus::CitationReadyGraphPending.is_terminal());
    assert!(ImportJobStatus::CitationReadyGraphSkipped.is_terminal());
    assert!(!ImportJobStatus::GraphRetryWaiting.is_terminal());
    assert!(ImportJobStatus::ContextReady.is_terminal());
    assert!(ImportJobStatus::Failed.is_terminal());
    assert!(ImportJobStatus::Cancelled.is_terminal());
}

#[test]
fn import_parse_progress_maps_to_lifecycle_states() {
    use etyma_engine_types::ParseProgress;

    assert_eq!(
        import_phase_from_parse_progress(&ParseProgress::Queued),
        (ImportJobPhase::Imported, 2)
    );
    assert_eq!(
        import_phase_from_parse_progress(&ParseProgress::Packaging),
        (ImportJobPhase::Packaging, 68)
    );
    assert_eq!(
        import_phase_from_parse_progress(&ParseProgress::Completed),
        (ImportJobPhase::Packaging, 70)
    );
}

#[test]
fn citation_ready_snapshot_serializes_status_and_readiness() {
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: None,
    };
    let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
    job.status = ImportJobStatus::CitationReady;
    job.phase = ImportJobPhase::CitationReady;
    job.progress_percent = 82;
    job.source_id = Some("source-1".into());
    job.evidence_count = Some(3);
    job.citation_ready = true;

    let value = job.to_value();
    assert_eq!(value["status"], json!("citation_ready"));
    assert_eq!(value["phase"], json!("citation_ready"));
    assert_eq!(value["citationReady"], json!(true));
    assert_eq!(value["evidenceCount"], json!(3));
}

#[test]
fn graph_pending_snapshot_serializes_retry_metadata() {
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: None,
    };
    let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
    job.status = ImportJobStatus::CitationReadyGraphPending;
    job.phase = ImportJobPhase::GraphPending;
    job.citation_ready = true;
    job.graph_ready = false;
    job.graph_status = Some("pending".into());
    job.graph_error_category = Some("db_locked".into());
    job.graph_generation_error_message = Some("database is locked".into());
    job.retryable = true;
    job.retry_attempt = 1;
    job.max_retry_attempts = 2;
    job.next_retry_at = Some(1234);
    job.manual_retry_available = true;

    let value = job.to_value();
    assert_eq!(value["status"], json!("citation_ready_graph_pending"));
    assert_eq!(value["phase"], json!("graph_pending"));
    assert_eq!(value["citationReady"], json!(true));
    assert_eq!(value["graphReady"], json!(false));
    assert_eq!(value["graphErrorCategory"], json!("db_locked"));
    assert_eq!(value["retryable"], json!(true));
    assert_eq!(value["retryAttempt"], json!(1));
    assert_eq!(value["maxRetryAttempts"], json!(2));
    assert_eq!(value["nextRetryAt"], json!(1234));
    assert_eq!(value["manualRetryAvailable"], json!(true));
}

#[test]
fn graph_failure_classifier_keeps_permission_readonly_permanent() {
    let locked = classify_graph_failure("SQLite error: database is locked");
    assert_eq!(locked.category, "db_locked");
    assert!(locked.retryable);

    let readonly_permission = classify_graph_failure("attempt to write a readonly database");
    assert_eq!(readonly_permission.category, "db_readonly");
    assert!(!readonly_permission.retryable);

    let provider_timeout = classify_graph_failure("provider_timeout: request timed out");
    assert_eq!(provider_timeout.category, "provider_timeout");
    assert!(provider_timeout.retryable);
}

#[test]
fn graph_error_sanitizer_redacts_local_paths() {
    let message = "failed to materialize /tmp/etyma/private/source.md\ncaused by more detail";
    let redacted = sanitize_graph_error_message(message);

    assert_eq!(redacted, "failed to materialize [redacted-local-path]");
    assert!(!redacted.contains("/tmp/etyma"));
    assert!(!redacted.contains("caused by"));
}

#[test]
fn graph_status_persist_failure_adds_warning_without_local_paths() {
    let registry = ImportJobRegistry::default();
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: None,
    };
    let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
    job.status = ImportJobStatus::CitationReadyGraphPending;
    job.phase = ImportJobPhase::GraphPending;
    registry.insert(job);

    record_graph_status_persist_result(
        &registry,
        "import-test",
        Err(anyhow!(
            "failed writing /tmp/etyma/private/knowledge.sqlite3"
        )),
    );
    record_graph_status_persist_result(&registry, "import-test", Ok(false));

    let job = registry.get("import-test").expect("job");
    assert_eq!(job.warnings.len(), 2);
    assert!(job.warnings[0].starts_with("graph_status_persist_failed:"));
    assert!(!job.warnings[0].contains("/tmp/etyma"));
    assert_eq!(
        job.warnings[1],
        "graph_status_persist_failed: citation-ready import job was not found"
    );
}

#[test]
fn import_job_cancel_prevents_later_active_updates() {
    let registry = ImportJobRegistry::default();
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: None,
    };
    let job_id = "import-test-cancel".to_string();
    registry.insert(ImportJobSnapshot::queued(job_id.clone(), &scope));

    let cancelled = registry.cancel(&job_id).expect("cancel job");
    assert_eq!(cancelled.status, ImportJobStatus::Cancelled);
    assert_eq!(cancelled.phase, ImportJobPhase::Cancelled);

    registry.update_active(&job_id, |job| {
        job.status = ImportJobStatus::Parsing;
        job.phase = ImportJobPhase::Parsing;
        job.progress_percent = 5;
    });
    let job = registry.get(&job_id).expect("job remains recorded");
    assert_eq!(job.status, ImportJobStatus::Cancelled);
    assert_eq!(job.phase, ImportJobPhase::Cancelled);
    assert_eq!(job.progress_percent, 100);
}
