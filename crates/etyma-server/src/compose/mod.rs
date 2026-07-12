use crate::blob::{get_verified, BlobStore};
use crate::graph::GraphStore;
use crate::knowledge::{EvidenceRow, KnowledgeStore, SourceRow};
use crate::store::{StoreError, StoreResult};
use etyma_engine_types::{
    ContextPackEvidenceV1, ContextPackFindingStatus, ContextPackFindingV0,
    ContextPackGraphFollowUpArgumentsV1, ContextPackGraphFollowUpToolV1,
    ContextPackGraphFollowUpV1, ContextPackGraphHandleTypeV1, ContextPackGraphReadNodeArgumentsV1,
    ContextPackGraphRecordKindV1, ContextPackGraphRecordV1, ContextPackGraphTrailV1,
    ContextPackParseConfidence, ContextPackRetrievalTraceV1, ContextPackSourceV0,
    ContextPackStaleness, ContextPackV1, ContextPackWarningSeverity, ContextPackWarningV0,
    EvidenceType, CONTEXT_PACK_V1_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Compose a cited V1 pack from server-owned Postgres evidence.
/// Source body text is loaded from the blob backend when needed for title/body matching.
/// Graph trails (when present) come from the Postgres graph live projection — never GraphQLite.
pub async fn compose_pack(
    knowledge: &KnowledgeStore,
    graph: &GraphStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    query: &str,
) -> StoreResult<ContextPackV1> {
    let sources = knowledge
        .list_sources(workspace_id)
        .await
        .map_err(StoreError::from)?;
    let evidence = knowledge
        .list_evidence(workspace_id)
        .await
        .map_err(StoreError::from)?;
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

    let selected: Vec<&EvidenceRow> = hits.into_iter().take(16).collect();
    let trail_index = load_graph_trail_index(graph, workspace_id, &selected).await?;

    for (idx, ev) in selected.into_iter().enumerate() {
        let page = parse_page(&ev.locator).unwrap_or(1);
        let evidence_type = evidence_type_for_kind(&ev.source_kind);
        let content_hash = ev.content_hash.clone();
        let graph_trail = trail_index.get(ev.id.as_str()).cloned();
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
            graph_trail,
        });
        findings.push(ContextPackFindingV0 {
            finding_id: format!("f_{idx}"),
            statement: ev.quote.clone(),
            status: ContextPackFindingStatus::DerivedSummary,
            statement_confidence: ContextPackParseConfidence::High,
            derived_from: vec![ev.id.clone()],
            relevance_reason: "cloud multi-source match".into(),
        });
        if let Some(source) = source_by_id.get(ev.source_id.as_str()) {
            source_set
                .entry(source.id.clone())
                .or_insert_with(|| ContextPackSourceV0 {
                    source_id: source.id.clone(),
                    original_filename: source.title.clone(),
                    // Stored content hash of original blob bytes (not re-derived from body column).
                    content_hash: source.content_hash.clone(),
                    page_count: 1,
                    ingestion_status: "ingested".into(),
                    staleness: ContextPackStaleness::Current,
                    provider_route: "etyma-server-cloud".into(),
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
            strategy: "etyma-server-postgres-term-match".into(),
            chunks_considered: evidence_pool,
            chunks_selected,
            budget_requested: 8000,
            budget_used: chunks_selected.saturating_mul(120),
            evidence_type_trace: Default::default(),
        },
        suggested_next_reads: vec![],
    })
}

/// Batch graph trail load: at most two queries for the whole selected set.
async fn load_graph_trail_index(
    graph: &GraphStore,
    workspace_id: &str,
    selected: &[&EvidenceRow],
) -> StoreResult<BTreeMap<String, ContextPackGraphTrailV1>> {
    if selected.is_empty() {
        return Ok(BTreeMap::new());
    }
    let evidence_ids: Vec<String> = selected.iter().map(|ev| ev.id.clone()).collect();
    let nodes = graph
        .live_nodes_for_evidence_ids(workspace_id, &evidence_ids)
        .await
        .map_err(StoreError::from)?;
    if nodes.is_empty() {
        return Ok(BTreeMap::new());
    }

    let by_evidence = GraphStore::index_nodes_by_evidence(&nodes);
    let all_node_ids: Vec<String> = nodes.iter().map(|n| n.logical_id.clone()).collect();
    let relations = graph
        .live_relations_touching(workspace_id, &all_node_ids)
        .await
        .map_err(StoreError::from)?;

    let mut out = BTreeMap::new();
    for ev_id in &evidence_ids {
        let Some(linked) = by_evidence.get(ev_id) else {
            continue;
        };
        if linked.is_empty() {
            continue;
        }
        let linked_ids: std::collections::HashSet<&str> =
            linked.iter().map(|n| n.logical_id.as_str()).collect();
        let direct = linked
            .iter()
            .map(|node| ContextPackGraphRecordV1 {
                record_type: ContextPackGraphRecordKindV1::Node,
                id: node.logical_id.clone(),
                reason: format!("live graph node cites evidence {ev_id}"),
            })
            .collect::<Vec<_>>();
        let adjacent = relations
            .iter()
            .filter(|rel| {
                linked_ids.contains(rel.source_logical_id.as_str())
                    || linked_ids.contains(rel.target_logical_id.as_str())
            })
            .map(|rel| ContextPackGraphRecordV1 {
                record_type: ContextPackGraphRecordKindV1::Relation,
                id: rel.logical_id.clone(),
                reason: "live relation adjacent to evidence-linked nodes".into(),
            })
            .collect::<Vec<_>>();
        let follow_up = linked
            .iter()
            .take(3)
            .map(|node| ContextPackGraphFollowUpV1 {
                tool: ContextPackGraphFollowUpToolV1::ReadNode,
                handle_type: ContextPackGraphHandleTypeV1::Node,
                arguments: ContextPackGraphFollowUpArgumentsV1::ReadNode(
                    ContextPackGraphReadNodeArgumentsV1 {
                        node_id: node.logical_id.clone(),
                    },
                ),
                reason: "inspect live cloud graph node".into(),
            })
            .collect();
        out.insert(
            ev_id.clone(),
            ContextPackGraphTrailV1 {
                direct,
                adjacent,
                follow_up,
                unavailable_reason: None,
            },
        );
    }
    Ok(out)
}

fn load_source_text(blobs: &dyn BlobStore, source: &SourceRow) -> StoreResult<String> {
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
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
