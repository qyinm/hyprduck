mod app;
mod cli;
mod eval;
mod mcp;
mod tui;
mod ui;

use anyhow::Result;
use cli::{Cli, Commands, GraphStateSelector};
use duckdocs_engine_client::{resolve_engine_launch, EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    BrainActor, BrainActorType, BrainReadScope, DocumentFormat, GetContextPackRequest,
    GraphHistoryEntry, ParseInput, ParseOptions, ParseOutputTarget, ParseProgress, ParseRequest,
    ProposeBrainUpdateRequest, ReadRecentEventsRequest, ReconstructBrainResponseData,
    SearchBrainRequest,
};

fn main() -> Result<()> {
    let cli = Cli::parse()?;

    match cli.command {
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Serve) => run_serve(),
        Some(Commands::Mcp { command }) => run_mcp(command),
        Some(Commands::Engines { command }) => run_engines(command),
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
    }
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
            println!("duckdocs-engine");
        }
    }
    Ok(())
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
        } => {
            let pack = client.get_context_pack(GetContextPackRequest {
                scope: BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                query,
                budget,
            })?;
            println!("{}", pack.summary);
            for warning in &pack.warnings {
                println!("warning: {warning}");
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
        cli::BrainCommand::ProposeUpdate {
            workspace,
            root_dir,
            kind,
            title,
            body,
            actor,
            target_node_id,
            target_source_id,
            relation_kind,
            source_refs,
            node_refs,
            evidence_refs,
        } => {
            let response = client.propose_brain_update(ProposeBrainUpdateRequest {
                scope: BrainReadScope {
                    workspace_id: workspace,
                    root_dir,
                },
                kind,
                title,
                body,
                actor: BrainActor {
                    actor_type: BrainActorType::Agent,
                    actor_id: actor,
                },
                target_node_id,
                target_source_id,
                relation_kind,
                source_description: None,
                source_user_context: None,
                source_ingest_instruction: None,
                source_refs,
                node_refs,
                evidence_refs,
                proposal_payload: None,
            })?;
            println!("proposal: {}", response.proposal.proposal_id);
            println!("event: {}", response.event.event_id);
            println!("status: {:?}", response.proposal.status);
            println!("path: {}", response.proposal_path);
        }
    }
    Ok(())
}

fn print_graph_history(
    response: duckdocs_engine_types::ReadGraphHistoryResponseData,
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
    events: duckdocs_engine_types::ReadRecentEventsResponseData,
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
    request: &duckdocs_engine_types::ReadGraphHistoryRequest,
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
    response: duckdocs_engine_types::ReadRecentEventsResponseData,
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
