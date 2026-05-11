use anyhow::{anyhow, Result};

#[derive(Debug)]
pub struct Cli {
    pub command: Option<Commands>,
}

impl Cli {
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let command = match args.next().as_deref() {
            None | Some("tui") => None,
            Some("doctor") => Some(Commands::Doctor),
            Some("serve") => Some(Commands::Serve),
            Some("parse") => {
                let input = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: duckdocs parse <input>"))?;
                Some(Commands::Parse { input })
            }
            Some("engines") => {
                let subcommand = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: duckdocs engines list"))?;
                match subcommand.as_str() {
                    "list" => Some(Commands::Engines {
                        command: EnginesCommand::List,
                    }),
                    _ => return Err(anyhow!("unknown engines subcommand: {subcommand}")),
                }
            }
            Some("brain") => {
                let subcommand = args
                    .next()
                    .ok_or_else(|| anyhow!("usage: duckdocs brain <search|context-pack>"))?;
                Some(Commands::Brain {
                    command: parse_brain_command(subcommand, args.collect())?,
                })
            }
            Some(other) => return Err(anyhow!("unknown command: {other}")),
        };

        Ok(Self { command })
    }
}

fn parse_brain_command(subcommand: String, args: Vec<String>) -> Result<BrainCommand> {
    let mut workspace = "default".to_string();
    let mut root_dir = None;
    let mut budget = None;
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                index += 1;
                workspace = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| anyhow!("--workspace needs a value"))?;
            }
            "--root" => {
                index += 1;
                root_dir = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| anyhow!("--root needs a value"))?,
                );
            }
            "--budget" => {
                index += 1;
                let raw = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--budget needs a value"))?;
                budget = Some(
                    raw.parse()
                        .map_err(|_| anyhow!("invalid --budget: {raw}"))?,
                );
            }
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    let query = positional.join(" ");
    if query.trim().is_empty() {
        return Err(anyhow!("duckdocs brain {subcommand} needs a query"));
    }
    match subcommand.as_str() {
        "search" => Ok(BrainCommand::Search {
            workspace,
            root_dir,
            query,
        }),
        "context-pack" => Ok(BrainCommand::ContextPack {
            workspace,
            root_dir,
            query,
            budget,
        }),
        _ => Err(anyhow!("unknown brain subcommand: {subcommand}")),
    }
}

#[derive(Debug)]
pub enum Commands {
    Doctor,
    Serve,
    Parse { input: String },
    Engines { command: EnginesCommand },
    Brain { command: BrainCommand },
}

#[derive(Debug)]
pub enum EnginesCommand {
    List,
}

#[derive(Debug)]
pub enum BrainCommand {
    Search {
        workspace: String,
        root_dir: Option<String>,
        query: String,
    },
    ContextPack {
        workspace: String,
        root_dir: Option<String>,
        query: String,
        budget: Option<usize>,
    },
}
