mod app;
mod cli;
mod eval;
mod mcp;
mod metrics;
mod tui;
mod ui;

use anyhow::Result;
use cli::{Cli, Commands, GraphStateSelector};
use hyprduck_engine_client::{resolve_engine_launch, EngineClient, SubprocessEngineClient};
use hyprduck_engine_types::{
    BrainReadScope, CompileProjectRequest, ContextPackParseConfidence, DocumentFormat,
    EvidenceIndexItemV0, EvidenceIndexV0, GetContextPackRequest, GraphHistoryEntry, IngestStatus,
    PageArtifact, ParseInput, ParseOptions, ParseOutputTarget, ParseProgress, ParseRequest,
    ReadRecentEventsRequest, ReconstructBrainResponseData, SearchBrainRequest,
    SourceArtifactManifest, SourcePackPageV0, SourcePackV0, EVIDENCE_INDEX_V0_SCHEMA_VERSION,
    SOURCE_PACK_V0_SCHEMA_VERSION,
};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DEMO_WORKSPACE_ID: &str = "demo";
const DEMO_SOURCE_ID: &str = "demo-source";
const DEMO_MARKDOWN: &str = r#"# HyprDuck Demo Contract

The demo document says agents should cite source, page, and evidence IDs when
they answer from private documents.

The reusable artifact is a local Context Pack v0 generated from a Source Pack
and Evidence Index.
"#;

fn main() -> Result<()> {
    let cli = Cli::parse()?;

    match cli.command {
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Serve) => run_serve(),
        Some(Commands::Mcp { command }) => run_mcp(command),
        Some(Commands::Engines { command }) => run_engines(command),
        Some(Commands::Demo { command }) => run_demo(command),
        Some(Commands::Brain { command }) => run_brain(command),
        Some(Commands::Eval { command }) => run_eval(command),
        Some(Commands::Parse { input }) => run_parse(input),
        None => tui::run_tui(),
    }
}

fn run_serve() -> Result<()> {
    let spec = resolve_engine_launch()?;
    let status = spec.command().arg("serve").status()?;
    if !status.success() {
        anyhow::bail!("engine runtime exited with status {status}");
    }
    Ok(())
}

fn run_mcp(command: cli::McpCommand) -> Result<()> {
    match command {
        cli::McpCommand::Serve => mcp::run_mcp_server(),
        cli::McpCommand::InstallClaudeCode => install_claude_code_mcp(),
        cli::McpCommand::InstallCodex => install_codex_mcp(),
    }
}

fn install_claude_code_mcp() -> Result<()> {
    let home = home_dir()?;
    let config_path = home
        .join(".config")
        .join("claude-code")
        .join("mcp_servers.json");
    let current_exe = current_exe_string()?;

    let mut config = if config_path.exists() {
        let raw = std::fs::read_to_string(&config_path)?;
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
            anyhow::anyhow!("failed to parse {}: {error}", config_path.display())
        })?
    } else {
        serde_json::json!({})
    };

    if !config.is_object() {
        anyhow::bail!("{} must contain a JSON object", config_path.display());
    }
    if config
        .get("mcpServers")
        .and_then(|value| value.as_object())
        .is_none()
    {
        config["mcpServers"] = serde_json::json!({});
    }

    config["mcpServers"]["hyprduck"] = serde_json::json!({
        "command": current_exe.clone(),
        "args": ["mcp", "serve"]
    });

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)? + "\n")?;

    println!("Installed HyprDuck MCP for Claude Code.");
    println!("config: {}", config_path.display());
    println!("command: {current_exe} mcp serve");
    install_shell_shim(&home, &current_exe)?;
    Ok(())
}

