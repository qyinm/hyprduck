use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use hyprduck_engine_client::{EngineClient, SubprocessEngineClient};
use hyprduck_engine_types::{
    BrainReadScope, CompileProjectRequest, DocumentFormat, GetBrainHealthRequest,
    GetContextPackRequest, ParseInput, ParseOptions, ParseOutputTarget, ParseRequest,
    ReadContextPackRequest, ReadGraphHistoryRequest, ReadGraphSnapshotRequest, ReadNodeRequest,
    ReadPageEvidenceRequest, ReadRecentEventsRequest, ReadSourceRequest, ReadWikiPageRequest,
    SearchBrainRequest, WriteCommitAllRequest, WriteCommitRequest, WriteListRequest,
    WriteProposeRequest, WriteRejectRequest,
};
use serde_json::{json, Map, Value};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const ROOT_DIR_ENV: &str = "HYPRDUCK_MCP_ALLOW_ROOT_DIR";
const ROOT_DIR_ALLOWED_ROOTS_ENV: &str = "HYPRDUCK_MCP_ALLOWED_ROOTS";
const IMPORT_ALLOWED_ROOTS_ENV: &str = "HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS";
const PROPOSAL_ID_PATTERN: &str = "^prop-[0-9A-Fa-f]{32}$";
const WRITE_CONTENT_TYPES: [&str; 3] = ["memory", "evidence_refresh", "link_repair"];

pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let client = SubprocessEngineClient::default();
    let state = McpServerState::default();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&client, &state, &line) {
            serde_json::to_writer(&mut stdout, &response)
                .context("failed to encode MCP response")?;
            stdout
                .write_all(b"\n")
                .context("failed to write MCP response newline")?;
            stdout.flush().context("failed to flush MCP response")?;
        }
    }

    Ok(())
}

#[derive(Clone, Default)]
struct McpServerState {
    import_jobs: ImportJobRegistry,
}

fn handle_message(client: &dyn EngineClient, state: &McpServerState, line: &str) -> Option<Value> {
    let message = match serde_json::from_str::<Value>(line) {
        Ok(message) => message,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("Parse error: {error}"),
            ))
        }
    };

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(error_response(
            message.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "Invalid Request: missing method",
        ));
    };

    let Some(id) = message.get("id").cloned() else {
        return handle_notification(method);
    };

    match method {
        "initialize" => Some(success_response(id, initialize_result(&message))),
        "ping" => Some(success_response(id, json!({}))),
        "tools/list" => Some(success_response(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => Some(handle_tool_call(client, state, id, message.get("params"))),
        "resources/list" => Some(success_response(
            id,
            json!({ "resources": resource_definitions() }),
        )),
        "resources/read" => Some(handle_resource_read(client, id, message.get("params"))),
        _ => Some(error_response(
            id,
            -32601,
            format!("Method not found: {method}"),
        )),
    }
}

fn handle_notification(method: &str) -> Option<Value> {
    match method {
        "notifications/initialized" | "notifications/cancelled" => None,
        _ => None,
    }
}

fn initialize_result(message: &Value) -> Value {
    let requested_protocol = message
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);

    json!({
        "protocolVersion": requested_protocol,
        "capabilities": {
            "tools": {
                "listChanged": false
            },
            "resources": {
                "subscribe": false,
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "hyprduck",
            "title": "HyprDuck Local Context",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "HyprDuck exposes local, desktop-first, evidence-governed document context through read, search, context-pack, snapshot, and health tools. Use get_context_pack first, then search_documents or open cited sources, page evidence, wiki pages, nodes, or event history as needed."
    })
}

fn handle_tool_call(
    client: &dyn EngineClient,
    state: &McpServerState,
    id: Value,
    params: Option<&Value>,
) -> Value {
    let params = params.unwrap_or(&Value::Null);
    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => return error_response(id, -32602, "Invalid params: missing tool name"),
    };
    let arguments = match params.get("arguments") {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return error_response(id, -32602, "Invalid params: arguments must be an object")
        }
        None => return error_response(id, -32602, "Invalid params: missing arguments"),
    };
    let include_local_paths = match optional_bool(arguments, "includeLocalPaths") {
        Ok(value) => value.unwrap_or(false),
        Err(error) => {
            return success_response(
                id,
                json!({
                    "content": [
                        {
                            "type": "text",
                            "text": error.to_string()
                        }
                    ],
                    "isError": true
                }),
            )
        }
    };

    let result = match call_tool(client, state, name, arguments) {
        Ok(tool_result) => {
            let value = if include_local_paths {
                tool_result.value
            } else {
                redact_local_paths(tool_result.value)
            };
            let mut result = json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string_pretty(&value)
                            .unwrap_or_else(|_| "{}".into())
                    }
                ],
                "isError": false
            });
            if let Some(cache_state) = tool_result.cache_state {
                result["_meta"] = json!({
                    "hyprduckGraphWikiCache": cache_state
                });
            }
            result
        }
        Err(error) => json!({
            "content": [
                {
                    "type": "text",
                    "text": error.to_string()
                }
            ],
            "isError": true
        }),
    };

    success_response(id, result)
}

