use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use etyma_engine_types::{
    BrainActor, BrainActorType, BrainEvent, BrainEventCausality, BrainEventKind, BrainRepoSnapshot,
    BrainScope, MemoryRecord, PolicyResult, WriteCommitAllRequest, WriteCommitAllResponseData,
    WriteCommitRequest, WriteCommitResponseData, WriteCommitResultItem, WriteListRequest,
    WriteListResponseData, WriteProposalSummary, WriteProposeRequest, WriteProposeResponseData,
    WriteRejectRequest, WriteRejectResponseData, BRAIN_EVENT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::brain_repo::{
    read_materialized_brain_snapshot, BrainArtifactRepository, BrainWorkspaceWriter,
};
use crate::{
    read_json_artifact, resolve_brain_workspace_root, unix_timestamp_seconds, write_json_pretty,
    AgentWriteProposalRecord, KnowledgeStore, MCP_WRITE_AGENT_ID,
};

pub(crate) fn handle_write_propose(
    request: WriteProposeRequest,
) -> Result<WriteProposeResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    let snapshot = read_materialized_brain_snapshot(&root, &request.scope.workspace_id)?;

    validate_write_content_type(&request.content_type)?;
    validate_evidence_refs(
        &snapshot,
        &request.scope.workspace_id,
        &request.evidence_refs,
    )?;

    let now = unix_timestamp_seconds();
    let proposal_id = format!("prop-{}", Uuid::now_v7().as_simple());
    let approval = write_proposal_approval_policy(&request.content_type, &request.body);
    let proposal = AgentWriteProposal {
        proposal_id: proposal_id.clone(),
        content_type: request.content_type,
        title: request.title,
        body: request.body,
        evidence_refs: request.evidence_refs,
        created_at: now,
        workspace_id: request.scope.workspace_id,
        requires_user_approval: approval.requires_user_approval,
        approval_reason: approval.reason,
    };
    let status = write_proposal_status(&proposal);
    let proposal_json =
        serde_json::to_string(&proposal).context("failed encoding agent write proposal")?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    store.persist_agent_write_proposal(&AgentWriteProposalRecord {
        proposal_id: proposal.proposal_id.clone(),
        workspace_id: proposal.workspace_id.clone(),
        content_type: proposal.content_type.clone(),
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        evidence_refs: proposal.evidence_refs.clone(),
        actor_id: MCP_WRITE_AGENT_ID.into(),
        validation_status: "validated".into(),
        requires_user_approval: proposal.requires_user_approval,
        approval_reason: proposal.approval_reason.clone(),
        approval_status: status.clone(),
        proposal_json,
        created_at: now as i64,
        updated_at: now as i64,
    })?;
    write_json_pretty(
        &writer
            .root()
            .join("proposals")
            .join(format!("{proposal_id}.json")),
        &proposal,
    )?;

    Ok(WriteProposeResponseData {
        proposal_id,
        status,
        created_at: now,
    })
}

