use hyprduck_engine_types::{AgentGraphProposalPayload, BrainActor};
use serde_json::{json, Value};

pub fn queued_proposal_provider_response_value(
    run_id: &str,
    workspace_id: &str,
    proposal_id: &str,
    actor: &BrainActor,
    proposal_payload: Option<&AgentGraphProposalPayload>,
    created_at: u64,
) -> Value {
    json!({
        "runId": run_id,
        "workspaceId": workspace_id,
        "proposalId": proposal_id,
        "status": "queued_proposal",
        "actor": actor,
        "providerResponse": null,
        "proposalPayload": proposal_payload,
        "createdAt": created_at,
    })
}
