use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "duckdocs")]
#[command(about = "DuckDocs Rust CLI and TUI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Tui,
    Doctor,
    Parse {
        input: String,
    },
    Engines {
        #[command(subcommand)]
        command: EnginesCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum EnginesCommand {
    List,
}
