use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use etyma_engine_types::SourceArtifactManifest;
use serde::{Deserialize, Serialize};

use crate::source_index::{read_workspace_source_chunks, SourceChunk};

const MAX_QUERY_TERMS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalQuery {
    pub query_id: String,
    pub query: String,
    pub terms: Vec<String>,
    pub source: RetrievalQuerySource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalQuerySource {
    FileTitle,
    Heading,
    Identifier,
    RepeatedTerm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedEvidenceChunk {
    pub evidence_ref_id: String,
    pub chunk_id: String,
    pub source_id: String,
    pub source_title: String,
    pub source_path: String,
    pub markdown_path: String,
    pub heading_path: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub matched_terms: Vec<String>,
    pub score: f32,
    pub text_hash: String,
    pub text: String,
}

pub fn build_retrieval_queries_from_import(
    manifest: &SourceArtifactManifest,
    markdown: &str,
) -> Vec<RetrievalQuery> {
    let mut queries = Vec::new();
    push_query(
        &mut queries,
        RetrievalQuerySource::FileTitle,
        manifest.output_name.clone(),
    );

    for heading in markdown_headings(markdown).into_iter().take(8) {
        push_query(&mut queries, RetrievalQuerySource::Heading, heading);
    }

    for identifier in identifiers(markdown).into_iter().take(8) {
        push_query(&mut queries, RetrievalQuerySource::Identifier, identifier);
    }

    for term in repeated_terms(markdown).into_iter().take(8) {
        push_query(&mut queries, RetrievalQuerySource::RepeatedTerm, term);
    }

    dedupe_queries(queries)
}

pub fn retrieve_import_evidence(
    workspace_root: &Path,
    trigger_source_id: &str,
    queries: &[RetrievalQuery],
    limit: usize,
) -> Result<Vec<RetrievedEvidenceChunk>> {
    let chunks = read_workspace_source_chunks(workspace_root)?;
    Ok(retrieve_from_chunks(
        &chunks,
        trigger_source_id,
        queries,
        limit,
    ))
}

pub fn retrieve_from_chunks(
    chunks: &[SourceChunk],
    trigger_source_id: &str,
    queries: &[RetrievalQuery],
    limit: usize,
) -> Vec<RetrievedEvidenceChunk> {
    let mut scored = chunks
        .iter()
        .filter(|chunk| chunk.source_id != trigger_source_id)
        .filter_map(|chunk| score_chunk(chunk, queries))
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then(left.source_id.cmp(&right.source_id))
            .then(left.line_start.cmp(&right.line_start))
    });

    let mut source_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut selected = Vec::new();
    for chunk in scored {
        let count = source_counts.entry(chunk.source_id.clone()).or_default();
        if *count >= 4 {
            continue;
        }
        *count += 1;
        selected.push(chunk);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn score_chunk(chunk: &SourceChunk, queries: &[RetrievalQuery]) -> Option<RetrievedEvidenceChunk> {
    let haystack = searchable_text(chunk);
    let mut matched_terms = BTreeSet::new();
    let mut score = 0.0f32;

    for query in queries {
        let normalized_query = normalize(&query.query);
        if !normalized_query.is_empty() && haystack.contains(&normalized_query) {
            score += 3.0;
            matched_terms.insert(query.query.clone());
        }
        for term in &query.terms {
            let normalized = normalize(term);
            if normalized.is_empty() {
                continue;
            }
            if haystack.contains(&normalized) {
                score += if is_identifier(term) { 5.0 } else { 1.0 };
                matched_terms.insert(term.clone());
            }
        }
    }

    if score <= 0.0 {
        return None;
    }
    if !chunk.heading_path.is_empty()
        && chunk.heading_path.iter().any(|heading| {
            matched_terms
                .iter()
                .any(|term| normalize(heading).contains(&normalize(term)))
        })
    {
        score += 0.5;
    }

    Some(RetrievedEvidenceChunk {
        evidence_ref_id: format!("retrieved:{}:{}", chunk.source_id, chunk.chunk_id),
        chunk_id: chunk.chunk_id.clone(),
        source_id: chunk.source_id.clone(),
        source_title: chunk.source_title.clone(),
        source_path: chunk.source_path.clone(),
        markdown_path: chunk.markdown_path.clone(),
        heading_path: chunk.heading_path.clone(),
        line_start: chunk.line_start,
        line_end: chunk.line_end,
        matched_terms: matched_terms.into_iter().collect(),
        score,
        text_hash: chunk.text_hash.clone(),
        text: chunk.text.clone(),
    })
}

fn push_query(queries: &mut Vec<RetrievalQuery>, source: RetrievalQuerySource, query: String) {
    let query = query.trim().to_string();
    if query.is_empty() {
        return;
    }
    let terms = query_terms(&query);
    if terms.is_empty() {
        return;
    }
    queries.push(RetrievalQuery {
        query_id: format!("query-{:016x}", fnv1a_hash(query.as_bytes())),
        query,
        terms,
        source,
    });
}

fn dedupe_queries(queries: Vec<RetrievalQuery>) -> Vec<RetrievalQuery> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for query in queries {
        let key = normalize(&query.query);
        if seen.insert(key) {
            deduped.push(query);
        }
    }
    deduped
}

fn markdown_headings(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let level = trimmed.chars().take_while(|char| *char == '#').count();
            if level == 0 || level > 6 {
                return None;
            }
            let heading = trimmed[level..].trim().trim_matches('#').trim();
            (!heading.is_empty()).then(|| heading.to_string())
        })
        .collect()
}

fn identifiers(markdown: &str) -> Vec<String> {
    let mut identifiers = markdown
        .split(|char: char| !(char.is_ascii_alphanumeric() || char == '_'))
        .filter(|token| token.len() >= 4)
        .filter(|token| token.chars().any(|char| char.is_ascii_lowercase()))
        .filter(|token| token.chars().any(|char| char.is_ascii_uppercase()))
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    identifiers.sort_by_key(|value| std::cmp::Reverse(value.len()));
    identifiers
}

fn repeated_terms(markdown: &str) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for term in query_terms(markdown) {
        *counts.entry(term).or_default() += 1;
    }
    let mut counts = counts
        .into_iter()
        .filter(|(term, count)| *count >= 2 && term.len() >= 4)
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    counts.into_iter().map(|(term, _)| term).collect()
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .split(|char: char| !(char.is_alphanumeric() || char == '_'))
        .filter_map(|token| {
            let token = token.trim();
            (token.chars().count() >= 3).then(|| token.to_string())
        })
        .take(MAX_QUERY_TERMS)
        .collect::<Vec<_>>();
    if terms.is_empty() && !query.trim().is_empty() {
        terms.push(query.trim().to_string());
    }
    terms
}

fn searchable_text(chunk: &SourceChunk) -> String {
    normalize(&format!(
        "{}\n{}\n{}\n{}",
        chunk.source_title,
        chunk.heading_path.join(" "),
        chunk.source_path,
        chunk.text
    ))
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|char| {
            if char.is_alphanumeric() || char == '_' {
                char
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_identifier(value: &str) -> bool {
    value.chars().any(|char| char.is_ascii_lowercase())
        && value.chars().any(|char| char.is_ascii_uppercase())
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
