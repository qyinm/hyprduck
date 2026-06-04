use anyhow::{anyhow, bail, Context, Result};
use hyprduck_engine_client::EngineClient;
use hyprduck_engine_types::{
    ApplyGraphPatchRequest, GetBrainHealthRequest, GetContextPackRequest, ReadContextPackRequest,
    ReadGraphHistoryRequest, ReadGraphSnapshotRequest, ReadNodeRequest, ReadPageEvidenceRequest,
    ReadRecentEventsRequest, ReadSourceRequest, ReadWikiPageRequest, SearchBrainRequest,
    WriteCommitAllRequest, WriteCommitRequest, WriteListRequest, WriteProposeRequest,
    WriteRejectRequest,
};
use serde_json::{json, Map, Value};

use super::args::{
    import_document_format, optional_bool, optional_string, optional_usize, read_scope,
    required_string, required_string_array, validate_mcp_proposal_id,
    validate_mcp_write_content_type,
};
use super::cache::{cache_sensitive_tool, read_graph_wiki_cache_state, McpGraphWikiCacheState};
use super::import_jobs::{
    ensure_import_job_scope, import_job_lookup, next_import_job_id, resolve_import_job,
    retry_import_graph, spawn_import_job, ImportJobRequest, ImportJobSnapshot,
};
use super::policy::validate_import_source_path;
use super::protocol::McpServerState;

pub(in crate::mcp) const LOCAL_PATH_DISCLOSURE_ENV: &str = "HYPRDUCK_MCP_ALLOW_LOCAL_PATHS";

pub(in crate::mcp) struct McpToolResult {
    pub(in crate::mcp) value: Value,
    pub(in crate::mcp) cache_state: Option<McpGraphWikiCacheState>,
}