pub(crate) fn handle_write_commit(request: WriteCommitRequest) -> Result<WriteCommitResponseData> {
    validate_proposal_id(&request.proposal_id)?;
    let root = resolve_brain_workspace_root(&request.scope)?;
    let writer = BrainWorkspaceWriter::open(root.clone())?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let proposal_path = root
        .join("proposals")
        .join(format!("{}.json", request.proposal_id));
    let proposal = read_agent_write_proposal(
        &store,
        &root,
        &request.scope.workspace_id,
        &request.proposal_id,
    )?;
    validate_committable_proposal(&proposal, &request)?;

    let now = unix_timestamp_seconds();
    let proposal_suffix = request
        .proposal_id
        .strip_prefix("prop-")
        .expect("proposal id validated");
    let memory_id = format!("memory-{proposal_suffix}");
    let event_id = format!("evt-{proposal_suffix}");

    let memory = MemoryRecord {
        memory_id: memory_id.clone(),
        workspace_id: request.scope.workspace_id.clone(),
        scope: BrainScope::Project,
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        source_refs: Vec::new(),
        evidence_refs: proposal.evidence_refs.clone(),
        created_at: now,
        updated_at: now,
    };

    let event = BrainEvent {
        event_id: event_id.clone(),
        schema_version: BRAIN_EVENT_SCHEMA_VERSION,
        workspace_id: request.scope.workspace_id.clone(),
        scope: BrainScope::Project,
        event_type: BrainEventKind::MemoryAccepted,
        operation_type: Some("agent_session_write".into()),
        actor: BrainActor {
            actor_type: BrainActorType::Agent,
            actor_id: MCP_WRITE_AGENT_ID.into(),
        },
        source_refs: Vec::new(),
        source_markdown_refs: Vec::new(),
        node_refs: Vec::new(),
        relation_refs: Vec::new(),
        claim_refs: Vec::new(),
        memory_refs: vec![memory_id.clone()],
        target_node_ids: Vec::new(),
        target_edge_ids: Vec::new(),
        target_claim_ids: Vec::new(),
        target_memory_ids: vec![memory_id.clone()],
        evidence_refs: proposal.evidence_refs.clone(),
        payload_json: serde_json::to_string(&memory)?,
        causality: BrainEventCausality::default(),
        confidence: None,
        policy_result: PolicyResult::accepted(),
        created_at: now,
    };

    append_event_once(writer.repo(), &event)?;
    let mut memories = writer.repo().read_memory_records()?;
    if let Some(existing) = memories.iter_mut().find(|m| m.memory_id == memory_id) {
        *existing = memory.clone();
    } else {
        memories.push(memory.clone());
    }
    memories.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
    writer.repo().write_memory_records(&memories)?;
    store.record_agent_write_commit(
        &request.scope.workspace_id,
        &request.proposal_id,
        &event,
        now as i64,
    )?;
    match fs::remove_file(&proposal_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed removing committed proposal file {proposal_path:?}")
            });
        }
    }

    Ok(WriteCommitResponseData {
        event_id,
        memory_id,
        stored_at: now,
    })
}

fn append_event_once(repo: &BrainArtifactRepository, event: &BrainEvent) -> Result<()> {
    if repo
        .read_brain_events()?
        .iter()
        .any(|existing| existing.event_id == event.event_id)
    {
        return Ok(());
    }
    repo.append_event(event)
}

pub(crate) fn handle_write_commit_all(
    request: WriteCommitAllRequest,
) -> Result<WriteCommitAllResponseData> {
    let mut results = Vec::new();
    for proposal_id in &request.proposal_ids {
        match handle_write_commit(WriteCommitRequest {
            scope: request.scope.clone(),
            proposal_id: proposal_id.clone(),
            user_approved: false,
        }) {
            Ok(response) => results.push(WriteCommitResultItem {
                proposal_id: proposal_id.clone(),
                status: "committed".into(),
                event_id: Some(response.event_id),
                memory_id: Some(response.memory_id),
                error_category: None,
                error: None,
            }),
            Err(error) => results.push(WriteCommitResultItem {
                proposal_id: proposal_id.clone(),
                status: "failed".into(),
                event_id: None,
                memory_id: None,
                error_category: Some(classify_agent_write_error(&error.to_string()).into()),
                error: Some(error.to_string()),
            }),
        }
    }
    Ok(WriteCommitAllResponseData { results })
}

