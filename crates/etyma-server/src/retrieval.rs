use crate::blob::{get_verified, BlobStore};
use crate::knowledge::{EvidenceRow, KnowledgeStore, SourceRow};
use crate::store::{StoreError, StoreResult};

#[derive(Debug, Clone)]
pub struct RetrievalResult {
    pub sources: Vec<SourceRow>,
    pub evidence: Vec<EvidenceRow>,
    pub selected_evidence: Vec<EvidenceRow>,
    pub chunks_considered: usize,
}

pub async fn retrieve_evidence(
    knowledge: &KnowledgeStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    query: &str,
    limit: usize,
) -> StoreResult<RetrievalResult> {
    let sources = knowledge
        .list_sources(workspace_id)
        .await
        .map_err(StoreError::from)?;
    let evidence = knowledge
        .list_evidence(workspace_id)
        .await
        .map_err(StoreError::from)?;
    let terms = query_terms(query);
    let mut selected: Vec<EvidenceRow> = evidence
        .iter()
        .filter(|ev| evidence_matches(ev, &terms))
        .take(limit)
        .cloned()
        .collect();

    if selected.is_empty() {
        for source in &sources {
            let body = load_source_text(blobs, source)?;
            if matches_terms(&body, &terms) || matches_terms(&source.title, &terms) {
                if let Some(ev) = evidence
                    .iter()
                    .find(|candidate| candidate.source_id == source.id)
                {
                    selected.push(ev.clone());
                    if selected.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    Ok(RetrievalResult {
        chunks_considered: evidence.len(),
        sources,
        evidence,
        selected_evidence: selected,
    })
}

fn evidence_matches(evidence: &EvidenceRow, terms: &[String]) -> bool {
    let retrieval_text = evidence
        .retrieval_text
        .as_deref()
        .unwrap_or(&evidence.quote);
    matches_terms(retrieval_text, terms)
        || matches_terms(&evidence.quote, terms)
        || matches_terms(&evidence.locator, terms)
}

fn load_source_text(blobs: &dyn BlobStore, source: &SourceRow) -> StoreResult<String> {
    let bytes = get_verified(blobs, &source.blob_key)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| !ch.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|term| term.len() >= 2)
        .collect()
}

fn matches_terms(haystack: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lower = haystack.to_lowercase();
    terms.iter().any(|term| lower.contains(term))
}
