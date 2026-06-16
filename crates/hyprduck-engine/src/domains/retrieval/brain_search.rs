//! Query-time brain search / hybrid retrieval domain logic (evidence + graph + wiki).
//! Policy (intent, scoring, stage combination) lives here in the retrieval domain.
//! Heavy data access (specific FTS queries, graph expansion SQL/Cypher) still reaches
//! into persistence adapters for the raw mechanics.

use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    BrainSearchResult, BrainSearchResultKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::adapters::persistence::context_pack_store::{
    evidence_snippet_from_ids, load_context_pack_evidence_row,
};
use crate::adapters::persistence::read_projection_store::{
    load_graph_canvas_nodes, load_graph_canvas_relations, load_graph_canvas_wiki_pages,
};
use crate::adapters::persistence::row_decode::{non_empty_string, row_i64, row_string_array};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct HybridRetrievalHit {
    pub(crate) evidence_id: String,
    pub(crate) source_id: String,
    pub(crate) evidence_type: String,
    pub(crate) snippet: String,
    pub(crate) lexical_rank: f64,
    pub(crate) graph_neighbor_count: i64,
    pub(crate) score: f64,
}

pub(crate) fn db_search_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}

pub(crate) fn db_match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let lower = haystack.to_lowercase();
    let matched = terms.iter().filter(|term| lower.contains(*term)).count();
    (matched > 0).then(|| matched * 100)
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

pub(crate) fn db_float_score(score: f64) -> usize {
    (score.max(0.0) * 1000.0).round() as usize
}

#[allow(dead_code)]
pub(crate) fn evidence_graph_neighbor_counts(
    graph: &Graph,
    workspace_id: &str,
) -> Result<BTreeMap<String, i64>> {
    let mut counts = BTreeMap::new();
    for node in graph
        .get_all_nodes(None)
        .context("failed reading GraphQLite nodes for hybrid retrieval")?
    {
        let graphqlite::Value::Object(properties) = node else {
            continue;
        };
        let Some(graphqlite::Value::String(node_workspace_id)) = properties.get("workspace_id")
        else {
            continue;
        };
        if node_workspace_id != workspace_id {
            continue;
        }
        let is_live = match properties.get("valid_to") {
            Some(graphqlite::Value::Integer(value)) => *value <= 0,
            Some(graphqlite::Value::Float(value)) => *value <= 0.0,
            Some(graphqlite::Value::String(value)) => value.trim().parse::<i64>().unwrap_or(0) <= 0,
            Some(graphqlite::Value::Null) | None => true,
            Some(_) => false,
        };
        if !is_live {
            continue;
        }
        let Some(graphqlite::Value::String(node_id)) = properties.get("id") else {
            continue;
        };
        let Some(graphqlite::Value::String(evidence_ids_json)) =
            properties.get("evidence_ids_json")
        else {
            continue;
        };
        let evidence_ids =
            serde_json::from_str::<Vec<String>>(evidence_ids_json).unwrap_or_default();
        if evidence_ids.is_empty() {
            continue;
        }
        let _ = node_id;
        let degree = 1;
        for evidence_id in evidence_ids {
            *counts.entry(evidence_id).or_insert(0) += degree;
        }
    }
    Ok(counts)
}

