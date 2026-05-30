use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn mcp_server_exposes_read_and_agent_session_write_brain_tools() {
    let root_dir = std::env::temp_dir().join(format!("hyprduck-mcp-empty-{}", std::process::id()));
    let root_dir_arg = root_dir.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&root_dir);
    fs::create_dir_all(&root_dir).expect("temp root");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "serve"])
        .env("HYPRDUCK_PROJECT_STORE", root_dir.join("knowledge.sqlite3"))
        .env("HYPRDUCK_MCP_ALLOW_ROOT_DIR", "1")
        .env("HYPRDUCK_MCP_ALLOWED_ROOTS", &root_dir_arg)
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
            "import_source",
            "import_status",
            "import_cancel",
            "get_context_pack",
            "read_context_pack",
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
            "write_propose",
            "write_commit",
            "write_commit_all",
            "write_list",
            "write_reject",
        ]
    );
    assert_eq!(tools[0]["name"], "import_source");
    assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);
    let read_only_by_name = tools
        .iter()
        .map(|tool| {
            (
                tool["name"].as_str().expect("tool name"),
                tool["annotations"]["readOnlyHint"]
                    .as_bool()
                    .expect("readOnlyHint"),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for name in [
        "get_context_pack",
        "import_status",
        "read_context_pack",
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
        "write_list",
    ] {
        assert_eq!(read_only_by_name[name], true, "{name} should be read-only");
    }
    for name in [
        "import_source",
        "import_cancel",
        "write_propose",
        "write_commit",
        "write_commit_all",
        "write_reject",
    ] {
        assert_eq!(read_only_by_name[name], false, "{name} should mutate state");
    }
    assert!(tools
        .iter()
        .all(|tool| tool["annotations"]["destructiveHint"] == false));
    let retired_surface_terms = ["trust console", "review queue", "governance", "rollback"];
    for tool in tools {
        let text = format!(
            "{} {}",
            tool["name"].as_str().unwrap_or_default(),
            tool["description"].as_str().unwrap_or_default()
        )
        .to_ascii_lowercase();
        for retired_term in retired_surface_terms {
            assert!(
                !text.contains(retired_term),
                "retired MCP surface term {retired_term:?} leaked through tool metadata: {text}"
            );
        }
    }

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
    assert_eq!(payload["knowledgeStore"]["primaryGraphStore"], "graphqlite");
    assert_eq!(
        payload["knowledgeStore"]["pureSqliteRelationalGraphRejected"],
        true
    );
    assert_eq!(
        payload["knowledgeStore"]["optionalGraphqliteAccelerationRejected"],
        true
    );
    assert_eq!(
        payload["knowledgeStore"]["graphStoreMode"],
        "required_primary"
    );
    assert_eq!(
        payload["knowledgeStore"]["graphNativeQuerySurface"],
        "graphqlite_cypher"
    );
    assert_eq!(
        payload["knowledgeStore"]["migrationMode"],
        "single_db_first_release"
    );
    assert_eq!(
        payload["knowledgeStore"]["longDualWriteTransitionRejected"],
        true
    );
    assert_eq!(payload["knowledgeStore"]["graphqliteReleaseGate"], "passed");
    assert_eq!(
        payload["knowledgeStore"]["releaseBlockedWithoutGraphqlite"],
        true
    );
    assert_eq!(payload["knowledgeStore"]["migrationBlastRadius"], "high");
    assert_eq!(payload["knowledgeStore"]["broadVerificationRequired"], true);
    assert_eq!(payload["governance"]["storageLocality"], "local_workspace");
    assert_eq!(payload["governance"]["interactionSurface"], "desktop_mcp");
    assert_eq!(payload["governance"]["evidenceGoverned"], true);
    assert_eq!(payload["governance"]["mutatingToolsRequireEvidence"], true);
    assert_eq!(
        payload["governance"]["localPathDisclosureDefault"],
        "redacted"
    );

    write_mcp_snapshot_workspace(&root_dir);
    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 120,
            "method": "tools/call",
            "params": {
                "name": "read_context_pack",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone()
                }
            }
        }),
    );
    let persisted_context_pack_tool = read_message(&mut reader);
    assert_eq!(
        persisted_context_pack_tool["result"]["isError"], false,
        "{persisted_context_pack_tool:#?}"
    );
    let text = persisted_context_pack_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("persisted context pack tool text");
    let persisted_context_pack: Value =
        serde_json::from_str(text).expect("persisted context pack payload");
    assert_eq!(
        persisted_context_pack["contextPack"]["schemaVersion"],
        "hyprduck.context_pack.v0"
    );
    assert_eq!(persisted_context_pack["contextPack"]["packId"], "ctx_mcp");
    let comparable_client_answer =
        cited_answer_from_context_pack(&persisted_context_pack["contextPack"]);
    assert!(
        comparable_client_answer.contains("Indexed MCP page evidence"),
        "{comparable_client_answer}"
    );
    assert!(
        comparable_client_answer.contains("sourceId=source-mcp"),
        "{comparable_client_answer}"
    );
    assert!(
        comparable_client_answer.contains("page=1"),
        "{comparable_client_answer}"
    );
    assert!(
        comparable_client_answer.contains("evidenceRef=evidence-mcp"),
        "{comparable_client_answer}"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 121,
            "method": "tools/call",
            "params": {
                "name": "read_context_pack",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "packId": "missing-pack"
                }
            }
        }),
    );
    let missing_context_pack_tool = read_message(&mut reader);
    assert_eq!(missing_context_pack_tool["result"]["isError"], true);
    let missing_text = missing_context_pack_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("missing context pack error text");
    assert!(missing_text.contains("persisted context pack could not be read or decoded"));
    assert!(!missing_text.contains(root_dir_arg.as_str()));

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 122,
            "method": "tools/call",
            "params": {
                "name": "get_context_pack",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "query": "MCP evidence",
                    "budget": 4000
                }
            }
        }),
    );
    let context_pack_tool = read_message(&mut reader);
    assert_eq!(
        context_pack_tool["result"]["isError"], false,
        "{context_pack_tool:#?}"
    );
    let text = context_pack_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("context pack tool text");
    let context_pack_payload: Value = serde_json::from_str(text).expect("context pack payload");
    assert_eq!(
        context_pack_payload["contextPack"]["schemaVersion"],
        "hyprduck.context_pack.v1"
    );
    assert_eq!(
        context_pack_payload["contextPackV1"]["schemaVersion"],
        "hyprduck.context_pack.v1"
    );
    assert_eq!(
        context_pack_payload["contextPackV0"]["schemaVersion"],
        "hyprduck.context_pack.v0"
    );
    assert!(context_pack_payload.get("contextPack").is_some());
    assert!(context_pack_payload.get("contextPackV1").is_some());
    assert!(context_pack_payload.get("contextPackV0").is_some());
    assert!(context_pack_payload["contextPack"]["selectedEvidence"][0]
        .get("evidenceType")
        .is_some());
    assert!(context_pack_payload["contextPack"]["retrievalTrace"]
        .get("evidenceTypeTrace")
        .is_some());
    assert!(!text.contains("originalPath"));
    assert!(!text.contains("sourcePath"));
    assert!(!text.contains("markdownPath"));
    assert!(!text.contains("provider-graph-candidates"));
    assert!(!text.contains("provider-graph-source-raw-merged"));
    let context_pack_answer =
        cited_answer_from_context_pack(&context_pack_payload["contextPackV0"]);
    assert!(
        context_pack_answer.contains("sourceId=source-mcp"),
        "{context_pack_answer}"
    );
    assert!(
        context_pack_answer.contains("evidenceRef=evidence-mcp"),
        "{context_pack_answer}"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 123,
            "method": "tools/call",
            "params": {
                "name": "search_documents",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "query": "MCP source",
                    "limit": 5
                }
            }
        }),
    );
    let search_tool = read_message(&mut reader);
    assert_eq!(search_tool["result"]["isError"], false, "{search_tool:#?}");
    let text = search_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("search tool text");
    let search_payload: Value = serde_json::from_str(text).expect("search payload");
    assert!(search_payload["results"]
        .as_array()
        .expect("search results")
        .iter()
        .any(|result| result["id"] == "source-mcp" || result["id"] == "evidence-mcp"));

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
    assert_eq!(
        snapshot["wikiPages"][0]["body"],
        "# MCP Snapshot\n\nLocal path: [redacted-local-path]\n"
    );
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
        "# MCP Snapshot\n\nLocal path: [redacted-local-path]\n"
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
    assert_eq!(
        wiki_text_after_health,
        "# MCP Snapshot\n\nLocal path: [redacted-local-path]\n"
    );

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
                "name": "read_page_evidence",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourceId": "source-mcp",
                    "page": 1
                }
            }
        }),
    );
    let page_evidence_tool = read_message(&mut reader);
    assert_eq!(
        page_evidence_tool["result"]["isError"], false,
        "{page_evidence_tool:#?}"
    );
    let text = page_evidence_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("page evidence tool text");
    let page_evidence: Value = serde_json::from_str(text).expect("page evidence payload");
    assert_eq!(page_evidence["evidence"][0]["evidenceRef"], "evidence-mcp");
    assert_eq!(
        page_evidence["evidence"][0]["quotedText"],
        "Indexed MCP page evidence"
    );
    assert_eq!(page_evidence["evidence"][0]["parseConfidence"], "high");
    assert_eq!(
        page_evidence["evidence"][0]["markdownPath"],
        "[redacted-local-path]"
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 31,
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
            "id": 32,
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
    assert_eq!(
        tool_wiki["page"]["body"],
        "# MCP Snapshot\n\nLocal path: [redacted-local-path]\n"
    );

    drop(stdin);
    let status = child.wait().expect("server exit");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(root_dir);
}

