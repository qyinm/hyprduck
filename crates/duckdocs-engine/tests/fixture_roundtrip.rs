use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use duckdocs_engine_types::{EngineConfigPayload, EngineSuccess, ParseResponseData};
use serde_json::json;
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture_path(name: &str) -> PathBuf {
    repo_root()
        .join("crates/duckdocs-engine/tests/fixtures")
        .join(name)
}

fn write_ollama_config(config_dir: &Path) {
    fs::create_dir_all(config_dir).expect("config dir");
    fs::write(
        config_dir.join("engine-config.json"),
        serde_json::to_vec_pretty(&json!({
            "provider": "ollama",
            "model_id": "llama3.2",
            "api_key": "",
            "base_url": "http://127.0.0.1:11434/api/generate",
            "prompt_template": "General"
        }))
        .expect("config json"),
    )
    .expect("config write");
}

fn run_parse(fixture: &str, format: &str) -> ParseResponseData {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());

    let output_dir = tempdir().expect("output dir");
    let request = json!({
        "command": "parse",
        "payload": {
            "version": "1",
            "template": "General",
            "input": {
                "path": fixture_path(fixture),
                "format": format
            },
            "options": {
                "debug_request_path": null,
                "debug_result_path": null
            },
            "output": {
                "root_dir": output_dir.path(),
                "name": format!("fixture-{format}")
            }
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_duckdocs-engine"))
        .env("DUCKDOCS_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("engine run");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            serde_json::to_vec(&request)
                .expect("request json")
                .as_slice(),
        )
        .expect("request write");
    let output = child.wait_with_output().expect("engine output");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: EngineSuccess<ParseResponseData> =
        serde_json::from_slice(&output.stdout).expect("parse success envelope");
    response.data
}

#[test]
fn pdf_fixture_round_trips_through_engine() {
    let result = run_parse("sample.pdf", "pdf");
    assert_eq!(result.result.pages.len(), 1);
    assert_eq!(result.result.assets.len(), 1);
    assert!(result.result.markdown.contains("## Page 1"));
    assert!(result.saved_output_path.is_some());
}

#[test]
fn docx_fixture_round_trips_through_engine() {
    let result = run_parse("sample.docx", "docx");
    assert_eq!(result.result.pages.len(), 1);
    assert!(result.saved_output_path.is_some());
}

#[test]
fn doc_fixture_round_trips_through_engine() {
    let result = run_parse("sample.doc", "doc");
    assert_eq!(result.result.pages.len(), 1);
    assert!(result.saved_output_path.is_some());
}

#[test]
fn serve_mode_handles_multiple_requests_without_restarting() {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_duckdocs-engine"))
        .arg("serve")
        .env("DUCKDOCS_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("engine server run");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    for _ in 0..2 {
        stdin
            .write_all(br#"{"command":"load_config","payload":{}}"#)
            .expect("request write");
        stdin.write_all(b"\n").expect("request newline");

        let mut line = String::new();
        reader.read_line(&mut line).expect("response line");
        let response: EngineSuccess<EngineConfigPayload> =
            serde_json::from_str(&line).expect("load config response");
        assert_eq!(response.data.provider, "ollama");
    }

    drop(stdin);
    let status = child.wait().expect("engine server exit");
    assert!(
        status.success(),
        "serve mode should exit cleanly on stdin close"
    );
}
