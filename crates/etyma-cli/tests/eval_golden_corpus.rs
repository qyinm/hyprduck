use std::process::Command;

#[test]
fn golden_corpus_eval_reports_fixture_metrics() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../etyma-engine/tests/fixtures/brain-corpus");
    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
        .args(["eval", "dry-run-log", "--input", log_path.to_str().unwrap()])
        .output()
        .expect("dry-run metrics eval should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("sensitive path or document content"));
}

#[test]
fn benchmark_report_validates_baselines_and_provider_variance() {
    let temp = tempfile::tempdir().unwrap();
    let report_path = temp.path().join("benchmark-report.json");
    std::fs::write(&report_path, benchmark_report_json(false)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
        .args([
            "eval",
            "benchmark-report",
            "--input",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("benchmark eval should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("benchmark documents: 2"));
    assert!(stdout.contains("benchmark runs: 24"));
    assert!(stdout.contains(
        "baselines: raw_text_dump, direct_upload_chat, context_pack, context_pack_page_evidence"
    ));
    assert!(stdout.contains("provider classes: hosted, local"));
    assert!(stdout.contains("context pack + page evidence citation score:"));
    assert!(stdout.contains("context pack + page evidence unsupported claim rate:"));
    assert!(stdout.contains("comparison outcomes: 1 win, 2 tie, 0 loss"));
    assert!(stdout.contains("benchmark export: valid"));
}

#[test]
fn benchmark_report_rejects_local_paths() {
    let temp = tempfile::tempdir().unwrap();
    let report_path = temp.path().join("benchmark-report.json");
    std::fs::write(&report_path, benchmark_report_json(true)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
        .args([
            "eval",
            "benchmark-report",
            "--input",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("benchmark eval should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("sensitive path or document content"));
}

#[test]
fn benchmark_report_rejects_uncomputed_comparison_claims() {
    let temp = tempfile::tempdir().unwrap();
    let report_path = temp.path().join("benchmark-report.json");
    let body =
        benchmark_report_json(false).replacen(r#""outcome": "tie""#, r#""outcome": "win""#, 1);
    std::fs::write(&report_path, body).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_etyma"))
        .args([
            "eval",
            "benchmark-report",
            "--input",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("benchmark eval should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("claimed win but computed tie"));
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
            "schemaVersion": "etyma.dry-run-metrics.v1",
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

fn benchmark_report_json(include_sensitive: bool) -> String {
    let docs = [
        ("benchmark-pdf", "pdf", ["task-a", "task-b", "task-c"]),
        ("benchmark-docx", "docx", ["task-a", "task-b", "task-c"]),
    ];
    let baselines = [
        "raw_text_dump",
        "direct_upload_chat",
        "context_pack",
        "context_pack_page_evidence",
    ];
    let mut runs = Vec::new();
    let mut index = 0usize;
    for (document_id, _, tasks) in docs {
        for task_id in tasks {
            for baseline in baselines {
                index += 1;
                let is_direct = baseline == "direct_upload_chat";
                let is_context_page = baseline == "context_pack_page_evidence";
                let citation = if is_context_page || baseline == "context_pack" {
                    "pass"
                } else if is_direct {
                    "partial"
                } else {
                    "fail"
                };
                runs.push(format!(
                    r#"{{
                        "runId": "bench-run-{index}",
                        "documentId": "{document_id}",
                        "taskId": "{task_id}",
                        "baseline": "{baseline}",
                        "providerRoute": "{}",
                        "providerClass": "{}",
                        "taskCorrectness": "{}",
                        "citationCorrectness": "{citation}",
                        "unsupportedClaim": {},
                        "usefulAnswerMs": {},
                        "visualTableHandling": "{}",
                        "repeatedUseSuccess": {},
                        "userConfidence": {},
                        "failureTaxonomy": {}
                    }}"#,
                    if index % 2 == 0 {
                        "ollama/local"
                    } else {
                        "openrouter/hosted"
                    },
                    if index % 2 == 0 { "local" } else { "hosted" },
                    if baseline == "raw_text_dump" {
                        "partial"
                    } else {
                        "pass"
                    },
                    if is_direct { "true" } else { "false" },
                    300 + index,
                    if document_id == "benchmark-pdf" {
                        "partial"
                    } else {
                        "pass"
                    },
                    if is_context_page { "true" } else { "false" },
                    if is_context_page { 5 } else { 4 },
                    if is_direct { r#"["citation"]"# } else { "[]" }
                ));
            }
        }
    }
    let doc_id = if include_sensitive {
        "/Users/example/report.pdf"
    } else {
        "benchmark-pdf"
    };
    format!(
        r#"{{
            "schemaVersion": "etyma.benchmark.v1",
            "generatedAt": "2026-05-19T00:00:00Z",
            "documents": [
                {{
                    "documentId": "{doc_id}",
                    "documentType": "pdf",
                    "taskIds": ["task-a", "task-b", "task-c"]
                }},
                {{
                    "documentId": "benchmark-docx",
                    "documentType": "docx",
                    "taskIds": ["task-a", "task-b", "task-c"]
                }}
            ],
            "runs": [{}],
            "comparisons": [
                {{
                    "metric": "citation correctness",
                    "outcome": "win"
                }},
                {{
                    "metric": "useful answer time",
                    "outcome": "tie"
                }},
                {{
                    "metric": "task correctness",
                    "outcome": "tie"
                }}
            ]
        }}"#,
        runs.join(",")
    )
}