#[test]
fn mcp_server_import_source_imports_allowlisted_markdown() {
    let root_dir = temp_test_dir("hyprduck-mcp-import-workspace");
    let import_root = temp_test_dir("hyprduck-mcp-import-source");
    let root_dir_arg = root_dir.to_string_lossy().to_string();
    let import_root_arg = import_root.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&root_dir);
    let _ = fs::remove_dir_all(&import_root);
    fs::create_dir_all(&root_dir).expect("temp root");
    fs::create_dir_all(&import_root).expect("import root");
    let source_path = import_root.join("agent-notes.md");
    fs::write(
        &source_path,
        "# Agent Notes\n\nMCP import should create cited evidence for agents.\n",
    )
    .expect("source markdown");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "serve"])
        .env("HYPRDUCK_PROJECT_STORE", root_dir.join("knowledge.sqlite3"))
        .env("HYPRDUCK_MCP_ALLOW_ROOT_DIR", "1")
        .env("HYPRDUCK_MCP_ALLOWED_ROOTS", &root_dir_arg)
        .env("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS", &import_root_arg)
        .env("HYPRDUCK_DISABLE_PROVIDER_GRAPH", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mcp server should start");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    initialize_mcp_session(&mut stdin, &mut reader);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "tools/call",
            "params": {
                "name": "import_source",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourcePath": source_path.to_string_lossy(),
                    "format": "markdown",
                    "skipGraphGeneration": true
                }
            }
        }),
    );
    let import_tool = read_message(&mut reader);
    assert_eq!(import_tool["result"]["isError"], false, "{import_tool:#?}");
    let text = import_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("import tool text");
    assert!(!text.contains(root_dir_arg.as_str()));
    assert!(!text.contains(import_root_arg.as_str()));
    let import_payload: Value = serde_json::from_str(text).expect("import payload");
    assert_eq!(import_payload["workspaceId"], "default");
    assert_eq!(import_payload["status"], "imported");
    assert_eq!(import_payload["phase"], "imported");
    assert_eq!(import_payload["citationReady"], false);
    let job_id = import_payload["jobId"]
        .as_str()
        .expect("import job id")
        .to_string();
    let status_payload = poll_import_until_citation_ready(
        &mut stdin,
        &mut reader,
        &job_id,
        "default",
        root_dir_arg.clone(),
    );
    assert!(!status_payload.to_string().contains(root_dir_arg.as_str()));
    assert!(!status_payload
        .to_string()
        .contains(import_root_arg.as_str()));
    assert_eq!(status_payload["workspaceId"], "default");
    assert_eq!(status_payload["status"], "context_ready");
    assert_eq!(status_payload["phase"], "context_ready");
    assert_eq!(status_payload["citationReady"], true);
    assert_eq!(status_payload["graphReady"], false);
    assert_eq!(status_payload["graphStatus"], "skipped");
    let source_id = status_payload["sourceId"]
        .as_str()
        .expect("import source id")
        .to_string();
    assert!(!source_id.trim().is_empty());
    assert!(
        status_payload["evidenceCount"]
            .as_u64()
            .expect("evidence count")
            > 0
    );

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "tools/call",
            "params": {
                "name": "read_page_evidence",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourceId": source_id,
                    "page": 1
                }
            }
        }),
    );
    let evidence_tool = read_message(&mut reader);
    assert_eq!(
        evidence_tool["result"]["isError"], false,
        "{evidence_tool:#?}"
    );
    let text = evidence_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("evidence tool text");
    assert!(!text.contains(root_dir_arg.as_str()));
    assert!(!text.contains(import_root_arg.as_str()));
    let evidence_payload: Value = serde_json::from_str(text).expect("evidence payload");
    assert!(
        evidence_payload["evidence"]
            .as_array()
            .expect("evidence array")
            .iter()
            .any(|evidence| evidence["quotedText"]
                .as_str()
                .unwrap_or_default()
                .contains("MCP import should create cited evidence")),
        "{evidence_payload:#?}"
    );

    drop(stdin);
    let status = child.wait().expect("server exit");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(root_dir);
    let _ = std::fs::remove_dir_all(import_root);
}