fn install_codex_mcp() -> Result<()> {
    let home = home_dir()?;
    let current_exe = current_exe_string()?;
    install_shell_shim(&home, &current_exe)?;

    let codex_bin = std::env::var_os("HYPRDUCK_CODEX_BIN").unwrap_or_else(|| "codex".into());
    let _ = std::process::Command::new(&codex_bin)
        .args(["mcp", "remove", "hyprduck"])
        .status();
    let status = std::process::Command::new(&codex_bin)
        .args(["mcp", "add", "hyprduck", "--", &current_exe, "mcp", "serve"])
        .status()
        .map_err(|error| anyhow::anyhow!("failed to run codex mcp add: {error}"))?;
    if !status.success() {
        anyhow::bail!("codex mcp add exited with status {status}");
    }

    println!("Installed HyprDuck MCP for Codex.");
    println!("command: {current_exe} mcp serve");
    Ok(())
}

fn home_dir() -> Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

fn current_exe_string() -> Result<String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| anyhow::anyhow!("failed to locate hyprduck binary: {error}"))?;
    let current_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe);
    Ok(current_exe.to_string_lossy().to_string())
}

fn install_shell_shim(home: &std::path::Path, current_exe: &str) -> Result<()> {
    let bin_dir = home.join(".local").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let shim_path = bin_dir.join("hyprduck");
    match std::fs::symlink_metadata(&shim_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = std::fs::read_link(&shim_path)?;
            let resolved_target = resolve_link_target(&shim_path, &target);
            let current_exe_path = std::path::PathBuf::from(current_exe);
            if resolved_target == current_exe_path {
                print_shell_shim_status(&bin_dir, &shim_path);
                return Ok(());
            }
            if !is_managed_hyprduck_cli_target(&resolved_target) {
                eprintln!(
                    "shell command already points elsewhere and was left unchanged: {}",
                    shim_path.display()
                );
                print_path_note(&bin_dir, &shim_path);
                return Ok(());
            }
            std::fs::remove_file(&shim_path)?;
        }
        Ok(_) => {
            eprintln!(
                "shell command already exists and was left unchanged: {}",
                shim_path.display()
            );
            print_path_note(&bin_dir, &shim_path);
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(current_exe, &shim_path)?;

    #[cfg(not(unix))]
    std::fs::copy(current_exe, &shim_path)?;

    print_shell_shim_status(&bin_dir, &shim_path);
    Ok(())
}

fn resolve_link_target(
    shim_path: &std::path::Path,
    target: &std::path::Path,
) -> std::path::PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        shim_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(target)
    }
}

fn is_managed_hyprduck_cli_target(target: &std::path::Path) -> bool {
    let Some(file_name) = target.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let target = target.to_string_lossy();
    file_name.starts_with("hyprduck-") && target.contains(".app/Contents/Resources/binaries/")
}

fn print_shell_shim_status(bin_dir: &std::path::Path, shim_path: &std::path::Path) {
    println!("shell command: {}", shim_path.display());
    print_path_note(bin_dir, shim_path);
}

fn print_path_note(bin_dir: &std::path::Path, shim_path: &std::path::Path) {
    if !is_directory_on_path(bin_dir) {
        println!(
            "path note: {} is not on PATH; use {} directly or add it to PATH",
            bin_dir.display(),
            shim_path.display()
        );
    }
}

fn is_directory_on_path(directory: &std::path::Path) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|entry| entry == directory))
        .unwrap_or(false)
}

fn run_doctor() -> Result<()> {
    println!("HyprDuck CLI is available.");
    println!("TUI backend: ratatui + crossterm");
    match resolve_engine_launch() {
        Ok(spec) => println!("Engine runtime: {}", spec.display()),
        Err(error) => println!("Engine runtime: unresolved ({error})"),
    }
    Ok(())
}

fn run_engines(command: cli::EnginesCommand) -> Result<()> {
    match command {
        cli::EnginesCommand::List => {
            println!("hyprduck-engine");
        }
    }
    Ok(())
}