fn handle_resource_read(client: &dyn EngineClient, id: Value, params: Option<&Value>) -> Value {
    match read_resource(client, params) {
        Ok(value) => success_response(id, value),
        Err(error) => error_response(id, -32602, error.to_string()),
    }
}

fn read_resource(client: &dyn EngineClient, params: Option<&Value>) -> Result<Value> {
    let params = params.unwrap_or(&Value::Null);
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| !uri.trim().is_empty())
        .ok_or_else(|| anyhow!("Invalid params: missing resource uri"))?;
    let resource = parse_resource_uri(uri)?;

    match resource.kind {
        BrainResourceKind::GraphSnapshot => {
            let snapshot = client.read_graph_snapshot(ReadGraphSnapshotRequest {
                scope: resource.scope,
                include_local_paths: false,
            })?;
            let snapshot = redact_local_paths(serde_json::to_value(snapshot)?);
            Ok(json!({
                "contents": [
                    {
                        "uri": public_resource_uri(uri),
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&snapshot)?
                    }
                ]
            }))
        }
        BrainResourceKind::WikiPage { path } => {
            let page = client.read_wiki_page(ReadWikiPageRequest {
                scope: resource.scope,
                path,
            })?;
            let body = redact_local_path_text(&page.page.body);
            Ok(json!({
                "contents": [
                    {
                        "uri": public_resource_uri(uri),
                        "mimeType": "text/markdown",
                        "text": body
                    }
                ]
            }))
        }
    }
}

fn public_resource_uri(uri: &str) -> &str {
    uri.split_once('?').map_or(uri, |(path, _)| path)
}

#[derive(Debug)]
struct BrainResource {
    scope: BrainReadScope,
    kind: BrainResourceKind,
}

#[derive(Debug)]
enum BrainResourceKind {
    GraphSnapshot,
    WikiPage { path: String },
}

fn parse_resource_uri(uri: &str) -> Result<BrainResource> {
    let Some(rest) = uri.strip_prefix("hyprduck://brain/") else {
        return Err(anyhow!("unsupported HyprDuck resource uri: {uri}"));
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (workspace_id, resource_path) = path
        .split_once('/')
        .ok_or_else(|| anyhow!("HyprDuck resource uri must include workspace and resource path"))?;
    if workspace_id.trim().is_empty() {
        return Err(anyhow!("HyprDuck resource uri workspace cannot be empty"));
    }
    let query = parse_resource_query(query)?;
    let root_dir = query
        .get("rootDir")
        .and_then(Value::as_str)
        .map(validate_root_dir_argument)
        .transpose()?;
    let scope = BrainReadScope {
        workspace_id: percent_decode(workspace_id)?,
        root_dir,
    };
    let kind = if resource_path == "graph/snapshot" {
        BrainResourceKind::GraphSnapshot
    } else if let Some(path) = resource_path.strip_prefix("wiki/") {
        BrainResourceKind::WikiPage {
            path: format!("wiki/{}", percent_decode(path)?),
        }
    } else {
        return Err(anyhow!(
            "unsupported HyprDuck resource path: {resource_path}"
        ));
    };
    Ok(BrainResource { scope, kind })
}

fn parse_resource_query(query: &str) -> Result<Map<String, Value>> {
    let mut values = Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key)?, Value::String(percent_decode(value)?));
    }
    Ok(values)
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .context("resource uri contains invalid percent encoding")?;
                let byte = u8::from_str_radix(hex, 16)
                    .context("resource uri contains invalid percent encoding")?;
                decoded.push(byte);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).context("resource uri contains invalid utf-8")
}

#[derive(Clone, Default)]
struct ImportJobRegistry {
    jobs: Arc<Mutex<BTreeMap<String, ImportJobSnapshot>>>,
}

