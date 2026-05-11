use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_server_exposes_read_only_brain_tools() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_duckdocs-cli"))
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mcp server should start");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "duckdocs-test", "version": "0.1.0" }
            }
        }),
    );
    let initialize = read_message(&mut reader);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "hyprduck");
    assert!(initialize["result"]["capabilities"]["tools"].is_object());

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    );
    let list = read_message(&mut reader);
    let tools = list["result"]["tools"].as_array().expect("tools");
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "search_brain",
            "get_context_pack",
            "read_source",
            "read_wiki_page",
            "read_node",
            "read_recent_events",
            "read_health"
        ]
    );
    assert!(!names.iter().any(|name| name.contains("propose")));
    assert!(tools.iter().all(|tool| {
        tool["annotations"]["readOnlyHint"] == true
            && tool["annotations"]["destructiveHint"] == false
    }));

    let root_dir = std::env::temp_dir().join(format!("duckdocs-mcp-empty-{}", std::process::id()));
    std::fs::create_dir_all(&root_dir).expect("temp root");
    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "read_health",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir
                }
            }
        }),
    );
    let health = read_message(&mut reader);
    assert_eq!(health["result"]["isError"], false);
    let text = health["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let payload: Value = serde_json::from_str(text).expect("health payload");
    assert_eq!(payload["status"], "clean");
    assert_eq!(payload["attentionCount"], 0);

    drop(stdin);
    let status = child.wait().expect("server exit");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(root_dir);
}

fn write_message(stdin: &mut std::process::ChildStdin, message: Value) {
    stdin
        .write_all(serde_json::to_string(&message).unwrap().as_bytes())
        .expect("write message");
    stdin.write_all(b"\n").expect("write newline");
    stdin.flush().expect("flush message");
}

fn read_message(reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read message");
    assert!(!line.trim().is_empty(), "server closed stdout");
    serde_json::from_str(&line).expect("json-rpc response")
}