pub(in crate::mcp) fn call_tool(
    client: &dyn EngineClient,
    state: &McpServerState,
    name: &str,
    arguments: &Map<String, Value>,
    include_local_paths: bool,
) -> Result<McpToolResult> {
    let scope = read_scope(arguments)?;
    let cache_scope = scope.clone();
    let cache_before = cache_sensitive_tool(name)
        .then(|| read_graph_wiki_cache_state(client, &cache_scope))
        .transpose()?
        .flatten();

    let value = match name {
        "import_source" => {
            let source_path = required_string(arguments, "sourcePath")?;
            let source_path = validate_import_source_path(&source_path)?;
            let format =
                import_document_format(&source_path, optional_string(arguments, "format")?)?;
            let name = optional_string(arguments, "name")?;
            let skip_graph_generation =
                optional_bool(arguments, "skipGraphGeneration")?.unwrap_or(false);
            let job_id = next_import_job_id();
            let job = ImportJobSnapshot::queued(job_id.clone(), &scope);
            state.import_jobs.insert(job.clone());
            spawn_import_job(
                state.import_jobs.clone(),
                ImportJobRequest {
                    job_id,
                    scope,
                    source_path,
                    format,
                    name,
                    skip_graph_generation,
                },
            );
            job.to_value()
        }
        "import_status" => {
            let (job_id, source_id) = import_job_lookup(arguments)?;
            let job = resolve_import_job(client, state, &scope, job_id, source_id)?;
            job.to_value()
        }
        "import_cancel" => {
            let job_id = required_string(arguments, "jobId")?;
            let job = state
                .import_jobs
                .get(&job_id)
                .ok_or_else(|| anyhow!("import job not found: {job_id}"))?;
            ensure_import_job_scope(&job, &scope)?;
            let job = state.import_jobs.cancel(&job_id)?;
            job.to_value()
        }
        "import_retry_graph" => {
            let (job_id, source_id) = import_job_lookup(arguments)?;
            let job = resolve_import_job(client, state, &scope, job_id, source_id)?;
            retry_import_graph(&state.import_jobs, &job)?;
            state
                .import_jobs
                .get(&job.job_id)
                .ok_or_else(|| anyhow!("import job not found after graph retry"))?
                .to_value()
        }
        "search_documents" | "search_brain" => {
            let query = required_string(arguments, "query")?;
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(client.search_brain(SearchBrainRequest {
                scope,
                query,
                limit,
            })?)?
        }
        "get_context_pack" => {
            let query = required_string(arguments, "query")?;
            let selected_node_id = optional_string(arguments, "nodeId")?;
            let budget = optional_usize(arguments, "budget")?;
            let response = client.get_context_pack(GetContextPackRequest {
                scope,
                query,
                selected_node_id,
                budget,
                persist: false,
            })?;
            json!({
                "contextPack": response.context_pack_v1.clone(),
                "contextPackV1": response.context_pack_v1,
                "contextPackV0": response.context_pack_v0,
                "persistedContextPackPath": response.persisted_context_pack_path,
            })
        }
        "read_context_pack" => {
            let pack_id = optional_string(arguments, "packId")?;
            serde_json::to_value(
                client.read_context_pack(ReadContextPackRequest { scope, pack_id })?,
            )?
        }
        "read_source" => {
            let source_id = required_string(arguments, "sourceId")?;
            serde_json::to_value(client.read_source(ReadSourceRequest {
                scope,
                source_id,
                include_local_paths,
            })?)?
        }
        "read_page_evidence" => {
            let source_id = required_string(arguments, "sourceId")?;
            let page = optional_usize(arguments, "page")?;
            if page == Some(0) {
                return Err(anyhow!("argument page must be a positive 1-based integer"));
            }
            serde_json::to_value(client.read_page_evidence(ReadPageEvidenceRequest {
                scope,
                source_id,
                page,
                include_local_paths,
            })?)?
        }
        "read_wiki_page" => {
            let path = required_string(arguments, "path")?;
            serde_json::to_value(client.read_wiki_page(ReadWikiPageRequest { scope, path })?)?
        }
        "read_node" => {
            let node_id = required_string(arguments, "nodeId")?;
            serde_json::to_value(client.read_node(ReadNodeRequest { scope, node_id })?)?
        }
        "read_recent_events" => {
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(client.read_recent_events(ReadRecentEventsRequest {
                scope,
                limit,
                run_id: optional_string(arguments, "runId")?,
                source_ref: optional_string(arguments, "sourceRef")?,
                node_id: optional_string(arguments, "nodeId")?,
                edge_id: optional_string(arguments, "edgeId")?,
                claim_id: optional_string(arguments, "claimId")?,
                memory_id: optional_string(arguments, "memoryId")?,
                change_type: optional_string(arguments, "changeType")?,
            })?)?
        }
        "read_graph_history" => {
            let limit = optional_usize(arguments, "limit")?;
            serde_json::to_value(
                client.read_graph_history(ReadGraphHistoryRequest { scope, limit })?,
            )?
        }
        "read_graph_snapshot" => {
            serde_json::to_value(client.read_graph_snapshot(ReadGraphSnapshotRequest {
                scope,
                include_local_paths,
            })?)?
        }
        "read_health" => {
            serde_json::to_value(client.get_brain_health(GetBrainHealthRequest { scope })?)?
        }
        "graph_patch_apply" => {
            let graph_patch_value = arguments
                .get("graphPatch")
                .cloned()
                .ok_or_else(|| anyhow!("missing required argument: graphPatch"))?;
            let graph_patch: hyprduck_engine_types::GraphPatch =
                serde_json::from_value(graph_patch_value)
                    .context("argument graphPatch does not match HyprDuck graph patch schema")?;
            let agent_id = optional_string(arguments, "agentId")?;
            serde_json::to_value(client.apply_graph_patch(ApplyGraphPatchRequest {
                scope,
                graph_patch,
                agent_id,
            })?)?
        }
        "write_propose" => {
            let content_type = required_string(arguments, "contentType")?;
            validate_mcp_write_content_type(&content_type)?;
            let title = required_string(arguments, "title")?;
            let body = required_string(arguments, "body")?;
            let evidence_refs = required_string_array(arguments, "evidenceRefs")?;
            serde_json::to_value(client.write_propose(WriteProposeRequest {
                scope,
                content_type,
                title,
                body,
                evidence_refs,
            })?)?
        }
        "write_commit" => {
            let proposal_id = required_string(arguments, "proposalId")?;
            validate_mcp_proposal_id(&proposal_id)?;
            let user_approved = arguments
                .get("userApproved")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            serde_json::to_value(client.write_commit(WriteCommitRequest {
                scope,
                proposal_id,
                user_approved,
            })?)?
        }
        "write_commit_all" => {
            let proposal_ids = required_string_array(arguments, "proposalIds")?;
            for proposal_id in &proposal_ids {
                validate_mcp_proposal_id(proposal_id)?;
            }
            serde_json::to_value(client.write_commit_all(WriteCommitAllRequest {
                scope,
                proposal_ids,
            })?)?
        }
        "write_list" => serde_json::to_value(client.write_list(WriteListRequest { scope })?)?,
        "write_reject" => {
            let proposal_id = required_string(arguments, "proposalId")?;
            validate_mcp_proposal_id(&proposal_id)?;
            serde_json::to_value(client.write_reject(WriteRejectRequest { scope, proposal_id })?)?
        }
        _ => return Err(anyhow!("Unknown HyprDuck MCP tool: {name}")),
    };

    let cache_after = cache_sensitive_tool(name)
        .then(|| read_graph_wiki_cache_state(client, &cache_scope))
        .transpose()?
        .flatten();
    Ok(McpToolResult {
        value,
        cache_state: cache_after.map(|after| McpGraphWikiCacheState {
            invalidated: cache_before.as_ref() != Some(&after),
            current: after,
        }),
    })
}

pub(in crate::mcp) fn local_path_disclosure_for_tool(
    name: &str,
    arguments: &Map<String, Value>,
) -> Result<bool> {
    let requested = optional_bool(arguments, "includeLocalPaths")?.unwrap_or(false);
    if !requested {
        return Ok(false);
    }
    if !supports_local_path_disclosure(name) {
        bail!("includeLocalPaths is not supported for tool {name}");
    }
    if !local_path_disclosure_allowed() {
        bail!("includeLocalPaths requires HYPRDUCK_MCP_ALLOW_LOCAL_PATHS=1");
    }
    Ok(true)
}

pub(in crate::mcp) fn supports_local_path_disclosure(name: &str) -> bool {
    matches!(
        name,
        "read_source" | "read_page_evidence" | "read_graph_snapshot"
    )
}

fn local_path_disclosure_allowed() -> bool {
    std::env::var(LOCAL_PATH_DISCLOSURE_ENV).is_ok_and(|value| value == "1")
}
