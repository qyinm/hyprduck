use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcp::automation_policy::AUTOMATIC_MCP_TOOLS;

const HOOKS_FILE: &str = "hooks.json";
const CONFIG_FILE: &str = "config.toml";
const CONFIG_BLOCK_START: &str = "# BEGIN HYPRDUCK HOOK AUTOMATION";
const CONFIG_BLOCK_END: &str = "# END HYPRDUCK HOOK AUTOMATION";
const HYPRDUCK_STATUS_PREFIX: &str = "HyprDuck:";

#[derive(Clone, Copy)]
struct CodexHookSpec {
    event: &'static str,
    matcher: Option<&'static str>,
    status: &'static str,
}

const CODEX_HOOK_SPECS: &[CodexHookSpec] = &[
    CodexHookSpec {
        event: "SessionStart",
        matcher: Some("startup|resume|clear|compact"),
        status: "HyprDuck: loading context",
    },
    CodexHookSpec {
        event: "UserPromptSubmit",
        matcher: None,
        status: "HyprDuck: checking context",
    },
    CodexHookSpec {
        event: "PreToolUse",
        matcher: Some("mcp__hyprduck__.*|apply_patch|Edit|Write"),
        status: "HyprDuck: checking automation policy",
    },
    CodexHookSpec {
        event: "PermissionRequest",
        matcher: Some("mcp__hyprduck__.*"),
        status: "HyprDuck: checking MCP approval policy",
    },
    CodexHookSpec {
        event: "PostToolUse",
        matcher: Some("mcp__hyprduck__.*|apply_patch|Edit|Write"),
        status: "HyprDuck: recording workflow guidance",
    },
    CodexHookSpec {
        event: "Stop",
        matcher: None,
        status: "HyprDuck: finalizing hook state",
    },
];

#[derive(Clone, Copy)]
struct CodexApprovalSpec {
    tool: &'static str,
    approval_mode: &'static str,
}

impl CodexApprovalSpec {
    fn toml_line(self) -> String {
        format!(
            "mcp_servers.hyprduck.tools.{}.approval_mode = \"{}\"",
            self.tool, self.approval_mode
        )
    }
}

fn codex_approval_specs() -> impl Iterator<Item = CodexApprovalSpec> {
    AUTOMATIC_MCP_TOOLS
        .iter()
        .copied()
        .map(|tool| CodexApprovalSpec {
            tool,
            approval_mode: "auto",
        })
        .chain(std::iter::once(CodexApprovalSpec {
            tool: "write_reject",
            approval_mode: "prompt",
        }))
}

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

    for spec in CODEX_HOOK_SPECS {
        replace_hyprduck_group(hooks, spec, codex_hook_group(*spec, command))?;
    }

    let pretty = serde_json::to_string_pretty(&root)?;
    write_file_atomically(path, format!("{pretty}\n"))?;
    Ok(())
}

fn codex_hook_group(spec: CodexHookSpec, command: &str) -> Value {
    let mut group = json!({
        "hooks": [{
            "type": "command",
            "command": command,
            "statusMessage": spec.status
        }]
    });
    if let Some(matcher) = spec.matcher {
        group["matcher"] = json!(matcher);
    }
    group
}

fn replace_hyprduck_group(
    hooks: &mut Map<String, Value>,
    spec: &CodexHookSpec,
    group: Value,
) -> Result<()> {
    let entries = hooks
        .entry(spec.event)
        .or_insert_with(|| Value::Array(Vec::new()));
    let array = entries
        .as_array_mut()
        .ok_or_else(|| anyhow!("hooks.{} must be an array", spec.event))?;
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
    let stripped = remove_managed_block(&existing)?;
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
            CODEX_HOOK_SPECS.iter().all(|spec| {
                hooks
                    .get(spec.event)
                    .and_then(Value::as_array)
                    .is_some_and(|array| {
                        array
                            .iter()
                            .any(|entry| is_hyprduck_managed_group_for_spec(entry, spec))
                    })
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
    Ok(ConfigCheck::valid(
        codex_approval_specs().all(|spec| block.contains(&spec.toml_line())),
    ))
}

struct ManagedConfigBlockSpan {
    full_start: usize,
    full_end: usize,
    content_start: usize,
    content_end: usize,
}

fn managed_config_block(text: &str) -> Result<Option<&str>> {
    let Some(span) = managed_config_block_span(text)? else {
        return Ok(None);
    };
    Ok(Some(&text[span.content_start..span.content_end]))
}

fn managed_config_block_span(text: &str) -> Result<Option<ManagedConfigBlockSpan>> {
    let Some(full_start) = text.find(CONFIG_BLOCK_START) else {
        return Ok(None);
    };
    let content_start = full_start + CONFIG_BLOCK_START.len();
    let Some(relative_end) = text[content_start..].find(CONFIG_BLOCK_END) else {
        return Err(anyhow!(
            "HyprDuck managed config block is missing its end marker"
        ));
    };
    let content_end = content_start + relative_end;
    Ok(Some(ManagedConfigBlockSpan {
        full_start,
        full_end: content_end + CONFIG_BLOCK_END.len(),
        content_start,
        content_end,
    }))
}

fn is_hyprduck_managed_group(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_hyprduck_managed_hook))
}

fn is_hyprduck_managed_group_for_spec(entry: &Value, spec: &CodexHookSpec) -> bool {
    let matcher_matches = match (spec.matcher, entry.get("matcher").and_then(Value::as_str)) {
        (Some(expected), Some(actual)) => expected == actual,
        (None, None) => true,
        _ => false,
    };
    matcher_matches
        && entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks
                    .iter()
                    .any(|hook| is_hyprduck_managed_hook_for_spec(hook, spec))
            })
}

fn is_hyprduck_managed_hook(hook: &Value) -> bool {
    let command_is_managed = hook
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| {
            command.ends_with("hooks run codex") || command.contains(" hooks run codex")
        });
    let status_is_managed = hook
        .get("statusMessage")
        .and_then(Value::as_str)
        .is_some_and(|status| status.starts_with(HYPRDUCK_STATUS_PREFIX));
    command_is_managed || status_is_managed
}

fn is_hyprduck_managed_hook_for_spec(hook: &Value, spec: &CodexHookSpec) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| {
            command.ends_with("hooks run codex") || command.contains(" hooks run codex")
        })
        && hook
            .get("statusMessage")
            .and_then(Value::as_str)
            .is_some_and(|status| status == spec.status)
}

fn remove_managed_block(text: &str) -> Result<String> {
    let Some(span) = managed_config_block_span(text)? else {
        return Ok(text.to_string());
    };
    let mut output = String::with_capacity(text.len() - (span.full_end - span.full_start));
    output.push_str(&text[..span.full_start]);
    output.push_str(&text[span.full_end..]);
    Ok(output)
}

fn codex_mcp_approval_block() -> String {
    let mut block = format!("\n{CONFIG_BLOCK_START}\n");
    block.push_str("# HyprDuck hooks keep destructive removal/overwrite actions approval-gated.\n");
    for spec in codex_approval_specs() {
        block.push_str(&spec.toml_line());
        block.push('\n');
    }
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