#[derive(Debug, Clone)]
struct ImportJobSnapshot {
    job_id: String,
    workspace_id: String,
    root_dir: Option<String>,
    status: ImportJobStatus,
    phase: ImportJobPhase,
    progress_percent: u8,
    source_id: Option<String>,
    page_count: Option<usize>,
    evidence_count: Option<usize>,
    citation_ready: bool,
    graph_ready: bool,
    graph_status: Option<String>,
    graph_generation_skipped_reason: Option<String>,
    graph_generation_error_message: Option<String>,
    warnings: Vec<String>,
    error: Option<String>,
    cancel_requested: bool,
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportJobStatus {
    Imported,
    Parsing,
    Packaging,
    CitationReady,
    ContextReady,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportJobPhase {
    Imported,
    Parsing,
    Packaging,
    CitationReady,
    ContextMaterializing,
    ContextReady,
    Failed,
    Cancelled,
}

impl ImportJobRegistry {
    fn insert(&self, job: ImportJobSnapshot) {
        self.jobs
            .lock()
            .expect("import job registry lock poisoned")
            .insert(job.job_id.clone(), job);
    }

    fn get(&self, job_id: &str) -> Option<ImportJobSnapshot> {
        self.jobs
            .lock()
            .expect("import job registry lock poisoned")
            .get(job_id)
            .cloned()
    }

    fn update<F>(&self, job_id: &str, apply: F)
    where
        F: FnOnce(&mut ImportJobSnapshot),
    {
        if let Some(job) = self
            .jobs
            .lock()
            .expect("import job registry lock poisoned")
            .get_mut(job_id)
        {
            apply(job);
            job.updated_at = unix_timestamp_seconds();
        }
    }

    fn update_active<F>(&self, job_id: &str, apply: F)
    where
        F: FnOnce(&mut ImportJobSnapshot),
    {
        self.update(job_id, |job| {
            if job.status.is_terminal() {
                return;
            }
            if job.cancel_requested {
                job.status = ImportJobStatus::Cancelled;
                job.phase = ImportJobPhase::Cancelled;
                job.progress_percent = 100;
                return;
            }
            apply(job);
        });
    }

    fn cancel(&self, job_id: &str) -> Result<ImportJobSnapshot> {
        let mut jobs = self.jobs.lock().expect("import job registry lock poisoned");
        let job = jobs
            .get_mut(job_id)
            .ok_or_else(|| anyhow!("import job not found: {job_id}"))?;
        if !job.status.is_terminal() {
            job.cancel_requested = true;
            job.warnings.push(
                "cancel_requested; running engine steps may finish before cancellation".into(),
            );
            if matches!(job.phase, ImportJobPhase::Imported) {
                job.status = ImportJobStatus::Cancelled;
                job.phase = ImportJobPhase::Cancelled;
                job.progress_percent = 100;
            }
            job.updated_at = unix_timestamp_seconds();
        }
        Ok(job.clone())
    }

    fn mark_cancelled_if_requested(&self, job_id: &str) -> bool {
        let mut jobs = self.jobs.lock().expect("import job registry lock poisoned");
        let Some(job) = jobs.get_mut(job_id) else {
            return false;
        };
        if !job.cancel_requested {
            return false;
        }
        if !job.status.is_terminal() {
            job.status = ImportJobStatus::Cancelled;
            job.phase = ImportJobPhase::Cancelled;
            job.progress_percent = 100;
            job.updated_at = unix_timestamp_seconds();
        }
        true
    }
}

impl ImportJobSnapshot {
    fn queued(job_id: String, scope: &BrainReadScope) -> Self {
        let now = unix_timestamp_seconds();
        Self {
            job_id,
            workspace_id: scope.workspace_id.clone(),
            root_dir: scope.root_dir.clone(),
            status: ImportJobStatus::Imported,
            phase: ImportJobPhase::Imported,
            progress_percent: 0,
            source_id: None,
            page_count: None,
            evidence_count: None,
            citation_ready: false,
            graph_ready: false,
            graph_status: None,
            graph_generation_skipped_reason: None,
            graph_generation_error_message: None,
            warnings: Vec::new(),
            error: None,
            cancel_requested: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "jobId": self.job_id,
            "workspaceId": self.workspace_id,
            "status": self.status.as_str(),
            "phase": self.phase.as_str(),
            "progressPercent": self.progress_percent,
            "sourceId": self.source_id,
            "pageCount": self.page_count,
            "evidenceCount": self.evidence_count,
            "citationReady": self.citation_ready,
            "graphReady": self.graph_ready,
            "graphStatus": self.graph_status,
            "graphGenerationSkippedReason": self.graph_generation_skipped_reason,
            "graphGenerationErrorMessage": self.graph_generation_error_message,
            "warnings": self.warnings,
            "error": self.error,
            "cancelRequested": self.cancel_requested,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

impl ImportJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Parsing => "parsing",
            Self::Packaging => "packaging",
            Self::CitationReady => "citation_ready",
            Self::ContextReady => "context_ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::ContextReady | Self::Failed | Self::Cancelled)
    }
}

impl ImportJobPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Parsing => "parsing",
            Self::Packaging => "packaging",
            Self::CitationReady => "citation_ready",
            Self::ContextMaterializing => "context_materializing",
            Self::ContextReady => "context_ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

struct ImportJobRequest {
    job_id: String,
    scope: BrainReadScope,
    source_path: PathBuf,
    format: DocumentFormat,
    name: Option<String>,
    skip_graph_generation: bool,
}

fn next_import_job_id() -> String {
    format!("import-{}-{}", std::process::id(), unix_timestamp_millis())
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn spawn_import_job(registry: ImportJobRegistry, request: ImportJobRequest) {
    thread::spawn(move || {
        if let Err(error) = run_import_job(&registry, &request) {
            registry.update(&request.job_id, |job| {
                if !matches!(job.status, ImportJobStatus::Cancelled) {
                    job.status = ImportJobStatus::Failed;
                    job.phase = ImportJobPhase::Failed;
                    job.progress_percent = 100;
                    job.error = Some(error.to_string());
                }
            });
        }
    });
}

fn run_import_job(registry: &ImportJobRegistry, request: &ImportJobRequest) -> Result<()> {
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let client = SubprocessEngineClient::default();
    registry.update_active(&request.job_id, |job| {
        job.status = ImportJobStatus::Parsing;
        job.phase = ImportJobPhase::Parsing;
        job.progress_percent = 5;
    });

    let parse = client.parse(
        ParseRequest {
            version: "1".into(),
            input: ParseInput {
                path: request.source_path.display().to_string(),
                format: request.format.clone(),
            },
            template: "General".into(),
            options: ParseOptions::default(),
            output: Some(ParseOutputTarget {
                root_dir: request.scope.root_dir.clone(),
                name: request.name.clone(),
                workspace_id: Some(request.scope.workspace_id.clone()),
                source_id: None,
            }),
        },
        &mut |progress| {
            let (phase, progress_percent) = import_phase_from_parse_progress(&progress);
            registry.update_active(&request.job_id, |job| {
                job.status = match phase {
                    ImportJobPhase::Imported => ImportJobStatus::Imported,
                    ImportJobPhase::Parsing => ImportJobStatus::Parsing,
                    ImportJobPhase::Packaging => ImportJobStatus::Packaging,
                    ImportJobPhase::Failed => ImportJobStatus::Failed,
                    ImportJobPhase::CitationReady
                    | ImportJobPhase::ContextMaterializing
                    | ImportJobPhase::ContextReady
                    | ImportJobPhase::Cancelled => job.status,
                };
                job.phase = phase;
                job.progress_percent = progress_percent;
            });
        },
    )?;
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let manifest = parse
        .source_manifest
        .ok_or_else(|| anyhow!("import_source parse did not produce a source manifest"))?;
    registry.update_active(&request.job_id, |job| {
        job.status = ImportJobStatus::Packaging;
        job.phase = ImportJobPhase::Packaging;
        job.progress_percent = 70;
        job.source_id = Some(manifest.source_id.clone());
        job.page_count = Some(parse.result.metadata.page_count);
    });

    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let compile = client.compile_project(CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(request.scope.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: Some(true),
    })?;
    let page_evidence = client.read_page_evidence(ReadPageEvidenceRequest {
        scope: request.scope.clone(),
        source_id: compile.source_id.clone(),
        page: None,
        include_local_paths: false,
    })?;
    let evidence_count = page_evidence.evidence.len();
    registry.update_active(&request.job_id, |job| {
        job.status = ImportJobStatus::CitationReady;
        job.phase = ImportJobPhase::CitationReady;
        job.progress_percent = if request.skip_graph_generation {
            100
        } else {
            82
        };
        job.source_id = Some(compile.source_id.clone());
        job.evidence_count = Some(evidence_count);
        job.citation_ready = evidence_count > 0;
        if request.skip_graph_generation {
            job.status = ImportJobStatus::ContextReady;
            job.phase = ImportJobPhase::ContextReady;
            job.graph_ready = false;
            job.graph_status = Some("skipped".into());
            job.graph_generation_skipped_reason = Some("skipGraphGeneration requested".into());
        }
    });
    if registry.mark_cancelled_if_requested(&request.job_id) || request.skip_graph_generation {
        return Ok(());
    }

    registry.update_active(&request.job_id, |job| {
        job.status = ImportJobStatus::CitationReady;
        job.phase = ImportJobPhase::ContextMaterializing;
        job.progress_percent = 88;
    });
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let graph_compile = client.compile_project(CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(request.scope.workspace_id.clone()),
        source_id: Some(manifest.source_id.clone()),
        skip_graph_generation: Some(false),
    })?;
    if registry.mark_cancelled_if_requested(&request.job_id) {
        return Ok(());
    }

    let graph_status = graph_compile.graph_generation_status.clone();
    let graph_ready = graph_status_is_ready(graph_status.as_deref());
    registry.update_active(&request.job_id, |job| {
        job.status = ImportJobStatus::ContextReady;
        job.phase = ImportJobPhase::ContextReady;
        job.progress_percent = 100;
        job.graph_ready = graph_ready;
        job.graph_status = graph_status.or_else(|| Some("unknown".into()));
        job.graph_generation_skipped_reason = graph_compile.graph_generation_skipped_reason;
        job.graph_generation_error_message = graph_compile.graph_generation_error_message;
    });
    Ok(())
}

fn graph_status_is_ready(status: Option<&str>) -> bool {
    matches!(status, Some("rebuilt" | "partially_applied"))
}

fn import_phase_from_parse_progress(
    progress: &hyprduck_engine_types::ParseProgress,
) -> (ImportJobPhase, u8) {
    use hyprduck_engine_types::ParseProgress;

    match progress {
        ParseProgress::Queued => (ImportJobPhase::Imported, 2),
        ParseProgress::ConvertingPages { current, total } => (
            ImportJobPhase::Parsing,
            scaled_progress(*current as usize, *total as usize, 10, 35),
        ),
        ParseProgress::Parsing { current, total } => (
            ImportJobPhase::Parsing,
            scaled_progress(*current as usize, *total as usize, 35, 65),
        ),
        ParseProgress::Packaging => (ImportJobPhase::Packaging, 68),
        ParseProgress::Completed => (ImportJobPhase::Packaging, 70),
        ParseProgress::Failed { .. } => (ImportJobPhase::Failed, 100),
    }
}

fn scaled_progress(current: usize, total: usize, start: u8, end: u8) -> u8 {
    if total == 0 {
        return start;
    }
    let span = end.saturating_sub(start) as usize;
    let bounded_current = current.min(total);
    (start as usize + (span * bounded_current / total)) as u8
}

fn ensure_import_job_scope(job: &ImportJobSnapshot, scope: &BrainReadScope) -> Result<()> {
    if job.workspace_id != scope.workspace_id || job.root_dir != scope.root_dir {
        return Err(anyhow!("import job not found in requested workspace scope"));
    }
    Ok(())
}

fn call_tool(
    client: &dyn EngineClient,
    state: &McpServerState,
    name: &str,
    arguments: &Map<String, Value>,
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
            let job_id = required_string(arguments, "jobId")?;
            let job = state
                .import_jobs
                .get(&job_id)
                .ok_or_else(|| anyhow!("import job not found: {job_id}"))?;
            ensure_import_job_scope(&job, &scope)?;
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
            let include_local_paths =
                optional_bool(arguments, "includeLocalPaths")?.unwrap_or(false);
            serde_json::to_value(client.read_source(ReadSourceRequest {
                scope,
                source_id,
                include_local_paths,
            })?)?
        }
        "read_page_evidence" => {
            let source_id = required_string(arguments, "sourceId")?;
            let page = optional_usize(arguments, "page")?;
            let include_local_paths =
                optional_bool(arguments, "includeLocalPaths")?.unwrap_or(false);
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
                include_local_paths: false,
            })?)?
        }
        "read_health" => {
            serde_json::to_value(client.get_brain_health(GetBrainHealthRequest { scope })?)?
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
    matches!(name, "read_health")
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

fn read_scope(arguments: &Map<String, Value>) -> Result<BrainReadScope> {
    let root_dir = optional_string(arguments, "rootDir")?;
    let root_dir = root_dir
        .as_deref()
        .map(validate_root_dir_argument)
        .transpose()?;
    Ok(BrainReadScope {
        workspace_id: optional_string(arguments, "workspaceId")?
            .unwrap_or_else(|| "default".into()),
        root_dir,
    })
}

fn validate_root_dir_argument(root_dir: &str) -> Result<String> {
    if !root_dir_argument_allowed() {
        return Err(anyhow!(
            "rootDir is disabled by default; set HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 and HYPRDUCK_MCP_ALLOWED_ROOTS for development roots"
        ));
    }
    let canonical_root_dir = canonicalize_mcp_root(root_dir)?;
    let allowed_roots = allowed_root_dirs()?;
    if allowed_roots
        .iter()
        .any(|allowed_root| canonical_root_dir.starts_with(allowed_root))
    {
        return canonical_root_dir
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow!("rootDir must be valid UTF-8 after canonicalization"));
    }
    Err(anyhow!("rootDir is not in HYPRDUCK_MCP_ALLOWED_ROOTS"))
}

