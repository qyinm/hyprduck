use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_server_exposes_brain_tools_and_policy_proposals() {
    let root_dir = std::env::temp_dir().join(format!("hyprduck-mcp-empty-{}", std::process::id()));
    let root_dir_arg = root_dir.to_string_lossy().to_string();
    fs::create_dir_all(&root_dir).expect("temp root");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck-cli"))
        .args(["mcp", "serve"])
        .env("HYPRDUCK_PROJECT_STORE", root_dir.join("knowledge.sqlite3"))
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
                "clientInfo": { "name": "hyprduck-test", "version": "0.1.0" }
            }
        }),
    );
    let initialize = read_message(&mut reader);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "hyprduck");
    assert!(initialize["result"]["capabilities"]["tools"].is_object());
    assert!(initialize["result"]["capabilities"]["resources"].is_object());

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
            "read_graph_history",
            "read_graph_snapshot",
            "read_health",
            "propose_node",
            "propose_memory",
            "propose_claim",
            "propose_link",
            "append_observation",
            "add_source_note",
            "request_consolidation",
        ]
    );
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[9]["annotations"]["readOnlyHint"], false);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["destructiveHint"] == false));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "resources/list",
            "params": {}
        }),
    );
    let resources = read_message(&mut reader);
    let resource_uris = resources["result"]["resources"]
        .as_array()
        .expect("resources")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("resource uri"))
        .collect::<Vec<_>>();
    assert!(resource_uris.contains(&"hyprduck://brain/default/graph/snapshot"));
    assert!(resource_uris.contains(&"hyprduck://brain/default/wiki/index.md"));

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

    write_mcp_snapshot_workspace(&root_dir);
    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "resources/read",
            "params": {
                "uri": format!("hyprduck://brain/default/graph/snapshot?rootDir={}", root_dir_arg)
            }
        }),
    );
    let snapshot_resource = read_message(&mut reader);
    let snapshot_text = snapshot_resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("snapshot text");
    let snapshot: Value = serde_json::from_str(snapshot_text).expect("snapshot payload");
    assert_eq!(snapshot["snapshotId"], "snapshot-mcp-readable");
    assert_eq!(
        snapshot["latestReadableSnapshotPath"],
        "state/latest-readable-snapshot.json"
    );
    assert_eq!(snapshot["nodes"][0]["nodeId"], "node-mcp-readable");
    assert_eq!(snapshot["wikiPages"][0]["body"], "# MCP Snapshot\n");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "resources/read",
            "params": {
                "uri": format!("hyprduck://brain/default/wiki/index.md?rootDir={}", root_dir_arg)
            }
        }),
    );
    let wiki_resource = read_message(&mut reader);
    assert_eq!(
        wiki_resource["result"]["contents"][0]["mimeType"],
        "text/markdown"
    );
    assert_eq!(
        wiki_resource["result"]["contents"][0]["text"],
        "# MCP Snapshot\n"
    );

    let source_dir = root_dir.join("default/sources");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::write(
        source_dir.join("agent-cache-refresh.md"),
        "# Agent Cache Refresh\n\n## Page 1\n\nMCP read paths refresh graph and wiki state after markdown ingest.\n",
    )
    .expect("markdown source");
    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 23,
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
    let refreshed_health = read_message(&mut reader);
    assert_eq!(
        refreshed_health["result"]["isError"], false,
        "{refreshed_health:#?}"
    );
    assert_eq!(
        refreshed_health["result"]["_meta"]["hyprduckGraphWikiCache"]["invalidated"], true,
        "{refreshed_health:#?}"
    );
    assert_ne!(
        refreshed_health["result"]["_meta"]["hyprduckGraphWikiCache"]["current"]["snapshotId"],
        "snapshot-mcp-readable"
    );
    assert_eq!(
        refreshed_health["result"]["_meta"]["hyprduckGraphWikiCache"]["current"]
            ["latestReadableSnapshotPath"],
        "state/latest-readable-snapshot.json"
    );
    let refreshed_snapshot_id = refreshed_health["result"]["_meta"]["hyprduckGraphWikiCache"]
        ["current"]["snapshotId"]
        .as_str()
        .expect("refreshed snapshot id")
        .to_string();

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 24,
            "method": "resources/read",
            "params": {
                "uri": format!("hyprduck://brain/default/graph/snapshot?rootDir={}", root_dir_arg)
            }
        }),
    );
    let refreshed_snapshot_resource = read_message(&mut reader);
    let refreshed_snapshot_text = refreshed_snapshot_resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("refreshed snapshot text");
    let refreshed_snapshot: Value =
        serde_json::from_str(refreshed_snapshot_text).expect("refreshed snapshot payload");
    assert_eq!(refreshed_snapshot["snapshotId"], refreshed_snapshot_id);
    assert!(refreshed_snapshot["sourcePaths"]
        .as_array()
        .expect("source paths")
        .iter()
        .any(|path| path
            .as_str()
            .is_some_and(|path| path.ends_with("/sources/agent-cache-refresh.md"))));
    assert!(refreshed_snapshot["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .any(|node| node["label"] == "Agent Cache Refresh"));
    assert!(refreshed_snapshot["wikiPages"]
        .as_array()
        .expect("wiki pages")
        .iter()
        .any(|page| page["body"]
            .as_str()
            .is_some_and(|body| body.contains("Agent Cache Refresh"))));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 25,
            "method": "resources/read",
            "params": {
                "uri": format!("hyprduck://brain/default/wiki/index.md?rootDir={}", root_dir_arg)
            }
        }),
    );
    let refreshed_wiki_resource = read_message(&mut reader);
    let refreshed_wiki_text = refreshed_wiki_resource["result"]["contents"][0]["text"]
        .as_str()
        .expect("refreshed wiki text");
    assert_ne!(refreshed_wiki_text, "# MCP Snapshot\n");
    assert!(refreshed_wiki_text.contains("Agent Cache Refresh"));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 26,
            "method": "tools/call",
            "params": {
                "name": "read_graph_snapshot",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone()
                }
            }
        }),
    );
    let refreshed_snapshot_tool = read_message(&mut reader);
    assert_eq!(
        refreshed_snapshot_tool["result"]["isError"], false,
        "{refreshed_snapshot_tool:#?}"
    );
    let text = refreshed_snapshot_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("tool text");
    let tool_snapshot: Value = serde_json::from_str(text).expect("tool snapshot payload");
    assert_eq!(tool_snapshot["snapshotId"], refreshed_snapshot_id);
    assert!(tool_snapshot["sourcePaths"]
        .as_array()
        .expect("tool source paths")
        .iter()
        .any(|path| path
            .as_str()
            .is_some_and(|path| path.ends_with("/sources/agent-cache-refresh.md"))));
    assert!(tool_snapshot["wikiPages"]
        .as_array()
        .expect("tool wiki pages")
        .iter()
        .any(|page| page["body"]
            .as_str()
            .is_some_and(|body| body.contains("Agent Cache Refresh"))));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 27,
            "method": "tools/call",
            "params": {
                "name": "read_wiki_page",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "path": "wiki/index.md"
                }
            }
        }),
    );
    let refreshed_wiki_tool = read_message(&mut reader);
    assert_eq!(
        refreshed_wiki_tool["result"]["isError"], false,
        "{refreshed_wiki_tool:#?}"
    );
    let text = refreshed_wiki_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("wiki tool text");
    let tool_wiki: Value = serde_json::from_str(text).expect("wiki tool payload");
    assert_eq!(tool_wiki["page"]["path"], "wiki/index.md");
    assert_ne!(tool_wiki["page"]["body"], "# MCP Snapshot\n");
    assert!(tool_wiki["page"]["body"]
        .as_str()
        .expect("wiki body")
        .contains("Agent Cache Refresh"));

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

