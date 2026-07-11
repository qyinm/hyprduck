use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

pub(crate) const ROOT_DIR_ENV: &str = "ETYMA_MCP_ALLOW_ROOT_DIR";
pub(crate) const ROOT_DIR_ALLOWED_ROOTS_ENV: &str = "ETYMA_MCP_ALLOWED_ROOTS";
pub(crate) const IMPORT_ALLOWED_ROOTS_ENV: &str = "ETYMA_MCP_ALLOWED_IMPORT_ROOTS";

pub(crate) fn validate_root_dir_argument(root_dir: &str) -> Result<String> {
    if !root_dir_argument_allowed() {
        return Err(anyhow!(
            "rootDir is disabled by default; set ETYMA_MCP_ALLOW_ROOT_DIR=1 and ETYMA_MCP_ALLOWED_ROOTS for development roots"
        ));
    }
    let canonical_root_dir = canonicalize_mcp_root(root_dir)?;
    let allowed_roots = allowed_root_dirs()?;
    if allowed_roots
        .iter()
        .any(|allowed_root| canonical_root_dir.starts_with(allowed_root))
    {
        return canonical_root_dir
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow!("rootDir must be valid UTF-8 after canonicalization"));
    }
    Err(anyhow!("rootDir is not in ETYMA_MCP_ALLOWED_ROOTS"))
}

pub(crate) fn validate_import_source_path(raw_path: &str) -> Result<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("argument sourcePath cannot be empty"));
    }

    let source = PathBuf::from(trimmed)
        .canonicalize()
        .with_context(|| "sourcePath does not exist or cannot be read")?;
    if !source.is_file() {
        return Err(anyhow!("sourcePath must point to a regular file"));
    }

    let roots = allowed_import_root_dirs()?;
    if roots.iter().any(|root| source.starts_with(root)) {
        Ok(source)
    } else {
        Err(anyhow!(
            "sourcePath is outside ETYMA_MCP_ALLOWED_IMPORT_ROOTS"
        ))
    }
}

pub(crate) fn redact_local_paths(value: Value) -> Value {
    redact_local_paths_with_key(None, value)
}

pub(crate) fn redact_local_path_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let remaining = &value[cursor..];
        let Some((relative_start, prefix_len)) = next_local_path_start(remaining) else {
            output.push_str(remaining);
            break;
        };
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        output.push_str("[redacted-local-path]");
        let path_start = start + prefix_len;
        let path_end = value[path_start..]
            .find(is_local_path_delimiter)
            .map(|offset| path_start + offset)
            .unwrap_or(value.len());
        cursor = path_end;
    }
    output
}

fn root_dir_argument_allowed() -> bool {
    std::env::var(ROOT_DIR_ENV).is_ok_and(|value| value == "1")
}

fn allowed_root_dirs() -> Result<Vec<PathBuf>> {
    let raw = std::env::var_os(ROOT_DIR_ALLOWED_ROOTS_ENV).ok_or_else(|| {
        anyhow!("rootDir requires ETYMA_MCP_ALLOWED_ROOTS to name approved roots")
    })?;
    let roots = std::env::split_paths(&raw)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| canonicalize_mcp_root(path))
        .collect::<Result<Vec<_>>>()?;
    if roots.is_empty() {
        return Err(anyhow!(
            "rootDir requires ETYMA_MCP_ALLOWED_ROOTS to name approved roots"
        ));
    }
    Ok(roots)
}

fn allowed_import_root_dirs() -> Result<Vec<PathBuf>> {
    let raw = std::env::var_os(IMPORT_ALLOWED_ROOTS_ENV).ok_or_else(|| {
        anyhow!("MCP import is disabled: set ETYMA_MCP_ALLOWED_IMPORT_ROOTS to one or more approved roots")
    })?;
    let roots = std::env::split_paths(&raw)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| {
            let root = path
                .canonicalize()
                .with_context(|| "allowed import root does not exist or cannot be read")?;
            if !root.is_dir() {
                return Err(anyhow!("allowed import root must be a directory"));
            }
            Ok(root)
        })
        .collect::<Result<Vec<_>>>()?;
    if roots.is_empty() {
        return Err(anyhow!(
            "MCP import is disabled: no allowed import roots configured"
        ));
    }
    Ok(roots)
}

fn canonicalize_mcp_root(path: impl AsRef<Path>) -> Result<PathBuf> {
    path.as_ref()
        .canonicalize()
        .map_err(|_| anyhow!("rootDir must exist and be canonicalizable"))
}

fn redact_local_paths_with_key(key: Option<&str>, value: Value) -> Value {
    match value {
        Value::String(value) if should_redact_path_field(key, &value) => {
            Value::String("[redacted-local-path]".into())
        }
        Value::String(value) if is_absolute_local_path(&value) => {
            Value::String("[redacted-local-path]".into())
        }
        Value::String(value) => Value::String(redact_local_path_text(&value)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_local_paths_with_key(key, value))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let redacted = redact_local_paths_with_key(Some(&key), value);
                    (key, redacted)
                })
                .collect(),
        ),
        value => value,
    }
}

fn should_redact_path_field(key: Option<&str>, value: &str) -> bool {
    let Some(key) = key else {
        return false;
    };
    if value.trim().is_empty() {
        return false;
    }
    matches!(
        key,
        "originalPath"
            | "sourcePath"
            | "markdownPath"
            | "imagePath"
            | "artifactRoot"
            | "manifestPath"
            | "sourcePaths"
            | "persistedContextPackPath"
    )
}

fn next_local_path_start(value: &str) -> Option<(usize, usize)> {
    for (index, _) in value.char_indices() {
        let candidate = &value[index..];
        if !is_local_path_start_boundary(value, index) {
            continue;
        }
        if candidate.starts_with("file:///") {
            return Some((index, "file://".len()));
        }
        if candidate.starts_with('/') || candidate.starts_with("~/") {
            return Some((index, 0));
        }
        if candidate
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
        {
            return Some((index, 0));
        }
    }
    None
}

fn is_local_path_start_boundary(value: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    value[..index]
        .chars()
        .next_back()
        .is_some_and(is_local_path_delimiter)
}

fn is_local_path_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '(' | '[' | '{' | '<' | ')' | ']' | '}' | '>' | '"' | '\'' | '`' | ',' | ';'
        )
}

fn is_absolute_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("~/")
        || value
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
}
