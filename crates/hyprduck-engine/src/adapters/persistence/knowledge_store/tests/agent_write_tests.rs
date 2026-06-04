use super::super::*;
use crate::brain_repo::{read_memory_records, BrainWorkspaceWriter};
use crate::knowledge::ensure_materialized_brain_repo_dirs;
use crate::*;
use std::fs;

#[test]
fn agent_session_write_proposal_commits_memory_and_reuses_in_context_pack() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = seed_agent_write_workspace(
        &temp,
        "ev-agent-write-1",
        "source-agent-write",
        "Agent-session write MCP stores approved knowledge.",
    );
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);

    let proposal = handle_write_propose(WriteProposeRequest {
        scope: scope.clone(),
        content_type: "memory".into(),
        title: "Agent-session write MCP".into(),
        body: "Approved agent-session write MCP knowledge is reusable.".into(),
        evidence_refs: vec!["ev-agent-write-1".into()],
    })
    .expect("propose write");

    let pending = handle_write_list(WriteListRequest {
        scope: scope.clone(),
    })
    .expect("list proposals");
    assert_eq!(pending.proposals.len(), 1);
    assert_eq!(pending.proposals[0].proposal_id, proposal.proposal_id);
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&workspace_root))
        .expect("open knowledge store");
    let persisted = store
        .load_agent_write_proposal(DEFAULT_WORKSPACE_ID, &proposal.proposal_id)
        .expect("load persisted proposal")
        .expect("persisted proposal exists");
    assert_eq!(persisted.actor_id, MCP_WRITE_AGENT_ID);
    assert_eq!(persisted.validation_status, "validated");
    assert_eq!(persisted.approval_status, "pending");
    assert_eq!(persisted.evidence_refs, vec!["ev-agent-write-1"]);
    assert!(!persisted.requires_user_approval);
    fs::remove_file(
        workspace_root
            .join("proposals")
            .join(format!("{}.json", proposal.proposal_id)),
    )
    .expect("remove compatibility proposal file");

    let committed = handle_write_commit(WriteCommitRequest {
        scope: scope.clone(),
        proposal_id: proposal.proposal_id.clone(),
        user_approved: false,
    })
    .expect("commit proposal");
    let proposal_suffix = proposal
        .proposal_id
        .strip_prefix("prop-")
        .expect("proposal suffix");
    assert_eq!(committed.event_id, format!("evt-{proposal_suffix}"));
    assert_eq!(committed.memory_id, format!("memory-{proposal_suffix}"));
    assert!(!workspace_root
        .join("proposals")
        .join(format!("{}.json", proposal.proposal_id))
        .exists());
    let persisted = store
        .load_agent_write_proposal(DEFAULT_WORKSPACE_ID, &proposal.proposal_id)
        .expect("reload persisted proposal")
        .expect("persisted proposal remains");
    assert_eq!(persisted.approval_status, "committed");
    let operation = store
        .load_brain_event_operation(DEFAULT_WORKSPACE_ID, &committed.event_id)
        .expect("load persisted commit event")
        .expect("commit event was persisted");
    assert_eq!(operation, "agent_session_write");

    let events = fs::read_to_string(workspace_root.join("events/brain_events.jsonl"))
        .expect("read event log");
    assert!(events.contains("memory_accepted"));
    assert!(events.contains("agent_session_write"));

    let search = handle_search_brain(SearchBrainRequest {
        scope: scope.clone(),
        query: "reusable".into(),
        limit: Some(10),
    })
    .expect("search brain");
    assert!(search
        .results
        .iter()
        .any(|result| result.id == committed.memory_id));

    let pack = handle_get_context_pack(GetContextPackRequest {
        scope,
        query: "reusable".into(),
        selected_node_id: None,
        budget: Some(8000),
        persist: false,
    })
    .expect("get context pack");
    assert!(pack
        .context_pack
        .memories
        .iter()
        .any(|memory| memory.memory_id == committed.memory_id));
}

#[test]
fn large_semantic_wiki_or_graph_proposals_require_user_approval() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = seed_agent_write_workspace(
        &temp,
        "ev-semantic-approval",
        "source-semantic-approval",
        "Large semantic wiki and graph changes require user approval.",
    );

    let proposal = handle_write_propose(WriteProposeRequest {
        scope: scope.clone(),
        content_type: "wiki_page".into(),
        title: "Large wiki rewrite".into(),
        body: (0..45)
            .map(|idx| format!("Semantic section {idx} updates graph-linked wiki content."))
            .collect::<Vec<_>>()
            .join("\n"),
        evidence_refs: vec!["ev-semantic-approval".into()],
    })
    .expect("propose semantic write");
    assert_eq!(proposal.status, "pending_user_approval");

    let error = handle_write_commit(WriteCommitRequest {
        scope,
        proposal_id: proposal.proposal_id,
        user_approved: false,
    })
    .expect_err("large semantic proposal requires user approval");

    assert!(error
        .to_string()
        .contains("large semantic wiki/graph changes require explicit user approval"));
}

