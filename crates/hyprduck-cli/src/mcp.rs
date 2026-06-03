#[cfg(test)]
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use hyprduck_engine_client::EngineClient;
#[cfg(test)]
use hyprduck_engine_types::{
    graph_status_is_ready, DocumentFormat, ImportLifecyclePhase as ImportJobPhase,
    ImportLifecycleStatus as ImportJobStatus,
};
use hyprduck_engine_types::{
    ApplyGraphPatchRequest, BrainReadScope, GetBrainHealthRequest, GetContextPackRequest,
    ReadContextPackRequest, ReadGraphHistoryRequest, ReadGraphSnapshotRequest, ReadNodeRequest,
    ReadPageEvidenceRequest, ReadRecentEventsRequest, ReadSourceRequest, ReadWikiPageRequest,
    SearchBrainRequest, WriteCommitAllRequest, WriteCommitRequest, WriteListRequest,
    WriteProposeRequest, WriteRejectRequest,
};
#[cfg(test)]
use serde_json::json;
use serde_json::{Map, Value};

mod args;
mod import_jobs;
mod policy;
mod protocol;
mod resources;
mod responses;
mod tool_catalog;

pub use protocol::run_mcp_server;

use args::{
    import_document_format, optional_bool, optional_string, optional_usize, read_scope,
    required_string, required_string_array, validate_mcp_proposal_id,
    validate_mcp_write_content_type,
};
#[cfg(test)]
use args::{PROPOSAL_ID_PATTERN, WRITE_CONTENT_TYPES};
#[cfg(test)]
use import_jobs::{
    classify_graph_failure, import_phase_from_parse_progress, record_graph_status_persist_result,
    sanitize_graph_error_message,
};
use import_jobs::{
    ensure_import_job_scope, import_job_lookup, next_import_job_id, resolve_import_job,
    retry_import_graph, spawn_import_job, ImportJobRegistry, ImportJobRequest, ImportJobSnapshot,
};
#[cfg(test)]
use policy::redact_local_path_text;
use policy::validate_import_source_path;
#[cfg(test)]
use policy::{IMPORT_ALLOWED_ROOTS_ENV, ROOT_DIR_ALLOWED_ROOTS_ENV, ROOT_DIR_ENV};
use protocol::McpServerState;
#[cfg(test)]
use resources::parse_resource_uri;
#[cfg(test)]
use responses::classify_mcp_error;
pub(in crate::mcp) use tool_catalog::tool_definitions;

