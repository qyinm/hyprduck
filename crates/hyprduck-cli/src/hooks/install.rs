use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use super::policy::AUTOMATIC_MCP_TOOLS;

const HOOKS_FILE: &str = "hooks.json";
const CONFIG_FILE: &str = "config.toml";
const CONFIG_BLOCK_START: &str = "# BEGIN HYPRDUCK HOOK AUTOMATION";
const CONFIG_BLOCK_END: &str = "# END HYPRDUCK HOOK AUTOMATION";
const HYPRDUCK_STATUS_PREFIX: &str = "HyprDuck:";

pub(crate) struct CodexInstallPaths {
    pub(crate) home: PathBuf,
    pub(crate) hooks_file: PathBuf,
    pub(crate) config_file: PathBuf,
}

pub(crate) fn codex_paths() -> Result<CodexInstallPaths> {
    let home = if let Some(value) = std::env::var_os("CODEX_HOME") {
        PathBuf::from(value)
    } else {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow!("HOME is not set; cannot locate Codex config directory"))?;
        PathBuf::from(home).join(".codex")
    };
    Ok(CodexInstallPaths {
        hooks_file: home.join(HOOKS_FILE),
        config_file: home.join(CONFIG_FILE),
        home,
    })
}

pub(crate) fn install_codex_config(command: &str) -> Result<CodexInstallPaths> {
    let paths = codex_paths()?;
    fs::create_dir_all(&paths.home)
        .with_context(|| format!("failed to create {}", paths.home.display()))?;
    merge_hooks_json(&paths.hooks_file, command)?;
    merge_config_toml(&paths.config_file)?;
    Ok(paths)
}

pub(crate) fn codex_status() -> Result<Value> {
    let paths = codex_paths()?;
    let hooks_check = codex_hooks_config_check(&paths.hooks_file)?;
    let approvals_check = codex_approvals_config_check(&paths.config_file)?;

    Ok(json!({
        "agent": "codex",
        "codexHome": paths.home,
        "hooksFile": paths.hooks_file,
        "configFile": paths.config_file,
        "hooksConfigured": hooks_check.configured,
        "hooksConfigValid": hooks_check.valid,
        "hooksConfigError": hooks_check.error,
        "mcpApprovalConfigConfigured": approvals_check.configured,
        "mcpApprovalConfigValid": approvals_check.valid,
        "mcpApprovalConfigError": approvals_check.error,
        "trustReview": "Codex requires /hooks trust review for non-managed command hooks before they run."
    }))
}

fn merge_hooks_json(path: &Path, command: &str) -> Result<()> {
    let mut root = if path.exists() {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str::<Value>(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?
    } else {
        json!({})
    };

    if !root.is_object() {
        return Err(anyhow!("{} must contain a JSON object", path.display()));
    }
    let root_obj = root.as_object_mut().expect("object checked");
    let hooks_value = root_obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| anyhow!("{}.hooks must be a JSON object", path.display()))?;

    for (event, matcher, status) in [
        (
            "SessionStart",
            Some("startup|resume|clear|compact"),
            "HyprDuck: loading context",
        ),
        ("UserPromptSubmit", None, "HyprDuck: checking context"),
        (
            "PreToolUse",
            Some("mcp__hyprduck__.*|apply_patch|Edit|Write"),
            "HyprDuck: checking automation policy",
        ),
        (
            "PermissionRequest",
            Some("mcp__hyprduck__.*"),
            "HyprDuck: checking MCP approval policy",
        ),
        (
            "PostToolUse",
            Some("mcp__hyprduck__.*|apply_patch|Edit|Write"),
            "HyprDuck: recording workflow guidance",
        ),
        ("Stop", None, "HyprDuck: finalizing hook state"),
    ] {
        let mut group = json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "statusMessage": status
            }]
        });
        if let Some(matcher) = matcher {
            group["matcher"] = json!(matcher);
        }
        replace_hyprduck_group(hooks, event, group)?;
    }

    let pretty = serde_json::to_string_pretty(&root)?;
    write_file_atomically(path, format!("{pretty}\n"))?;
    Ok(())
}

fn replace_hyprduck_group(hooks: &mut Map<String, Value>, event: &str, group: Value) -> Result<()> {
    let entries = hooks.entry(event).or_insert_with(|| Value::Array(Vec::new()));
    let array = entries
        .as_array_mut()
        .ok_or_else(|| anyhow!("hooks.{event} must be an array"))?;
    array.retain(|entry| !is_hyprduck_managed_group(entry));
    array.push(group);
    Ok(())
}