fn root_dir_argument_allowed() -> bool {
    std::env::var(ROOT_DIR_ENV).is_ok_and(|value| value == "1")
}

fn allowed_root_dirs() -> Result<Vec<PathBuf>> {
    let raw = std::env::var_os(ROOT_DIR_ALLOWED_ROOTS_ENV).ok_or_else(|| {
        anyhow!("rootDir requires HYPRDUCK_MCP_ALLOWED_ROOTS to name approved roots")
    })?;
    let roots = std::env::split_paths(&raw)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| canonicalize_mcp_root(path))
        .collect::<Result<Vec<_>>>()?;
    if roots.is_empty() {
        return Err(anyhow!(
            "rootDir requires HYPRDUCK_MCP_ALLOWED_ROOTS to name approved roots"
        ));
    }
    Ok(roots)
}

fn validate_import_source_path(raw_path: &str) -> Result<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("argument sourcePath cannot be empty"));
    }

    let source = PathBuf::from(trimmed)
        .canonicalize()
        .with_context(|| "sourcePath does not exist or cannot be read")?;
    if !source.is_file() {
        return Err(anyhow!("sourcePath must point to a regular file"));
    }

    let roots = allowed_import_root_dirs()?;
    if roots.iter().any(|root| source.starts_with(root)) {
        Ok(source)
    } else {
        Err(anyhow!(
            "sourcePath is outside HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS"
        ))
    }
}

