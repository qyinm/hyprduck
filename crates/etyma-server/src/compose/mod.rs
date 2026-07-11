use crate::store::{EvidenceRow, Store};
use anyhow::Result;
use etyma_engine_types::{
    BrainContextPack, ContextPackEvidenceV0, ContextPackEvidenceV1, ContextPackFindingStatus,
    ContextPackFindingV0, ContextPackParseConfidence, ContextPackRetrievalTraceV0,
    ContextPackRetrievalTraceV1, ContextPackSourceV0, ContextPackStaleness, ContextPackV0,
    ContextPackV1, ContextPackWarningSeverity, ContextPackWarningV0, EvidenceType,
    GetContextPackResponseData, CONTEXT_PACK_V0_SCHEMA_VERSION, CONTEXT_PACK_V1_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Compose a cited pack from server-owned multi-source evidence (spike path).
pub fn compose_pack(
    store: &Store,
    workspace_id: &str,
    query: &str,
) -> Result<GetContextPackResponseData> {
    // Prefer engine when a knowledge store already exists for this workspace root.
    if let Some(ws) = store.get_workspace(workspace_id)? {
        let candidates = ["knowledge.sqlite3", "knowledge.db", "etyma.sqlite3"];
        if candidates
            .iter()
            .any(|name| ws.engine_root.join(name).exists())
        {
            if let Ok(pack) = etyma_engine::cloud::get_context_pack(
                etyma_engine_types::GetContextPackRequest {
                    scope: etyma_engine_types::BrainReadScope {
                        workspace_id: workspace_id.to_string(),
                        root_dir: Some(ws.engine_root.to_string_lossy().into_owned()),
                    },
                    query: query.to_string(),
                    selected_node_id: None,
                    budget: Some(8000),
                    persist: false,
                },
            ) {
                if !pack.context_pack_v1.selected_evidence.is_empty() {
                    return Ok(pack);
                }
            }
        }
    }

    let sources = store.list_sources(workspace_id)?;
    let evidence = store.list_evidence(workspace_id)?;
    let terms = query_terms(query);
    let mut hits: Vec<&EvidenceRow> = evidence
        .iter()
        .filter(|ev| matches_terms(&ev.quote, &terms) || matches_terms(&ev.locator, &terms))
        .collect();
    if hits.is_empty() {
        for source in &sources {
            if matches_terms(&source.body, &terms) || matches_terms(&source.title, &terms) {
                if let Some(ev) = evidence.iter().find(|e| e.source_id == source.id) {
                    hits.push(ev);
                }
            }
        }
    }

    let pack_id = format!("ctx_{}", Uuid::now_v7().simple());
    let generated_at = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let source_by_id: BTreeMap<_, _> = sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut kinds_hit = std::collections::BTreeSet::new();
    let mut selected_v1 = Vec::new();
    let mut selected_v0 = Vec::new();
    let mut findings = Vec::new();
    let mut source_set = BTreeMap::new();
    let mut considered = 0usize;

    for (idx, ev) in hits.into_iter().take(16).enumerate() {
        considered += 1;
        kinds_hit.insert(ev.source_kind.clone());
        let page = parse_page(&ev.locator).unwrap_or(1);
        let evidence_type = evidence_type_for_kind(&ev.source_kind);
        let content_hash = format!("fnv64:{}", short_hash(&ev.quote));
        selected_v1.push(ContextPackEvidenceV1 {
            evidence_ref: ev.id.clone(),
            source_id: ev.source_id.clone(),
            page,
            region: Some(ev.locator.clone()),
            span: None,
            quoted_text: ev.quote.clone(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: format!("matched query terms in {} source", ev.source_kind),
            content_hash: content_hash.clone(),
            evidence_type,
            graph_trail: None,
        });
        selected_v0.push(ContextPackEvidenceV0 {
            evidence_ref: ev.id.clone(),
            source_id: ev.source_id.clone(),
            page,
            region: Some(ev.locator.clone()),
            span: None,
            quoted_text: ev.quote.clone(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: format!("matched query terms in {} source", ev.source_kind),
            content_hash: content_hash.clone(),
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
            source_set.entry(source.id.clone()).or_insert_with(|| {
                ContextPackSourceV0 {
                    source_id: source.id.clone(),
                    original_filename: source.title.clone(),
                    content_hash: format!("fnv64:{}", short_hash(&source.body)),
                    page_count: 1,
                    ingestion_status: "ingested".into(),
                    staleness: ContextPackStaleness::Current,
                    provider_route: "etyma-server-spike".into(),
                    local_only: false,
                }
            });
        }
    }

    let mut warnings = Vec::new();
    if selected_v1.is_empty() {
        warnings.push(ContextPackWarningV0 {
            warning_type: "no_matching_evidence".into(),
            severity: ContextPackWarningSeverity::Low,
            message: "No evidence matched the query in this workspace.".into(),
            page_refs: vec![],
        });
    }

    let source_set: Vec<_> = source_set.into_values().collect();
    let chunks_selected = selected_v1.len();
    let context_pack_v1 = ContextPackV1 {
        schema_version: CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
        pack_id: pack_id.clone(),
        workspace_id: workspace_id.to_string(),
        query: query.to_string(),
        generated_at: generated_at.clone(),
        source_set: source_set.clone(),
        selected_evidence: selected_v1,
        findings: findings.clone(),
        warnings: warnings.clone(),
        retrieval_trace: ContextPackRetrievalTraceV1 {
            strategy: "etyma-server-spike-term-match".into(),
            chunks_considered: considered.max(evidence.len()),
            chunks_selected,
            budget_requested: 8000,
            budget_used: chunks_selected.saturating_mul(120),
            evidence_type_trace: Default::default(),
        },
        suggested_next_reads: vec![],
    };
    let context_pack_v0 = ContextPackV0 {
        schema_version: CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id,
        workspace_id: workspace_id.to_string(),
        query: query.to_string(),
        generated_at,
        source_set,
        selected_evidence: selected_v0,
        findings,
        warnings: warnings.clone(),
        retrieval_trace: ContextPackRetrievalTraceV0 {
            strategy: "etyma-server-spike-term-match".into(),
            chunks_considered: considered.max(evidence.len()),
            chunks_selected,
            budget_requested: 8000,
            budget_used: chunks_selected.saturating_mul(120),
        },
        suggested_next_reads: vec![],
    };

    let brain = BrainContextPack {
        workspace_id: workspace_id.to_string(),
        query: query.to_string(),
        token_budget: 8000,
        summary: format!(
            "Spike pack from etyma-server ({} evidence, kinds={:?}).",
            chunks_selected, kinds_hit
        ),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![],
        memories: vec![],
        entities: vec![],
        claims: vec![],
        relations: vec![],
        evidence: vec![],
        recent_events: vec![],
        warnings: warnings.iter().map(|w| w.message.clone()).collect(),
    };

    Ok(GetContextPackResponseData {
        context_pack: brain,
        context_pack_v1,
        context_pack_v0,
        persisted_context_pack_path: None,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use tempfile::tempdir;

    #[test]
    fn compose_hits_document_and_issue() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite3")).unwrap();
        store
            .create_workspace("ws", &dir.path().join("ws"))
            .unwrap();
        let doc = store
            .insert_source(
                "ws",
                "document",
                "Auth Spec",
                "The alpha-token boundary requires explicit workspace binding.",
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
        let issue = store
            .insert_source(
                "ws",
                "issue",
                "ENG-1 alpha-token",
                "Track alpha-token migration for multi-tenant packs.",
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

        let pack = compose_pack(&store, "ws", "alpha-token").unwrap();
        let kinds: std::collections::BTreeSet<_> = pack
            .context_pack_v1
            .selected_evidence
            .iter()
            .map(|e| e.selection_reason.clone())
            .collect();
        assert!(
            pack.context_pack_v1
                .selected_evidence
                .iter()
                .any(|e| e.selection_reason.contains("document")),
            "{kinds:?}"
        );
        assert!(
            pack.context_pack_v1
                .selected_evidence
                .iter()
                .any(|e| e.selection_reason.contains("issue")),
            "{kinds:?}"
        );
    }
}