pub(crate) fn handle_write_list(request: WriteListRequest) -> Result<WriteListResponseData> {
    let root = resolve_brain_workspace_root(&request.scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let proposals_dir = root.join("proposals");
    let mut proposals = store
        .list_pending_agent_write_proposals(&request.scope.workspace_id)?
        .into_iter()
        .map(write_proposal_summary_from_record)
        .collect::<Vec<_>>();
    let mut seen = proposals
        .iter()
        .map(|proposal| proposal.proposal_id.clone())
        .collect::<BTreeSet<_>>();
    if !proposals_dir.exists() {
        return Ok(WriteListResponseData { proposals });
    }
    for entry in fs::read_dir(&proposals_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(proposal) = read_json_artifact::<Value>(&path) {
                let proposal_id = proposal["proposalId"].as_str().unwrap_or("").to_string();
                if proposal_id.is_empty() || seen.contains(&proposal_id) {
                    continue;
                }
                seen.insert(proposal_id.clone());
                proposals.push(WriteProposalSummary {
                    proposal_id,
                    content_type: proposal["contentType"]
                        .as_str()
                        .unwrap_or("memory")
                        .to_string(),
                    title: proposal["title"].as_str().unwrap_or("").to_string(),
                    body: proposal["body"].as_str().unwrap_or("").to_string(),
                    evidence_refs: proposal["evidenceRefs"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    created_at: proposal["createdAt"].as_u64().unwrap_or(0),
                });
            }
        }
    }
    Ok(WriteListResponseData { proposals })
}

pub(crate) fn handle_write_reject(request: WriteRejectRequest) -> Result<WriteRejectResponseData> {
    validate_proposal_id(&request.proposal_id)?;
    let root = resolve_brain_workspace_root(&request.scope)?;
    let _writer = BrainWorkspaceWriter::open(root.clone())?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    let proposal_path = root
        .join("proposals")
        .join(format!("{}.json", request.proposal_id));
    store.update_agent_write_proposal_status(
        &request.scope.workspace_id,
        &request.proposal_id,
        "rejected",
        unix_timestamp_seconds() as i64,
    )?;
    if proposal_path.exists() {
        fs::remove_file(&proposal_path)?;
    }
    Ok(WriteRejectResponseData {
        proposal_id: request.proposal_id,
        status: "rejected".into(),
    })
}

fn read_agent_write_proposal(
    store: &KnowledgeStore,
    root: &Path,
    workspace_id: &str,
    proposal_id: &str,
) -> Result<AgentWriteProposal> {
    if let Some(record) = store.load_agent_write_proposal(workspace_id, proposal_id)? {
        if record.approval_status != "pending" && record.approval_status != "pending_user_approval"
        {
            bail!(
                "proposal {} was not found (already committed or rejected)",
                proposal_id
            );
        }
        if record.validation_status != "validated" {
            bail!(
                "proposal {} is not committable because validation status is {}",
                proposal_id,
                record.validation_status
            );
        }
        return Ok(agent_write_proposal_from_record(record));
    }

    let proposal_path = root.join("proposals").join(format!("{proposal_id}.json"));
    if !proposal_path.exists() {
        bail!(
            "proposal {} was not found (already committed or rejected)",
            proposal_id
        );
    }
    read_json_artifact(&proposal_path)
}

fn agent_write_proposal_from_record(record: AgentWriteProposalRecord) -> AgentWriteProposal {
    AgentWriteProposal {
        proposal_id: record.proposal_id,
        content_type: record.content_type,
        title: record.title,
        body: record.body,
        evidence_refs: record.evidence_refs,
        created_at: record.created_at.max(0) as u64,
        workspace_id: record.workspace_id,
        requires_user_approval: record.requires_user_approval,
        approval_reason: record.approval_reason,
    }
}

fn write_proposal_summary_from_record(record: AgentWriteProposalRecord) -> WriteProposalSummary {
    WriteProposalSummary {
        proposal_id: record.proposal_id,
        content_type: record.content_type,
        title: record.title,
        body: record.body,
        evidence_refs: record.evidence_refs,
        created_at: record.created_at.max(0) as u64,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentWriteProposal {
    proposal_id: String,
    content_type: String,
    title: String,
    body: String,
    evidence_refs: Vec<String>,
    created_at: u64,
    workspace_id: String,
    #[serde(default)]
    requires_user_approval: bool,
    #[serde(default)]
    approval_reason: Option<String>,
}

fn validate_proposal_id(proposal_id: &str) -> Result<()> {
    let suffix = proposal_id
        .strip_prefix("prop-")
        .ok_or_else(|| anyhow!("invalid proposalId: expected prop-<uuid>"))?;
    if suffix.len() != 32 || !suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("invalid proposalId: expected prop-<uuid>");
    }
    Ok(())
}

fn validate_write_content_type(content_type: &str) -> Result<()> {
    match content_type.trim() {
        "memory" | "wiki_page" | "graph_change" | "evidence_refresh" | "link_repair" => Ok(()),
        other => bail!(
            "unsupported contentType {other}; supported contentTypes: memory, wiki_page, graph_change, evidence_refresh, link_repair"
        ),
    }
}

struct WriteApprovalPolicy {
    requires_user_approval: bool,
    reason: Option<String>,
}

fn write_proposal_status(proposal: &AgentWriteProposal) -> String {
    if proposal.requires_user_approval {
        "pending_user_approval".into()
    } else {
        "pending".into()
    }
}

fn write_proposal_approval_policy(content_type: &str, body: &str) -> WriteApprovalPolicy {
    let semantic_content = matches!(content_type.trim(), "wiki_page" | "graph_change");
    let large_body = body.chars().count() > 2_000 || body.lines().count() > 40;
    let requires_user_approval = semantic_content && large_body;
    WriteApprovalPolicy {
        requires_user_approval,
        reason: requires_user_approval.then(|| {
            "large semantic wiki/graph changes require explicit user approval".to_string()
        }),
    }
}

fn validate_evidence_refs(
    snapshot: &BrainRepoSnapshot,
    workspace_id: &str,
    evidence_refs: &[String],
) -> Result<()> {
    if evidence_refs.is_empty() {
        bail!("at least one evidence_ref is required");
    }
    let valid_ids: BTreeSet<&str> = snapshot.evidence.iter().map(|ev| ev.id.as_str()).collect();
    for evidence_id in evidence_refs {
        if !valid_ids.contains(evidence_id.as_str()) {
            bail!(
                "evidence_ref {} was not found in workspace {}",
                evidence_id,
                workspace_id
            );
        }
    }
    Ok(())
}

fn classify_agent_write_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("evidence_ref") || lower.contains("evidence ref") {
        "evidence_scope"
    } else if lower.contains("proposalid")
        || lower.contains("proposal ")
        || lower.contains("not found")
        || lower.contains("already committed")
        || lower.contains("already rejected")
    {
        "proposal_state"
    } else if lower.contains("unsupported contenttype")
        || lower.contains("not implemented")
        || lower.contains("schema")
    {
        "schema"
    } else if lower.contains("approval") {
        "approval_required"
    } else if lower.contains("permission")
        || lower.contains("readonly")
        || lower.contains("read-only")
        || lower.contains("database is locked")
        || lower.contains("database is busy")
        || lower.contains("failed writing")
        || lower.contains("failed removing")
        || lower.contains("failed committing")
    {
        "persistence"
    } else {
        "unknown"
    }
}

fn validate_committable_proposal(
    proposal: &AgentWriteProposal,
    request: &WriteCommitRequest,
) -> Result<()> {
    validate_proposal_id(&proposal.proposal_id)?;
    if proposal.proposal_id != request.proposal_id {
        bail!("proposalId mismatch between request and proposal file");
    }
    if proposal.workspace_id != request.scope.workspace_id {
        bail!("proposal workspaceId does not match request workspaceId");
    }
    validate_write_content_type(&proposal.content_type)?;
    if proposal.requires_user_approval && !request.user_approved {
        bail!(
            "{}",
            proposal
                .approval_reason
                .as_deref()
                .unwrap_or("proposal requires explicit user approval before commit")
        );
    }
    if !is_committable_agent_write_content_type(&proposal.content_type) {
        bail!(
            "committing contentType {} is not implemented yet",
            proposal.content_type
        );
    }
    if proposal.title.trim().is_empty() {
        bail!("proposal title must not be empty");
    }
    if proposal.body.trim().is_empty() {
        bail!("proposal body must not be empty");
    }

    let root = resolve_brain_workspace_root(&request.scope)?;
    let snapshot = read_materialized_brain_snapshot(&root, &request.scope.workspace_id)?;
    validate_evidence_refs(
        &snapshot,
        &request.scope.workspace_id,
        &proposal.evidence_refs,
    )
}

fn is_committable_agent_write_content_type(content_type: &str) -> bool {
    matches!(
        content_type.trim(),
        "memory" | "evidence_refresh" | "link_repair"
    )
}