#[test]
fn mcp_server_import_source_rejects_source_path_outside_allowed_roots() {
    let root_dir = temp_test_dir("hyprduck-mcp-import-reject-workspace");
    let import_root = temp_test_dir("hyprduck-mcp-import-reject-source");
    let outside_root = temp_test_dir("hyprduck-mcp-import-reject-outside");
    let root_dir_arg = root_dir.to_string_lossy().to_string();
    let import_root_arg = import_root.to_string_lossy().to_string();
    let outside_root_arg = outside_root.to_string_lossy().to_string();
    let _ = fs::remove_dir_all(&root_dir);
    let _ = fs::remove_dir_all(&import_root);
    let _ = fs::remove_dir_all(&outside_root);
    fs::create_dir_all(&root_dir).expect("temp root");
    fs::create_dir_all(&import_root).expect("import root");
    fs::create_dir_all(&outside_root).expect("outside root");
    let outside_source = outside_root.join("outside.md");
    fs::write(&outside_source, "# Outside\n\nNot allowlisted.\n").expect("outside source");

    let mut child = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "serve"])
        .env("HYPRDUCK_PROJECT_STORE", root_dir.join("knowledge.sqlite3"))
        .env("HYPRDUCK_MCP_ALLOW_ROOT_DIR", "1")
        .env("HYPRDUCK_MCP_ALLOWED_ROOTS", &root_dir_arg)
        .env("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS", &import_root_arg)
        .env("HYPRDUCK_DISABLE_PROVIDER_GRAPH", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mcp server should start");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);
    initialize_mcp_session(&mut stdin, &mut reader);

    write_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tools/call",
            "params": {
                "name": "import_source",
                "arguments": {
                    "workspaceId": "default",
                    "rootDir": root_dir_arg.clone(),
                    "sourcePath": outside_source.to_string_lossy(),
                    "format": "markdown"
                }
            }
        }),
    );
    let import_tool = read_message(&mut reader);
    assert_eq!(import_tool["result"]["isError"], true, "{import_tool:#?}");
    let text = import_tool["result"]["content"][0]["text"]
        .as_str()
        .expect("import rejection text");
    assert!(text.contains("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"));
    assert!(!text.contains(root_dir_arg.as_str()));
    assert!(!text.contains(import_root_arg.as_str()));
    assert!(!text.contains(outside_root_arg.as_str()));

    drop(stdin);
    let status = child.wait().expect("server exit");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(root_dir);
    let _ = std::fs::remove_dir_all(import_root);
    let _ = std::fs::remove_dir_all(outside_root);
}