#[test]
fn small_evidence_refresh_and_link_repair_proposals_can_commit_without_user_approval() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = seed_agent_write_workspace(
        &temp,
        "ev-maintenance-auto",
        "source-maintenance-auto",
        "Small evidence refresh and link repair changes may auto-commit.",
    );

    let proposal = handle_write_propose(WriteProposeRequest {
        scope: scope.clone(),
        content_type: "link_repair".into(),
        title: "Repair stale source link".into(),
        body: "Refresh evidence link metadata for an existing cited source.".into(),
        evidence_refs: vec!["ev-maintenance-auto".into()],
    })
    .expect("propose maintenance write");
    assert_eq!(proposal.status, "pending");

    let committed = handle_write_commit(WriteCommitRequest {
        scope,
        proposal_id: proposal.proposal_id,
        user_approved: false,
    })
    .expect("small maintenance write commits without user approval");
    assert!(committed.event_id.starts_with("evt-"));
    assert!(committed.memory_id.starts_with("memory-"));
}

#[test]
fn write_commit_all_reports_stable_error_categories_per_item() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };

    let response = handle_write_commit_all(WriteCommitAllRequest {
        scope,
        proposal_ids: vec!["prop-0123456789abcdef0123456789abcdef".into()],
    })
    .expect("commit all returns per-item failures");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].status, "failed");
    assert_eq!(
        response.results[0].error_category.as_deref(),
        Some("proposal_state")
    );
    assert!(response.results[0]
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("not found"));
}

#[test]
fn brain_writer_bootstraps_missing_memory_and_events_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    ensure_materialized_brain_repo_dirs(&workspace_root).expect("ensure repo dirs");
    write_json_pretty(
        &workspace_root.join("brain-manifest.json"),
        &BrainRepoSnapshot {
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            generated_at: 1,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        },
    )
    .expect("write manifest");
    write_json_pretty::<Vec<BrainNodeRecord>>(&workspace_root.join("graph/nodes.json"), &vec![])
        .expect("write nodes");
    write_json_pretty::<Vec<BrainRelationRecord>>(
        &workspace_root.join("graph/edges.json"),
        &vec![],
    )
    .expect("write edges");
    write_json_pretty::<Vec<EvidenceRef>>(&workspace_root.join("graph/evidence.json"), &vec![])
        .expect("write evidence");
    let _ = fs::remove_file(workspace_root.join("memory/records.json"));
    let _ = fs::remove_file(workspace_root.join("events/brain_events.jsonl"));

    let memory = MemoryRecord {
        memory_id: "memory-bootstrap".into(),
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        scope: BrainScope::Project,
        title: "Remember bootstrap behavior".into(),
        body: "Missing memory and event files should be recreated safely.".into(),
        source_refs: Vec::new(),
        evidence_refs: Vec::new(),
        created_at: 42,
        updated_at: 42,
    };
    let writer = BrainWorkspaceWriter::open(workspace_root.clone()).expect("open writer");
    writer
        .upsert_memory_record(memory.clone())
        .expect("write memory with missing file");
    writer
        .append_event(&BrainEvent {
            event_id: "evt-memory-bootstrap".into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: DEFAULT_WORKSPACE_ID.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::MemoryAccepted,
            operation_type: Some("memory_recorded".into()),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "test-agent".into(),
            },
            source_refs: Vec::new(),
            source_markdown_refs: Vec::new(),
            node_refs: Vec::new(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: vec![memory.memory_id.clone()],
            target_node_ids: Vec::new(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: vec![memory.memory_id.clone()],
            evidence_refs: Vec::new(),
            payload_json: serde_json::to_string(&memory).expect("memory payload"),
            causality: BrainEventCausality::default(),
            confidence: None,
            policy_result: "accepted".into(),
            created_at: 42,
        })
        .expect("append memory event");
    drop(writer);

    let memories = read_memory_records(&workspace_root).expect("read bootstrapped memories");
    assert_eq!(memories.len(), 1);
    assert!(workspace_root.join("events/brain_events.jsonl").exists());
    let scope = BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    };
    let events = handle_read_recent_events(ReadRecentEventsRequest {
        scope,
        limit: Some(10),
        run_id: None,
        source_ref: None,
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    })
    .expect("read bootstrapped events");
    assert!(events
        .events
        .iter()
        .any(|event| event.event_type == BrainEventKind::MemoryAccepted));
    assert!(!workspace_root.join(BRAIN_LOCK_DIRECTORY_NAME).exists());
}

