use std::path::{Component, Path};

use hyprduck_engine_types::{
    BrainRepoSnapshot, ReadPageEvidenceResponseData, ReadSourceResponseData, SourceRecord,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    match_score, search_terms, BrainNodeRecord, BrainRelationKind, BrainRelationRecord,
    ClaimRecord, EvidenceRef,
};

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
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
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
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
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
            && index
                .checked_sub(1)
                .map(|prev| bytes[prev] != b':')
                .unwrap_or(true)
    })
}

fn has_unix_absolute_path(normalized: &str) -> bool {
    let bytes = normalized.as_bytes();
    bytes.windows(2).enumerate().any(|(index, window)| {
        window[0] == b'/' && window[1] != b'/' && path_token_starts_at(bytes, index)
    })
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
// Enrichment / expand helpers were moved here (Phase 1 continuation) so they
// accept snapshot data only (not &BrainReader) for reuse from DB projections.
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

// -----------------------------------------------------------------------------
// Local-path enrichment + expansion for include_local_paths reads.
// These operate on snapshot data (sources/evidence carrying the raw paths from
// artifact) + workspace root to produce absolute local paths in responses.
// They were extracted from brain_read_service so the policy layer owns the
// agent read contract and the helpers no longer require a BrainReader.
// -----------------------------------------------------------------------------

pub(crate) fn enrich_read_source_with_local_paths(
    response: &mut ReadSourceResponseData,
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
) {
    if let Some(source) = snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.id.as_str()) {
            evidence.source_path = raw.source_path.clone();
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

pub(crate) fn expand_read_source_local_paths(
    response: &mut ReadSourceResponseData,
    workspace_root: &Path,
) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    for evidence in &mut response.evidence {
        let source_id = evidence
            .source_id
            .as_deref()
            .unwrap_or(response.source.source_id.as_str());
        expand_optional_path(
            &mut evidence.source_path,
            workspace_root,
            &["sources", source_id],
        );
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

pub(crate) fn enrich_page_evidence_with_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    snapshot: &BrainRepoSnapshot,
    source_id: &str,
) {
    if let Some(source) = snapshot
        .sources
        .iter()
        .find(|source| source.source_id == source_id)
    {
        response.source.original_path = source.original_path.clone();
        response.source.source_path = source.source_path.clone();
        response.source.markdown_path = source.markdown_path.clone();
    }

    let evidence_by_id = snapshot
        .evidence
        .iter()
        .map(|evidence| (evidence.id.as_str(), evidence))
        .collect::<BTreeMap<_, _>>();
    for evidence in &mut response.evidence {
        if let Some(raw) = evidence_by_id.get(evidence.evidence_ref.as_str()) {
            evidence.markdown_path = raw.markdown_path.clone();
            evidence.image_path = raw.image_path.clone();
        }
    }
}

pub(crate) fn expand_page_evidence_local_paths(
    response: &mut ReadPageEvidenceResponseData,
    workspace_root: &Path,
) {
    expand_source_record_local_paths(&mut response.source, workspace_root);
    let source_id = response.source.source_id.as_str();
    for evidence in &mut response.evidence {
        expand_optional_path(
            &mut evidence.markdown_path,
            workspace_root,
            &["artifacts", source_id, "pages"],
        );
        expand_optional_path(
            &mut evidence.image_path,
            workspace_root,
            &["artifacts", source_id, "images"],
        );
    }
}

fn expand_source_record_local_paths(source: &mut SourceRecord, workspace_root: &Path) {
    expand_string_path(&mut source.original_path, workspace_root, &[]);
    expand_string_path(
        &mut source.source_path,
        workspace_root,
        &["sources", source.source_id.as_str()],
    );
    expand_string_path(
        &mut source.markdown_path,
        workspace_root,
        &["artifacts", source.source_id.as_str()],
    );
}

fn expand_optional_path(value: &mut Option<String>, workspace_root: &Path, segments: &[&str]) {
    if let Some(path) = value {
        expand_string_path(path, workspace_root, segments);
    }
}

fn expand_string_path(value: &mut String, workspace_root: &Path, segments: &[&str]) {
    if value.is_empty() || value == "[redacted-local-path]" || Path::new(value).is_absolute() {
        return;
    }
    let mut path = workspace_root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    path.push(value.as_str());
    *value = path.to_string_lossy().into_owned();
}

/// Graph trail warning helper centralized in policy (moved from context_pack_service
/// during Phase 3 DB-first cleanup; reusable, matches prior duplicate in knowledge_store).
pub(crate) fn graph_trail_unavailable_warning(
    message: &str,
) -> hyprduck_engine_types::ContextPackWarningV0 {
    hyprduck_engine_types::ContextPackWarningV0 {
        warning_type: "graph_trail_unavailable".into(),
        severity: hyprduck_engine_types::ContextPackWarningSeverity::Low,
        message: message.into(),
        page_refs: Vec::new(),
    }
}

// -----------------------------------------------------------------------------
// Legacy snapshot context pack assembly helpers (Phase 4 reader shrink continuation)
// Moved out of domains/brain/reader.rs so the legacy search + context_pack*
// assembly can be maintained without bloating the reader. These are only for the
// artifact/snapshot fallback path (selected_node case + when DB assemble unavailable).
// The primary path uses KnowledgeStore::assemble_context_pack_v1_from_db + retrieval.
// No behavior change; only relocation + comments.
// -----------------------------------------------------------------------------
const DEFAULT_CONTEXT_PACK_EVIDENCE_LIMIT: usize = 15;
const SMALL_CONTEXT_PACK_EVIDENCE_LIMIT: usize = 8;
const DEFAULT_CONTEXT_PACK_GRAPH_FACT_LIMIT: usize = 12;
const SMALL_CONTEXT_PACK_GRAPH_FACT_LIMIT: usize = 5;
const SMALL_CONTEXT_PACK_BUDGET_THRESHOLD: usize = 4_000;

fn context_pack_evidence_limit(budget: usize) -> usize {
    if budget <= SMALL_CONTEXT_PACK_BUDGET_THRESHOLD {
        SMALL_CONTEXT_PACK_EVIDENCE_LIMIT
    } else {
        DEFAULT_CONTEXT_PACK_EVIDENCE_LIMIT
    }
}

fn context_pack_graph_fact_limit(budget: usize) -> usize {
    if budget <= SMALL_CONTEXT_PACK_BUDGET_THRESHOLD {
        SMALL_CONTEXT_PACK_GRAPH_FACT_LIMIT
    } else {
        DEFAULT_CONTEXT_PACK_GRAPH_FACT_LIMIT
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cap_context_pack_records(
    query: &str,
    budget: usize,
    selected_bias_node_ids: &BTreeSet<String>,
    selected_bias_evidence_ids: &BTreeSet<String>,
    nodes: &mut Vec<BrainNodeRecord>,
    claims: &mut Vec<ClaimRecord>,
    relations: &mut Vec<BrainRelationRecord>,
    evidence: &mut Vec<EvidenceRef>,
) {
    let terms = search_terms(query);
    evidence.sort_by(|left, right| {
        context_pack_evidence_score(right, &terms, selected_bias_evidence_ids)
            .cmp(&context_pack_evidence_score(
                left,
                &terms,
                selected_bias_evidence_ids,
            ))
            .then(left.id.cmp(&right.id))
    });
    evidence.truncate(context_pack_evidence_limit(budget));
    let selected_evidence_ids = evidence
        .iter()
        .map(|evidence| evidence.id.clone())
        .collect::<BTreeSet<_>>();

    claims.retain(|claim| {
        claim
            .evidence_refs
            .iter()
            .any(|evidence_ref| selected_evidence_ids.contains(evidence_ref))
    });
    relations.retain(|relation| {
        relation
            .evidence_ids
            .iter()
            .any(|evidence_id| selected_evidence_ids.contains(evidence_id))
    });

    claims.sort_by(|left, right| {
        context_pack_claim_score(
            right,
            &terms,
            &selected_evidence_ids,
            selected_bias_node_ids,
        )
        .cmp(&context_pack_claim_score(
            left,
            &terms,
            &selected_evidence_ids,
            selected_bias_node_ids,
        ))
        .then(left.claim_id.cmp(&right.claim_id))
    });
    let graph_fact_limit = context_pack_graph_fact_limit(budget);
    claims.truncate(graph_fact_limit);
    let relation_limit = graph_fact_limit.saturating_sub(claims.len());
    relations.sort_by(|left, right| {
        context_pack_relation_score(right, &selected_evidence_ids, selected_bias_node_ids)
            .cmp(&context_pack_relation_score(
                left,
                &selected_evidence_ids,
                selected_bias_node_ids,
            ))
            .then(left.relation_id.cmp(&right.relation_id))
    });
    relations.truncate(relation_limit);

    let mut required_node_ids = claims
        .iter()
        .flat_map(|claim| claim.topic_refs.iter().cloned())
        .collect::<BTreeSet<_>>();
    for relation in relations.iter() {
        required_node_ids.insert(relation.source_node_id.clone());
        required_node_ids.insert(relation.target_node_id.clone());
    }
    for node in nodes.iter() {
        if node
            .evidence_ids
            .iter()
            .any(|evidence_id| selected_evidence_ids.contains(evidence_id))
        {
            required_node_ids.insert(node.node_id.clone());
        }
    }
    nodes.retain(|node| required_node_ids.contains(&node.node_id));
    nodes.sort_by(|left, right| {
        context_pack_node_score(
            right,
            &terms,
            &selected_evidence_ids,
            selected_bias_node_ids,
        )
        .cmp(&context_pack_node_score(
            left,
            &terms,
            &selected_evidence_ids,
            selected_bias_node_ids,
        ))
        .then(left.node_id.cmp(&right.node_id))
    });
    nodes.truncate(graph_fact_limit.saturating_mul(2).max(1));
}

fn context_pack_evidence_score(
    evidence: &EvidenceRef,
    terms: &[String],
    selected_bias_evidence_ids: &BTreeSet<String>,
) -> usize {
    match_score(terms, &evidence.snippet).unwrap_or(0)
        + evidence
            .source_id
            .as_ref()
            .and_then(|source_id| match_score(terms, source_id))
            .unwrap_or(0)
        + if selected_bias_evidence_ids.contains(&evidence.id) {
            10_000
        } else {
            0
        }
}

fn context_pack_claim_score(
    claim: &ClaimRecord,
    terms: &[String],
    selected_evidence_ids: &BTreeSet<String>,
    selected_bias_node_ids: &BTreeSet<String>,
) -> usize {
    let selected_evidence_count = claim
        .evidence_refs
        .iter()
        .filter(|evidence_id| selected_evidence_ids.contains(*evidence_id))
        .count();
    let selected_node_bias = claim
        .topic_refs
        .iter()
        .any(|node_id| selected_bias_node_ids.contains(node_id));
    selected_evidence_count * 100
        + match_score(terms, &claim.statement).unwrap_or(0)
        + if selected_node_bias { 10_000 } else { 0 }
}

fn context_pack_relation_score(
    relation: &BrainRelationRecord,
    selected_evidence_ids: &BTreeSet<String>,
    selected_bias_node_ids: &BTreeSet<String>,
) -> usize {
    let selected_evidence_count = relation
        .evidence_ids
        .iter()
        .filter(|evidence_id| selected_evidence_ids.contains(*evidence_id))
        .count();
    let kind_score = match relation.kind {
        BrainRelationKind::RelatedTo | BrainRelationKind::SourceOf => 1,
        _ => 4,
    };
    let selected_node_bias = selected_bias_node_ids.contains(&relation.source_node_id)
        || selected_bias_node_ids.contains(&relation.target_node_id);
    selected_evidence_count * 100 + kind_score + if selected_node_bias { 10_000 } else { 0 }
}

fn context_pack_node_score(
    node: &BrainNodeRecord,
    terms: &[String],
    selected_evidence_ids: &BTreeSet<String>,
    selected_bias_node_ids: &BTreeSet<String>,
) -> usize {
    let selected_evidence_count = node
        .evidence_ids
        .iter()
        .filter(|evidence_id| selected_evidence_ids.contains(*evidence_id))
        .count();
    selected_evidence_count * 100
        + match_score(terms, &format!("{} {}", node.label, node.aliases.join(" "))).unwrap_or(0)
        + if selected_bias_node_ids.contains(&node.node_id) {
            10_000
        } else {
            0
        }
}
