use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::BrainRepoSnapshot;

pub(super) fn persist_brain_events_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for event in &snapshot.events {
        let actor_json =
            serde_json::to_string(&event.actor).context("failed encoding brain event actor")?;
        let evidence_refs_json = serde_json::to_string(&event.evidence_refs)
            .context("failed encoding brain event evidence refs")?;
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
    }
    Ok(())
}