fn run_demo(command: cli::DemoCommand) -> Result<()> {
    let started = Instant::now();
    let root = match command.root_dir {
        Some(root_dir) => PathBuf::from(root_dir),
        None => unique_demo_root()?,
    };
    std::fs::create_dir_all(&root)?;
    let fixture_path = root.join("demo-source.md");
    std::fs::write(&fixture_path, DEMO_MARKDOWN)?;

    let previous_store = std::env::var_os("HYPRDUCK_PROJECT_STORE");
    let previous_output_dir = std::env::var_os("HYPRDUCK_OUTPUT_DIR");
    let previous_provider_graph = std::env::var_os("HYPRDUCK_DISABLE_PROVIDER_GRAPH");
    std::env::set_var("HYPRDUCK_PROJECT_STORE", root.join("knowledge.sqlite3"));
    std::env::set_var("HYPRDUCK_OUTPUT_DIR", &root);
    std::env::set_var("HYPRDUCK_DISABLE_PROVIDER_GRAPH", "1");

    let result = run_demo_with_root(&root, &fixture_path, &command.query);

    restore_env_var("HYPRDUCK_PROJECT_STORE", previous_store);
    restore_env_var("HYPRDUCK_OUTPUT_DIR", previous_output_dir);
    restore_env_var("HYPRDUCK_DISABLE_PROVIDER_GRAPH", previous_provider_graph);

    let report = result?;
    println!("demo-root: {}", root.display());
    println!("workspace: {DEMO_WORKSPACE_ID}");
    println!("source-pack: {}", report.source_pack_path.display());
    println!("evidence-index: {}", report.evidence_index_path.display());
    println!("context-pack: {}", report.context_pack_path);
    println!("context-pack-v0: {}", report.schema_version);
    println!("context-pack-v0-sources: {}", report.source_count);
    println!("context-pack-v0-evidence: {}", report.evidence_count);
    println!("context-pack-v0-findings: {}", report.finding_count);
    println!("context-pack-v0-warnings: {}", report.warning_count);
    println!("elapsed-ms: {}", started.elapsed().as_millis());
    println!(
        "next: hyprduck context --root {} --workspace {DEMO_WORKSPACE_ID} --write-context-pack \"{}\"",
        root.display(),
        command.query.replace('"', "\\\"")
    );
    Ok(())
}

struct DemoReport {
    source_pack_path: PathBuf,
    evidence_index_path: PathBuf,
    context_pack_path: String,
    schema_version: String,
    source_count: usize,
    evidence_count: usize,
    finding_count: usize,
    warning_count: usize,
}

fn run_demo_with_root(root: &Path, fixture_path: &Path, query: &str) -> Result<DemoReport> {
    let manifest = write_demo_source_artifacts(root, fixture_path)?;
    let client = SubprocessEngineClient::default();
    client.compile_project(CompileProjectRequest {
        source_markdown_path: manifest.markdown_path.clone(),
        source_document_path: Some(manifest.source_path.clone()),
        source_manifest_path: Some(manifest.manifest_path.clone()),
        workspace_id: Some(DEMO_WORKSPACE_ID.into()),
        source_id: Some(DEMO_SOURCE_ID.into()),
        skip_graph_generation: Some(true),
    })?;

    let context_response = client.get_context_pack(GetContextPackRequest {
        scope: BrainReadScope {
            workspace_id: DEMO_WORKSPACE_ID.into(),
            root_dir: Some(root.display().to_string()),
        },
        query: query.to_string(),
        budget: Some(4000),
        persist: true,
    })?;
    let context_pack = context_response.context_pack_v0;
    let context_pack_path = context_response
        .persisted_context_pack_path
        .ok_or_else(|| anyhow::anyhow!("demo context pack was not persisted"))?;

    Ok(DemoReport {
        source_pack_path: PathBuf::from(&manifest.artifact_root).join("source_pack.json"),
        evidence_index_path: PathBuf::from(&manifest.artifact_root).join("evidence_index.json"),
        context_pack_path,
        schema_version: context_pack.schema_version,
        source_count: context_pack.source_set.len(),
        evidence_count: context_pack.selected_evidence.len(),
        finding_count: context_pack.findings.len(),
        warning_count: context_pack.warnings.len(),
    })
}

