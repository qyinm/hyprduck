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
            Some(other) => return Err(anyhow!("unknown command: {other}")),
        };

        Ok(Self { command })
    }
}

#[derive(Debug)]
pub enum Commands {
    Doctor,
    Parse { input: String },
    Engines { command: EnginesCommand },
}

#[derive(Debug)]
pub enum EnginesCommand {
    List,
}
