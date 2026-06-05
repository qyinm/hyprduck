use anyhow::Result;

use super::install::{codex_status, install_codex_config, shell_quote};
use super::runtime::{parse_codex_input, run_codex};

pub(crate) fn install_codex_hooks() -> Result<()> {
    let exe = std::env::current_exe()?;
    let command = format!("{} hooks run codex", shell_quote(&exe.to_string_lossy()));
    let paths = install_codex_config(&command)?;
    println!("Installed HyprDuck hooks for Codex.");
    println!("hooks: {}", paths.hooks_file.display());
    println!("config: {}", paths.config_file.display());
    println!("Run /hooks in Codex to review and trust the HyprDuck command hooks.");
    println!("command: {command}");
    Ok(())
}

pub(crate) fn print_codex_hook_status() -> Result<()> {
    let status = codex_status()?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

pub(crate) fn run_codex_hook(input: &str) -> Result<String> {
    let input = parse_codex_input(input)?;
    let output = run_codex(input);
    Ok(serde_json::to_string(&output)?)
}
