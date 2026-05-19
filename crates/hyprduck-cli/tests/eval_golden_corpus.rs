use std::process::Command;

#[test]
fn golden_corpus_eval_reports_fixture_metrics() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hyprduck-engine/tests/fixtures/brain-corpus");
    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args([
            "eval",
            "golden-corpus",
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "source-evidence",
        ])
        .output()
        .expect("golden corpus eval should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("golden-corpus cases: 6"));
    assert!(stdout.contains("mode: source-evidence"));
    assert!(stdout.contains("entity recall:"));
    assert!(stdout.contains("claim citation coverage: 12/12 (1.00)"));
    assert!(stdout.contains("relation evidence coverage:"));
    assert!(stdout.contains("context-pack relevance:"));
    assert!(stdout.contains("citation audited answers: 6"));
    assert!(stdout.contains("source ref resolve: 6/6 (1.00)"));
    assert!(stdout.contains("page ref resolve: 6/6 (1.00)"));
    assert!(stdout.contains("evidence ref resolve: 6/6 (1.00)"));
    assert!(stdout.contains("citation correctness: 6/6 (1.00)"));
    assert!(stdout.contains("unsupported claim rate: 0/12 (0.00)"));
    assert!(stdout.contains("latency ms:"));
}

#[test]
fn dry_run_log_reports_conversion_and_reuse_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("dry-run-log.json");
    std::fs::write(&log_path, dry_run_log_json("document_type")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["eval", "dry-run-log", "--input", log_path.to_str().unwrap()])
        .output()
        .expect("dry-run metrics eval should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("dry-run records: 10"));
    assert!(stdout.contains("first cited answer: 6/10"));
    assert!(stdout.contains("same-source second query: 3/10"));
    assert!(stdout.contains("repeated-use events: 10"));
    assert!(stdout.contains("MCP setup time recorded: 6/6"));
    assert!(stdout
        .contains("failure taxonomy: provider_config, parsing, mcp_registration, path, citation"));
    assert!(stdout.contains("stop condition decision: continue"));
    assert!(stdout.contains("sensitive metrics scan: passed"));
}

#[test]
fn dry_run_log_rejects_local_paths_and_content_fields() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("dry-run-log.json");
    std::fs::write(&log_path, dry_run_log_json("/Users/example/private.pdf")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["eval", "dry-run-log", "--input", log_path.to_str().unwrap()])
        .output()
        .expect("dry-run metrics eval should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("sensitive path or document content"));
}

fn dry_run_log_json(document_type: &str) -> String {
    let mut runs = Vec::new();
    for index in 1..=10 {
        let success = index <= 6;
        let milestones = if success {
            let mut milestones = vec![
                "install_started",
                "install_completed",
                "first_import_completed",
                "first_mcp_client_connected",
                "first_get_context_pack_succeeded",
                "first_cited_answer_with_source_refs",
                "first_cited_answer_with_page_refs",
                "first_cited_answer_with_evidence_refs",
            ];
            if index <= 3 {
                milestones.push("same_source_second_query");
                milestones.push("same_source_second_agent_task");
            }
            if index == 2 {
                milestones.push("second_private_document_imported");
            }
            milestones
        } else {
            vec!["install_started"]
        };
        let failure_causes = match index {
            7 => vec!["provider_config"],
            8 => vec!["parsing"],
            9 => vec!["mcp_registration", "path"],
            10 => vec!["citation"],
            _ => Vec::new(),
        };
        runs.push(format!(
            r#"{{
                "runId": "dry-run-{index}",
                "documentType": "{document_type}",
                "providerRoute": "{}",
                "mcpClient": "codex",
                "status": "{}",
                "milestones": {},
                "primaryFailureCauses": {},
                "mcpSetupTimeToSuccessMinutes": {},
                "firstCitedAnswerStepCount": {},
                "citationCorrectness": "{}",
                "unsupportedClaimRate": {}
            }}"#,
            if index % 2 == 0 {
                "ollama/local"
            } else {
                "openrouter/hosted"
            },
            if success { "success" } else { "failure" },
            serde_json::to_string(&milestones).unwrap(),
            serde_json::to_string(&failure_causes).unwrap(),
            if success { "4" } else { "null" },
            if success { "5" } else { "null" },
            if success { "pass" } else { "not_applicable" },
            if success { "0.0" } else { "1.0" }
        ));
    }

    let repeated_use_events = (1..=10)
        .map(|index| {
            format!(
                r#"{{
                    "eventId": "reuse-{index}",
                    "sourceSetHash": "source-set-{}",
                    "sameSourceSetSecondAgentTask": true,
                    "dayOffset": {}
                }}"#,
                (index % 3) + 1,
                if index == 9 {
                    7
                } else if index == 8 {
                    1
                } else {
                    0
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        r#"{{
            "schemaVersion": "hyprduck.dry-run-metrics.v1",
            "generatedAt": "2026-05-19T00:00:00Z",
            "dryRuns": [{}],
            "repeatedUseEvents": [{}],
            "stopConditionReviews": [
                {{
                    "reviewId": "review-1",
                    "dryRunCount": 10,
                    "decision": "continue"
                }}
            ]
        }}"#,
        runs.join(","),
        repeated_use_events
    )
}
