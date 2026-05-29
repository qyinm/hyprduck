use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use hyprduck_engine_types::{
    EngineConfigPayload, EngineSuccess, EvidenceIndexV1, EvidenceType, ParseResponseData,
    RuntimeReadinessResponseData, SourcePackV0,
};
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
        .join("crates/hyprduck-engine/tests/fixtures")
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
    let output_root = output_dir.keep();
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
                "root_dir": output_root,
                "name": format!("fixture-{format}")
            }
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-engine"))
        .env("HYPRDUCK_CONFIG_DIR", config_dir.path())
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
    assert_eq!(
        result.result.pages[0].image_asset_path.as_deref(),
        Some("images/page_1.png")
    );
    assert!(result.result.markdown.contains("## Page 1"));
    assert!(result
        .result
        .markdown
        .contains("![Page 1](images/page_1.png)"));
    assert!(result.saved_output_path.is_some());
    assert_source_artifacts(&result, "pdf");
}

#[test]
fn docx_fixture_round_trips_through_engine() {
    let result = run_parse("sample.docx", "docx");
    assert_eq!(result.result.pages.len(), 1);
    assert_eq!(result.result.assets.len(), 1);
    assert_eq!(
        result.result.pages[0].image_asset_path.as_deref(),
        Some("images/page_1.svg")
    );
    assert!(result
        .result
        .markdown
        .contains("![Page 1](images/page_1.svg)"));
    assert!(result.saved_output_path.is_some());
    assert_source_artifacts(&result, "docx");
}

#[test]
fn doc_fixture_round_trips_through_engine() {
    let result = run_parse("sample.doc", "doc");
    assert_eq!(result.result.pages.len(), 1);
    assert_eq!(result.result.assets.len(), 1);
    assert_eq!(
        result.result.pages[0].image_asset_path.as_deref(),
        Some("images/page_1.svg")
    );
    assert!(result
        .result
        .markdown
        .contains("![Page 1](images/page_1.svg)"));
    assert!(result.saved_output_path.is_some());
    assert_source_artifacts(&result, "doc");
}

fn assert_source_artifacts(result: &ParseResponseData, format: &str) {
    let manifest = result.source_manifest.as_ref().expect("source manifest");
    assert_eq!(
        manifest.status,
        hyprduck_engine_types::IngestStatus::Ingested
    );
    assert_eq!(manifest.pages.len(), 1);
    assert!(Path::new(&manifest.source_path).exists());
    assert!(Path::new(&manifest.markdown_path).exists());
    assert!(manifest.pages[0]
        .markdown_path
        .as_deref()
        .is_some_and(|path| Path::new(path).exists()));
    assert!(manifest.pages[0]
        .image_path
        .as_deref()
        .is_some_and(|path| Path::new(path).exists()));

    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let source_pack: SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(&source_pack_path).expect("source pack json"))
            .expect("source pack");
    assert_eq!(source_pack.source_id, manifest.source_id);
    assert_eq!(source_pack.format, manifest.format);
    assert_eq!(source_pack.page_count, 1, "{format} page count");
    assert_eq!(source_pack.pages.len(), 1, "{format} source pack pages");
    assert!(source_pack.pages[0].image_path.is_some());

    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    let evidence_index: EvidenceIndexV1 = serde_json::from_str(
        &fs::read_to_string(&evidence_index_path).expect("evidence index json"),
    )
    .expect("evidence index");
    assert_eq!(
        source_pack.schema_version,
        hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
    );
    assert_eq!(
        evidence_index.schema_version,
        hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION
    );
    assert_eq!(source_pack.workspace_id, manifest.workspace_id);
    assert_eq!(evidence_index.workspace_id, manifest.workspace_id);
    assert_eq!(evidence_index.source_id, manifest.source_id);
    assert_eq!(source_pack.content_hash, evidence_index.content_hash);
    assert_eq!(source_pack.provider_route, evidence_index.provider_route);
    assert_eq!(source_pack.local_only, evidence_index.local_only);

    if format != "doc" {
        assert_eq!(evidence_index.evidence.len(), 1, "{format} evidence count");
        assert_eq!(evidence_index.evidence[0].source_id, manifest.source_id);
        assert_eq!(evidence_index.evidence[0].page, 1);
        assert_eq!(evidence_index.evidence[0].evidence_type, EvidenceType::Text);
        assert!(!evidence_index.evidence[0].evidence_ref.is_empty());
        assert!(!evidence_index.evidence[0].quoted_text.trim().is_empty());
        assert_eq!(
            evidence_index.evidence[0].content_hash,
            source_pack.content_hash
        );
    }
}

