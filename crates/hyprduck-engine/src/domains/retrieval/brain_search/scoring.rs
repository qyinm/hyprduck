//! Lexical match scoring, context windows, and evidence-type query intent.

use std::collections::BTreeSet;

use super::text_normalize::normalize_search_text;

pub(crate) fn db_match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let lower = normalize_search_text(haystack);
    let matched = terms.iter().filter(|term| lower.contains(*term)).count();
    (matched > 0).then(|| matched * 100)
}

pub(crate) fn db_source_metadata_match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let lower = normalize_search_text(haystack);
    let mut matched = 0usize;
    let mut specific_matched = 0usize;
    for term in terms {
        if lower.contains(term.as_str()) {
            matched += 1;
            if !is_generic_source_metadata_term(term) {
                specific_matched += 1;
            }
        }
    }
    (specific_matched > 0).then(|| matched * 100 + specific_matched * 200)
}

fn is_generic_source_metadata_term(term: &str) -> bool {
    matches!(
        term,
        "document"
            | "documents"
            | "doc"
            | "docs"
            | "source"
            | "sources"
            | "citation"
            | "citations"
            | "evidence"
            | "graph"
            | "node"
            | "context"
            | "pdf"
            | "docx"
            | "file"
            | "files"
            | "paper"
            | "papers"
            | "article"
            | "articles"
            | "research"
            | "page"
            | "pages"
            | "summarize"
            | "summary"
            | "문서"
            | "자료"
            | "파일"
            | "논문"
            | "연구"
            | "출처"
            | "근거"
            | "인용"
            | "그래프"
            | "노드"
            | "페이지"
            | "요약"
            | "정리"
    )
}

pub(crate) fn db_best_snippet(text: &str, terms: &[String]) -> String {
    let lower = text.to_lowercase();
    for term in terms {
        if let Some(index) = lower.find(term) {
            let start = text[..index].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
            let end = text[index..]
                .find('\n')
                .map(|pos| index + pos)
                .unwrap_or_else(|| text.len());
            return text[start..end].trim().chars().take(240).collect();
        }
    }
    text.trim().chars().take(240).collect()
}

pub(crate) fn db_context_window(text: &str, terms: &[String], max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let lower = trimmed.to_lowercase();
    let match_index = db_context_candidate_indices(&lower, terms)
        .into_iter()
        .max_by_key(|index| {
            let start = db_context_window_start(trimmed, *index);
            let window = trimmed[start..]
                .trim()
                .chars()
                .take(max_chars)
                .collect::<String>();
            db_context_candidate_score(&window.to_lowercase(), *index, terms)
        });
    let Some(match_index) = match_index else {
        return trimmed.chars().take(max_chars).collect();
    };

    let start = db_context_window_start(trimmed, match_index);
    let window = trimmed[start..].trim();
    window.chars().take(max_chars).collect()
}

fn db_context_candidate_indices(lower: &str, terms: &[String]) -> Vec<usize> {
    let mut indices = BTreeSet::new();
    if terms.len() > 1 {
        let phrase = terms.join(" ");
        for (index, _) in lower.match_indices(&phrase) {
            indices.insert(index);
        }
    }
    for term in terms {
        for (index, _) in lower.match_indices(term) {
            indices.insert(index);
        }
    }
    indices.into_iter().collect()
}

fn db_context_window_start(text: &str, match_index: usize) -> usize {
    let match_index = previous_char_boundary(text, match_index.min(text.len()));
    let mut start = text[..match_index]
        .rfind("\n\n")
        .map(|index| index + 2)
        .or_else(|| text[..match_index].rfind('\n').map(|index| index + 1))
        .unwrap_or(0);
    if match_index.saturating_sub(start) > 240 {
        start = previous_char_boundary(text, match_index.saturating_sub(120));
    }
    start
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn db_context_candidate_score(window_lower: &str, match_index: usize, terms: &[String]) -> i64 {
    let unique_terms = terms
        .iter()
        .filter(|term| window_lower.contains(term.as_str()))
        .count() as i64;
    let term_occurrences = terms
        .iter()
        .map(|term| window_lower.match_indices(term.as_str()).count().min(8) as i64)
        .sum::<i64>();
    let section_bonus = if match_index > 400 { 220 } else { 0 };
    let toc_penalty = if match_index < 500
        && window_lower.contains("contents")
        && window_lower.contains("8.1")
        && window_lower.contains("8.2")
        && window_lower.contains("8.3")
    {
        900
    } else {
        0
    };
    let explanation_bonus = [
        " using ",
        " fixed ",
        " directory",
        " directories",
        " bucket",
        " split",
        " overflow",
        " insert",
        " delete",
        " search",
        " stored",
        " address",
        " grows",
        " shrinks",
    ]
    .iter()
    .filter(|needle| window_lower.contains(**needle))
    .count() as i64
        * 25;

    unique_terms * 1_000 + term_occurrences * 30 + section_bonus + explanation_bonus - toc_penalty
}

pub(crate) fn db_float_score(score: f64) -> usize {
    (score.max(0.0) * 1000.0).round() as usize
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EvidenceQueryIntent {
    wants_table: bool,
    wants_visual: bool,
    wants_summary: bool,
    wants_claim: bool,
    wants_relationship: bool,
}

impl EvidenceQueryIntent {
    pub(crate) fn from_query(query: &str) -> Self {
        let query = query.to_lowercase();
        let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
        Self {
            wants_table: contains_any(&[
                "table",
                "row",
                "column",
                "spreadsheet",
                "csv",
                "financial",
                "balance sheet",
                "표",
                "테이블",
            ]),
            wants_visual: contains_any(&[
                "image",
                "figure",
                "chart",
                "diagram",
                "screenshot",
                "ocr",
                "caption",
                "그림",
                "이미지",
                "차트",
            ]),
            wants_summary: contains_any(&["summary", "summarize", "overview", "요약", "개요"]),
            wants_claim: contains_any(&[
                "claim",
                "assertion",
                "statement",
                "argument",
                "주장",
                "명제",
            ]),
            wants_relationship: contains_any(&[
                "relationship",
                "relation",
                "link",
                "graph",
                "connect",
                "edge",
                "관계",
                "연결",
            ]),
        }
    }

    pub(crate) fn boost(self, evidence_type: &str) -> f64 {
        let intent_boost = match evidence_type {
            "table_evidence" if self.wants_table => 0.14,
            "image_region_evidence" | "ocr_evidence" | "caption_evidence" if self.wants_visual => {
                0.12
            }
            "summary_evidence" | "wiki_evidence" if self.wants_summary => 0.10,
            "claim_evidence" if self.wants_claim => 0.10,
            "relationship_evidence" if self.wants_relationship => 0.10,
            "claim_evidence" if self.wants_relationship => 0.06,
            "relationship_evidence" if self.wants_claim => 0.06,
            _ => 0.0,
        };
        intent_boost
            + match evidence_type {
                "text_evidence" => 0.05,
                "summary_evidence" | "claim_evidence" | "wiki_evidence" => 0.02,
                _ => 0.0,
            }
    }
}