fn write_demo_source_artifacts(root: &Path, fixture_path: &Path) -> Result<SourceArtifactManifest> {
    let workspace_root = root.join(DEMO_WORKSPACE_ID);
    let source_root = workspace_root.join("sources").join(DEMO_SOURCE_ID);
    let artifact_root = workspace_root.join("artifacts").join(DEMO_SOURCE_ID);
    let pages_root = artifact_root.join("pages");
    std::fs::create_dir_all(&source_root)?;
    std::fs::create_dir_all(&pages_root)?;
    std::fs::create_dir_all(workspace_root.join("graph"))?;
    std::fs::create_dir_all(workspace_root.join("wiki"))?;

    let source_path = source_root.join("demo-source.md");
    let markdown_path = artifact_root.join("hyprduck-demo.md");
    let page_markdown_path = pages_root.join("page_1.md");
    std::fs::copy(fixture_path, &source_path)?;
    std::fs::write(
        &markdown_path,
        format!("# hyprduck-demo\n\n## Page 1\n\n{DEMO_MARKDOWN}\n"),
    )?;
    std::fs::write(&page_markdown_path, DEMO_MARKDOWN)?;

    let now = unix_timestamp_seconds()?;
    let content_hash = format!("fnv64:{:016x}", fnv1a64(DEMO_MARKDOWN.as_bytes()));
    let manifest_path = artifact_root.join("source-manifest.json");
    let page = PageArtifact {
        index: 0,
        label: "Page 1".into(),
        image_path: None,
        markdown_path: Some(page_markdown_path.display().to_string()),
        plain_text_path: None,
        error_message: None,
    };
    let manifest = SourceArtifactManifest {
        workspace_id: DEMO_WORKSPACE_ID.into(),
        source_id: DEMO_SOURCE_ID.into(),
        original_path: fixture_path.display().to_string(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: artifact_root.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: DocumentFormat::Markdown,
        output_name: "hyprduck-demo".into(),
        status: IngestStatus::Ingested,
        description: "Built-in HyprDuck demo fixture.".into(),
        user_context: "Local demo; no hosted provider call.".into(),
        ingest_instruction: "Demonstrate source/evidence/context pack artifacts.".into(),
        pages: vec![page.clone()],
        created_at: now,
        updated_at: now,
    };
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    let source_pack = SourcePackV0 {
        schema_version: SOURCE_PACK_V0_SCHEMA_VERSION.into(),
        workspace_id: DEMO_WORKSPACE_ID.into(),
        source_id: DEMO_SOURCE_ID.into(),
        original_filename: "demo-source.md".into(),
        original_path: fixture_path.display().to_string(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: artifact_root.display().to_string(),
        content_hash: content_hash.clone(),
        format: DocumentFormat::Markdown,
        page_count: 1,
        ingestion_status: IngestStatus::Ingested,
        provider_route: "local_demo".into(),
        local_only: true,
        pages: vec![SourcePackPageV0 {
            page: 1,
            label: "Page 1".into(),
            image_path: None,
            markdown_path: Some(page_markdown_path.display().to_string()),
            plain_text_path: None,
            error_message: None,
        }],
        warnings: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    let evidence_index = EvidenceIndexV0 {
        schema_version: EVIDENCE_INDEX_V0_SCHEMA_VERSION.into(),
        workspace_id: DEMO_WORKSPACE_ID.into(),
        source_id: DEMO_SOURCE_ID.into(),
        content_hash: content_hash.clone(),
        provider_route: "local_demo".into(),
        local_only: true,
        evidence: vec![EvidenceIndexItemV0 {
            evidence_ref: format!("ev-{DEMO_SOURCE_ID}-source-1"),
            source_id: DEMO_SOURCE_ID.into(),
            page: 1,
            region: "page:Page 1".into(),
            span: Some("page".into()),
            quoted_text: excerpt(DEMO_MARKDOWN, 280),
            parse_confidence: ContextPackParseConfidence::High,
            content_hash,
            markdown_path: Some(page_markdown_path.display().to_string()),
            image_path: None,
        }],
        warnings: Vec::new(),
        generated_at: now,
    };
    std::fs::write(
        artifact_root.join("source_pack.json"),
        serde_json::to_string_pretty(&source_pack)? + "\n",
    )?;
    std::fs::write(
        artifact_root.join("evidence_index.json"),
        serde_json::to_string_pretty(&evidence_index)? + "\n",
    )?;

    Ok(manifest)
}

fn unique_demo_root() -> Result<PathBuf> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("hyprduck-demo-{nanos}")))
}