#[test]
fn readiness_reports_runtime_config_and_dependencies() {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());
    let request = json!({
        "command": "check_readiness",
        "payload": {}
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-engine"))
        .env("HYPRDUCK_CONFIG_DIR", config_dir.path())
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
    let response: EngineSuccess<RuntimeReadinessResponseData> =
        serde_json::from_slice(&output.stdout).expect("readiness success envelope");
    assert_eq!(
        response.command,
        hyprduck_engine_types::EngineCommand::CheckReadiness
    );
    assert_eq!(response.data.provider, "ollama");
    assert!(response
        .data
        .checks
        .iter()
        .any(|check| check.id == "runtime_process" && check.ready));
    assert!(response
        .data
        .checks
        .iter()
        .any(|check| check.id == "config_file" && check.ready));
    assert!(response
        .data
        .checks
        .iter()
        .any(|check| check.id == "provider_config" && check.ready));
    assert!(response
        .data
        .checks
        .iter()
        .any(|check| check.id == "pdf_converter"));
    assert!(response
        .data
        .checks
        .iter()
        .any(|check| check.id == "ollama_endpoint"));
}

#[test]
fn serve_mode_handles_multiple_requests_without_restarting() {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-engine"))
        .arg("serve")
        .env("HYPRDUCK_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("engine server run");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    let request_ids = [
        "019e0b95-7f53-7502-8886-e8c01d3aaad4",
        "019e0b95-7f54-7502-8886-e8c01d3aaad4",
    ];

    for request_id in request_ids {
        stdin
            .write_all(
                format!(r#"{{"id":"{request_id}","command":"load_config","payload":{{}}}}"#)
                    .as_bytes(),
            )
            .expect("request write");
        stdin.write_all(b"\n").expect("request newline");

        let mut line = String::new();
        reader.read_line(&mut line).expect("response line");
        let response: serde_json::Value =
            serde_json::from_str(&line).expect("load config envelope");
        assert_eq!(response["id"], request_id);
        assert_eq!(response["type"], "response");
        let response: EngineSuccess<EngineConfigPayload> =
            serde_json::from_value(response).expect("load config response");
        assert_eq!(response.data.provider, "ollama");
    }

    drop(stdin);
    let status = child.wait().expect("engine server exit");
    assert!(
        status.success(),
        "serve mode should exit cleanly on stdin close"
    );
}

#[test]
fn serve_mode_rejects_non_uuidv7_request_ids() {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-engine"))
        .arg("serve")
        .env("HYPRDUCK_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("engine server run");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    stdin
        .write_all(
            br#"{"id":"67e55044-10b1-426f-9247-bb680e5fe0c8","command":"load_config","payload":{}}"#,
        )
        .expect("request write");
    stdin.write_all(b"\n").expect("request newline");

    let mut line = String::new();
    reader.read_line(&mut line).expect("response line");
    let response: serde_json::Value = serde_json::from_str(&line).expect("failure envelope");
    assert_eq!(response["id"], "67e55044-10b1-426f-9247-bb680e5fe0c8");
    assert_eq!(response["type"], "response");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "invalid_request_id");

    drop(stdin);
    let status = child.wait().expect("engine server exit");
    assert!(status.success());
}

#[test]
fn serve_mode_wraps_parse_progress_events_with_request_id() {
    let config_dir = tempdir().expect("config dir");
    write_ollama_config(config_dir.path());
    let output_dir = tempdir().expect("output dir");
    let request_id = "019e0b95-7f55-7502-8886-e8c01d3aaad4";

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-engine"))
        .arg("serve")
        .env("HYPRDUCK_CONFIG_DIR", config_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("engine server run");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let mut reader = BufReader::new(stdout);
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let stderr_reader = BufReader::new(stderr);
        for line in stderr_reader.lines().map_while(Result::ok) {
            if stderr_tx.send(line).is_err() {
                break;
            }
        }
    });
    let request = json!({
        "id": request_id,
        "command": "parse",
        "payload": {
            "version": "1",
            "template": "General",
            "input": {
                "path": fixture_path("sample.pdf"),
                "format": "pdf"
            },
            "options": {
                "debug_request_path": null,
                "debug_result_path": null
            },
            "output": {
                "root_dir": output_dir.path(),
                "name": "runtime-envelope-fixture"
            }
        }
    });
    stdin
        .write_all(
            serde_json::to_vec(&request)
                .expect("request json")
                .as_slice(),
        )
        .expect("request write");
    stdin.write_all(b"\n").expect("request newline");

    let mut line = String::new();
    reader.read_line(&mut line).expect("runtime line");
    let envelope: serde_json::Value = serde_json::from_str(&line).expect("runtime envelope");
    assert_eq!(envelope["id"], request_id);
    match envelope["type"].as_str() {
        Some("response") => {
            assert_eq!(envelope["command"], "parse");
            assert_eq!(envelope["ok"], true);
        }
        other => panic!("unexpected runtime envelope type: {other:?}"),
    }

    let mut saw_event = false;
    for line in stderr_rx
        .recv_timeout(Duration::from_secs(1))
        .into_iter()
        .chain(stderr_rx.try_iter())
    {
        let envelope: serde_json::Value =
            serde_json::from_str(&line).expect("runtime event envelope");
        if envelope["type"] == "event" {
            assert_eq!(envelope["id"], request_id);
            assert!(envelope["event"]["type"].is_string());
            saw_event = true;
        }
    }
    assert!(
        saw_event,
        "parse should emit request-scoped event envelopes on stderr"
    );

    drop(stdin);
    let status = child.wait().expect("engine server exit");
    assert!(status.success());
}