fn initialize_mcp_session(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
) {
    write_message(
        stdin,
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
    let initialize = read_message(reader);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "hyprduck");
    write_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );
}

fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn poll_import_until_citation_ready(
    stdin: &mut std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    job_id: &str,
    workspace_id: &str,
    root_dir: String,
) -> Value {
    let mut last_payload = Value::Null;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut attempt = 0;
    while std::time::Instant::now() < deadline {
        write_message(
            stdin,
            json!({
                "jsonrpc": "2.0",
                "id": 4100 + attempt,
                "method": "tools/call",
                "params": {
                    "name": "import_status",
                    "arguments": {
                        "workspaceId": workspace_id,
                        "rootDir": root_dir.clone(),
                        "jobId": job_id
                    }
                }
            }),
        );
        let status_tool = read_message(reader);
        assert_eq!(status_tool["result"]["isError"], false, "{status_tool:#?}");
        let text = status_tool["result"]["content"][0]["text"]
            .as_str()
            .expect("status tool text");
        let payload: Value = serde_json::from_str(text).expect("status payload");
        if payload["citationReady"].as_bool().unwrap_or(false) {
            return payload;
        }
        last_payload = payload;
        std::thread::sleep(std::time::Duration::from_millis(50));
        attempt += 1;
    }
    panic!("import job did not become citation-ready: {last_payload:#?}");
}