#[allow(dead_code)]
pub(crate) fn append_graph_neighbor_hits(
    graph: &Graph,
    workspace_id: &str,
    limit: usize,
    graph_neighbor_counts: &BTreeMap<String, i64>,
    evidence_intent: &EvidenceQueryIntent,
    hits: &mut Vec<HybridRetrievalHit>,
) -> Result<()> {
    let seed_evidence_ids = hits
        .iter()
        .map(|hit| hit.evidence_id.clone())
        .collect::<BTreeSet<_>>();
    if seed_evidence_ids.is_empty() {
        return Ok(());
    }

    let mut seed_node_ids = BTreeSet::new();
    let seed_rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n {workspace_id: $workspace_id})
             RETURN n.id AS node_id,
                    n.evidence_ids_json AS evidence_ids_json,
                    n.valid_to AS valid_to",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed finding GraphQLite retrieval seed nodes")?;
    for row in &seed_rows {
        if !row_is_live(row, "valid_to")? {
            continue;
        }
        let node_id = row.get::<String>("node_id").context("read seed node id")?;
        let evidence_ids =
            row_string_array(row, "evidence_ids_json").context("read seed node evidence refs")?;
        if evidence_ids
            .iter()
            .any(|evidence_id| seed_evidence_ids.contains(evidence_id))
        {
            seed_node_ids.insert(node_id);
        }
    }

    let mut candidate_evidence_ids = BTreeSet::new();
    for seed_node_id in &seed_node_ids {
        append_cypher_neighbor_evidence_ids(
            graph,
            workspace_id,
            seed_node_id.as_str(),
            "MATCH (seed {id: $seed_node_id})-[r]->(neighbor)
             RETURN neighbor.workspace_id AS neighbor_workspace_id,
                    neighbor.valid_to AS neighbor_valid_to,
                    r.valid_to AS relationship_valid_to,
                    neighbor.evidence_ids_json AS neighbor_evidence_ids_json,
                    r.evidence_ids_json AS relationship_evidence_ids_json",
            &mut candidate_evidence_ids,
        )?;
        append_cypher_neighbor_evidence_ids(
            graph,
            workspace_id,
            seed_node_id.as_str(),
            "MATCH (neighbor)-[r]->(seed {id: $seed_node_id})
             RETURN neighbor.workspace_id AS neighbor_workspace_id,
                    neighbor.valid_to AS neighbor_valid_to,
                    r.valid_to AS relationship_valid_to,
                    neighbor.evidence_ids_json AS neighbor_evidence_ids_json,
                    r.evidence_ids_json AS relationship_evidence_ids_json",
            &mut candidate_evidence_ids,
        )?;
    }
    append_cypher_seed_relationship_endpoint_evidence_ids(
        graph,
        workspace_id,
        &seed_evidence_ids,
        &mut candidate_evidence_ids,
    )?;

    let sqlite = graph.connection().sqlite_connection();
    for evidence_id in candidate_evidence_ids {
        if hits.len() >= limit {
            break;
        }
        if seed_evidence_ids.contains(&evidence_id)
            || hits.iter().any(|hit| hit.evidence_id == evidence_id)
        {
            continue;
        }
        let mut statement = sqlite
            .prepare(
                "SELECT e.evidence_id, e.source_id, e.evidence_type, e.snippet
                 FROM evidence_items e
                 JOIN sources s ON s.source_id = e.source_id
                 WHERE e.workspace_id = ?1 AND e.evidence_id = ?2
                   AND e.status = 'active'
                   AND s.status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')",
            )
            .context("failed preparing graph neighbor evidence query")?;
        let mut rows = statement
            .query((workspace_id, evidence_id.as_str()))
            .context("failed running graph neighbor evidence query")?;
        let Some(row) = rows
            .next()
            .context("failed reading graph neighbor evidence row")?
        else {
            continue;
        };
        let evidence_id: String = row.get(0).context("failed reading evidence id")?;
        let source_id: String = row.get(1).context("failed reading source id")?;
        let evidence_type: String = row.get(2).context("failed reading evidence type")?;
        let snippet: String = row.get(3).context("failed reading evidence snippet")?;
        let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
        let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
        let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id,
            evidence_type,
            snippet,
            lexical_rank: 0.0,
            graph_neighbor_count,
            score: 0.04 + typed_evidence_boost + graph_boost,
        });
    }

    Ok(())
}

#[allow(dead_code)]
fn append_cypher_seed_relationship_endpoint_evidence_ids(
    graph: &Graph,
    workspace_id: &str,
    seed_evidence_ids: &BTreeSet<String>,
    candidate_evidence_ids: &mut BTreeSet<String>,
) -> Result<()> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (source)-[r]->(target)
             RETURN source.workspace_id AS source_workspace_id,
                    target.workspace_id AS target_workspace_id,
                    source.valid_to AS source_valid_to,
                    target.valid_to AS target_valid_to,
                    r.valid_to AS relationship_valid_to,
                    source.evidence_ids_json AS source_evidence_ids_json,
                    target.evidence_ids_json AS target_evidence_ids_json,
                    r.evidence_ids_json AS relationship_evidence_ids_json",
        )
        .run()
        .context("failed querying GraphQLite seed relationships")?;
    for row in &rows {
        let source_workspace_id = row
            .get::<String>("source_workspace_id")
            .context("read source workspace id")?;
        let target_workspace_id = row
            .get::<String>("target_workspace_id")
            .context("read target workspace id")?;
        if source_workspace_id != workspace_id || target_workspace_id != workspace_id {
            continue;
        }
        if !row_is_live(row, "source_valid_to")?
            || !row_is_live(row, "target_valid_to")?
            || !row_is_live(row, "relationship_valid_to")?
        {
            continue;
        }
        let relationship_evidence_ids = row_string_array(row, "relationship_evidence_ids_json")
            .context("read relationship evidence refs")?;
        if !relationship_evidence_ids
            .iter()
            .any(|evidence_id| seed_evidence_ids.contains(evidence_id))
        {
            continue;
        }
        candidate_evidence_ids.extend(
            row_string_array(row, "source_evidence_ids_json")
                .context("read source evidence refs")?,
        );
        candidate_evidence_ids.extend(
            row_string_array(row, "target_evidence_ids_json")
                .context("read target evidence refs")?,
        );
    }
    Ok(())
}

