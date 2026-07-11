//! FTS hybrid retrieval and brain search orchestration.

use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{BrainSearchResult, BrainSearchResultKind};
use std::collections::BTreeMap;
use std::path::Path;

use crate::adapters::persistence::context_pack_store::{
    evidence_snippet_from_ids, load_context_pack_evidence_row,
};
use crate::adapters::persistence::read_projection_store::{
    load_graph_canvas_nodes, load_graph_canvas_relations, load_graph_canvas_wiki_pages,
};
use crate::adapters::persistence::row_decode::non_empty_string;

use super::graph_expand::{append_graph_neighbor_hits, evidence_graph_neighbor_counts};
use super::scoring::{
    db_best_snippet, db_context_window, db_float_score, db_match_score,
    db_source_metadata_match_score, EvidenceQueryIntent,
};
use super::text_normalize::{db_search_terms, fts_phrase_query};
use super::HybridRetrievalHit;

#[allow(dead_code)]
pub(crate) fn append_source_page_fts_hits(
    graph: &Graph,
    workspace_id: &str,
    fts_query: &str,
    terms: &[String],
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
        let evidence_id: String = row.get(0).context("failed reading evidence id")?;
        let source_id: String = row.get(1).context("failed reading source id")?;
        let evidence_type: String = row.get(2).context("failed reading evidence type")?;
        let page_text: String = row.get(3).context("failed reading source page text")?;
        let page_context = db_context_window(&page_text, terms, 1_600);
        if let Some(existing) = hits.iter_mut().find(|hit| hit.evidence_id == evidence_id) {
            if existing
                .quoted_text
                .as_deref()
                .map(|value| value.chars().count())
                .unwrap_or_else(|| existing.snippet.chars().count())
                < page_context.chars().count()
            {
                existing.quoted_text = Some(page_context);
                existing.score += 0.08;
            }
            continue;
        }
        if hits.len() >= limit {
            break;
        }
        let lexical_rank: f64 = row.get(4).context("failed reading lexical rank")?;
        let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
        let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
        let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id,
            evidence_type,
            snippet: page_context.clone(),
            quoted_text: Some(page_context),
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + 0.03 + typed_evidence_boost + graph_boost,
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn append_source_metadata_hits(
    graph: &Graph,
    workspace_id: &str,
    terms: &[String],
    limit: usize,
    graph_neighbor_counts: &BTreeMap<String, i64>,
    evidence_intent: &EvidenceQueryIntent,
    hits: &mut Vec<HybridRetrievalHit>,
) -> Result<()> {
    if terms.is_empty() || limit == 0 {
        return Ok(());
    }

    let sqlite = graph.connection().sqlite_connection();
    let mut source_statement = sqlite
        .prepare(
            "SELECT source_id, title, original_path_redacted, source_path_redacted, markdown_path_redacted
             FROM sources
             WHERE workspace_id = ?1
               AND status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')
             ORDER BY updated_at DESC
             LIMIT 200",
        )
        .context("failed preparing source metadata retrieval query")?;
    let mut source_rows = source_statement
        .query([workspace_id])
        .context("failed running source metadata retrieval query")?;
    let per_source_limit = limit.min(4).max(1) as i64;
    let mut additions = 0usize;
    while let Some(row) = source_rows
        .next()
        .context("failed reading source metadata retrieval row")?
    {
        if additions >= limit {
            break;
        }
        let source_id: String = row.get(0).context("failed reading source id")?;
        let title: String = row.get(1).context("failed reading source title")?;
        let original_path_redacted: String =
            row.get(2).context("failed reading source original path")?;
        let source_path_redacted: String = row.get(3).context("failed reading source path")?;
        let markdown_path_redacted: String =
            row.get(4).context("failed reading source markdown path")?;
        let haystack = format!(
            "{source_id} {title} {original_path_redacted} {source_path_redacted} {markdown_path_redacted}"
        );
        let Some(metadata_score) = db_source_metadata_match_score(terms, &haystack) else {
            continue;
        };
        let source_boost = 0.75 + (metadata_score as f64 / 100.0) * 0.35;
        let mut evidence_statement = sqlite
            .prepare(
                "SELECT
                    e.evidence_id,
                    e.source_id,
                    e.evidence_type,
                    e.snippet,
                    e.page_index,
                    sp.plain_text
                 FROM evidence_items e
                 LEFT JOIN source_pages sp
                   ON sp.source_id = e.source_id
                  AND sp.page_index = e.page_index
                 WHERE e.workspace_id = ?1
                   AND e.source_id = ?2
                   AND e.status = 'active'
                 ORDER BY
                   CASE WHEN e.page_index IS NULL THEN 1 ELSE 0 END ASC,
                   e.page_index ASC,
                   CASE e.evidence_type
                     WHEN 'summary_evidence' THEN 0
                     WHEN 'text_evidence' THEN 1
                     ELSE 2
                   END ASC,
                   e.evidence_id ASC
                 LIMIT ?3",
            )
            .context("failed preparing source metadata evidence query")?;
        let mut evidence_rows = evidence_statement
            .query((workspace_id, source_id.as_str(), per_source_limit))
            .context("failed running source metadata evidence query")?;
        while let Some(row) = evidence_rows
            .next()
            .context("failed reading source metadata evidence row")?
        {
            let evidence_id: String = row.get(0).context("failed reading evidence id")?;
            let source_id: String = row.get(1).context("failed reading source id")?;
            let evidence_type: String = row.get(2).context("failed reading evidence type")?;
            let snippet: String = row.get(3).context("failed reading evidence snippet")?;
            let page_index: Option<i64> =
                row.get(4).context("failed reading evidence page index")?;
            let page_text: Option<String> =
                row.get(5).context("failed reading source page text")?;
            let quoted_text = page_text
                .filter(|text| !text.trim().is_empty())
                .map(|text| db_context_window(&text, terms, 1_600));
            let snippet = quoted_text.clone().unwrap_or(snippet);
            let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
            let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
            let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
            let page_boost = page_index.map(|_| 0.08).unwrap_or(0.0);
            let score = source_boost + typed_evidence_boost + graph_boost + page_boost;
            if let Some(existing) = hits.iter_mut().find(|hit| hit.evidence_id == evidence_id) {
                existing.score += source_boost;
                if existing.quoted_text.is_none() {
                    existing.quoted_text = quoted_text;
                    existing.snippet = snippet;
                }
                continue;
            }
            hits.push(HybridRetrievalHit {
                evidence_id,
                source_id,
                evidence_type,
                snippet,
                quoted_text,
                lexical_rank: 0.0,
                graph_neighbor_count,
                score,
            });
            additions += 1;
            if additions >= limit {
                break;
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn append_wiki_fts_hits(
    graph: &Graph,
    workspace_id: &str,
    fts_query: &str,
    terms: &[String],
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
        let wiki_context = db_context_window(&format!("{title}\n{text}"), terms, 1_600);
        hits.push(HybridRetrievalHit {
            evidence_id,
            source_id: wiki_page_id,
            evidence_type: "wiki_evidence".into(),
            snippet: wiki_context.clone(),
            quoted_text: Some(wiki_context),
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + 0.02 + typed_evidence_boost + graph_boost,
        });
    }
    Ok(())
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
    let terms = db_search_terms(query);
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
            quoted_text: None,
            lexical_rank,
            graph_neighbor_count,
            score: -lexical_rank + typed_evidence_boost + graph_boost,
        });
    }
    let expanded_limit = limit.saturating_mul(3).max(limit);
    append_source_page_fts_hits(
        &graph,
        workspace_id,
        fts_query.as_str(),
        &terms,
        expanded_limit,
        &graph_neighbor_counts,
        &evidence_intent,
        &mut hits,
    )?;
    append_source_metadata_hits(
        &graph,
        workspace_id,
        &terms,
        expanded_limit,
        &graph_neighbor_counts,
        &evidence_intent,
        &mut hits,
    )?;
    if append_graph_neighbor_hits(
        &graph,
        workspace_id,
        expanded_limit,
        &graph_neighbor_counts,
        &evidence_intent,
        &mut hits,
    )
    .is_err()
    {
        // Graph expansion is an enrichment stage. Keep lexical/source evidence
        // usable when GraphQLite projection rows are incomplete or stale.
    }
    append_wiki_fts_hits(
        &graph,
        workspace_id,
        fts_query.as_str(),
        &terms,
        limit,
        &graph_neighbor_counts,
        &evidence_intent,
        &mut hits,
    )?;
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
        for term in &terms {
            if hits.len() >= limit {
                break;
            }
            let mut fallback_rows = fallback_statement
                .query((workspace_id, term.as_str(), limit as i64))
                .context("failed running hybrid retrieval fallback query")?;
            while let Some(row) = fallback_rows
                .next()
                .context("failed reading hybrid retrieval fallback row")?
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
                let snippet: String = row.get(3).context("failed reading evidence text")?;
                let context = db_context_window(&snippet, &terms, 1_600);
                let graph_neighbor_count = *graph_neighbor_counts.get(&evidence_id).unwrap_or(&1);
                let typed_evidence_boost = evidence_intent.boost(evidence_type.as_str());
                let graph_boost = (graph_neighbor_count as f64).min(10.0) * 0.01;
                hits.push(HybridRetrievalHit {
                    evidence_id,
                    source_id,
                    evidence_type,
                    snippet: context.clone(),
                    quoted_text: Some(context),
                    lexical_rank: 0.0,
                    graph_neighbor_count,
                    score: typed_evidence_boost + graph_boost,
                });
            }
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
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
