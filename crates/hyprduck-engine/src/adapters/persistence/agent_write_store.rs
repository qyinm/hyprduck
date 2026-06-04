use anyhow::{Context, Result};
use graphqlite::Graph;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::BrainEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AgentWriteProposalRecord {
    pub(crate) proposal_id: String,
    pub(crate) workspace_id: String,
    pub(crate) content_type: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) actor_id: String,
    pub(crate) validation_status: String,
    pub(crate) requires_user_approval: bool,
    pub(crate) approval_reason: Option<String>,
    pub(crate) approval_status: String,
    pub(crate) proposal_json: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

pub(super) fn persist_agent_write_proposal(
    path: &Path,
    proposal: &AgentWriteProposalRecord,
) -> Result<()> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute_batch("BEGIN IMMEDIATE")
        .context("failed starting agent proposal transaction")?;

    let result = (|| -> Result<()> {
        let evidence_refs_json = serde_json::to_string(&proposal.evidence_refs)
            .context("failed encoding agent proposal evidence refs")?;
        sqlite
            .execute(
                "INSERT INTO agent_write_proposals (
                        proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                    ON CONFLICT(proposal_id) DO UPDATE SET
                        workspace_id=excluded.workspace_id,
                        content_type=excluded.content_type,
                        title=excluded.title,
                        body=excluded.body,
                        evidence_refs_json=excluded.evidence_refs_json,
                        actor_id=excluded.actor_id,
                        validation_status=excluded.validation_status,
                        requires_user_approval=excluded.requires_user_approval,
                        approval_reason=excluded.approval_reason,
                        approval_status=excluded.approval_status,
                        proposal_json=excluded.proposal_json,
                        updated_at=excluded.updated_at",
                (
                    proposal.proposal_id.as_str(),
                    proposal.workspace_id.as_str(),
                    proposal.content_type.as_str(),
                    proposal.title.as_str(),
                    proposal.body.as_str(),
                    evidence_refs_json.as_str(),
                    proposal.actor_id.as_str(),
                    proposal.validation_status.as_str(),
                    if proposal.requires_user_approval {
                        1
                    } else {
                        0
                    },
                    proposal.approval_reason.as_deref(),
                    proposal.approval_status.as_str(),
                    proposal.proposal_json.as_str(),
                    proposal.created_at,
                    proposal.updated_at,
                ),
            )
            .with_context(|| {
                format!(
                    "failed inserting agent write proposal {}",
                    proposal.proposal_id
                )
            })?;
        sqlite
            .execute(
                "DELETE FROM agent_write_proposal_evidence_refs WHERE proposal_id = ?1",
                [proposal.proposal_id.as_str()],
            )
            .with_context(|| {
                format!(
                    "failed clearing agent write proposal evidence refs {}",
                    proposal.proposal_id
                )
            })?;
        for evidence_ref in &proposal.evidence_refs {
            sqlite
                .execute(
                    "INSERT INTO agent_write_proposal_evidence_refs (proposal_id, evidence_ref)
                         VALUES (?1, ?2)",
                    (proposal.proposal_id.as_str(), evidence_ref.as_str()),
                )
                .with_context(|| {
                    format!(
                        "failed inserting agent write proposal evidence ref {}",
                        proposal.proposal_id
                    )
                })?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            sqlite
                .execute_batch("COMMIT")
                .context("failed committing agent proposal transaction")?;
            Ok(())
        }
        Err(error) => {
            let _ = sqlite.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

pub(super) fn load_agent_write_proposal(
    path: &Path,
    workspace_id: &str,
    proposal_id: &str,
) -> Result<Option<AgentWriteProposalRecord>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let mut statement = graph
        .connection()
        .sqlite_connection()
        .prepare(
            "SELECT proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                 FROM agent_write_proposals
                 WHERE workspace_id = ?1 AND proposal_id = ?2",
        )
        .context("failed preparing agent write proposal lookup")?;
    let mut rows = statement
        .query((workspace_id, proposal_id))
        .context("failed querying agent write proposal")?;
    if let Some(row) = rows
        .next()
        .context("failed reading agent write proposal row")?
    {
        let evidence_refs_json: String = row.get(5).context("read proposal evidence refs")?;
        let requires_user_approval: i64 =
            row.get(8).context("read proposal approval requirement")?;
        return Ok(Some(AgentWriteProposalRecord {
            proposal_id: row.get(0).context("read proposal id")?,
            workspace_id: row.get(1).context("read proposal workspace")?,
            content_type: row.get(2).context("read proposal content type")?,
            title: row.get(3).context("read proposal title")?,
            body: row.get(4).context("read proposal body")?,
            evidence_refs: decode_agent_write_proposal_evidence_refs(&evidence_refs_json)?,
            actor_id: row.get(6).context("read proposal actor")?,
            validation_status: row.get(7).context("read proposal validation status")?,
            requires_user_approval: requires_user_approval != 0,
            approval_reason: row.get(9).context("read proposal approval reason")?,
            approval_status: row.get(10).context("read proposal approval status")?,
            proposal_json: row.get(11).context("read proposal json")?,
            created_at: row.get(12).context("read proposal created at")?,
            updated_at: row.get(13).context("read proposal updated at")?,
        }));
    }
    Ok(None)
}

pub(super) fn list_pending_agent_write_proposals(
    path: &Path,
    workspace_id: &str,
) -> Result<Vec<AgentWriteProposalRecord>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let mut statement = graph
        .connection()
        .sqlite_connection()
        .prepare(
            "SELECT proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                 FROM agent_write_proposals
                 WHERE workspace_id = ?1
                   AND approval_status IN ('pending', 'pending_user_approval')
                 ORDER BY created_at DESC, proposal_id DESC",
        )
        .context("failed preparing pending agent proposal list")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying pending agent proposals")?;
    let mut proposals = Vec::new();
    while let Some(row) = rows.next().context("failed reading agent proposal row")? {
        let evidence_refs_json: String = row.get(5).context("read proposal evidence refs")?;
        let requires_user_approval: i64 =
            row.get(8).context("read proposal approval requirement")?;
        proposals.push(AgentWriteProposalRecord {
            proposal_id: row.get(0).context("read proposal id")?,
            workspace_id: row.get(1).context("read proposal workspace")?,
            content_type: row.get(2).context("read proposal content type")?,
            title: row.get(3).context("read proposal title")?,
            body: row.get(4).context("read proposal body")?,
            evidence_refs: decode_agent_write_proposal_evidence_refs(&evidence_refs_json)?,
            actor_id: row.get(6).context("read proposal actor")?,
            validation_status: row.get(7).context("read proposal validation status")?,
            requires_user_approval: requires_user_approval != 0,
            approval_reason: row.get(9).context("read proposal approval reason")?,
            approval_status: row.get(10).context("read proposal approval status")?,
            proposal_json: row.get(11).context("read proposal json")?,
            created_at: row.get(12).context("read proposal created at")?,
            updated_at: row.get(13).context("read proposal updated at")?,
        });
    }
    Ok(proposals)
}

pub(super) fn update_agent_write_proposal_status(
    path: &Path,
    workspace_id: &str,
    proposal_id: &str,
    approval_status: &str,
    updated_at: i64,
) -> Result<()> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    graph
        .connection()
        .sqlite_connection()
        .execute(
            "UPDATE agent_write_proposals
                 SET approval_status = ?3, updated_at = ?4
                 WHERE workspace_id = ?1 AND proposal_id = ?2",
            (workspace_id, proposal_id, approval_status, updated_at),
        )
        .with_context(|| format!("failed updating proposal status {proposal_id}"))?;
    Ok(())
}