fn write_mcp_snapshot_workspace(root_dir: &std::path::Path) {
    let workspace = root_dir.join("default");
    fs::create_dir_all(workspace.join("events")).expect("events dir");
    fs::create_dir_all(workspace.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace.join("memory")).expect("memory dir");
    fs::create_dir_all(workspace.join("state")).expect("state dir");
    fs::create_dir_all(workspace.join("wiki")).expect("wiki dir");
    fs::write(
        workspace.join("brain-manifest.json"),
        r##"{"workspaceId":"default","generatedAt":42,"sources":[],"nodes":[{"nodeId":"node-mcp-readable","kind":"concept","label":"MCP readable","scope":"project","aliases":[],"evidenceIds":[],"sourceIds":["source-mcp"],"confidence":null,"updatedAt":42}],"relations":[],"evidence":[],"memories":[],"wikiPages":[{"pageId":"wiki-mcp-readable","workspaceId":"default","path":"wiki/index.md","title":"MCP Snapshot","body":"","nodeRefs":["node-mcp-readable"],"sourceRefs":["source-mcp"],"evidenceRefs":[],"updatedAt":42}],"entities":[],"claims":[],"extractions":[],"events":[]}"##,
    )
    .expect("manifest");
    fs::write(
        workspace.join("graph/nodes.json"),
        r#"[{"nodeId":"node-mcp-readable","kind":"concept","label":"MCP readable","scope":"project","aliases":[],"evidenceIds":[],"sourceIds":["source-mcp"],"confidence":null,"updatedAt":42}]"#,
    )
    .expect("nodes");
    fs::write(workspace.join("graph/edges.json"), "[]").expect("edges");
    fs::write(workspace.join("graph/evidence.json"), "[]").expect("evidence");
    fs::write(workspace.join("graph/claims.json"), "[]").expect("claims");
    fs::write(workspace.join("memory/records.json"), "[]").expect("memories");
    fs::write(workspace.join("wiki/index.md"), "# MCP Snapshot\n").expect("wiki index");
    fs::write(
        workspace.join("events/brain_events.jsonl"),
        concat!(
            r#"{"eventId":"event-mcp-readable","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"mcp-test"},"sourceRefs":["source-mcp"],"sourceMarkdownRefs":["wiki/index.md"],"nodeRefs":["node-mcp-readable"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"nodeCount\":1,\"relationCount\":0,\"claimCount\":0,\"memoryCount\":0,\"wikiPageCount\":1}","causality":{"causedByEventIds":[],"causedByProposalId":null,"causedBySourceIds":["source-mcp"],"snapshotId":"snapshot-mcp-readable","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":42},"confidence":null,"policyResult":"applied","createdAt":42}"#,
            "\n"
        ),
    )
    .expect("events");
    fs::write(
        workspace.join("state/latest-readable-snapshot.json"),
        r#"{"schemaVersion":1,"workspaceId":"default","snapshotId":"snapshot-mcp-readable","eventId":"event-mcp-readable","sourceIngestId":"source-mcp","materializedAt":42,"publishedAt":42,"sourceMarkdownRefs":["wiki/index.md"],"materializedFiles":["brain-manifest.json","events/brain_events.jsonl","graph/claims.json","graph/edges.json","graph/nodes.json","memory/records.json","wiki/index.md"]}"#,
    )
    .expect("latest readable marker");
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