fn write_mcp_snapshot_workspace(root_dir: &std::path::Path) {
    let workspace = root_dir.join("default");
    fs::create_dir_all(workspace.join("events")).expect("events dir");
    fs::create_dir_all(workspace.join("graph")).expect("graph dir");
    fs::create_dir_all(workspace.join("memory")).expect("memory dir");
    fs::create_dir_all(workspace.join("state")).expect("state dir");
    fs::create_dir_all(workspace.join("wiki")).expect("wiki dir");
    fs::create_dir_all(workspace.join("context_packs")).expect("context packs dir");
    fs::create_dir_all(workspace.join("sources/source-mcp")).expect("source dir");
    fs::create_dir_all(workspace.join("artifacts/source-mcp/pages")).expect("pages dir");
    fs::create_dir_all(workspace.join("artifacts/source-mcp/images")).expect("images dir");
    fs::write(
        workspace.join("sources/source-mcp/source.pdf"),
        b"source bytes",
    )
    .expect("source file");
    fs::write(
        workspace.join("artifacts/source-mcp/source.md"),
        "# MCP source\n",
    )
    .expect("source markdown");
    fs::write(
        workspace.join("artifacts/source-mcp/pages/page_1.md"),
        "# MCP page\n",
    )
    .expect("page markdown");
    fs::write(
        workspace.join("artifacts/source-mcp/images/page_1.png"),
        b"image bytes",
    )
    .expect("page image");
    fs::write(
        workspace.join("brain-manifest.json"),
        format!(
            r##"{{"workspaceId":"default","generatedAt":42,"sources":[{{"sourceId":"source-mcp","workspaceId":"default","originalPath":"{root}/source.pdf","sourcePath":"{root}/sources/source-mcp/source.pdf","markdownPath":"{root}/artifacts/source-mcp/source.md","format":"pdf","status":"ingested","pageCount":1,"description":"","userContext":"","ingestInstruction":"","updatedAt":42}}],"nodes":[{{"nodeId":"node-mcp-readable","kind":"concept","label":"MCP readable","scope":"project","aliases":[],"evidenceIds":["evidence-mcp"],"sourceIds":["source-mcp"],"confidence":null,"updatedAt":42}}],"relations":[],"evidence":[{{"id":"evidence-mcp","pageLabel":"Page 1","pageIndex":0,"snippet":"MCP source evidence","sourcePath":"{root}/sources/source-mcp/source.pdf","sourceId":"source-mcp","markdownPath":"{root}/artifacts/source-mcp/pages/page_1.md","imagePath":"{root}/artifacts/source-mcp/images/page_1.png","provenance":null}}],"memories":[],"wikiPages":[{{"pageId":"wiki-mcp-readable","workspaceId":"default","path":"wiki/index.md","title":"MCP Snapshot","body":"# MCP Snapshot\n\nLocal path: {root}\n","nodeRefs":["node-mcp-readable"],"sourceRefs":["source-mcp"],"evidenceRefs":["evidence-mcp"],"updatedAt":42}}],"entities":[],"claims":[],"extractions":[],"events":[]}}"##,
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
    fs::write(
        workspace.join("wiki/index.md"),
        format!("# MCP Snapshot\n\nLocal path: {}\n", workspace.display()),
    )
    .expect("wiki index");
    fs::write(
        workspace.join("artifacts/source-mcp/source_pack.json"),
        format!(
            r#"{{"schemaVersion":"hyprduck.source_pack.v0","workspaceId":"default","sourceId":"source-mcp","originalFilename":"source.pdf","originalPath":"{root}/source.pdf","sourcePath":"{root}/sources/source-mcp/source.pdf","markdownPath":"{root}/artifacts/source-mcp/source.md","artifactRoot":"{root}/artifacts/source-mcp","contentHash":"fnv64:mcp-source","format":"pdf","pageCount":1,"ingestionStatus":"ingested","providerRoute":"local_demo","localOnly":true,"pages":[],"warnings":[],"createdAt":42,"updatedAt":42}}"#,
            root = workspace.display()
        ),
    )
    .expect("source pack");
    fs::write(
        workspace.join("artifacts/source-mcp/evidence_index.json"),
        format!(
            r#"{{"schemaVersion":"hyprduck.evidence_index.v1","workspaceId":"default","sourceId":"source-mcp","contentHash":"fnv64:mcp-source","providerRoute":"local_demo","localOnly":true,"evidence":[{{"evidenceRef":"evidence-mcp","sourceId":"source-mcp","page":1,"region":"page:Page 1","span":"page","quotedText":"Indexed MCP page evidence","parseConfidence":"high","contentHash":"fnv64:mcp-source","markdownPath":"{root}/artifacts/source-mcp/pages/page_1.md","imagePath":"{root}/artifacts/source-mcp/images/page_1.png","evidenceType":"text"}}],"warnings":[],"generatedAt":42}}"#,
            root = workspace.display()
        ),
    )
    .expect("evidence index");
    let context_pack = r#"{"schemaVersion":"hyprduck.context_pack.v1","packId":"ctx_mcp","workspaceId":"default","query":"MCP evidence","generatedAt":"2026-05-18T09:00:00Z","sourceSet":[{"sourceId":"source-mcp","originalFilename":"source.pdf","contentHash":"fnv64:mcp-source","pageCount":1,"ingestionStatus":"ingested","staleness":"current","providerRoute":"local_demo","localOnly":true}],"selectedEvidence":[{"evidenceRef":"evidence-mcp","sourceId":"source-mcp","page":1,"region":"page:Page 1","span":"page","quotedText":"Indexed MCP page evidence","parseConfidence":"high","selectionReason":"MCP fixture evidence.","contentHash":"fnv64:mcp-source","evidenceType":"text"}],"findings":[],"warnings":[],"retrievalTrace":{"strategy":"fixture","chunksConsidered":1,"chunksSelected":1,"budgetRequested":4000,"budgetUsed":100,"evidenceTypeTrace":{"considered":{"text":1},"selected":{"text":1}}},"suggestedNextReads":[]}"#;
    fs::write(workspace.join("context_pack.json"), context_pack).expect("latest context pack");
    fs::write(workspace.join("context_packs/ctx_mcp.json"), context_pack)
        .expect("history context pack");
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

fn cited_answer_from_context_pack(context_pack: &Value) -> String {
    let evidence = &context_pack["selectedEvidence"][0];
    let quoted_text = evidence["quotedText"]
        .as_str()
        .expect("selected evidence quoted text");
    let source_id = evidence["sourceId"]
        .as_str()
        .expect("selected evidence sourceId");
    let page = evidence["page"].as_u64().expect("selected evidence page");
    let evidence_ref = evidence["evidenceRef"]
        .as_str()
        .expect("selected evidence ref");
    format!("{quoted_text} [sourceId={source_id}, page={page}, evidenceRef={evidence_ref}]")
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
