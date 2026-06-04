use std::path::Path;

use anyhow::{anyhow, Result};
use hyprduck_engine_types::{BrainReadScope, DocumentFormat};
use serde_json::{Map, Value};

use super::policy::validate_root_dir_argument;

pub(super) const PROPOSAL_ID_PATTERN: &str = "^prop-[0-9A-Fa-f]{32}$";
pub(super) const WRITE_CONTENT_TYPES: [&str; 3] = ["memory", "evidence_refresh", "link_repair"];

pub(super) fn read_scope(arguments: &Map<String, Value>) -> Result<BrainReadScope> {
    let root_dir = optional_string(arguments, "rootDir")?;
    let root_dir = root_dir
        .as_deref()
        .map(validate_root_dir_argument)
        .transpose()?;
    Ok(BrainReadScope {
        workspace_id: optional_string(arguments, "workspaceId")?
            .unwrap_or_else(|| "default".into()),
        root_dir,
    })
}

pub(super) fn import_document_format(
    path: &Path,
    explicit: Option<String>,
) -> Result<DocumentFormat> {
    let raw = explicit
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase())
        });
    match raw.as_deref() {
        Some("pdf") => Ok(DocumentFormat::Pdf),
        Some("docx") => Ok(DocumentFormat::Docx),
        Some("doc") => Ok(DocumentFormat::Doc),
        Some("md") | Some("markdown") => Ok(DocumentFormat::Markdown),
        Some("image") | Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("heic")
        | Some("tiff") => Ok(DocumentFormat::Image),
        Some(other) => Err(anyhow!(
            "unsupported import format: {other}; supported formats: pdf, docx, doc, markdown, image"
        )),
        None => Err(anyhow!(
            "cannot infer import format; pass format as pdf, docx, doc, markdown, or image"
        )),
    }
}

pub(super) fn required_string(arguments: &Map<String, Value>, name: &str) -> Result<String> {
    optional_string(arguments, name)?.ok_or_else(|| anyhow!("missing required argument: {name}"))
}

pub(super) fn optional_string(
    arguments: &Map<String, Value>,
    name: &str,
) -> Result<Option<String>> {
    match arguments.get(name) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(Value::String(_)) => Err(anyhow!("argument {name} cannot be empty")),
        Some(_) => Err(anyhow!("argument {name} must be a string")),
        None => Ok(None),
    }
}

pub(super) fn optional_usize(arguments: &Map<String, Value>, name: &str) -> Result<Option<usize>> {
    match arguments.get(name) {
        Some(Value::Number(value)) => value
            .as_u64()
            .map(|value| Some(value as usize))
            .ok_or_else(|| anyhow!("argument {name} must be a positive integer")),
        Some(Value::String(value)) => value
            .parse::<usize>()
            .map(Some)
            .map_err(|_| anyhow!("argument {name} must be a positive integer")),
        Some(_) => Err(anyhow!("argument {name} must be a positive integer")),
        None => Ok(None),
    }
}

pub(super) fn optional_bool(arguments: &Map<String, Value>, name: &str) -> Result<Option<bool>> {
    match arguments.get(name) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(anyhow!("argument {name} must be a boolean")),
        None => Ok(None),
    }
}

pub(super) fn required_string_array(
    arguments: &Map<String, Value>,
    name: &str,
) -> Result<Vec<String>> {
    let values: Vec<String> = match arguments.get(name) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(s) if !s.trim().is_empty() => Ok(s.clone()),
                Value::String(_) => Err(anyhow!("element in {name} cannot be empty")),
                _ => Err(anyhow!("each element in {name} must be a string")),
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err(anyhow!("argument {name} must be an array of strings")),
        None => return Err(anyhow!("missing required argument: {name}")),
    };
    if values.is_empty() {
        return Err(anyhow!("argument {name} must contain at least one item"));
    }
    Ok(values)
}

pub(super) fn validate_mcp_proposal_id(proposal_id: &str) -> Result<()> {
    let suffix = proposal_id
        .strip_prefix("prop-")
        .ok_or_else(|| anyhow!("invalid proposalId: expected prop-<32 hex chars>"))?;
    if suffix.len() != 32 || !suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid proposalId: expected prop-<32 hex chars>"));
    }
    Ok(())
}

pub(super) fn validate_mcp_write_content_type(content_type: &str) -> Result<()> {
    let content_type = content_type.trim();
    if WRITE_CONTENT_TYPES.contains(&content_type) {
        Ok(())
    } else {
        Err(anyhow!(
            "unsupported contentType {content_type}; supported contentTypes: {}",
            WRITE_CONTENT_TYPES.join(", ")
        ))
    }
}