fn merge_config_toml(path: &Path) -> Result<()> {
    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    let stripped = remove_managed_block(&existing);
    let block = codex_mcp_approval_block();
    write_file_atomically(path, format!("{}{}", stripped.trim_end(), block))?;
    Ok(())
}

struct ConfigCheck {
    configured: bool,
    valid: bool,
    error: Option<String>,
}

impl ConfigCheck {
    fn missing() -> Self {
        Self {
            configured: false,
            valid: false,
            error: None,
        }
    }

    fn valid(configured: bool) -> Self {
        Self {
            configured,
            valid: true,
            error: None,
        }
    }

    fn invalid(error: String) -> Self {
        Self {
            configured: false,
            valid: false,
            error: Some(error),
        }
    }
}

fn codex_hooks_config_check(path: &Path) -> Result<ConfigCheck> {
    if !path.exists() {
        return Ok(ConfigCheck::missing());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let root = match serde_json::from_str::<Value>(&text) {
        Ok(root) => root,
        Err(error) => return Ok(ConfigCheck::invalid(error.to_string())),
    };
    let configured = root
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| {
            hooks.values().any(|entries| {
                entries
                    .as_array()
                    .is_some_and(|array| array.iter().any(is_hyprduck_managed_group))
            })
        });
    Ok(ConfigCheck::valid(configured))
}

fn codex_approvals_config_check(path: &Path) -> Result<ConfigCheck> {
    if !path.exists() {
        return Ok(ConfigCheck::missing());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let block = match managed_config_block(&text) {
        Ok(Some(block)) => block,
        Ok(None) => return Ok(ConfigCheck::valid(false)),
        Err(error) => return Ok(ConfigCheck::invalid(error.to_string())),
    };
    let automatic_tools_configured = AUTOMATIC_MCP_TOOLS.iter().all(|tool| {
        block.contains(&format!(
            "mcp_servers.hyprduck.tools.{tool}.approval_mode = \"auto\""
        ))
    });
    let destructive_tools_prompted =
        block.contains("mcp_servers.hyprduck.tools.write_reject.approval_mode = \"prompt\"");
    Ok(ConfigCheck::valid(
        automatic_tools_configured && destructive_tools_prompted,
    ))
}

fn managed_config_block(text: &str) -> Result<Option<String>> {
    let Some(start) = text.find(CONFIG_BLOCK_START) else {
        return Ok(None);
    };
    let block_start = start + CONFIG_BLOCK_START.len();
    let Some(relative_end) = text[block_start..].find(CONFIG_BLOCK_END) else {
        return Err(anyhow!("HyprDuck managed config block is missing its end marker"));
    };
    let end = block_start + relative_end;
    Ok(Some(text[block_start..end].to_string()))
}

fn is_hyprduck_managed_group(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_hyprduck_managed_hook))
}

fn is_hyprduck_managed_hook(hook: &Value) -> bool {
    let command_is_managed = hook
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.ends_with("hooks run codex") || command.contains(" hooks run codex"));
    let status_is_managed = hook
        .get("statusMessage")
        .and_then(Value::as_str)
        .is_some_and(|status| status.starts_with(HYPRDUCK_STATUS_PREFIX));
    command_is_managed || status_is_managed
}

fn remove_managed_block(text: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.trim() == CONFIG_BLOCK_START {
            skipping = true;
            continue;
        }
        if line.trim() == CONFIG_BLOCK_END {
            skipping = false;
            continue;
        }
        if !skipping {
            output.push(line);
        }
    }
    let mut joined = output.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

fn codex_mcp_approval_block() -> String {
    let mut block = format!("\n{CONFIG_BLOCK_START}\n");
    block.push_str("# HyprDuck hooks keep destructive removal/overwrite actions approval-gated.\n");
    for tool in AUTOMATIC_MCP_TOOLS {
        block.push_str(&format!(
            "mcp_servers.hyprduck.tools.{tool}.approval_mode = \"auto\"\n"
        ));
    }
    block.push_str("mcp_servers.hyprduck.tools.write_reject.approval_mode = \"prompt\"\n");
    block.push_str(CONFIG_BLOCK_END);
    block.push('\n');
    block
}

fn write_file_atomically(path: &Path, content: String) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temporary = path.with_file_name(format!(".{file_name}.hyprduck.tmp"));
    fs::write(&temporary, content)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| {
            let _ = fs::remove_file(&temporary);
            error
        })
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
