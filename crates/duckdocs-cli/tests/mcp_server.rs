use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_server_exposes_brain_tools_and_policy_proposals() {
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
            "read_health",
            "propose_memory",
            "propose_claim",
            "propose_link",
            "append_observation",
            "add_source_note",
            "request_consolidation",
        ]
    );
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[7]["annotations"]["readOnlyHint"], false);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["destructiveHint"] == false));

    let root_dir = std::env::temp_dir().join(format!("duckdocs-mcp-empty-{}", std::process::id()));
    let root_dir_arg = root_dir.to_string_lossy().to_string();
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
                    "rootDir": root_dir_arg.clone()
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

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "propose_memory",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "actorId": "mcp-test-agent",
                    "title": "Remember MCP contract",
                    "body": "The MCP server should route safe memory through the policy path."
                }
            }
        }),
    );
    let memory = read_message(&mut reader);
    assert_eq!(memory["result"]["isError"], false);
    let text = memory["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let payload: Value = serde_json::from_str(text).expect("memory payload");
    assert_eq!(payload["proposal"]["kind"], "memory");
    assert_eq!(payload["proposal"]["status"], "accepted");
    assert_eq!(payload["event"]["policyResult"], "auto_applied");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "propose_claim",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "actorId": "mcp-test-agent",
                    "title": "Claim needs review",
                    "body": "Claims should wait for review before becoming trusted graph state.",
                    "evidenceRefs": ["evidence-test"]
                }
            }
        }),
    );
    let claim = read_message(&mut reader);
    assert_eq!(claim["result"]["isError"], false);
    let text = claim["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let payload: Value = serde_json::from_str(text).expect("claim payload");
    assert_eq!(payload["proposal"]["kind"], "claim");
    assert_eq!(payload["proposal"]["status"], "pending_review");
    assert_eq!(payload["event"]["policyResult"], "needs_review");
    assert!(std::path::Path::new(payload["proposalPath"].as_str().unwrap()).exists());

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
