use std::process::Command;
use std::{fs, time};

use hyprduck_engine_types::{ContextPackV0, EvidenceIndexV0, SourcePackV0};

#[test]
fn doctor_reports_engine_resolution() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .arg("doctor")
        .output()
        .expect("doctor command should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("HyprDuck CLI is available."));
}

#[test]
fn context_alias_writes_context_pack_v0() {
    let root = unique_temp_dir("hyprduck-cli-context-alias");
    fs::create_dir_all(root.join("default")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args([
            "context",
            "--root",
            root.to_str().unwrap(),
            "--write-context-pack",
            "agent reuse",
        ])
        .output()
        .expect("context alias should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("context-pack-v0: hyprduck.context_pack.v0"));
    assert!(stdout.contains("context-pack-v0-path:"));
    assert!(root.join("default/context_pack.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn documents_search_alias_keeps_brain_search_compatibility() {
    let root = unique_temp_dir("hyprduck-cli-documents-search");
    fs::create_dir_all(root.join("default")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args([
            "documents",
            "search",
            "--root",
            root.to_str().unwrap(),
            "agent reuse",
        ])
        .output()
        .expect("documents search alias should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ingest_alias_reaches_parse_format_validation() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["ingest", "fixture.unsupported"])
        .output()
        .expect("ingest alias should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported input format: unsupported"),
        "stderr: {stderr}"
    );
}

#[test]
fn demo_writes_local_context_pack_artifacts() {
    let root = unique_temp_dir("hyprduck-cli-demo");

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["demo", "--root", root.to_str().unwrap()])
        .output()
        .expect("demo command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("context-pack-v0: hyprduck.context_pack.v0"));
    assert!(stdout.contains("source-pack:"));
    assert!(stdout.contains("evidence-index:"));
    let elapsed_ms = stdout_value(&stdout, "elapsed-ms:")
        .expect("elapsed")
        .parse::<u64>()
        .expect("elapsed number");
    assert!(elapsed_ms < 60_000, "demo exceeded 60s: {elapsed_ms}ms");

    let context_pack_path = root.join("demo/context_pack.json");
    let source_pack_path = root.join("demo/artifacts/demo-source/source_pack.json");
    let evidence_index_path = root.join("demo/artifacts/demo-source/evidence_index.json");
    assert!(context_pack_path.exists());
    assert!(source_pack_path.exists());
    assert!(evidence_index_path.exists());

    let context_pack: ContextPackV0 =
        serde_json::from_str(&fs::read_to_string(&context_pack_path).unwrap())
            .expect("schema-valid context pack");
    assert_eq!(context_pack.schema_version, "hyprduck.context_pack.v0");
    assert_eq!(context_pack.workspace_id, "demo");
    assert_eq!(context_pack.source_set.len(), 1);
    assert_eq!(context_pack.source_set[0].source_id, "demo-source");
    assert_eq!(context_pack.source_set[0].provider_route, "local_demo");
    assert!(context_pack.source_set[0].local_only);
    assert_eq!(context_pack.selected_evidence.len(), 1);
    assert_eq!(
        context_pack.selected_evidence[0].source_id,
        context_pack.source_set[0].source_id
    );
    assert_eq!(context_pack.selected_evidence[0].page, 1);
    assert!(context_pack
        .selected_evidence
        .iter()
        .all(|evidence| !evidence.evidence_ref.is_empty()));
    assert!(context_pack
        .findings
        .iter()
        .flat_map(|finding| finding.derived_from.iter())
        .all(|evidence_ref| context_pack
            .selected_evidence
            .iter()
            .any(|evidence| &evidence.evidence_ref == evidence_ref)));
    assert!(context_pack.retrieval_trace.chunks_selected >= 1);

    let source_pack: SourcePackV0 =
        serde_json::from_str(&fs::read_to_string(&source_pack_path).unwrap())
            .expect("schema-valid source pack");
    assert_eq!(source_pack.schema_version, "hyprduck.source_pack.v0");
    assert_eq!(source_pack.source_id, "demo-source");
    assert_eq!(source_pack.provider_route, "local_demo");
    assert!(source_pack.local_only);

    let evidence_index: EvidenceIndexV0 =
        serde_json::from_str(&fs::read_to_string(&evidence_index_path).unwrap())
            .expect("schema-valid evidence index");
    assert_eq!(evidence_index.schema_version, "hyprduck.evidence_index.v0");
    assert_eq!(evidence_index.source_id, "demo-source");
    assert_eq!(evidence_index.provider_route, "local_demo");
    assert!(evidence_index.local_only);
    assert_eq!(evidence_index.content_hash, source_pack.content_hash);
    assert_eq!(evidence_index.evidence.len(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mcp_install_claude_code_writes_production_server_entry() {
    let home = unique_temp_dir("hyprduck-cli-mcp-install");
    let config_dir = home.join(".config/claude-code");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("mcp_servers.json"),
        r#"{"mcpServers":{"linear":{"command":"npx","args":["-y","mcp-remote","https://mcp.linear.app/sse"]}}}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "install", "claude-code"])
        .env("HOME", &home)
        .output()
        .expect("mcp install command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let raw = fs::read_to_string(config_dir.join("mcp_servers.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(config["mcpServers"]["linear"]["command"], "npx");
    assert_eq!(config["mcpServers"]["hyprduck"]["args"][0], "mcp");
    assert_eq!(config["mcpServers"]["hyprduck"]["args"][1], "serve");
    assert!(config["mcpServers"]["hyprduck"]["command"]
        .as_str()
        .unwrap()
        .contains("hyprduck"));
    assert!(home.join(".local/bin/hyprduck").exists());
    let _ = fs::remove_dir_all(home);
}

#[test]
fn mcp_install_codex_registers_server_and_shell_command() {
    let home = unique_temp_dir("hyprduck-cli-mcp-install-codex");
    let fake_codex = home.join("fake-codex");
    let log = home.join("codex-args.log");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        &fake_codex,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    make_executable(&fake_codex);

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args(["mcp", "install", "codex"])
        .env("HOME", &home)
        .env("HYPRDUCK_CODEX_BIN", &fake_codex)
        .output()
        .expect("mcp install command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains("mcp remove hyprduck"));
    assert!(calls.contains("mcp add hyprduck --"));
    assert!(calls.contains("mcp serve"));
    assert!(home.join(".local/bin/hyprduck").exists());
    let _ = fs::remove_dir_all(home);
}

#[test]
fn brain_inspect_state_prints_selected_graph_state_and_related_events() {
    let root = unique_temp_dir("hyprduck-cli-graph-history");
    let workspace = root.join("default");
    fs::create_dir_all(workspace.join("events")).unwrap();
    fs::create_dir_all(workspace.join("graph")).unwrap();
    fs::write(
        workspace.join("brain-manifest.json"),
        r#"{"workspaceId":"default","generatedAt":20,"sources":[],"nodes":[],"relations":[],"evidence":[],"memories":[],"wikiPages":[],"entities":[],"claims":[],"extractions":[],"events":[]}"#,
    )
    .unwrap();
    fs::write(workspace.join("graph/nodes.json"), "[]").unwrap();
    fs::write(workspace.join("graph/edges.json"), "[]").unwrap();
    fs::write(workspace.join("graph/evidence.json"), "[]").unwrap();
    fs::write(
        workspace.join("events/brain_events.jsonl"),
        [
            r#"{"eventId":"event-source-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"source_imported","operationType":"source_imported","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":["source-a"],"sourceMarkdownRefs":["sources/a.md"],"nodeRefs":["node-a"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{}","causality":{"causedByEventIds":[],"causedBySourceIds":["source-a"],"snapshotId":null,"previousSnapshotId":null,"schemaVersion":1,"materializedVersion":null},"confidence":null,"policyResult":"applied","createdAt":10}"#,
            r#"{"eventId":"event-graph-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":["source-a"],"sourceMarkdownRefs":["sources/a.md"],"nodeRefs":["node-a"],"relationRefs":["edge-a"],"claimRefs":["claim-a"],"memoryRefs":["memory-a"],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"nodeCount\":1,\"relationCount\":1,\"claimCount\":1,\"memoryCount\":1,\"wikiPageCount\":1}","causality":{"causedByEventIds":["event-source-1"],"causedBySourceIds":["source-a"],"snapshotId":"snapshot-1","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":20},"confidence":null,"policyResult":"applied","createdAt":20}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args([
            "brain",
            "inspect-state",
            "--root",
            root.to_str().unwrap(),
            "--snapshot",
            "snapshot-1",
        ])
        .output()
        .expect("inspect-state command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("snapshot: snapshot-1"));
    assert!(stdout.contains("event: event-graph-1"));
    assert!(stdout.contains("rollback-target: --event event-graph-1"));
    assert!(
        stdout.contains("nodes: 1 edges: 1 claims: 1 memories: 1 wiki-pages: 1"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("related-events: 2"));
    assert!(stdout.contains("event-source-1"));
    assert!(stdout.contains("event-graph-1"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn brain_rollback_state_applies_selected_graph_state() {
    let root = unique_temp_dir("hyprduck-cli-graph-rollback");
    let workspace = root.join("default");
    fs::create_dir_all(workspace.join("events")).unwrap();
    fs::create_dir_all(workspace.join("graph")).unwrap();
    fs::write(
        workspace.join("brain-manifest.json"),
        r#"{"workspaceId":"default","generatedAt":30,"sources":[],"nodes":[{"nodeId":"node-current","kind":"concept","label":"Current","scope":"project","aliases":[],"evidenceIds":[],"sourceIds":[],"confidence":null,"updatedAt":30}],"relations":[],"evidence":[],"memories":[],"wikiPages":[],"entities":[],"claims":[],"extractions":[],"events":[]}"#,
    )
    .unwrap();
    fs::write(
        workspace.join("graph/nodes.json"),
        r#"[{"nodeId":"node-current","kind":"concept","label":"Current","scope":"project","aliases":[],"evidenceIds":[],"sourceIds":[],"confidence":null,"updatedAt":30}]"#,
    )
    .unwrap();
    fs::write(workspace.join("graph/edges.json"), "[]").unwrap();
    fs::write(workspace.join("graph/evidence.json"), "[]").unwrap();
    fs::write(
        workspace.join("events/brain_events.jsonl"),
        [
            r#"{"eventId":"event-graph-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":[],"sourceMarkdownRefs":[],"nodeRefs":["node-prior"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"materializedGraph\":{\"generatedAt\":20,\"sources\":[],\"nodes\":[{\"nodeId\":\"node-prior\",\"kind\":\"concept\",\"label\":\"Prior\",\"scope\":\"project\",\"aliases\":[],\"evidenceIds\":[],\"sourceIds\":[],\"confidence\":null,\"updatedAt\":20}],\"edges\":[],\"evidence\":[],\"memories\":[],\"wikiPages\":[],\"entities\":[],\"claims\":[],\"extractions\":[]}}","causality":{"causedByEventIds":[],"causedBySourceIds":[],"snapshotId":"snapshot-1","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":20},"confidence":null,"policyResult":"applied","createdAt":20}"#,
            r#"{"eventId":"event-graph-2","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":[],"sourceMarkdownRefs":[],"nodeRefs":["node-current"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"materializedGraph\":{\"generatedAt\":30,\"sources\":[],\"nodes\":[{\"nodeId\":\"node-current\",\"kind\":\"concept\",\"label\":\"Current\",\"scope\":\"project\",\"aliases\":[],\"evidenceIds\":[],\"sourceIds\":[],\"confidence\":null,\"updatedAt\":30}],\"edges\":[],\"evidence\":[],\"memories\":[],\"wikiPages\":[],\"entities\":[],\"claims\":[],\"extractions\":[]}}","causality":{"causedByEventIds":[],"causedBySourceIds":[],"snapshotId":"snapshot-2","previousSnapshotId":"snapshot-1","schemaVersion":1,"materializedVersion":30},"confidence":null,"policyResult":"applied","createdAt":30}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck"))
        .args([
            "brain",
            "rollback-state",
            "--root",
            root.to_str().unwrap(),
            "--snapshot",
            "snapshot-1",
        ])
        .output()
        .expect("rollback-state command should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rollback-applied: snapshot-1"));
    let nodes = fs::read_to_string(workspace.join("graph/nodes.json")).unwrap();
    assert!(nodes.contains("node-prior"));
    assert!(!nodes.contains("node-current"));
    let events = fs::read_to_string(workspace.join("events/brain_events.jsonl")).unwrap();
    assert!(events.contains("\"operationType\":\"graph_rollback\""));
    assert_eq!(events.lines().count(), 3);
    let _ = fs::remove_dir_all(root);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

fn stdout_value<'a>(stdout: &'a str, label: &str) -> Option<&'a str> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(label).map(str::trim))
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}
