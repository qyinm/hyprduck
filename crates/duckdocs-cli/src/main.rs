mod app;
mod cli;
mod tui;
mod ui;

use anyhow::Result;
use cli::{Cli, Commands};
use duckdocs_engine_client::{resolve_engine_launch, EngineClient, SubprocessEngineClient};
use duckdocs_engine_types::{
    DocumentFormat, ParseInput, ParseOptions, ParseOutputTarget, ParseProgress, ParseRequest,
};

fn main() -> Result<()> {
    let cli = Cli::parse()?;

    match cli.command {
        Some(Commands::Doctor) => run_doctor(),
        Some(Commands::Engines { command }) => run_engines(command),
        Some(Commands::Parse { input }) => run_parse(input),
        None => tui::run_tui(),
    }
}

fn run_doctor() -> Result<()> {
    println!("DuckDocs CLI is available.");
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