fn allowed_import_root_dirs() -> Result<Vec<PathBuf>> {
    let raw = std::env::var_os(IMPORT_ALLOWED_ROOTS_ENV).ok_or_else(|| {
        anyhow!("MCP import is disabled: set HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS to one or more approved roots")
    })?;
    let roots = std::env::split_paths(&raw)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| {
            let root = path
                .canonicalize()
                .with_context(|| "allowed import root does not exist or cannot be read")?;
            if !root.is_dir() {
                return Err(anyhow!("allowed import root must be a directory"));
            }
            Ok(root)
        })
        .collect::<Result<Vec<_>>>()?;
    if roots.is_empty() {
        return Err(anyhow!(
            "MCP import is disabled: no allowed import roots configured"
        ));
    }
    Ok(roots)
}

fn import_document_format(path: &Path, explicit: Option<String>) -> Result<DocumentFormat> {
    let raw = explicit
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
        });
    match raw.as_deref() {
        Some("pdf") => Ok(DocumentFormat::Pdf),
        Some("docx") => Ok(DocumentFormat::Docx),
        Some("doc") => Ok(DocumentFormat::Doc),
        Some("md") | Some("markdown") => Ok(DocumentFormat::Markdown),
        Some("image") | Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("heic")
        | Some("tiff") => Ok(DocumentFormat::Image),
        Some(other) => Err(anyhow!(
            "unsupported import format: {other}; supported formats: pdf, docx, doc, markdown, image"
        )),
        None => Err(anyhow!(
            "cannot infer import format; pass format as pdf, docx, doc, markdown, or image"
        )),
    }
}