const LOCAL_PATH_DISCLOSURE_ENV: &str = "HYPRDUCK_MCP_ALLOW_LOCAL_PATHS";

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
            serde_json::json!({
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

struct McpToolResult {
    value: Value,
    cache_state: Option<McpGraphWikiCacheState>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpGraphWikiCacheState {
    invalidated: bool,
    current: McpGraphWikiCacheToken,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct McpGraphWikiCacheToken {
    workspace_id: String,
    snapshot_id: String,
    source_ingest_id: String,
    materialized_at: u64,
    latest_readable_snapshot_path: String,
    materialized_paths: Vec<String>,
}

fn cache_sensitive_tool(name: &str) -> bool {
    matches!(name, "graph_patch_apply" | "read_health")
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

fn supports_local_path_disclosure(name: &str) -> bool {
    matches!(
        name,
        "read_source" | "read_page_evidence" | "read_graph_snapshot"
    )
}

fn local_path_disclosure_allowed() -> bool {
    std::env::var(LOCAL_PATH_DISCLOSURE_ENV).is_ok_and(|value| value == "1")
}

fn read_graph_wiki_cache_state(
    client: &dyn EngineClient,
    scope: &BrainReadScope,
) -> Result<Option<McpGraphWikiCacheToken>> {
    match client.read_graph_snapshot(ReadGraphSnapshotRequest {
        scope: scope.clone(),
        include_local_paths: false,
    }) {
        Ok(snapshot) => Ok(Some(McpGraphWikiCacheToken {
            workspace_id: snapshot.workspace_id,
            snapshot_id: snapshot.snapshot_id,
            source_ingest_id: snapshot.source_ingest_id,
            materialized_at: snapshot.materialized_at,
            latest_readable_snapshot_path: snapshot.latest_readable_snapshot_path,
            materialized_paths: snapshot.materialized_paths,
        })),
        Err(error)
            if error.to_string().contains("No such file")
                || error.to_string().contains("not found") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_root_dir_env() {
        std::env::remove_var(ROOT_DIR_ENV);
        std::env::remove_var(ROOT_DIR_ALLOWED_ROOTS_ENV);
        std::env::remove_var(IMPORT_ALLOWED_ROOTS_ENV);
        std::env::remove_var(LOCAL_PATH_DISCLOSURE_ENV);
    }

    fn set_allowed_roots(paths: &[&Path]) {
        let joined = std::env::join_paths(paths).expect("join allowed roots");
        std::env::set_var(ROOT_DIR_ALLOWED_ROOTS_ENV, joined);
    }

    fn set_allowed_import_roots(paths: &[&Path]) {
        let joined = std::env::join_paths(paths).expect("join allowed import roots");
        std::env::set_var(IMPORT_ALLOWED_ROOTS_ENV, joined);
    }

    fn canonical_path_string(path: &Path) -> String {
        path.canonicalize()
            .expect("canonical path")
            .into_os_string()
            .into_string()
            .expect("utf-8 canonical path")
    }

    #[test]
    fn tool_definitions_expose_agent_session_write_tools_as_mutating_tools() {
        let tools = tool_definitions();
        let tool_by_name = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap_or_else(|| panic!("missing tool {name}"))
        };

        for name in [
            "import_source",
            "import_cancel",
            "import_retry_graph",
            "graph_patch_apply",
            "write_propose",
            "write_commit",
            "write_commit_all",
            "write_list",
            "write_reject",
        ] {
            let tool = tool_by_name(name);
            assert_eq!(tool["name"], name);
            assert!(tool["inputSchema"]["properties"]
                .get("workspaceId")
                .is_some());
            if name != "write_list" {
                assert!(tool["annotations"].get("hyprduckMutationPolicy").is_some());
            }
        }
        assert_eq!(
            tool_by_name("write_propose")["inputSchema"]["required"],
            json!(["contentType", "title", "body", "evidenceRefs"])
        );
        assert_eq!(
            tool_by_name("write_propose")["inputSchema"]["properties"]["contentType"]["enum"],
            json!(WRITE_CONTENT_TYPES)
        );
        assert_eq!(
            tool_by_name("write_propose")["inputSchema"]["properties"]["evidenceRefs"]["minItems"],
            json!(1)
        );
        assert_eq!(
            tool_by_name("write_propose")["inputSchema"]["properties"]["evidenceRefs"]
                ["uniqueItems"],
            json!(true)
        );
        assert_eq!(
            tool_by_name("import_source")["inputSchema"]["required"],
            json!(["sourcePath"])
        );
        assert_eq!(
            tool_by_name("import_source")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("import_source")["annotations"]["idempotentHint"],
            false
        );
        assert_eq!(
            tool_by_name("import_status")["inputSchema"]["required"],
            json!([])
        );
        assert!(tool_by_name("import_status")["inputSchema"]["properties"]
            .get("sourceId")
            .is_some());
        assert_eq!(
            tool_by_name("import_status")["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(
            tool_by_name("import_cancel")["inputSchema"]["required"],
            json!(["jobId"])
        );
        assert_eq!(
            tool_by_name("import_cancel")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("import_retry_graph")["inputSchema"]["required"],
            json!([])
        );
        assert!(
            tool_by_name("import_retry_graph")["inputSchema"]["properties"]
                .get("sourceId")
                .is_some()
        );
        assert_eq!(
            tool_by_name("import_retry_graph")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("graph_patch_apply")["inputSchema"]["required"],
            json!(["graphPatch"])
        );
        assert_eq!(
            tool_by_name("graph_patch_apply")["inputSchema"]["properties"]["graphPatch"]
                ["properties"]["schemaVersion"]["const"],
            json!(hyprduck_engine_types::GRAPH_PATCH_SCHEMA_VERSION)
        );
        assert_eq!(
            tool_by_name("graph_patch_apply")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("write_commit_all")["inputSchema"]["required"],
            json!(["proposalIds"])
        );
        assert_eq!(
            tool_by_name("write_commit")["inputSchema"]["properties"]["proposalId"]["pattern"],
            json!(PROPOSAL_ID_PATTERN)
        );
        assert_eq!(
            tool_by_name("write_commit")["inputSchema"]["properties"]
                .get("userApproved")
                .and_then(Value::as_object)
                .and_then(|property| property.get("type"))
                .and_then(Value::as_str),
            Some("boolean")
        );
        assert_eq!(
            tool_by_name("write_commit_all")["inputSchema"]["properties"]["proposalIds"]
                ["minItems"],
            json!(1)
        );
        assert_eq!(
            tool_by_name("write_commit_all")["inputSchema"]["properties"]["proposalIds"]["items"]
                ["pattern"],
            json!(PROPOSAL_ID_PATTERN)
        );
        assert_eq!(
            tool_by_name("write_reject")["inputSchema"]["properties"]["proposalId"]["pattern"],
            json!(PROPOSAL_ID_PATTERN)
        );
        assert_eq!(
            tool_by_name("write_propose")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("write_commit")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("write_commit_all")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("write_reject")["annotations"]["readOnlyHint"],
            false
        );
        assert_eq!(
            tool_by_name("write_commit")["annotations"]["hyprduckMutationPolicy"]["replayPolicy"],
            json!("already committed, rejected, or missing proposal IDs fail with proposal_state")
        );
    }

    #[test]
    fn mcp_error_classifier_returns_stable_categories() {
        assert_eq!(
            classify_mcp_error("rootDir is disabled unless HYPRDUCK_MCP_ALLOW_ROOT_DIR=1"),
            "path_policy"
        );
        assert_eq!(
            classify_mcp_error("graphPatch references unknown or out-of-scope evidence ref ev-1"),
            "evidence_scope"
        );
        assert_eq!(
            classify_mcp_error("argument graphPatch does not match HyprDuck graph patch schema"),
            "schema"
        );
        assert_eq!(
            classify_mcp_error("OpenRouter API key is missing"),
            "provider"
        );
        assert_eq!(
            classify_mcp_error("failed writing graph materialization snapshot"),
            "graph_materialization"
        );
        assert_eq!(
            classify_mcp_error("import job not found after graph retry"),
            "lifecycle"
        );
        assert_eq!(
            classify_mcp_error("GraphQLite failed to open knowledge DB"),
            "persistence"
        );
    }

    #[test]
    fn graph_patch_mcp_schema_covers_engine_contract_fields() {
        let tools = tool_definitions();
        let graph_patch_schema = tools
            .iter()
            .find(|tool| tool["name"] == "graph_patch_apply")
            .expect("graph_patch_apply tool")["inputSchema"]["properties"]["graphPatch"]
            .clone();
        let graph_patch_properties = graph_patch_schema["properties"]
            .as_object()
            .expect("graphPatch schema properties");
        for field in [
            "schemaVersion",
            "sourceIds",
            "evidenceRefs",
            "nodes",
            "relations",
            "claims",
            "wikiPages",
            "agentMetadata",
        ] {
            assert!(
                graph_patch_properties.contains_key(field),
                "missing graphPatch schema field {field}"
            );
        }

        let object_item_properties = |field: &str| {
            graph_patch_properties[field]["items"]["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("missing {field} item properties"))
        };
        for field in [
            "nodeId",
            "kind",
            "label",
            "scope",
            "aliases",
            "sourceIds",
            "evidenceIds",
        ] {
            assert!(object_item_properties("nodes").contains_key(field));
        }
        for field in [
            "relationId",
            "kind",
            "sourceNodeId",
            "targetNodeId",
            "label",
            "evidenceIds",
        ] {
            assert!(object_item_properties("relations").contains_key(field));
        }
        for field in [
            "claimId",
            "statement",
            "topicRefs",
            "sourceRefs",
            "evidenceRefs",
            "status",
        ] {
            assert!(object_item_properties("claims").contains_key(field));
        }
        for field in [
            "pageId",
            "path",
            "title",
            "body",
            "nodeRefs",
            "sourceRefs",
            "evidenceRefs",
        ] {
            assert!(object_item_properties("wikiPages").contains_key(field));
        }

        let graph_patch_value = json!({
            "schemaVersion": hyprduck_engine_types::GRAPH_PATCH_SCHEMA_VERSION,
            "sourceIds": ["source-agent"],
            "evidenceRefs": ["ev-agent-1"],
            "nodes": [{
                "nodeId": "concept-agent",
                "kind": "concept",
                "label": "Agent",
                "scope": "project",
                "aliases": ["Codex"],
                "sourceIds": ["source-agent"],
                "evidenceIds": ["ev-agent-1"]
            }],
            "relations": [{
                "relationId": "rel-agent-source",
                "kind": "mentions",
                "sourceNodeId": "source:source-agent",
                "targetNodeId": "concept-agent",
                "label": "mentions",
                "evidenceIds": ["ev-agent-1"]
            }],
            "claims": [{
                "claimId": "claim-agent",
                "statement": "Agents can submit graph patches.",
                "topicRefs": ["concept-agent"],
                "sourceRefs": ["source-agent"],
                "evidenceRefs": ["ev-agent-1"],
                "status": "agent_generated"
            }],
            "wikiPages": [{
                "pageId": "wiki-agent",
                "path": "wiki/agent.md",
                "title": "Agent",
                "body": "Evidence-backed agent page.",
                "nodeRefs": ["concept-agent"],
                "sourceRefs": ["source-agent"],
                "evidenceRefs": ["ev-agent-1"]
            }],
            "agentMetadata": { "agent": "codex" }
        });
        let graph_patch: hyprduck_engine_types::GraphPatch =
            serde_json::from_value(graph_patch_value).expect("deserialize graph patch");

        assert_eq!(
            graph_patch.nodes[0].scope,
            Some(hyprduck_engine_types::BrainScope::Project)
        );
        assert_eq!(graph_patch.relations[0].label, "mentions");
        assert_eq!(graph_patch.claims[0].status, "agent_generated");
        assert_eq!(
            graph_patch.agent_metadata.get("agent"),
            Some(&json!("codex"))
        );
    }

    #[test]
    fn mcp_write_arguments_reject_broad_or_unauditable_inputs() {
        assert!(validate_mcp_write_content_type("memory").is_ok());
        assert!(validate_mcp_write_content_type("wiki_page").is_err());
        assert!(validate_mcp_write_content_type("shell_command").is_err());
        assert!(validate_mcp_write_content_type("../memory").is_err());

        assert!(validate_mcp_proposal_id("prop-0123456789abcdef0123456789ABCDEF").is_ok());
        assert!(validate_mcp_proposal_id("prop-1234").is_err());
        assert!(validate_mcp_proposal_id("../prop-0123456789abcdef0123456789abcdef").is_err());

        let mut arguments = Map::new();
        arguments.insert("evidenceRefs".into(), json!([]));
        let error = required_string_array(&arguments, "evidenceRefs")
            .expect_err("empty evidence refs rejected");
        assert!(error
            .to_string()
            .contains("evidenceRefs must contain at least one item"));
    }

    #[test]
    fn graph_ready_requires_materialized_graph_status() {
        assert!(graph_status_is_ready(Some("rebuilt")));
        assert!(graph_status_is_ready(Some("partially_applied")));
        assert!(!graph_status_is_ready(Some("skipped")));
        assert!(!graph_status_is_ready(Some("empty")));
        assert!(!graph_status_is_ready(Some("failed")));
        assert!(!graph_status_is_ready(Some("failed_no_materialization")));
        assert!(!graph_status_is_ready(None));
    }

    #[test]
    fn import_job_status_strings_use_hyprduck_lifecycle_names() {
        assert_eq!(ImportJobStatus::Imported.as_str(), "imported");
        assert_eq!(ImportJobStatus::Parsing.as_str(), "parsing");
        assert_eq!(ImportJobStatus::Packaging.as_str(), "packaging");
        assert_eq!(ImportJobStatus::CitationReady.as_str(), "citation_ready");
        assert_eq!(
            ImportJobStatus::CitationReadyGraphPending.as_str(),
            "citation_ready_graph_pending"
        );
        assert_eq!(
            ImportJobStatus::CitationReadyGraphSkipped.as_str(),
            "citation_ready_graph_skipped"
        );
        assert_eq!(
            ImportJobStatus::GraphRetryWaiting.as_str(),
            "graph_retry_waiting"
        );
        assert_eq!(ImportJobStatus::ContextReady.as_str(), "context_ready");
        assert_eq!(ImportJobStatus::Failed.as_str(), "failed");
        assert_eq!(ImportJobStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn queued_import_job_serializes_as_imported_state() {
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: None,
        };
        let job = ImportJobSnapshot::queued("import-test".into(), &scope);
        let value = job.to_value();

        assert_eq!(value["status"], json!("imported"));
        assert_eq!(value["phase"], json!("imported"));
        assert_eq!(value["citationReady"], json!(false));
        assert_eq!(value["graphReady"], json!(false));
        assert_eq!(value["progressPercent"], json!(0));
    }

    #[test]
    fn import_job_terminal_states_are_context_ready_failed_or_cancelled() {
        assert!(!ImportJobStatus::Imported.is_terminal());
        assert!(!ImportJobStatus::Parsing.is_terminal());
        assert!(!ImportJobStatus::Packaging.is_terminal());
        assert!(!ImportJobStatus::CitationReady.is_terminal());
        assert!(ImportJobStatus::CitationReadyGraphPending.is_terminal());
        assert!(ImportJobStatus::CitationReadyGraphSkipped.is_terminal());
        assert!(!ImportJobStatus::GraphRetryWaiting.is_terminal());
        assert!(ImportJobStatus::ContextReady.is_terminal());
        assert!(ImportJobStatus::Failed.is_terminal());
        assert!(ImportJobStatus::Cancelled.is_terminal());
    }

    #[test]
    fn import_parse_progress_maps_to_lifecycle_states() {
        use hyprduck_engine_types::ParseProgress;

        assert_eq!(
            import_phase_from_parse_progress(&ParseProgress::Queued),
            (ImportJobPhase::Imported, 2)
        );
        assert_eq!(
            import_phase_from_parse_progress(&ParseProgress::Packaging),
            (ImportJobPhase::Packaging, 68)
        );
        assert_eq!(
            import_phase_from_parse_progress(&ParseProgress::Completed),
            (ImportJobPhase::Packaging, 70)
        );
    }

    #[test]
    fn citation_ready_snapshot_serializes_status_and_readiness() {
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: None,
        };
        let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
        job.status = ImportJobStatus::CitationReady;
        job.phase = ImportJobPhase::CitationReady;
        job.progress_percent = 82;
        job.source_id = Some("source-1".into());
        job.evidence_count = Some(3);
        job.citation_ready = true;

        let value = job.to_value();
        assert_eq!(value["status"], json!("citation_ready"));
        assert_eq!(value["phase"], json!("citation_ready"));
        assert_eq!(value["citationReady"], json!(true));
        assert_eq!(value["evidenceCount"], json!(3));
    }

    #[test]
    fn graph_pending_snapshot_serializes_retry_metadata() {
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: None,
        };
        let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
        job.status = ImportJobStatus::CitationReadyGraphPending;
        job.phase = ImportJobPhase::GraphPending;
        job.citation_ready = true;
        job.graph_ready = false;
        job.graph_status = Some("pending".into());
        job.graph_error_category = Some("db_locked".into());
        job.graph_generation_error_message = Some("database is locked".into());
        job.retryable = true;
        job.retry_attempt = 1;
        job.max_retry_attempts = 2;
        job.next_retry_at = Some(1234);
        job.manual_retry_available = true;

        let value = job.to_value();
        assert_eq!(value["status"], json!("citation_ready_graph_pending"));
        assert_eq!(value["phase"], json!("graph_pending"));
        assert_eq!(value["citationReady"], json!(true));
        assert_eq!(value["graphReady"], json!(false));
        assert_eq!(value["graphErrorCategory"], json!("db_locked"));
        assert_eq!(value["retryable"], json!(true));
        assert_eq!(value["retryAttempt"], json!(1));
        assert_eq!(value["maxRetryAttempts"], json!(2));
        assert_eq!(value["nextRetryAt"], json!(1234));
        assert_eq!(value["manualRetryAvailable"], json!(true));
    }

    #[test]
    fn graph_failure_classifier_keeps_permission_readonly_permanent() {
        let locked = classify_graph_failure("SQLite error: database is locked");
        assert_eq!(locked.category, "db_locked");
        assert!(locked.retryable);

        let readonly_permission = classify_graph_failure("attempt to write a readonly database");
        assert_eq!(readonly_permission.category, "db_readonly");
        assert!(!readonly_permission.retryable);

        let provider_timeout = classify_graph_failure("provider_timeout: request timed out");
        assert_eq!(provider_timeout.category, "provider_timeout");
        assert!(provider_timeout.retryable);
    }

    #[test]
    fn graph_error_sanitizer_redacts_local_paths() {
        let message =
            "failed to materialize /tmp/hyprduck/private/source.md\ncaused by more detail";
        let redacted = sanitize_graph_error_message(message);

        assert_eq!(redacted, "failed to materialize [redacted-local-path]");
        assert!(!redacted.contains("/tmp/hyprduck"));
        assert!(!redacted.contains("caused by"));
    }

    #[test]
    fn graph_status_persist_failure_adds_warning_without_local_paths() {
        let registry = ImportJobRegistry::default();
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: None,
        };
        let mut job = ImportJobSnapshot::queued("import-test".into(), &scope);
        job.status = ImportJobStatus::CitationReadyGraphPending;
        job.phase = ImportJobPhase::GraphPending;
        registry.insert(job);

        record_graph_status_persist_result(
            &registry,
            "import-test",
            Err(anyhow!(
                "failed writing /tmp/hyprduck/private/knowledge.sqlite3"
            )),
        );
        record_graph_status_persist_result(&registry, "import-test", Ok(false));

        let job = registry.get("import-test").expect("job");
        assert_eq!(job.warnings.len(), 2);
        assert!(job.warnings[0].starts_with("graph_status_persist_failed:"));
        assert!(!job.warnings[0].contains("/tmp/hyprduck"));
        assert_eq!(
            job.warnings[1],
            "graph_status_persist_failed: citation-ready import job was not found"
        );
    }

    #[test]
    fn import_job_cancel_prevents_later_active_updates() {
        let registry = ImportJobRegistry::default();
        let scope = BrainReadScope {
            workspace_id: "default".into(),
            root_dir: None,
        };
        let job_id = "import-test-cancel".to_string();
        registry.insert(ImportJobSnapshot::queued(job_id.clone(), &scope));

        let cancelled = registry.cancel(&job_id).expect("cancel job");
        assert_eq!(cancelled.status, ImportJobStatus::Cancelled);
        assert_eq!(cancelled.phase, ImportJobPhase::Cancelled);

        registry.update_active(&job_id, |job| {
            job.status = ImportJobStatus::Parsing;
            job.phase = ImportJobPhase::Parsing;
            job.progress_percent = 5;
        });
        let job = registry.get(&job_id).expect("job remains recorded");
        assert_eq!(job.status, ImportJobStatus::Cancelled);
        assert_eq!(job.phase, ImportJobPhase::Cancelled);
        assert_eq!(job.progress_percent, 100);
    }

    #[test]
    fn validate_import_source_path_accepts_file_inside_allowed_root() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");
        let source = allowed.path().join("source.md");
        std::fs::write(&source, "# Source\n").expect("source file");

        set_allowed_import_roots(&[allowed.path()]);
        let validated =
            validate_import_source_path(&source.display().to_string()).expect("valid source path");

        assert_eq!(validated, source.canonicalize().expect("canonical source"));
        clear_root_dir_env();
    }

    #[test]
    fn validate_import_source_path_rejects_file_outside_allowed_root() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");
        let outside = tempfile::tempdir().expect("outside dir");
        let source = outside.path().join("source.md");
        std::fs::write(&source, "# Source\n").expect("source file");

        set_allowed_import_roots(&[allowed.path()]);
        let error = validate_import_source_path(&source.display().to_string())
            .expect_err("outside source rejected");

        assert!(error
            .to_string()
            .contains("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    fn validate_import_source_path_rejects_directory() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");

        set_allowed_import_roots(&[allowed.path()]);
        let error = validate_import_source_path(&allowed.path().display().to_string())
            .expect_err("directory source rejected");

        assert!(error.to_string().contains("regular file"));
        clear_root_dir_env();
    }

    #[test]
    fn validate_import_source_path_rejects_file_as_allowed_root() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");
        let source = allowed.path().join("source.md");
        std::fs::write(&source, "# Source\n").expect("source file");

        set_allowed_import_roots(&[source.as_path()]);
        let error = validate_import_source_path(&source.display().to_string())
            .expect_err("file root rejected");

        assert!(error.to_string().contains("must be a directory"));
        clear_root_dir_env();
    }

    #[test]
    #[cfg(unix)]
    fn validate_import_source_path_rejects_symlink_escape() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&allowed).expect("allowed dir");
        std::fs::create_dir_all(&outside).expect("outside dir");
        let outside_file = outside.join("source.md");
        let symlink = allowed.join("linked.md");
        std::fs::write(&outside_file, "# Source\n").expect("outside source");
        std::os::unix::fs::symlink(&outside_file, &symlink).expect("symlink");

        set_allowed_import_roots(&[allowed.as_path()]);
        let error = validate_import_source_path(&symlink.display().to_string())
            .expect_err("symlink escape rejected");

        assert!(error
            .to_string()
            .contains("HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    fn import_document_format_infers_pdf() {
        assert_eq!(
            import_document_format(Path::new("source.pdf"), None).expect("pdf format"),
            DocumentFormat::Pdf
        );
    }

    #[test]
    fn import_document_format_infers_markdown() {
        assert_eq!(
            import_document_format(Path::new("source.md"), None).expect("markdown format"),
            DocumentFormat::Markdown
        );
        assert_eq!(
            import_document_format(Path::new("source.markdown"), None).expect("markdown format"),
            DocumentFormat::Markdown
        );
    }

    #[test]
    fn import_document_format_infers_office_and_image_formats() {
        assert_eq!(
            import_document_format(Path::new("source.docx"), None).expect("docx format"),
            DocumentFormat::Docx
        );
        assert_eq!(
            import_document_format(Path::new("source.doc"), None).expect("doc format"),
            DocumentFormat::Doc
        );
        assert_eq!(
            import_document_format(Path::new("source.png"), None).expect("image format"),
            DocumentFormat::Image
        );
    }

    #[test]
    fn import_document_format_uses_explicit_format() {
        assert_eq!(
            import_document_format(Path::new("source.txt"), Some("IMAGE".into()))
                .expect("explicit image format"),
            DocumentFormat::Image
        );
    }

    #[test]
    fn import_document_format_rejects_unknown_extension() {
        let error = import_document_format(Path::new("source.txt"), None)
            .expect_err("unknown extension rejected");
        assert!(error.to_string().contains("unsupported import format"));
    }

    #[test]
    fn read_scope_rejects_root_dir_without_dev_env() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let mut arguments = Map::new();
        arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

        let error = read_scope(&arguments).expect_err("rootDir should be disabled by default");
        assert!(error.to_string().contains("rootDir is disabled"));
        clear_root_dir_env();
    }

    #[test]
    fn read_scope_rejects_root_dir_when_dev_env_is_not_one() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let mut arguments = Map::new();
        arguments.insert("rootDir".into(), Value::String("/tmp/hyprduck-test".into()));

        std::env::set_var(ROOT_DIR_ENV, "0");
        let zero_error = read_scope(&arguments).expect_err("rootDir=0 should stay disabled");
        assert!(zero_error.to_string().contains("rootDir is disabled"));

        std::env::set_var(ROOT_DIR_ENV, "");
        let empty_error =
            read_scope(&arguments).expect_err("empty rootDir env should stay disabled");
        assert!(empty_error.to_string().contains("rootDir is disabled"));

        clear_root_dir_env();
    }

    #[test]
    fn read_scope_rejects_root_dir_without_allowed_roots() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let mut arguments = Map::new();
        arguments.insert(
            "rootDir".into(),
            Value::String(temp.path().display().to_string()),
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        let error = read_scope(&arguments).expect_err("allowlist should be required");
        assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    fn read_scope_accepts_allowlisted_root_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let mut arguments = Map::new();
        arguments.insert(
            "rootDir".into(),
            Value::String(temp.path().display().to_string()),
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[temp.path()]);
        let scope = read_scope(&arguments).expect("allowlisted rootDir");
        let expected_root_dir = canonical_path_string(temp.path());
        assert_eq!(scope.root_dir.as_deref(), Some(expected_root_dir.as_str()));
        clear_root_dir_env();
    }

    #[test]
    #[cfg(unix)]
    fn read_scope_stores_canonical_root_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let actual = temp.path().join("actual");
        let symlink = temp.path().join("linked-root");
        std::fs::create_dir_all(&actual).expect("actual dir");
        std::os::unix::fs::symlink(&actual, &symlink).expect("symlink");
        let mut arguments = Map::new();
        arguments.insert(
            "rootDir".into(),
            Value::String(symlink.display().to_string()),
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[actual.as_path()]);
        let scope = read_scope(&arguments).expect("allowlisted symlink rootDir");
        let expected_root_dir = canonical_path_string(actual.as_path());
        assert_eq!(scope.root_dir.as_deref(), Some(expected_root_dir.as_str()));
        clear_root_dir_env();
    }

    #[test]
    fn read_scope_rejects_root_dir_outside_allowed_roots() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");
        let outside = tempfile::tempdir().expect("outside dir");
        let mut arguments = Map::new();
        arguments.insert(
            "rootDir".into(),
            Value::String(outside.path().display().to_string()),
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[allowed.path()]);
        let error = read_scope(&arguments).expect_err("outside rootDir rejected");
        assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    #[cfg(unix)]
    fn read_scope_rejects_symlinked_root_dir_outside_allowed_roots() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        let symlink = temp.path().join("linked-root");
        std::fs::create_dir_all(&allowed).expect("allowed dir");
        std::fs::create_dir_all(&outside).expect("outside dir");
        std::os::unix::fs::symlink(&outside, &symlink).expect("symlink");
        let mut arguments = Map::new();
        arguments.insert(
            "rootDir".into(),
            Value::String(symlink.display().to_string()),
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[allowed.as_path()]);
        let error = read_scope(&arguments).expect_err("symlink escape rejected");
        assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    fn resource_uri_rejects_root_dir_without_dev_env() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();

        let error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("resource rootDir should be disabled by default");
        assert!(error.to_string().contains("rootDir is disabled"));
        clear_root_dir_env();
    }

    #[test]
    fn resource_uri_rejects_root_dir_when_dev_env_is_not_one() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        std::env::set_var(ROOT_DIR_ENV, "0");
        let zero_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("rootDir=0 should stay disabled for resources");
        assert!(zero_error.to_string().contains("rootDir is disabled"));

        std::env::set_var(ROOT_DIR_ENV, "");
        let empty_error = parse_resource_uri("hyprduck://brain/default/wiki/index.md?rootDir=/tmp")
            .expect_err("empty rootDir env should stay disabled for resources");
        assert!(empty_error.to_string().contains("rootDir is disabled"));

        clear_root_dir_env();
    }

    #[test]
    fn redacts_local_paths_embedded_in_markdown_text() {
        let text = "Plain /Users/hippoo/file.md, link [doc](/Users/hippoo/doc.pdf), code `/tmp/raw.txt`, file URL file:///Users/hippoo/source.pdf and windows C:\\Users\\hippoo\\note.txt";
        let redacted = redact_local_path_text(text);

        assert!(!redacted.contains("/Users/hippoo"));
        assert!(!redacted.contains("/tmp/raw.txt"));
        assert!(!redacted.contains("file:///"));
        assert!(!redacted.contains("C:\\Users\\hippoo"));
        assert_eq!(redacted.matches("[redacted-local-path]").count(), 5);
        assert!(redacted.contains("[doc]([redacted-local-path])"));
        assert!(redacted.contains("`[redacted-local-path]`"));
        assert_eq!(
            redact_local_path_text("relative state/latest-readable-snapshot.json stays"),
            "relative state/latest-readable-snapshot.json stays"
        );
    }

    #[test]
    fn include_local_paths_requires_server_opt_in_and_supported_tool() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();

        let mut arguments = Map::new();
        arguments.insert("includeLocalPaths".into(), Value::Bool(true));
        let error = local_path_disclosure_for_tool("read_source", &arguments)
            .expect_err("server opt-in required");
        assert!(error
            .to_string()
            .contains("HYPRDUCK_MCP_ALLOW_LOCAL_PATHS=1"));

        std::env::set_var(LOCAL_PATH_DISCLOSURE_ENV, "1");
        let unsupported = local_path_disclosure_for_tool("search_documents", &arguments)
            .expect_err("unsupported tool rejects local path disclosure");
        assert!(unsupported
            .to_string()
            .contains("includeLocalPaths is not supported"));

        assert!(local_path_disclosure_for_tool("read_source", &arguments)
            .expect("supported tool can disclose paths when server opted in"));

        clear_root_dir_env();
    }

    #[test]
    fn resource_uri_accepts_allowlisted_root_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let uri = format!(
            "hyprduck://brain/default/wiki/index.md?rootDir={}",
            temp.path().display()
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[temp.path()]);
        let resource = parse_resource_uri(&uri).expect("allowlisted resource rootDir");
        let expected_root_dir = canonical_path_string(temp.path());
        assert_eq!(
            resource.scope.root_dir.as_deref(),
            Some(expected_root_dir.as_str())
        );
        clear_root_dir_env();
    }

    #[test]
    fn resource_uri_rejects_root_dir_outside_allowed_roots() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let allowed = tempfile::tempdir().expect("allowed dir");
        let outside = tempfile::tempdir().expect("outside dir");
        let uri = format!(
            "hyprduck://brain/default/wiki/index.md?rootDir={}",
            outside.path().display()
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[allowed.path()]);
        let error = parse_resource_uri(&uri).expect_err("outside resource rootDir rejected");
        assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    #[cfg(unix)]
    fn resource_uri_rejects_symlinked_root_dir_outside_allowed_roots() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        let symlink = temp.path().join("linked-root");
        std::fs::create_dir_all(&allowed).expect("allowed dir");
        std::fs::create_dir_all(&outside).expect("outside dir");
        std::os::unix::fs::symlink(&outside, &symlink).expect("symlink");
        let uri = format!(
            "hyprduck://brain/default/wiki/index.md?rootDir={}",
            symlink.display()
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[allowed.as_path()]);
        let error = parse_resource_uri(&uri).expect_err("symlink escape resource rootDir rejected");
        assert!(error.to_string().contains("HYPRDUCK_MCP_ALLOWED_ROOTS"));
        clear_root_dir_env();
    }

    #[test]
    #[cfg(unix)]
    fn resource_uri_stores_canonical_root_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        clear_root_dir_env();
        let temp = tempfile::tempdir().expect("temp dir");
        let actual = temp.path().join("actual");
        let symlink = temp.path().join("linked-root");
        std::fs::create_dir_all(&actual).expect("actual dir");
        std::os::unix::fs::symlink(&actual, &symlink).expect("symlink");
        let uri = format!(
            "hyprduck://brain/default/wiki/index.md?rootDir={}",
            symlink.display()
        );

        std::env::set_var(ROOT_DIR_ENV, "1");
        set_allowed_roots(&[actual.as_path()]);
        let resource = parse_resource_uri(&uri).expect("allowlisted resource rootDir");
        let expected_root_dir = canonical_path_string(actual.as_path());
        assert_eq!(
            resource.scope.root_dir.as_deref(),
            Some(expected_root_dir.as_str())
        );
        clear_root_dir_env();
    }
}
