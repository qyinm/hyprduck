use crate::blob::{get_verified, BlobStore};
use crate::store::{EvidenceRow, Store, StoreResult};
use etyma_engine_types::{
    ContextPackEvidenceV1, ContextPackFindingStatus, ContextPackFindingV0,
    ContextPackParseConfidence, ContextPackRetrievalTraceV1, ContextPackSourceV0,
    ContextPackStaleness, ContextPackV1, ContextPackWarningSeverity, ContextPackWarningV0,
    EvidenceType, CONTEXT_PACK_V1_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Compose a cited V1 pack from server-owned multi-source evidence (spike path).
/// Source body text is loaded from the blob backend when needed for title/body matching.
pub fn compose_pack(
    store: &Store,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    query: &str,
) -> StoreResult<ContextPackV1> {
    let sources = store.list_sources(workspace_id)?;
    let evidence = store.list_evidence(workspace_id)?;
    let terms = query_terms(query);
    let mut hits: Vec<&EvidenceRow> = evidence
        .iter()
        .filter(|ev| matches_terms(&ev.quote, &terms) || matches_terms(&ev.locator, &terms))
        .collect();
    if hits.is_empty() {
        for source in &sources {
            let body = load_source_text(blobs, source)?;
            if matches_terms(&body, &terms) || matches_terms(&source.title, &terms) {
                if let Some(ev) = evidence.iter().find(|e| e.source_id == source.id) {
                    hits.push(ev);
                }
            }
        }
    }

    let pack_id = format!("ctx_{}", Uuid::now_v7().simple());
    let generated_at = format!("{}", unix_now());
    let source_by_id: BTreeMap<_, _> = sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut selected_evidence = Vec::new();
    let mut findings = Vec::new();
    let mut source_set = BTreeMap::new();
    let evidence_pool = evidence.len();

    for (idx, ev) in hits.into_iter().take(16).enumerate() {
        let page = parse_page(&ev.locator).unwrap_or(1);
        let evidence_type = evidence_type_for_kind(&ev.source_kind);
        let content_hash = format!("sha256:{}", short_hash(&ev.quote));
        selected_evidence.push(ContextPackEvidenceV1 {
            evidence_ref: ev.id.clone(),
            source_id: ev.source_id.clone(),
            page,
            region: Some(ev.locator.clone()),
            span: None,
            quoted_text: ev.quote.clone(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: format!("matched query terms in {} source", ev.source_kind),
            content_hash,
            evidence_type,
            graph_trail: None,
        });
        findings.push(ContextPackFindingV0 {
            finding_id: format!("f_{idx}"),
            statement: ev.quote.clone(),
            status: ContextPackFindingStatus::DerivedSummary,
            statement_confidence: ContextPackParseConfidence::High,
            derived_from: vec![ev.id.clone()],
            relevance_reason: "spike multi-source match".into(),
        });
        if let Some(source) = source_by_id.get(ev.source_id.as_str()) {
            source_set.entry(source.id.clone()).or_insert_with(|| ContextPackSourceV0 {
                source_id: source.id.clone(),
                original_filename: source.title.clone(),
                // Stored content hash of original blob bytes (not re-derived from body column).
                content_hash: source.content_hash.clone(),
                page_count: 1,
                ingestion_status: "ingested".into(),
                staleness: ContextPackStaleness::Current,
                provider_route: "etyma-server-spike".into(),
                local_only: false,
            });
        }
    }

    let mut warnings = Vec::new();
    if selected_evidence.is_empty() {
        warnings.push(ContextPackWarningV0 {
            warning_type: "no_matching_evidence".into(),
            severity: ContextPackWarningSeverity::Low,
            message: "No evidence matched the query in this workspace.".into(),
            page_refs: vec![],
        });
    }

    let chunks_selected = selected_evidence.len();
    Ok(ContextPackV1 {
        schema_version: CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
        pack_id,
        workspace_id: workspace_id.to_string(),
        query: query.to_string(),
        generated_at,
        source_set: source_set.into_values().collect(),
        selected_evidence,
        findings,
        warnings,
        retrieval_trace: ContextPackRetrievalTraceV1 {
            strategy: "etyma-server-spike-term-match".into(),
            chunks_considered: evidence_pool,
            chunks_selected,
            budget_requested: 8000,
            budget_used: chunks_selected.saturating_mul(120),
            evidence_type_trace: Default::default(),
        },
        suggested_next_reads: vec![],
    })
}

fn load_source_text(
    blobs: &dyn BlobStore,
    source: &crate::store::SourceRow,
) -> StoreResult<String> {
    let bytes = get_verified(blobs, &source.blob_key)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn evidence_type_for_kind(kind: &str) -> EvidenceType {
    match kind {
        "issue" | "pull_request" => EvidenceType::Claim,
        _ => EvidenceType::Text,
    }
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|t| t.len() >= 2)
        .collect()
}

fn matches_terms(haystack: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lower = haystack.to_lowercase();
    terms.iter().any(|t| lower.contains(t))
}

fn parse_page(locator: &str) -> Option<usize> {
    locator.strip_prefix("page:").and_then(|s| s.parse().ok())
}

fn short_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(&h.finalize()[..8])
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::LocalFsBlobStore;
    use crate::ingest::ingest_source;
    use crate::store::Store;
    use tempfile::tempdir;

    #[test]
    fn compose_hits_document_and_issue() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite3")).unwrap();
        let blobs = LocalFsBlobStore::open(dir.path().join("blobs")).unwrap();
        store.create_org("org1", "Test").unwrap();
        store.create_workspace("org1", "ws").unwrap();
        let doc = ingest_source(
            &store,
            &blobs,
            "ws",
            "document",
            "Auth Spec",
            b"The alpha-token boundary requires explicit workspace binding.",
            "text/plain",
            None,
        )
        .unwrap();
        store
            .insert_evidence(
                "ws",
                &doc.id,
                "document",
                "The alpha-token boundary requires explicit workspace binding.",
                "page:1",
            )
            .unwrap();
        let issue = ingest_source(
            &store,
            &blobs,
            "ws",
            "issue",
            "ENG-1 alpha-token",
            b"Track alpha-token migration for multi-tenant packs.",
            "text/plain",
            Some("ENG-1"),
        )
        .unwrap();
        store
            .insert_evidence(
                "ws",
                &issue.id,
                "issue",
                "Track alpha-token migration for multi-tenant packs.",
                "issue:ENG-1",
            )
            .unwrap();

        let pack = compose_pack(&store, &blobs, "ws", "alpha-token").unwrap();
        assert_eq!(pack.workspace_id, "ws");
        assert!(
            pack.selected_evidence
                .iter()
                .any(|e| e.selection_reason.contains("document"))
        );
        assert!(
            pack.selected_evidence
                .iter()
                .any(|e| e.selection_reason.contains("issue"))
        );
        // Source set uses blob content hashes from metadata.
        assert!(pack
            .source_set
            .iter()
            .all(|s| s.content_hash.starts_with("sha256:")));
        assert_eq!(pack.source_set.len(), 2);
    }
}