fn canonicalize_mcp_root(path: impl AsRef<Path>) -> Result<PathBuf> {
    path.as_ref()
        .canonicalize()
        .map_err(|_| anyhow!("rootDir must exist and be canonicalizable"))
}

fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String> {
    optional_string(arguments, name)?.ok_or_else(|| anyhow!("missing required argument: {name}"))
}

fn optional_string(arguments: &Map<String, Value>, name: &str) -> Result<Option<String>> {
    match arguments.get(name) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(Value::String(_)) => Err(anyhow!("argument {name} cannot be empty")),
        Some(_) => Err(anyhow!("argument {name} must be a string")),
        None => Ok(None),
    }
}

fn optional_usize(arguments: &Map<String, Value>, name: &str) -> Result<Option<usize>> {
    match arguments.get(name) {
        Some(Value::Number(value)) => value
            .as_u64()
            .map(|value| Some(value as usize))
            .ok_or_else(|| anyhow!("argument {name} must be a positive integer")),
        Some(Value::String(value)) => value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| anyhow!("argument {name} must be a positive integer")),
        Some(_) => Err(anyhow!("argument {name} must be a positive integer")),
        None => Ok(None),
    }
}

fn optional_bool(arguments: &Map<String, Value>, name: &str) -> Result<Option<bool>> {
    match arguments.get(name) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(anyhow!("argument {name} must be a boolean")),
        None => Ok(None),
    }
}

fn required_string_array(arguments: &Map<String, Value>, name: &str) -> Result<Vec<String>> {
    let values: Vec<String> = match arguments.get(name) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(s) if !s.trim().is_empty() => Ok(s.clone()),
                Value::String(_) => Err(anyhow!("element in {name} cannot be empty")),
                _ => Err(anyhow!("each element in {name} must be a string")),
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err(anyhow!("argument {name} must be an array of strings")),
        None => return Err(anyhow!("missing required argument: {name}")),
    };
    if values.is_empty() {
        return Err(anyhow!("argument {name} must contain at least one item"));
    }
    Ok(values)
}

fn validate_mcp_proposal_id(proposal_id: &str) -> Result<()> {
    let suffix = proposal_id
        .strip_prefix("prop-")
        .ok_or_else(|| anyhow!("invalid proposalId: expected prop-<32 hex chars>"))?;
    if suffix.len() != 32 || !suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid proposalId: expected prop-<32 hex chars>"));
    }
    Ok(())
}

fn validate_mcp_write_content_type(content_type: &str) -> Result<()> {
    let content_type = content_type.trim();
    if WRITE_CONTENT_TYPES.contains(&content_type) {
        Ok(())
    } else {
        Err(anyhow!(
            "unsupported contentType {content_type}; supported contentTypes: {}",
            WRITE_CONTENT_TYPES.join(", ")
        ))
    }
}

fn redact_local_paths(value: Value) -> Value {
    redact_local_paths_with_key(None, value)
}

fn redact_local_paths_with_key(key: Option<&str>, value: Value) -> Value {
    match value {
        Value::String(value) if should_redact_path_field(key, &value) => {
            Value::String("[redacted-local-path]".into())
        }
        Value::String(value) if is_absolute_local_path(&value) => {
            Value::String("[redacted-local-path]".into())
        }
        Value::String(value) => Value::String(redact_local_path_text(&value)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_local_paths_with_key(key, value))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let redacted = redact_local_paths_with_key(Some(&key), value);
                    (key, redacted)
                })
                .collect(),
        ),
        value => value,
    }
}

fn should_redact_path_field(key: Option<&str>, value: &str) -> bool {
    let Some(key) = key else {
        return false;
    };
    if value.trim().is_empty() {
        return false;
    }
    matches!(
        key,
        "originalPath"
            | "sourcePath"
            | "markdownPath"
            | "imagePath"
            | "artifactRoot"
            | "manifestPath"
            | "sourcePaths"
            | "persistedContextPackPath"
    )
}

