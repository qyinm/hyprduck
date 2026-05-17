use std::collections::{BTreeMap, BTreeSet};

use hyprduck_engine_types::{BrainNodeRecord, EvidenceRef, WikiPage};

pub(crate) fn search_terms(query: &str) -> Vec<String> {
    query
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_search_token)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let frequencies = search_token_frequencies(haystack);
    let mut matched_terms = 0usize;
    let mut score = 0usize;
    for term in terms {
        if let Some(frequency) = frequencies.get(term) {
            matched_terms += 1;
            score += 8 + frequency.saturating_mul(2);
            continue;
        }
        if term.len() > 3
            && frequencies
                .keys()
                .any(|token| token.starts_with(term) || term.starts_with(token))
        {
            matched_terms += 1;
            score += 3;
        }
    }
    score += matched_terms.saturating_mul(matched_terms);
    if matched_terms == terms.len() {
        score += 10;
    }
    (score > 0).then_some(score)
}

pub(crate) fn evidence_snippet(evidence_ids: &[String]) -> String {
    if evidence_ids.is_empty() {
        return "evidence: none".into();
    }
    format!("evidence: {}", evidence_ids.join(", "))
}

pub(crate) fn search_token_frequencies(text: &str) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for token in text
        .split(|char: char| !char.is_ascii_alphanumeric())
        .filter_map(normalize_search_token)
    {
        *frequencies.entry(token).or_insert(0) += 1;
    }
    frequencies
}

pub(crate) fn normalize_search_token(raw: &str) -> Option<String> {
    let mut token = raw.trim().to_ascii_lowercase();
    if token.len() <= 1 {
        return None;
    }
    if token.ends_with("ies") && token.len() > 4 {
        token.truncate(token.len() - 3);
        token.push('y');
    } else if token.ends_with("ing") && token.len() > 5 {
        token.truncate(token.len() - 3);
    } else if (token.ends_with("ed") || token.ends_with("es") && !token.ends_with("ses"))
        && token.len() > 4
    {
        token.truncate(token.len() - 2);
    } else if token.ends_with('s')
        && token.len() > 4
        && !token.ends_with("ss")
        && !token.ends_with("us")
    {
        token.truncate(token.len() - 1);
    }
    (token.len() > 1).then_some(token)
}

pub(crate) fn best_snippet(text: &str, terms: &[String]) -> String {
    let lower = text.to_ascii_lowercase();
    let start = terms
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0)
        .saturating_sub(48);
    text.chars().skip(start).take(180).collect()
}

pub(crate) fn context_pack_warnings(
    nodes: &[BrainNodeRecord],
    evidence: &[EvidenceRef],
    budget: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if nodes.is_empty() {
        warnings.push(
            "No graph nodes matched the query; pack falls back to workspace wiki pages.".into(),
        );
    }
    if evidence.is_empty() {
        warnings.push("No direct evidence refs matched the query.".into());
    }
    if nodes
        .iter()
        .any(|node| node.confidence.unwrap_or(1.0) < 0.6)
    {
        warnings.push("Some selected nodes have low confidence.".into());
    }
    if budget < 2000 {
        warnings.push("Small budget may omit relevant wiki pages or graph context.".into());
    }
    warnings
}

pub(crate) fn trim_context_pack_to_budget(
    budget: usize,
    wiki_pages: &mut [WikiPage],
    nodes: &mut Vec<BrainNodeRecord>,
) {
    let mut remaining_chars = budget.saturating_mul(4);
    for page in wiki_pages.iter_mut() {
        if page.body.len() > remaining_chars {
            page.body = page.body.chars().take(remaining_chars).collect();
            remaining_chars = 0;
        } else {
            remaining_chars = remaining_chars.saturating_sub(page.body.len());
        }
    }
    if remaining_chars == 0 {
        nodes.truncate(nodes.len().min(3));
    }
}
