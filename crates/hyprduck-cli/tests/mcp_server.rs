use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_server_exposes_read_only_brain_tools() {
    let root_dir = std::env::temp_dir().join(format!("hyprduck-mcp-empty-{}", std::process::id()));
    let root_dir_arg = root_dir.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&root_dir);
    fs::create_dir_all(&root_dir).expect("temp root");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "serve"])
        .env("HYPRDUCK_PROJECT_STORE", root_dir.join("knowledge.sqlite3"))
        .env("HYPRDUCK_MCP_ALLOW_ROOT_DIR", "1")
        .env("HYPRDUCK_DISABLE_PROVIDER_GRAPH", "1")
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
            "get_context_pack",
            "search_documents",
            "search_brain",
            "read_source",
            "read_page_evidence",
            "read_wiki_page",
            "read_node",
            "read_recent_events",
            "read_graph_history",
            "read_graph_snapshot",
            "read_health",
        ]
    );
    assert_eq!(tools[0]["name"], "get_context_pack");
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["readOnlyHint"] == true));
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["destructiveHint"] == false));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 19,
            "method": "tools/call",
            "params": {
                "name": "read_page_evidence",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourceId": "source-mcp",
                    "page": 0
                }
            }
        }),
    );
    let invalid_page = read_message(&mut reader);
    assert_eq!(invalid_page["result"]["isError"], true);
    assert!(invalid_page["result"]["content"][0]["text"]
        .as_str()
        .expect("error text")
        .contains("positive 1-based"));

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
    assert_eq!(
        snapshot_resource["result"]["contents"][0]["uri"],
        "hyprduck://brain/default/graph/snapshot"
    );
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
    assert_eq!(snapshot["sourcePaths"][0], "[redacted-local-path]");

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
        wiki_resource["result"]["contents"][0]["uri"],
        "hyprduck://brain/default/wiki/index.md"
    );
    assert_eq!(
        wiki_resource["result"]["contents"][0]["mimeType"],
        "text/markdown"
    );
    assert_eq!(
        wiki_resource["result"]["contents"][0]["text"],
        "# MCP Snapshot\n"
    );

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
    let snapshot_health = read_message(&mut reader);
    assert_eq!(
        snapshot_health["result"]["isError"], false,
        "{snapshot_health:#?}"
    );
    assert_eq!(
        snapshot_health["result"]["_meta"]["hyprduckGraphWikiCache"]["invalidated"], false,
        "{snapshot_health:#?}"
    );
    assert_eq!(
        snapshot_health["result"]["_meta"]["hyprduckGraphWikiCache"]["current"]["snapshotId"],
        "snapshot-mcp-readable"
    );
    assert_eq!(
        snapshot_health["result"]["_meta"]["hyprduckGraphWikiCache"]["current"]
            ["latestReadableSnapshotPath"],
        "state/latest-readable-snapshot.json"
    );
    assert!(!root_dir
        .join("default/state/maintenance-latest.json")
        .exists());

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
    let snapshot_resource_after_health = read_message(&mut reader);
    let snapshot_text_after_health = snapshot_resource_after_health["result"]["contents"][0]
        ["text"]
        .as_str()
        .expect("snapshot text after health");
    let snapshot_after_health: Value =
        serde_json::from_str(snapshot_text_after_health).expect("snapshot payload after health");
    assert_eq!(snapshot_after_health["snapshotId"], "snapshot-mcp-readable");

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
    let wiki_resource_after_health = read_message(&mut reader);
    let wiki_text_after_health = wiki_resource_after_health["result"]["contents"][0]["text"]
        .as_str()
        .expect("wiki text after health");
    assert_eq!(wiki_text_after_health, "# MCP Snapshot\n");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 28,
            "method": "tools/call",
            "params": {
                "name": "read_source",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourceId": "source-mcp"
                }
            }
        }),
    );
    let redacted_source_tool = read_message(&mut reader);
    let text = redacted_source_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("redacted source text");
    let redacted_source: Value = serde_json::from_str(text).expect("redacted source payload");
    assert_eq!(
        redacted_source["source"]["sourcePath"],
        "[redacted-local-path]"
    );
    assert_eq!(
        redacted_source["evidence"][0]["markdownPath"],
        "[redacted-local-path]"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 29,
            "method": "tools/call",
            "params": {
                "name": "read_source",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourceId": "source-mcp",
                    "includeLocalPaths": true
                }
            }
        }),
    );
    let unredacted_source_tool = read_message(&mut reader);
    let text = unredacted_source_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("unredacted source text");
    let unredacted_source: Value = serde_json::from_str(text).expect("unredacted source payload");
    assert!(unredacted_source["source"]["sourcePath"]
        .as_str()
        .expect("source path")
        .contains(root_dir_arg.as_str()));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 30,
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
    assert_eq!(tool_snapshot["snapshotId"], "snapshot-mcp-readable");

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 31,
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
    assert_eq!(tool_wiki["page"]["body"], "# MCP Snapshot\n");

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
        format!(
            r##"{{"workspaceId":"default","generatedAt":42,"sources":[{{"sourceId":"source-mcp","workspaceId":"default","originalPath":"{root}/source.pdf","sourcePath":"{root}/sources/source-mcp/source.pdf","markdownPath":"{root}/artifacts/source-mcp/source.md","format":"pdf","status":"ingested","pageCount":1,"description":"","userContext":"","ingestInstruction":"","updatedAt":42}}],"nodes":[{{"nodeId":"node-mcp-readable","kind":"concept","label":"MCP readable","scope":"project","aliases":[],"evidenceIds":["evidence-mcp"],"sourceIds":["source-mcp"],"confidence":null,"updatedAt":42}}],"relations":[],"evidence":[{{"id":"evidence-mcp","pageLabel":"Page 1","pageIndex":0,"snippet":"MCP source evidence","sourcePath":"{root}/sources/source-mcp/source.pdf","sourceId":"source-mcp","markdownPath":"{root}/artifacts/source-mcp/pages/page_1.md","imagePath":"{root}/artifacts/source-mcp/images/page_1.png","provenance":null}}],"memories":[],"wikiPages":[{{"pageId":"wiki-mcp-readable","workspaceId":"default","path":"wiki/index.md","title":"MCP Snapshot","body":"","nodeRefs":["node-mcp-readable"],"sourceRefs":["source-mcp"],"evidenceRefs":["evidence-mcp"],"updatedAt":42}}],"entities":[],"claims":[],"extractions":[],"events":[]}}"##,
            root = workspace.display()
        ),
    )
    .expect("manifest");
    fs::write(
        workspace.join("graph/nodes.json"),
        r#"[{"nodeId":"node-mcp-readable","kind":"concept","label":"MCP readable","scope":"project","aliases":[],"evidenceIds":[],"sourceIds":["source-mcp"],"confidence":null,"updatedAt":42}]"#,
    )
    .expect("nodes");
    fs::write(workspace.join("graph/edges.json"), "[]").expect("edges");
    fs::write(
        workspace.join("graph/evidence.json"),
        format!(
            r#"[{{"id":"evidence-mcp","pageLabel":"Page 1","pageIndex":0,"snippet":"MCP source evidence","sourcePath":"{root}/sources/source-mcp/source.pdf","sourceId":"source-mcp","markdownPath":"{root}/artifacts/source-mcp/pages/page_1.md","imagePath":"{root}/artifacts/source-mcp/images/page_1.png","provenance":null}}]"#,
            root = workspace.display()
        ),
    )
    .expect("evidence");
    fs::write(workspace.join("graph/claims.json"), "[]").expect("claims");
    fs::write(workspace.join("memory/records.json"), "[]").expect("memories");
    fs::write(workspace.join("wiki/index.md"), "# MCP Snapshot\n").expect("wiki index");
    fs::write(
        workspace.join("events/brain_events.jsonl"),
        concat!(
            r#"{"eventId":"event-mcp-readable","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"mcp-test"},"sourceRefs":["source-mcp"],"sourceMarkdownRefs":["wiki/index.md"],"nodeRefs":["node-mcp-readable"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"nodeCount\":1,\"relationCount\":0,\"claimCount\":0,\"memoryCount\":0,\"wikiPageCount\":1}","causality":{"causedByEventIds":[],"causedBySourceIds":["source-mcp"],"snapshotId":"snapshot-mcp-readable","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":42},"confidence":null,"policyResult":"applied","createdAt":42}"#,
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
