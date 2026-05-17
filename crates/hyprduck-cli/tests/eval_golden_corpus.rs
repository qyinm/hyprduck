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
    assert!(stdout.contains("claim citation coverage:"));
    assert!(stdout.contains("relation evidence coverage:"));
    assert!(stdout.contains("context-pack relevance:"));
    assert!(stdout.contains("latency ms:"));
}
