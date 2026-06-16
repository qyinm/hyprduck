use std::path::{Component, Path};

use hyprduck_engine_types::{ReadPageEvidenceResponseData, ReadSourceResponseData, SourceRecord};

pub(crate) fn redact_path_for_agent(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    Path::new(trimmed)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "<redacted>".into())
}

/// Returns true if the value is safe to expose to an agent (MCP, CLI, etc.).
/// Rejects empty, absolute paths, parent-dir components, home (~), UNC (//), Windows drive letters,
/// and known forbidden markers (docs/private, file://, etc.).
pub(crate) fn is_agent_text_safe(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let normalized = value.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if has_home_path(&lower)
        || has_windows_absolute_path(&normalized)
        || has_unix_absolute_path(&normalized)
        || has_forbidden_path_marker(&lower)
    {
        return false;
    }
    let path = Path::new(&normalized);
    !path.is_absolute() && path.components().all(|component| !matches!(component, Component::ParentDir))
}

/// Variant for wiki paths that must start with "wiki/" and otherwise pass the normal agent text safety rules.
pub(crate) fn is_safe_agent_wiki_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if !lower.starts_with("wiki/") || has_forbidden_path_marker(&lower) {
        return false;
    }
    let path_after_prefix = &normalized["wiki/".len()..];
    let lower_after_prefix = &lower["wiki/".len()..];
    if has_home_path(lower_after_prefix)
        || has_windows_absolute_path(path_after_prefix)
        || has_unix_absolute_path(path_after_prefix)
    {
        return false;
    }
    let path = Path::new(&normalized);
    !path.is_absolute() && path.components().all(|component| !matches!(component, Component::ParentDir))
}

fn has_home_path(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    bytes
        .windows(2)
        .enumerate()
        .any(|(index, window)| window == b"~/" && path_token_starts_at(bytes, index))
}

fn has_windows_absolute_path(normalized: &str) -> bool {
    let bytes = normalized.as_bytes();
    has_unc_path(normalized)
        || bytes.windows(3).enumerate().any(|(index, window)| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && window[2] == b'/'
                && path_token_starts_at(bytes, index)
        })
}

fn has_unc_path(normalized: &str) -> bool {
    let bytes = normalized.as_bytes();
    bytes.windows(2).enumerate().any(|(index, window)| {
        window == b"//"
            && path_token_starts_at(bytes, index)
            && index.checked_sub(1).map(|prev| bytes[prev] != b':').unwrap_or(true)
    })
}

fn has_unix_absolute_path(normalized: &str) -> bool {
    let bytes = normalized.as_bytes();
    bytes
        .windows(2)
        .enumerate()
        .any(|(index, window)| window[0] == b'/' && window[1] != b'/' && path_token_starts_at(bytes, index))
}

fn path_token_starts_at(bytes: &[u8], index: usize) -> bool {
    index == 0
        || bytes[index - 1].is_ascii_whitespace()
        || matches!(
            bytes[index - 1],
            b'(' | b'[' | b'{' | b'<' | b'"' | b'\'' | b'=' | b':'
        )
}

fn has_forbidden_path_marker(lower: &str) -> bool {
    lower.contains("docs/private")
        || lower.contains("docs%2fprivate")
        || lower.contains("docs%5cprivate")
        || lower.contains("file://")
        || lower.contains("../")
        || lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
}

// -----------------------------------------------------------------------------
// Agent redaction / enrichment policy (centralized here alongside safety checks).
// These were previously duplicated in brain_read_service.rs and tied to the
// legacy BrainReader path. Moving them makes the "what an agent is allowed to see"
// contract canonical and reusable from both DB and artifact paths.
// -----------------------------------------------------------------------------

/// Redact a single path for agent consumption (same logic as the basic one,
/// provided here for convenience and to keep all redaction in one place).
pub(crate) fn redact_agent_path(value: &str) -> String {
    redact_path_for_agent(value)
}

pub(crate) fn redact_optional_agent_path(value: &mut Option<String>) {
    if let Some(path) = value {
        *path = redact_agent_path(path);
    }
}

pub(crate) fn redact_source_record_agent_paths(source: &mut SourceRecord) {
    source.original_path = redact_agent_path(&source.original_path);
    source.source_path = redact_agent_path(&source.source_path);
    source.markdown_path = redact_agent_path(&source.markdown_path);
}

pub(crate) fn redact_read_source_agent_paths(response: &mut ReadSourceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.source_path);
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}

pub(crate) fn redact_page_evidence_agent_paths(response: &mut ReadPageEvidenceResponseData) {
    redact_source_record_agent_paths(&mut response.source);
    for evidence in &mut response.evidence {
        redact_optional_agent_path(&mut evidence.markdown_path);
        redact_optional_agent_path(&mut evidence.image_path);
    }
}
