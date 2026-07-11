//! Graph-neighbor expansion for hybrid retrieval.

use anyhow::{Context, Result};
use graphqlite::Graph;
use std::collections::{BTreeMap, BTreeSet};

use crate::adapters::persistence::row_decode::{row_i64, row_string_array};

use super::scoring::EvidenceQueryIntent;
use super::HybridRetrievalHit;

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
            quoted_text: None,
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