fn unix_timestamp_seconds() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for value in text.split_whitespace() {
        let extra = usize::from(!output.is_empty());
        if output.len() + extra + value.len() > max_chars {
            break;
        }
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(value);
    }
    output
}

fn restore_env_var(key: &str, previous: Option<std::ffi::OsString>) {
    match previous {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn run_brain(command: cli::BrainCommand) -> Result<()> {
    let client = SubprocessEngineClient::default();
    match command {
        cli::BrainCommand::Search {
            workspace,
            root_dir,
            query,
        } => {
            let response = client.search_brain(SearchBrainRequest {
                scope: BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                query,
                limit: Some(10),
            })?;
            for result in response.results {
                let path = result.path.unwrap_or_else(|| "-".into());
                println!(
                    "{}\t{}\t{}\t{}",
                    result.score,
                    format!("{:?}", result.kind).to_ascii_lowercase(),
                    result.title,
                    path
                );
                println!("  {}", result.snippet.replace('\n', " "));
            }
        }
        cli::BrainCommand::ContextPack {
            workspace,
            root_dir,
            query,
            budget,
            persist,
        } => {
            let response = client.get_context_pack(GetContextPackRequest {
                scope: BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                query,
                budget,
                persist,
            })?;
            let context_pack_v0 = &response.context_pack_v0;
            let pack = response.context_pack;
            println!("{}", pack.summary);
            for warning in &pack.warnings {
                println!("warning: {warning}");
            }
            println!("context-pack-v0: {}", context_pack_v0.schema_version);
            println!(
                "context-pack-v0-sources: {}",
                context_pack_v0.source_set.len()
            );
            println!(
                "context-pack-v0-evidence: {}",
                context_pack_v0.selected_evidence.len()
            );
            println!(
                "context-pack-v0-findings: {}",
                context_pack_v0.findings.len()
            );
            println!(
                "context-pack-v0-warnings: {}",
                context_pack_v0.warnings.len()
            );
            if let Some(path) = &response.persisted_context_pack_path {
                println!("context-pack-v0-path: {path}");
            }
            println!("wiki-pages: {}", pack.wiki_pages.len());
            println!("nodes: {}", pack.nodes.len());
            println!("sources: {}", pack.sources.len());
            println!("memories: {}", pack.memories.len());
            println!("entities: {}", pack.entities.len());
            println!("claims: {}", pack.claims.len());
            println!("relations: {}", pack.relations.len());
            println!("evidence: {}", pack.evidence.len());
            println!("recent-events: {}", pack.recent_events.len());
        }
        cli::BrainCommand::EventHistory { request } => {
            print_event_history(client.read_recent_events(request)?)?;
        }
        cli::BrainCommand::GraphHistory { request } => {
            print_graph_history(client.read_graph_history(request)?)?;
        }
        cli::BrainCommand::InspectState { request, selector } => {
            let response = client.read_graph_history(request.clone())?;
            let state = response
                .states
                .into_iter()
                .find(|state| match &selector {
                    GraphStateSelector::Snapshot(snapshot_id) => state.snapshot_id == *snapshot_id,
                    GraphStateSelector::Event(event_id) => state.event_id == *event_id,
                })
                .ok_or_else(|| match selector {
                    GraphStateSelector::Snapshot(snapshot_id) => {
                        anyhow::anyhow!("graph snapshot not found: {snapshot_id}")
                    }
                    GraphStateSelector::Event(event_id) => {
                        anyhow::anyhow!("graph materialization event not found: {event_id}")
                    }
                })?;
            let related_request = related_events_request_for_state(&request, &state);
            let related_events = client.read_recent_events(related_request)?;
            print_graph_state_inspection(&state, related_events)?;
        }
        cli::BrainCommand::RollbackState {
            history_request,
            mut request,
            selector,
        } => {
            let response = client.read_graph_history(history_request)?;
            let state = response
                .states
                .into_iter()
                .find(|state| match &selector {
                    GraphStateSelector::Snapshot(snapshot_id) => state.snapshot_id == *snapshot_id,
                    GraphStateSelector::Event(event_id) => state.event_id == *event_id,
                })
                .ok_or_else(|| match selector {
                    GraphStateSelector::Snapshot(snapshot_id) => {
                        anyhow::anyhow!("graph snapshot not found: {snapshot_id}")
                    }
                    GraphStateSelector::Event(event_id) => {
                        anyhow::anyhow!("graph materialization event not found: {event_id}")
                    }
                })?;
            request.up_to_event_id = Some(state.event_id.clone());
            let restored = client.reconstruct_brain(request)?;
            print_graph_rollback_result(&state, restored)?;
        }
    }
    Ok(())
}

fn print_graph_history(
    response: hyprduck_engine_types::ReadGraphHistoryResponseData,
) -> Result<()> {
    for state in response.states {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            state.materialized_at,
            state.snapshot_id,
            state.event_id,
            state.operation_type.as_deref().unwrap_or("-"),
            state.node_count,
            state.edge_count,
            state.claim_count,
            state.memory_count,
        );
        println!(
            "  wiki-pages: {} sources: {} locations: {}",
            state.wiki_page_count,
            printable_refs(&state.source_markdown_refs),
            printable_refs(&state.storage_locations)
        );
        println!(
            "  rollback-target: {}",
            state.rollback_target.replay_selector
        );
    }
    Ok(())
}

fn print_graph_state_inspection(
    state: &GraphHistoryEntry,
    events: hyprduck_engine_types::ReadRecentEventsResponseData,
) -> Result<()> {
    println!("snapshot: {}", state.snapshot_id);
    println!("event: {}", state.event_id);
    println!("materialized-at: {}", state.materialized_at);
    println!(
        "operation: {}",
        state.operation_type.as_deref().unwrap_or("-")
    );
    println!(
        "nodes: {} edges: {} claims: {} memories: {} wiki-pages: {}",
        state.node_count,
        state.edge_count,
        state.claim_count,
        state.memory_count,
        state.wiki_page_count
    );
    println!("source-runs: {}", printable_refs(&state.source_run_ids));
    println!(
        "source-markdown: {}",
        printable_refs(&state.source_markdown_refs)
    );
    println!("rollback-target: {}", state.rollback_target.replay_selector);
    println!("storage: {}", printable_refs(&state.storage_locations));
    println!("related-events: {}", events.events.len());
    print_event_history(events)?;
    Ok(())
}

fn print_graph_rollback_result(
    state: &GraphHistoryEntry,
    restored: ReconstructBrainResponseData,
) -> Result<()> {
    println!("rollback-applied: {}", state.snapshot_id);
    println!("event: {}", state.event_id);
    println!("new-snapshot: {}", restored.snapshot_id);
    println!("output-root: {}", restored.output_root);
    println!("replayed-events: {}", restored.replayed_event_count);
    println!("changed-files: {}", restored.changed_files.len());
    for path in restored.changed_files {
        println!("  {path}");
    }
    Ok(())
}

fn related_events_request_for_state(
    request: &hyprduck_engine_types::ReadGraphHistoryRequest,
    state: &GraphHistoryEntry,
) -> ReadRecentEventsRequest {
    ReadRecentEventsRequest {
        scope: request.scope.clone(),
        limit: request.limit.or(Some(50)),
        run_id: None,
        source_ref: state
            .source_markdown_refs
            .first()
            .cloned()
            .or_else(|| state.source_run_ids.first().cloned()),
        node_id: None,
        edge_id: None,
        claim_id: None,
        memory_id: None,
        change_type: None,
    }
}

fn print_event_history(
    response: hyprduck_engine_types::ReadRecentEventsResponseData,
) -> Result<()> {
    for event in response.events {
        let source_refs = if event.source_refs.is_empty() {
            "-".to_string()
        } else {
            event.source_refs.join(",")
        };
        let node_refs = if event.node_refs.is_empty() {
            "-".to_string()
        } else {
            event.node_refs.join(",")
        };
        let relation_refs = if event.relation_refs.is_empty() {
            "-".to_string()
        } else {
            event.relation_refs.join(",")
        };
        let claim_refs = if event.claim_refs.is_empty() {
            "-".to_string()
        } else {
            event.claim_refs.join(",")
        };
        let memory_refs = if event.memory_refs.is_empty() {
            "-".to_string()
        } else {
            event.memory_refs.join(",")
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            event.created_at,
            event.event_id,
            format!("{:?}", event.event_type).to_ascii_lowercase(),
            event.operation_type.as_deref().unwrap_or("-"),
            source_refs,
            node_refs,
            relation_refs,
            claim_refs,
        );
        println!("  memories: {memory_refs}");
    }
    Ok(())
}

fn printable_refs(values: &[String]) -> String {
    if values.is_empty() {
        "-".into()
    } else {
        values.join(",")
    }
}

fn run_eval(command: cli::EvalCommand) -> Result<()> {
    match command {
        cli::EvalCommand::GoldenCorpus { fixtures, mode } => {
            let mode = eval::GoldenEvalMode::parse(&mode)?;
            println!("{}", eval::run_golden_corpus(fixtures, mode)?);
        }
        cli::EvalCommand::DryRunLog { input } => {
            println!("{}", metrics::run_dry_run_log(input)?);
        }
    }
    Ok(())
}

fn run_parse(input: String) -> Result<()> {
    let format = infer_format(&input)?;
    let request = ParseRequest {
        version: "1".to_string(),
        input: ParseInput {
            path: input.clone(),
            format,
        },
        template: "General".to_string(),
        options: ParseOptions::default(),
        output: Some(ParseOutputTarget {
            root_dir: None,
            name: std::path::Path::new(&input)
                .file_stem()
                .map(|name| name.to_string_lossy().to_string()),
            workspace_id: None,
            source_id: None,
        }),
    };

    let client = SubprocessEngineClient::default();
    let mut progress_log = Vec::new();
    let response = client.parse(request, &mut |progress| {
        progress_log.push(progress_label(&progress).to_string());
    })?;
    let result = response.result;

    for entry in progress_log {
        println!("progress: {entry}");
    }
    println!("markdown-bytes: {}", result.markdown.len());
    println!("pages: {}", result.metadata.page_count);
    println!(
        "success: {} failed: {}",
        result.success_count, result.failed_count
    );
    println!("engine: {}", result.metadata.engine_id);
    if let Some(saved_output_path) = response.saved_output_path {
        println!("saved-output: {saved_output_path}");
    }
    Ok(())
}

fn infer_format(path: &str) -> Result<DocumentFormat> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "pdf" => Ok(DocumentFormat::Pdf),
        "docx" => Ok(DocumentFormat::Docx),
        "doc" => Ok(DocumentFormat::Doc),
        "md" | "markdown" => Ok(DocumentFormat::Markdown),
        "png" | "jpg" | "jpeg" | "webp" | "heic" | "tiff" => Ok(DocumentFormat::Image),
        _ => Err(anyhow::anyhow!("unsupported input format: {extension}")),
    }
}

fn progress_label(progress: &ParseProgress) -> &'static str {
    match progress {
        ParseProgress::Queued => "queued",
        ParseProgress::ConvertingPages { .. } => "converting_pages",
        ParseProgress::Parsing { .. } => "parsing",
        ParseProgress::Packaging => "packaging",
        ParseProgress::Completed => "completed",
        ParseProgress::Failed { .. } => "failed",
    }
}