fn redact_local_path_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let remaining = &value[cursor..];
        let Some((relative_start, prefix_len)) = next_local_path_start(remaining) else {
            output.push_str(remaining);
            break;
        };
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        output.push_str("[redacted-local-path]");
        let path_start = start + prefix_len;
        let path_end = value[path_start..]
            .find(is_local_path_delimiter)
            .map(|offset| path_start + offset)
            .unwrap_or(value.len());
        cursor = path_end;
    }
    output
}

fn next_local_path_start(value: &str) -> Option<(usize, usize)> {
    for (index, _) in value.char_indices() {
        let candidate = &value[index..];
        if !is_local_path_start_boundary(value, index) {
            continue;
        }
        if candidate.starts_with("file:///") {
            return Some((index, "file://".len()));
        }
        if candidate.starts_with('/') || candidate.starts_with("~/") {
            return Some((index, 0));
        }
        if candidate
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
        {
            return Some((index, 0));
        }
    }
    None
}

fn is_local_path_start_boundary(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(is_local_path_delimiter)
}

fn is_local_path_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '(' | '[' | '{' | '<' | ')' | ']' | '}' | '>' | '"' | '\'' | '`' | ',' | ';'
        )
}

fn is_absolute_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("~/")
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

fn resource_definitions() -> Vec<Value> {
    vec![
        json!({
            "uri": "hyprduck://brain/default/graph/snapshot",
            "name": "Latest graph/wiki snapshot",
            "description": "Resolved latest completed materialized graph/wiki snapshot for the default workspace.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "hyprduck://brain/default/wiki/index.md",
            "name": "Wiki index",
            "description": "Current materialized wiki index for the default workspace.",
            "mimeType": "text/markdown"
        }),
    ]
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool_definition(
            "import_source",
            "Start importing an allowlisted local document into HyprDuck and return an import job for polling.",
            json!({
                "sourcePath": { "type": "string", "description": "Path to a local PDF, DOCX, DOC, Markdown, or image file under HYPRDUCK_MCP_ALLOWED_IMPORT_ROOTS." },
                "name": { "type": "string", "description": "Optional display/output name for the imported source." },
                "format": { "type": "string", "enum": ["pdf", "docx", "doc", "markdown", "image"], "description": "Optional import format. Defaults to extension inference." },
                "skipGraphGeneration": { "type": "boolean", "description": "Skip provider-backed graph/wiki generation after citation-ready import. Defaults to false; citation readiness is reported before graph readiness." },
            }),
            vec!["sourcePath"],
            false,
        ),
        tool_definition(
            "import_status",
            "Poll a HyprDuck import job until citationReady and, when enabled, graphReady.",
            json!({
                "jobId": { "type": "string", "description": "Import job ID returned by import_source." },
            }),
            vec!["jobId"],
            true,
        ),
        tool_definition(
            "import_cancel",
            "Request cancellation for a HyprDuck import job.",
            json!({
                "jobId": { "type": "string", "description": "Import job ID returned by import_source." },
            }),
            vec!["jobId"],
            false,
        ),
        tool_definition(
            "get_context_pack",
            "Build an agent-ready document context pack with selected sources, evidence, findings, warnings, and retrieval trace.",
            json!({
                "query": { "type": "string", "description": "Task or question to build context for." },
                "nodeId": { "type": "string", "description": "Optional selected graph node ID used as a retrieval bias, not as a full graph export." },
                "budget": { "type": "integer", "minimum": 1, "description": "Approximate token budget." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "read_context_pack",
            "Read the latest persisted Context Pack v0, or a specific pack by packId.",
            json!({
                "packId": { "type": "string", "description": "Optional packId under context_packs/. Defaults to the latest context_pack.json." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "search_documents",
            "Search local HyprDuck document context artifacts and return ranked evidence-backed IDs.",
            json!({
                "query": { "type": "string", "description": "Search query." },
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum result count." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "search_brain",
            "Compatibility alias for search_documents.",
            json!({
                "query": { "type": "string", "description": "Search query." },
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum result count." },
            }),
            vec!["query"],
            true,
        ),
        tool_definition(
            "read_source",
            "Read an immutable source record with adjacent wiki and evidence refs.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_documents or get_context_pack." },
            }),
            vec!["sourceId"],
            true,
        ),
        tool_definition(
            "read_page_evidence",
            "Read source evidence refs for a source, optionally narrowed to one 1-based page.",
            json!({
                "sourceId": { "type": "string", "description": "Source ID returned by search_documents or get_context_pack." },
                "page": { "type": "integer", "minimum": 1, "description": "Optional 1-based page number." },
            }),
            vec!["sourceId"],
            true,
        ),
        tool_definition(
            "read_wiki_page",
            "Read a generated or saved-back wiki page by repo-relative path.",
            json!({
                "path": { "type": "string", "description": "Wiki page path returned by search_brain or get_context_pack." },
            }),
            vec!["path"],
            true,
        ),
        tool_definition(
            "read_node",
            "Read a graph node with its evidence and adjacent relations.",
            json!({
                "nodeId": { "type": "string", "description": "Graph node ID returned by search_brain or get_context_pack." },
            }),
            vec!["nodeId"],
            true,
        ),
        tool_definition(
            "read_recent_events",
            "Read append-only graph loop events, optionally filtered by run, source, node, edge, claim, memory, or change type.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum event count." },
                "runId": { "type": "string", "description": "Filter by ingest run, source run, caused-by event, or payload runId." },
                "sourceRef": { "type": "string", "description": "Filter by source ID or markdown/source path ref." },
                "nodeId": { "type": "string", "description": "Filter by node ref or target node ID." },
                "edgeId": { "type": "string", "description": "Filter by edge/relation ref or target edge ID." },
                "claimId": { "type": "string", "description": "Filter by claim ref or target claim ID." },
                "memoryId": { "type": "string", "description": "Filter by memory ref or target memory ID." },
                "changeType": { "type": "string", "description": "Filter by event type, operation type, or payload changeType." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_graph_history",
            "List prior materialized graph states with timestamps, source run IDs, and storage locations.",
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "Maximum graph state count." },
            }),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_graph_snapshot",
            "Read the latest completed materialized graph/wiki snapshot and its loading paths for UI, MCP, and agent consumers.",
            json!({}),
            Vec::new(),
            true,
        ),
        tool_definition(
            "read_health",
            "Read workspace context readiness without mutating artifacts.",
            json!({}),
            Vec::new(),
            true,
        ),
        tool_definition(
            "write_propose",
            "Propose an evidence-backed knowledge item for approval. Evidence refs must exist in the current workspace snapshot.",
            json!({
                "contentType": { "type": "string", "enum": WRITE_CONTENT_TYPES, "description": "Supported evidence-backed write type." },
                "title": { "type": "string", "description": "Human-readable title for the knowledge item." },
                "body": { "type": "string", "description": "The knowledge content body." },
                "evidenceRefs": { "type": "array", "items": { "type": "string" }, "minItems": 1, "uniqueItems": true, "description": "Evidence IDs from the current workspace snapshot that back this knowledge item." },
            }),
            vec!["contentType", "title", "body", "evidenceRefs"],
            false,
        ),
        tool_definition(
            "write_commit",
            "Approve a pending proposal and persist it as an audited brain event. Pass userApproved=true only after explicit user approval for proposals that require it.",
            json!({
                "proposalId": { "type": "string", "pattern": PROPOSAL_ID_PATTERN, "description": "Proposal ID returned by write_propose." },
                "userApproved": { "type": "boolean", "description": "Set true only when the user explicitly approved a proposal marked as requiring approval." },
            }),
            vec!["proposalId"],
            false,
        ),
        tool_definition(
            "write_commit_all",
            "Approve multiple pending proposals in a single batch call.",
            json!({
                "proposalIds": { "type": "array", "items": { "type": "string", "pattern": PROPOSAL_ID_PATTERN }, "minItems": 1, "uniqueItems": true, "description": "Array of proposal IDs to commit." },
            }),
            vec!["proposalIds"],
            false,
        ),
        tool_definition(
            "write_list",
            "List all pending proposals.",
            json!({}),
            Vec::new(),
            true,
        ),
        tool_definition(
            "write_reject",
            "Reject a pending proposal and remove it from the proposals directory.",
            json!({
                "proposalId": { "type": "string", "pattern": PROPOSAL_ID_PATTERN, "description": "Proposal ID to reject." },
            }),
            vec!["proposalId"],
            false,
        ),
    ]
}

fn tool_definition(
    name: &str,
    description: &str,
    properties: Value,
    required: Vec<&str>,
    read_only: bool,
) -> Value {
    let mut merged_properties = properties.as_object().cloned().unwrap_or_default();
    merged_properties.insert(
        "workspaceId".into(),
        json!({
            "type": "string",
            "description": "HyprDuck workspace ID. Defaults to default."
        }),
    );
    merged_properties.insert(
        "rootDir".into(),
        json!({
            "type": "string",
            "description": "Optional development-only materialized workspace root. Disabled unless HYPRDUCK_MCP_ALLOW_ROOT_DIR=1 and HYPRDUCK_MCP_ALLOWED_ROOTS allow it."
        }),
    );
    merged_properties.insert(
        "includeLocalPaths".into(),
        json!({
            "type": "boolean",
            "description": "Include absolute local filesystem paths in responses. Defaults to false; keep false for agent-facing calls."
        }),
    );

    json!({
        "name": name,
        "title": title_case_tool_name(name),
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": merged_properties,
            "required": required,
            "additionalProperties": false
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "idempotentHint": read_only,
            "openWorldHint": false
        }
    })
}

fn title_case_tool_name(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut title = first.to_ascii_uppercase().to_string();
                    title.push_str(chars.as_str());
                    title
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_root_dir_env() {
        std::env::remove_var(ROOT_DIR_ENV);
        std::env::remove_var(ROOT_DIR_ALLOWED_ROOTS_ENV);
        std::env::remove_var(IMPORT_ALLOWED_ROOTS_ENV);
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
            json!(["jobId"])
        );
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
