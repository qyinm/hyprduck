use std::process::Command;
use std::{fs, time};

#[test]
fn doctor_reports_engine_resolution() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck-cli"))
        .arg("doctor")
        .output()
        .expect("doctor command should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("HyprDuck CLI is available."));
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
            r#"{"eventId":"event-source-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"source_imported","operationType":"source_imported","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":["source-a"],"sourceMarkdownRefs":["sources/a.md"],"nodeRefs":["node-a"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{}","causality":{"causedByEventIds":[],"causedByProposalId":null,"causedBySourceIds":["source-a"],"snapshotId":null,"previousSnapshotId":null,"schemaVersion":1,"materializedVersion":null},"confidence":null,"policyResult":"applied","createdAt":10}"#,
            r#"{"eventId":"event-graph-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":["source-a"],"sourceMarkdownRefs":["sources/a.md"],"nodeRefs":["node-a"],"relationRefs":["edge-a"],"claimRefs":["claim-a"],"memoryRefs":["memory-a"],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"nodeCount\":1,\"relationCount\":1,\"claimCount\":1,\"memoryCount\":1,\"wikiPageCount\":1}","causality":{"causedByEventIds":["event-source-1"],"causedByProposalId":null,"causedBySourceIds":["source-a"],"snapshotId":"snapshot-1","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":20},"confidence":null,"policyResult":"applied","createdAt":20}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck-cli"))
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
            r#"{"eventId":"event-graph-1","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":[],"sourceMarkdownRefs":[],"nodeRefs":["node-prior"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"materializedGraph\":{\"generatedAt\":20,\"sources\":[],\"nodes\":[{\"nodeId\":\"node-prior\",\"kind\":\"concept\",\"label\":\"Prior\",\"scope\":\"project\",\"aliases\":[],\"evidenceIds\":[],\"sourceIds\":[],\"confidence\":null,\"updatedAt\":20}],\"edges\":[],\"evidence\":[],\"memories\":[],\"wikiPages\":[],\"entities\":[],\"claims\":[],\"extractions\":[]}}","causality":{"causedByEventIds":[],"causedByProposalId":null,"causedBySourceIds":[],"snapshotId":"snapshot-1","previousSnapshotId":null,"schemaVersion":1,"materializedVersion":20},"confidence":null,"policyResult":"applied","createdAt":20}"#,
            r#"{"eventId":"event-graph-2","schemaVersion":1,"workspaceId":"default","scope":"project","eventType":"graph_materialized","operationType":"graph_materialized","actor":{"actorType":"agent","actorId":"test"},"sourceRefs":[],"sourceMarkdownRefs":[],"nodeRefs":["node-current"],"relationRefs":[],"claimRefs":[],"memoryRefs":[],"targetNodeIds":[],"targetEdgeIds":[],"targetClaimIds":[],"targetMemoryIds":[],"evidenceRefs":[],"payloadJson":"{\"materializedGraph\":{\"generatedAt\":30,\"sources\":[],\"nodes\":[{\"nodeId\":\"node-current\",\"kind\":\"concept\",\"label\":\"Current\",\"scope\":\"project\",\"aliases\":[],\"evidenceIds\":[],\"sourceIds\":[],\"confidence\":null,\"updatedAt\":30}],\"edges\":[],\"evidence\":[],\"memories\":[],\"wikiPages\":[],\"entities\":[],\"claims\":[],\"extractions\":[]}}","causality":{"causedByEventIds":[],"causedByProposalId":null,"causedBySourceIds":[],"snapshotId":"snapshot-2","previousSnapshotId":"snapshot-1","schemaVersion":1,"materializedVersion":30},"confidence":null,"policyResult":"applied","createdAt":30}"#,
        ]
        .join("\n"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hyprduck-cli"))
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
