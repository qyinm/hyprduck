pub(crate) mod codex;
pub(crate) mod events;
pub(crate) mod install;
pub(crate) mod runtime;

pub(crate) use codex::{install_codex_hooks, print_codex_hook_status, run_codex_hook};

use anyhow::Result;

use crate::cli::HooksCommand;

pub(crate) fn run_hooks_command(command: HooksCommand) -> Result<()> {
    match command {
        HooksCommand::InstallCodex => install_codex_hooks(),
        HooksCommand::StatusCodex => print_codex_hook_status(),
        HooksCommand::RunCodex => {
            use std::io::Read;

            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let output = run_codex_hook(&input)?;
            println!("{output}");
            Ok(())
        }
    }
}