fn seed_agent_write_workspace(
    temp: &tempfile::TempDir,
    evidence_id: &str,
    source_id: &str,
    snippet: &str,
) -> BrainReadScope {
    let workspace_root = temp.path().join(DEFAULT_WORKSPACE_ID);
    fs::create_dir_all(workspace_root.join("graph")).expect("graph dir");
    let evidence = EvidenceRef {
        id: evidence_id.into(),
        page_label: "Page 1".into(),
        page_index: Some(0),
        snippet: snippet.into(),
        source_path: Some("/private/docs/source.pdf".into()),
        source_id: Some(source_id.into()),
        markdown_path: Some(format!("artifacts/{source_id}/pages/page_1.md")),
        image_path: None,
        provenance: Some("test fixture".into()),
    };
    let snapshot = BrainRepoSnapshot {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        generated_at: 1,
        sources: Vec::new(),
        nodes: Vec::new(),
        relations: Vec::new(),
        evidence: vec![evidence.clone()],
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: Vec::new(),
    };
    write_json_pretty(&workspace_root.join("brain-manifest.json"), &snapshot)
        .expect("write manifest");
    write_json_pretty::<Vec<BrainNodeRecord>>(&workspace_root.join("graph/nodes.json"), &vec![])
        .expect("write nodes");
    write_json_pretty::<Vec<BrainRelationRecord>>(
        &workspace_root.join("graph/edges.json"),
        &vec![],
    )
    .expect("write edges");
    write_json_pretty(&workspace_root.join("graph/evidence.json"), &vec![evidence])
        .expect("write evidence");

    BrainReadScope {
        workspace_id: DEFAULT_WORKSPACE_ID.into(),
        root_dir: Some(temp.path().display().to_string()),
    }
}

#[allow(dead_code)]
fn brain_event_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
    let graph = Graph::open(&store.path).context("open graph")?;
    let count = graph
        .connection()
        .sqlite_connection()
        .query_row(
            "SELECT COUNT(*) FROM brain_events WHERE workspace_id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .context("query brain event count")?;
    Ok(count)
}

#[allow(dead_code)]
fn single_event_snapshot(
    event_id: &str,
    source_id: &str,
    evidence_id: &str,
    label: &str,
) -> BrainRepoSnapshot {
    BrainRepoSnapshot {
        workspace_id: "workspace-default".into(),
        generated_at: 10,
        sources: vec![SourceRecord {
            source_id: source_id.into(),
            workspace_id: "workspace-default".into(),
            original_path: format!("/tmp/{source_id}.pdf"),
            source_path: format!("sources/{source_id}.pdf"),
            markdown_path: format!("sources/{source_id}.md"),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 10,
        }],
        nodes: vec![BrainNodeRecord {
            node_id: format!("node-{source_id}"),
            kind: BrainNodeKind::Concept,
            label: label.into(),
            scope: BrainScope::Project,
            aliases: Vec::new(),
            evidence_ids: vec![evidence_id.into()],
            source_ids: vec![source_id.into()],
            confidence: Some(0.9),
            updated_at: 10,
        }],
        relations: Vec::new(),
        evidence: vec![EvidenceRef {
            id: evidence_id.into(),
            page_label: "p1".into(),
            page_index: Some(0),
            snippet: format!("{label} evidence."),
            source_path: Some(format!("sources/{source_id}.pdf")),
            source_id: Some(source_id.into()),
            markdown_path: Some(format!("sources/{source_id}.md")),
            image_path: None,
            provenance: Some("test".into()),
        }],
        memories: Vec::new(),
        wiki_pages: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        extractions: Vec::new(),
        events: vec![test_brain_event(
            event_id,
            "workspace-default",
            &[evidence_id],
        )],
    }
}

#[allow(dead_code)]
fn test_brain_event(event_id: &str, workspace_id: &str, evidence_refs: &[&str]) -> BrainEvent {
    BrainEvent {
        event_id: event_id.into(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: workspace_id.into(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::GraphMaterialized,
        operation_type: Some("graph_materialized".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: "test-agent".into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: Vec::new(),
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: Vec::new(),
        evidence_refs: evidence_refs.iter().map(|value| (*value).into()).collect(),
        payload_json: "{}".into(),
        causality: BrainEventCausality::default(),
        confidence: None,
        policy_result: "materialized".into(),
        created_at: 10,
    }
}
