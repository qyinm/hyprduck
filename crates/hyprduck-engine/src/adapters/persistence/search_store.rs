//! Internal helpers extracted from the engine facade module.

use super::row_decode::{row_i64, row_string_array};
use anyhow::{Context, Result};
use graphqlite::Graph;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

pub(super) fn db_search_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}

pub(super) fn db_match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let lower = haystack.to_lowercase();
    let matched = terms.iter().filter(|term| lower.contains(*term)).count();
    (matched > 0).then(|| matched * 100)
}

pub(super) fn db_best_snippet(text: &str, terms: &[String]) -> String {
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

pub(super) fn db_float_score(score: f64) -> usize {
    (score.max(0.0) * 1000.0).round() as usize
}

#[allow(dead_code)]
pub(super) fn evidence_graph_neighbor_counts(
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
pub(super) fn append_graph_neighbor_hits(
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
pub(super) struct EvidenceQueryIntent {
    wants_table: bool,
    wants_visual: bool,
    wants_summary: bool,
    wants_claim: bool,
    wants_relationship: bool,
}

impl EvidenceQueryIntent {
    pub(super) fn from_query(query: &str) -> Self {
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

    pub(super) fn boost(self, evidence_type: &str) -> f64 {
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
pub(super) fn append_source_page_fts_hits(
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
pub(super) fn append_wiki_fts_hits(
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
pub(super) fn fts_phrase_query(query: &str) -> String {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