#[allow(dead_code)]
fn append_cypher_neighbor_evidence_ids(
    graph: &Graph,
    workspace_id: &str,
    seed_node_id: &str,
    cypher: &str,
    candidate_evidence_ids: &mut BTreeSet<String>,
) -> Result<()> {
    let rows = graph
        .connection()
        .cypher_builder(cypher)
        .param("seed_node_id", seed_node_id)
        .run()
        .with_context(|| format!("failed querying GraphQLite neighbors for {seed_node_id}"))?;
    for row in &rows {
        let neighbor_workspace_id = row
            .get::<String>("neighbor_workspace_id")
            .context("read neighbor workspace id")?;
        if neighbor_workspace_id != workspace_id {
            continue;
        }
        if !row_is_live(row, "neighbor_valid_to")? || !row_is_live(row, "relationship_valid_to")? {
            continue;
        }
        for column in [
            "neighbor_evidence_ids_json",
            "relationship_evidence_ids_json",
        ] {
            let evidence_ids =
                row_string_array(row, column).with_context(|| format!("read {column}"))?;
            candidate_evidence_ids.extend(evidence_ids);
        }
    }
    Ok(())
}

fn row_is_live(row: &graphqlite::Row, column: &str) -> Result<bool> {
    Ok(row_i64(row, column).with_context(|| format!("read {column}"))? <= 0)
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

#[allow(dead_code)]
pub(crate) fn append_source_page_fts_hits(
    graph: &Graph,
    workspace_id: &str,
    fts_query: &str,
    limit: usize,
    graph_neighbor_counts: &BTreeMap<String, i64>,
    evidence_intent: &EvidenceQueryIntent,
    hits: &mut Vec<HybridRetrievalHit>,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                e.evidence_id,
                p.source_id,
                e.evidence_type,
                p.text,
                bm25(source_page_fts) AS lexical_rank
             FROM source_page_fts p
             JOIN sources s ON s.source_id = p.source_id
             JOIN evidence_items e ON e.source_id = p.source_id AND e.page_index = p.page_index
             WHERE s.workspace_id = ?1 AND source_page_fts MATCH ?2
               AND e.status = 'active'
               AND s.status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')
             ORDER BY lexical_rank ASC
             LIMIT ?3",
        )
        .context("failed preparing source page FTS retrieval query")?;
    let mut rows = statement
        .query((workspace_id, fts_query, limit as i64))
        .context("failed running source page FTS retrieval query")?;
    while let Some(row) = rows
        .next()
        .context("failed reading source page FTS retrieval row")?
    {
        if hits.len() >= limit {
            break;
        }
        let evidence_id: String = row.get(0).context("failed reading evidence id")?;
        if hits.iter().any(|hit| hit.evidence_id == evidence_id) {
            continue;
        }
        let source_id: String = row.get(1).context("failed reading source id")?;
        let evidence_type: String = row.get(2).context("failed reading evidence type")?;
        let snippet: String = row.get(3).context("failed reading source page text")?;
        let lexical_rank: f64 = row.get(4).context("failed reading lexical rank")?;
        let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
        let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
        let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id,
            evidence_type,
            snippet,
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + 0.03 + typed_evidence_boost + graph_boost,
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn append_wiki_fts_hits(
    graph: &Graph,
    workspace_id: &str,
    fts_query: &str,
    limit: usize,
    graph_neighbor_counts: &BTreeMap<String, i64>,
    evidence_intent: &EvidenceQueryIntent,
    hits: &mut Vec<HybridRetrievalHit>,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                w.wiki_page_id,
                w.title,
                w.text,
                wp.evidence_refs_json,
                bm25(wiki_fts) AS lexical_rank
             FROM wiki_fts w
             JOIN wiki_pages wp
               ON wp.wiki_page_id = w.wiki_page_id
              AND wp.revision = w.revision
             WHERE w.workspace_id = ?1 AND wiki_fts MATCH ?2
               AND wp.approval_status IN ('materialized', 'approved')
               AND wp.valid_to <= 0
             ORDER BY lexical_rank ASC
             LIMIT ?3",
        )
        .context("failed preparing wiki FTS retrieval query")?;
    let mut rows = statement
        .query((workspace_id, fts_query, limit as i64))
        .context("failed running wiki FTS retrieval query")?;
    while let Some(row) = rows
        .next()
        .context("failed reading wiki FTS retrieval row")?
    {
        if hits.len() >= limit {
            break;
        }
        let wiki_page_id: String = row.get(0).context("failed reading wiki page id")?;
        let title: String = row.get(1).context("failed reading wiki title")?;
        let text: String = row.get(2).context("failed reading wiki text")?;
        let evidence_refs_json: String = row.get(3).context("failed reading wiki evidence refs")?;
        let lexical_rank: f64 = row.get(4).context("failed reading lexical rank")?;
        let evidence_refs =
            serde_json::from_str::<Vec<String>>(&evidence_refs_json).unwrap_or_default();
        let Some(evidence_id) = evidence_refs.first().cloned() else {
            continue;
        };
        if hits.iter().any(|hit| hit.evidence_id == evidence_id) {
            continue;
        }
        let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
        let typed_evidence_boost = evidence_intent.boost("wiki_evidence");
        let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id: wiki_page_id,
            evidence_type: "wiki_evidence".into(),
            snippet: format!("{title}\n{text}"),
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + 0.02 + typed_evidence_boost + graph_boost,
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn fts_phrase_query(query: &str) -> String {
    db_search_terms(query)
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('\"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
pub(crate) fn hybrid_retrieve_from_db(
    path: &Path,
    workspace_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<HybridRetrievalHit>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let graph_neighbor_counts = evidence_graph_neighbor_counts(&graph, workspace_id)?;
    let fts_query = fts_phrase_query(query);
    if fts_query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let evidence_intent = EvidenceQueryIntent::from_query(query);

    let mut statement = graph
        .connection()
        .sqlite_connection()
        .prepare(
            "SELECT
                    f.evidence_id,
                    f.source_id,
                    f.evidence_type,
                    f.text,
                    bm25(evidence_fts) AS lexical_rank
                 FROM evidence_fts f
                 JOIN evidence_items e ON e.evidence_id = f.evidence_id
                 JOIN sources s ON s.source_id = e.source_id
                 WHERE e.workspace_id = ?1 AND evidence_fts MATCH ?2
                   AND e.status = 'active'
                   AND s.status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')
                 ORDER BY lexical_rank ASC
                 LIMIT ?3",
        )
        .context("failed preparing hybrid retrieval query")?;
    let mut rows = statement
        .query((workspace_id, fts_query.as_str(), limit as i64))
        .context("failed running hybrid retrieval query")?;
    let mut hits = Vec::new();
    while let Some(row) = rows.next().context("failed reading hybrid retrieval row")? {
        let evidence_id: String = row.get(0).context("failed reading evidence id")?;
        let source_id: String = row.get(1).context("failed reading source id")?;
        let evidence_type: String = row.get(2).context("failed reading evidence type")?;
        let snippet: String = row.get(3).context("failed reading evidence text")?;
        let lexical_rank: f64 = row.get(4).context("failed reading lexical rank")?;
        let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
        let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
        let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id,
            evidence_type,
            snippet,
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + typed_evidence_boost + graph_boost,
        });
    }
    if hits.len() < limit {
        append_graph_neighbor_hits(
            &graph,
            workspace_id,
            limit,
            &graph_neighbor_counts,
            &evidence_intent,
            &mut hits,
        )?;
    }
    if hits.len() < limit {
        append_source_page_fts_hits(
            &graph,
            workspace_id,
            fts_query.as_str(),
            limit,
            &graph_neighbor_counts,
            &evidence_intent,
            &mut hits,
        )?;
    }
    if hits.len() < limit {
        append_wiki_fts_hits(
            &graph,
            workspace_id,
            fts_query.as_str(),
            limit,
            &graph_neighbor_counts,
            &evidence_intent,
            &mut hits,
        )?;
    }
    if hits.is_empty() {
        let mut fallback_statement = graph
            .connection()
            .sqlite_connection()
            .prepare(
                "SELECT e.evidence_id, e.source_id, e.evidence_type, e.snippet
                 FROM evidence_items e
                 JOIN sources s ON s.source_id = e.source_id
                     WHERE e.workspace_id = ?1
                       AND e.snippet LIKE '%' || ?2 || '%'
                       AND e.status = 'active'
                       AND s.status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')
                     LIMIT ?3",
            )
            .context("failed preparing hybrid retrieval fallback query")?;
        let mut fallback_rows = fallback_statement
            .query((workspace_id, query, limit as i64))
            .context("failed running hybrid retrieval fallback query")?;
        while let Some(row) = fallback_rows
            .next()
            .context("failed reading hybrid retrieval fallback row")?
        {
            let evidence_id: String = row.get(0).context("failed reading evidence id")?;
            let source_id: String = row.get(1).context("failed reading source id")?;
            let evidence_type: String = row.get(2).context("failed reading evidence type")?;
            let snippet: String = row.get(3).context("failed reading evidence text")?;
            let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
            let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
            let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
            hits.push(HybridRetrievalHit {
                evidence_id,
                source_id,
                evidence_type,
                snippet,
                lexical_rank: 0.0,
                graph_neighbor_count,
                score: typed_evidence_boost + graph_boost,
            });
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(hits)
}

#[allow(dead_code)]
pub(crate) fn search_brain_from_db(
    path: &Path,
    workspace_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<BrainSearchResult>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let terms = db_search_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let mut results = Vec::new();
    for hit in hybrid_retrieve_from_db(path, workspace_id, query, limit)? {
        let row = load_context_pack_evidence_row(&graph, workspace_id, &hit.evidence_id)?;
        let path = row
            .as_ref()
            .and_then(|row| non_empty_string(row.markdown_path_redacted.clone()));
        results.push(BrainSearchResult {
            kind: if hit.evidence_type == "wiki_evidence" {
                BrainSearchResultKind::WikiPage
            } else {
                BrainSearchResultKind::Evidence
            },
            id: hit.evidence_id,
            title: hit.source_id,
            path,
            score: db_float_score(hit.score),
            snippet: hit.snippet,
        });
    }

    for node in load_graph_canvas_nodes(&graph, workspace_id)? {
        let haystack = format!(
            "{} {} {} {}",
            node.node_id,
            node.label,
            node.aliases.join(" "),
            node.source_ids.join(" ")
        );
        if let Some(score) = db_match_score(&terms, &haystack) {
            results.push(BrainSearchResult {
                kind: BrainSearchResultKind::Node,
                id: node.node_id,
                title: node.label,
                path: None,
                score,
                snippet: evidence_snippet_from_ids(&node.evidence_ids),
            });
        }
    }

    for relation in load_graph_canvas_relations(&graph, workspace_id)? {
        let haystack = format!(
            "{} {:?} {} {} {} {}",
            relation.relation_id,
            relation.kind,
            relation.source_node_id,
            relation.target_node_id,
            relation.label,
            relation.evidence_ids.join(" ")
        );
        if let Some(score) = db_match_score(&terms, &haystack) {
            results.push(BrainSearchResult {
                kind: BrainSearchResultKind::Relation,
                id: relation.relation_id,
                title: relation.label,
                path: None,
                score,
                snippet: format!(
                    "{:?}: {} -> {}; {}",
                    relation.kind,
                    relation.source_node_id,
                    relation.target_node_id,
                    evidence_snippet_from_ids(&relation.evidence_ids)
                ),
            });
        }
    }

    for page in load_graph_canvas_wiki_pages(&graph, workspace_id)? {
        let haystack = format!("{} {} {}", page.path, page.title, page.body);
        if let Some(score) = db_match_score(&terms, &haystack) {
            results.push(BrainSearchResult {
                kind: BrainSearchResultKind::WikiPage,
                id: page.page_id,
                title: page.title,
                path: Some(page.path),
                score,
                snippet: db_best_snippet(&page.body, &terms),
            });
        }
    }

    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(&right.title))
    });
    results.dedup_by(|left, right| left.kind == right.kind && left.id == right.id);
    results.truncate(limit);
    Ok(results)
}
