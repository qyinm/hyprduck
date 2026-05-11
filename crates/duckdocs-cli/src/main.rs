mod app;
mod cli;
mod eval;
mod tui;
mod ui;

use anyhow::Result;
use cli::{Cli, Commands};
use duckdocs_engine_client::{resolve_engine_launch, EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    BrainActor, BrainActorType, BrainReadScope, DocumentFormat, GetContextPackRequest, ParseInput,
    ParseOptions, ParseOutputTarget, ParseProgress, ParseRequest, ProposeBrainUpdateRequest,
    SearchBrainRequest,
};

fn main() -> Result<()> {
    let cli = Cli::parse()?;

    match cli.command {
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Serve) => run_serve(),
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
            })?;
            println!("proposal: {}", response.proposal.proposal_id);
            println!("event: {}", response.event.event_id);
            println!("status: {:?}", response.proposal.status);
            println!("path: {}", response.proposal_path);
        }
    }
    Ok(())
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