pub(super) fn record_agent_write_commit(
    path: &Path,
    workspace_id: &str,
    proposal_id: &str,
    event: &BrainEvent,
    updated_at: i64,
) -> Result<()> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute_batch("BEGIN IMMEDIATE")
        .context("failed starting agent write commit audit transaction")?;

    let result = (|| -> Result<()> {
        let actor_json = serde_json::to_string(&event.actor)
            .context("failed encoding agent write event actor")?;
        let evidence_refs_json = serde_json::to_string(&event.evidence_refs)
            .context("failed encoding agent write event evidence refs")?;
        let operation_type = event
            .operation_type
            .clone()
            .unwrap_or_else(|| format!("{:?}", event.event_type).to_ascii_lowercase());
        let payload_json = if event.payload_json.trim().is_empty() {
            "{}"
        } else {
            event.payload_json.as_str()
        };
        sqlite
            .execute(
                "INSERT INTO brain_events (
                        event_id,
                        workspace_id,
                        actor_json,
                        operation_type,
                        evidence_refs_json,
                        payload_json,
                        created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(event_id) DO UPDATE SET
                        workspace_id=excluded.workspace_id,
                        actor_json=excluded.actor_json,
                        operation_type=excluded.operation_type,
                        evidence_refs_json=excluded.evidence_refs_json,
                        payload_json=excluded.payload_json,
                        created_at=excluded.created_at",
                (
                    event.event_id.as_str(),
                    event.workspace_id.as_str(),
                    actor_json.as_str(),
                    operation_type.as_str(),
                    evidence_refs_json.as_str(),
                    payload_json,
                    event.created_at as i64,
                ),
            )
            .with_context(|| format!("failed inserting brain event row {}", event.event_id))?;
        sqlite
            .execute(
                "UPDATE agent_write_proposals
                     SET approval_status = 'committed', updated_at = ?3
                     WHERE workspace_id = ?1 AND proposal_id = ?2",
                (workspace_id, proposal_id, updated_at),
            )
            .with_context(|| format!("failed marking proposal {proposal_id} committed"))?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            sqlite
                .execute_batch("COMMIT")
                .context("failed committing agent write audit transaction")?;
            Ok(())
        }
        Err(error) => {
            let _ = sqlite.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

#[cfg(test)]
pub(super) fn load_brain_event_operation(
    path: &Path,
    workspace_id: &str,
    event_id: &str,
) -> Result<Option<String>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let mut statement = graph
        .connection()
        .sqlite_connection()
        .prepare(
            "SELECT operation_type
                 FROM brain_events
                 WHERE workspace_id = ?1 AND event_id = ?2",
        )
        .context("failed preparing brain event lookup")?;
    let mut rows = statement
        .query((workspace_id, event_id))
        .context("failed querying brain event")?;
    if let Some(row) = rows.next().context("failed reading brain event row")? {
        return Ok(Some(row.get(0).context("read operation type")?));
    }
    Ok(None)
}

fn decode_agent_write_proposal_evidence_refs(value: &str) -> Result<Vec<String>> {
    serde_json::from_str(value).context("failed decoding agent proposal evidence refs")
}
