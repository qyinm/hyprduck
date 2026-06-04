//! Read-side projections for the knowledge store facade.

use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainScope,
    BrainSearchResult, BrainSearchResultKind, GraphSnapshotSourceRecord, PageEvidenceV0,
    ReadNodeResponseData, ReadPageEvidenceResponseData, ReadSourceResponseData, SourceFormat,
    SourceStatus, WikiPage,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use super::context_pack_store::{
    db_parse_confidence, evidence_snippet_from_ids, load_context_pack_evidence_row,
    load_context_pack_evidence_rows_for_source, load_context_pack_source_row,
    load_evidence_refs_by_ids, load_evidence_refs_for_source, load_wiki_page_by_path,
    load_wiki_page_for_source, source_record_from_context_row,
};
use super::graph_snapshot_store::KnowledgeGraphPersistReport;
use super::row_decode::{non_empty_string, row_i64, row_string, row_string_array};
use super::search_store::{
    append_graph_neighbor_hits, append_source_page_fts_hits, append_wiki_fts_hits, db_best_snippet,
    db_float_score, db_match_score, db_search_terms, evidence_graph_neighbor_counts,
    fts_phrase_query, EvidenceQueryIntent, HybridRetrievalHit,
};

#[derive(Debug)]
struct NodeRelationRow {
    relation: BrainRelationRecord,
    source_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RelationalEvidenceProof {
    pub(crate) evidence_id: String,
    pub(crate) workspace_id: String,
    pub(crate) source_id: String,
    pub(crate) page_index: Option<i64>,
    pub(crate) page_label: String,
    pub(crate) evidence_type: String,
    pub(crate) snippet: String,
    pub(crate) source_path_redacted: String,
    pub(crate) markdown_path_redacted: String,
    pub(crate) image_path_redacted: String,
    pub(crate) provenance: String,
    pub(crate) producer_run_id: String,
    pub(crate) confidence: Option<f64>,
    pub(crate) status: String,
    pub(crate) created_at: i64,
}

#[allow(dead_code)]
pub(super) fn hybrid_retrieve_from_db(
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
                     WHERE e.snippet LIKE '%' || ?1 || '%'
                       AND e.status = 'active'
                       AND s.status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')
                     LIMIT ?2",
            )
            .context("failed preparing hybrid retrieval fallback query")?;
        let mut fallback_rows = fallback_statement
            .query((query, limit as i64))
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
pub(super) fn read_source_from_db(
    path: &Path,
    workspace_id: &str,
    source_id: &str,
    include_local_paths: bool,
) -> Result<Option<ReadSourceResponseData>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let Some(source) = load_context_pack_source_row(&graph, workspace_id, source_id)? else {
        return Ok(None);
    };
    let evidence = load_evidence_refs_for_source(&graph, workspace_id, source_id, None)?;
    let wiki_page = load_wiki_page_for_source(&graph, workspace_id, source_id)?;
    Ok(Some(ReadSourceResponseData {
        source: source_record_from_context_row(source, include_local_paths),
        wiki_page,
        evidence,
    }))
}

#[allow(dead_code)]
pub(super) fn read_page_evidence_from_db(
    path: &Path,
    workspace_id: &str,
    source_id: &str,
    page: Option<usize>,
    include_local_paths: bool,
) -> Result<Option<ReadPageEvidenceResponseData>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let Some(source) = load_context_pack_source_row(&graph, workspace_id, source_id)? else {
        return Ok(None);
    };
    let rows = load_context_pack_evidence_rows_for_source(
        &graph,
        workspace_id,
        source_id,
        page.map(|page| page.saturating_sub(1) as i64),
    )?;
    let content_hash = source.content_hash.clone();
    let mut evidence = rows
        .into_iter()
        .map(|row| PageEvidenceV0 {
            evidence_ref: row.evidence_id,
            source_id: row.source_id,
            page: row.page_index.unwrap_or(0).max(0) as usize + 1,
            region: format!("page:{}", row.page_index.unwrap_or(0).max(0) + 1),
            span: None,
            quoted_text: row.snippet,
            parse_confidence: db_parse_confidence(row.confidence),
            content_hash: content_hash.clone(),
            markdown_path: non_empty_string(row.markdown_path_redacted),
            image_path: non_empty_string(row.image_path_redacted),
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| {
        left.page
            .cmp(&right.page)
            .then_with(|| left.evidence_ref.cmp(&right.evidence_ref))
    });
    Ok(Some(ReadPageEvidenceResponseData {
        source: source_record_from_context_row(source, include_local_paths),
        evidence,
        warnings: Vec::new(),
    }))
}

#[allow(dead_code)]
pub(super) fn read_wiki_page_from_db(
    path: &Path,
    workspace_id: &str,
    page_path: &str,
) -> Result<Option<WikiPage>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    load_wiki_page_by_path(&graph, workspace_id, page_path)
}

#[allow(dead_code)]
pub(super) fn read_node_from_db(
    path: &Path,
    workspace_id: &str,
    node_id: &str,
) -> Result<Option<ReadNodeResponseData>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let Some(node) = load_graph_canvas_nodes(&graph, workspace_id)?
        .into_iter()
        .find(|node| node.node_id == node_id)
    else {
        return Ok(None);
    };
    let evidence_ids = node.evidence_ids.clone();
    let source_ids = node.source_ids.clone();
    let mut evidence = load_evidence_refs_by_ids(&graph, workspace_id, &evidence_ids)?;
    if evidence.is_empty() {
        for source_id in &source_ids {
            evidence.extend(load_evidence_refs_for_source(
                &graph,
                workspace_id,
                source_id,
                None,
            )?);
        }
    }
    let relations = load_node_relations_from_db(&graph, workspace_id, node_id)?;
    Ok(Some(ReadNodeResponseData {
        node,
        evidence,
        relations,
    }))
}

fn load_node_relations_from_db(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
) -> Result<Vec<BrainRelationRecord>> {
    let mut relation_rows = load_node_relation_rows_from_db(
        graph,
        workspace_id,
        node_id,
        "MATCH (source {id: $node_id, workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_node_id,
                target.id AS target_node_id,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at",
    )?;
    relation_rows.extend(load_node_relation_rows_from_db(
        graph,
        workspace_id,
        node_id,
        "MATCH (source {workspace_id: $workspace_id})-[r]->(target {id: $node_id, workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_node_id,
                target.id AS target_node_id,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at",
    )?);
    let (eligible_evidence_ids, eligible_source_ids) =
        load_node_relation_eligible_refs(graph, workspace_id, &relation_rows)?;
    let mut relations = relation_rows
        .into_iter()
        .filter(|row| {
            relation_refs_are_eligible(
                &eligible_evidence_ids,
                &eligible_source_ids,
                &row.relation.evidence_ids,
                &row.source_ids,
            )
        })
        .map(|row| row.relation)
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| left.relation_id.cmp(&right.relation_id));
    relations.dedup_by(|left, right| left.relation_id == right.relation_id);
    Ok(relations)
}

fn load_node_relation_rows_from_db(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
    cypher: &str,
) -> Result<Vec<NodeRelationRow>> {
    let rows = graph
        .connection()
        .cypher_builder(cypher)
        .param("workspace_id", workspace_id)
        .param("node_id", node_id)
        .run()
        .context("failed querying GraphQLite node relations")?;
    let mut relations = Vec::new();
    for row in &rows {
        relations.push(NodeRelationRow {
            relation: graph_canvas_relation_from_row(row)?,
            source_ids: row_string_array(row, "source_ids_json")
                .context("read node relation source refs")?,
        });
    }
    Ok(relations)
}

fn load_node_relation_eligible_refs(
    graph: &Graph,
    workspace_id: &str,
    relation_rows: &[NodeRelationRow],
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let evidence_ids = relation_rows
        .iter()
        .flat_map(|row| row.relation.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let source_ids = relation_rows
        .iter()
        .flat_map(|row| row.source_ids.iter().cloned())
        .collect::<BTreeSet<_>>();

    let mut eligible_evidence_ids = BTreeSet::new();
    for evidence_id in evidence_ids {
        let Some(evidence_row) = load_context_pack_evidence_row(graph, workspace_id, &evidence_id)?
        else {
            continue;
        };
        if load_context_pack_source_row(graph, workspace_id, &evidence_row.source_id)?.is_some() {
            eligible_evidence_ids.insert(evidence_id);
        }
    }

    let mut eligible_source_ids = BTreeSet::new();
    for source_id in source_ids {
        if load_context_pack_source_row(graph, workspace_id, &source_id)?.is_some() {
            eligible_source_ids.insert(source_id);
        }
    }

    Ok((eligible_evidence_ids, eligible_source_ids))
}

fn relation_refs_are_eligible(
    eligible_evidence_ids: &BTreeSet<String>,
    eligible_source_ids: &BTreeSet<String>,
    evidence_ids: &[String],
    source_ids: &[String],
) -> bool {
    if !evidence_ids.is_empty() {
        return evidence_ids
            .iter()
            .any(|evidence_id| eligible_evidence_ids.contains(evidence_id));
    }
    source_ids
        .iter()
        .any(|source_id| eligible_source_ids.contains(source_id))
}

#[allow(dead_code)]
pub(super) fn read_graph_canvas_projection_from_db(
    path: &Path,
    workspace_id: &str,
) -> Result<
    Option<(
        Vec<BrainNodeRecord>,
        Vec<BrainRelationRecord>,
        Vec<WikiPage>,
    )>,
> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let nodes = load_graph_canvas_nodes(&graph, workspace_id)?;
    let relations = load_graph_canvas_relations(&graph, workspace_id)?;
    let wiki_pages = load_graph_canvas_wiki_pages(&graph, workspace_id)?;
    if nodes.is_empty() && relations.is_empty() && wiki_pages.is_empty() {
        return Ok(None);
    }
    Ok(Some((nodes, relations, wiki_pages)))
}

pub(super) fn read_graph_snapshot_sources_from_db(
    path: &Path,
    workspace_id: &str,
    include_local_paths: bool,
) -> Result<Vec<GraphSnapshotSourceRecord>> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                    sources.source_id,
                    sources.workspace_id,
                    sources.original_path,
                    sources.source_path,
                    sources.markdown_path,
                    sources.original_path_redacted,
                    sources.source_path_redacted,
                    sources.markdown_path_redacted,
                    sources.format,
                    sources.status,
                    sources.page_count,
                    sources.success_count,
                    sources.failed_count,
                    sources.updated_at,
                    COALESCE(import_jobs.citation_ready, CASE WHEN sources.success_count > 0 THEN 1 ELSE 0 END),
                    COALESCE(import_jobs.graph_ready, 0),
                    COALESCE(import_jobs.graph_status, ''),
                    COALESCE(import_jobs.manual_retry_available, 0)
                 FROM sources
                 LEFT JOIN import_jobs ON import_jobs.workspace_id = sources.workspace_id
                    AND import_jobs.source_id = sources.source_id
                 WHERE sources.workspace_id = ?1
                 ORDER BY sources.source_id ASC",
        )
        .context("failed preparing graph snapshot source query")?;
    let mut rows = statement
        .query((workspace_id,))
        .context("failed querying graph snapshot sources")?;
    let mut sources = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading graph snapshot source row")?
    {
        let original_path: String = row.get(2).context("read source original path")?;
        let source_path: String = row.get(3).context("read source path")?;
        let markdown_path: String = row.get(4).context("read source markdown path")?;
        let original_path_redacted: String =
            row.get(5).context("read redacted source original path")?;
        let source_path_redacted: String = row.get(6).context("read redacted source path")?;
        let markdown_path_redacted: String =
            row.get(7).context("read redacted source markdown path")?;
        sources.push(GraphSnapshotSourceRecord {
            source_id: row.get(0).context("read source id")?,
            workspace_id: row.get(1).context("read source workspace")?,
            original_path: if include_local_paths {
                original_path
            } else {
                original_path_redacted
            },
            source_path: if include_local_paths {
                source_path
            } else {
                source_path_redacted
            },
            markdown_path: if include_local_paths {
                markdown_path
            } else {
                markdown_path_redacted
            },
            format: SourceFormat::from(row.get::<_, String>(8).context("read source format")?),
            status: SourceStatus::from(row.get::<_, String>(9).context("read source status")?),
            page_count: row
                .get::<_, i64>(10)
                .context("read source page count")?
                .max(0) as usize,
            success_count: row
                .get::<_, i64>(11)
                .context("read source success count")?
                .max(0) as usize,
            failed_count: row
                .get::<_, i64>(12)
                .context("read source failed count")?
                .max(0) as usize,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            citation_ready: row.get::<_, i64>(14).context("read citation_ready")? != 0,
            graph_ready: row.get::<_, i64>(15).context("read graph_ready")? != 0,
            graph_status: row.get(16).context("read graph_status")?,
            manual_retry_available: row
                .get::<_, i64>(17)
                .context("read manual_retry_available")?
                != 0,
            updated_at: row
                .get::<_, i64>(13)
                .context("read source updated at")?
                .max(0) as u64,
        });
    }
    Ok(sources)
}

pub(super) fn search_brain_from_db(
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

#[allow(dead_code)]
pub(super) fn resolve_evidence_proof(
    path: &Path,
    workspace_id: &str,
    evidence_id: &str,
) -> Result<RelationalEvidenceProof> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                    evidence_id,
                    workspace_id,
                    source_id,
                    page_index,
                    page_label,
                    evidence_type,
                    snippet,
                    source_path_redacted,
                    markdown_path_redacted,
                    image_path_redacted,
                    provenance,
                    producer_run_id,
                    confidence,
                    status,
                    created_at
                 FROM evidence_items
                 WHERE workspace_id = ?1 AND evidence_id = ?2",
        )
        .context("failed preparing relational evidence proof query")?;
    let mut rows = statement
        .query((workspace_id, evidence_id))
        .context("failed querying relational evidence proof")?;
    let Some(row) = rows
        .next()
        .context("failed reading relational evidence proof row")?
    else {
        return Err(anyhow!(
            "missing relational evidence row {} in workspace {}",
            evidence_id,
            workspace_id
        ));
    };

    Ok(RelationalEvidenceProof {
        evidence_id: row.get(0).context("read evidence_id")?,
        workspace_id: row.get(1).context("read workspace_id")?,
        source_id: row.get(2).context("read source_id")?,
        page_index: row.get(3).context("read page_index")?,
        page_label: row.get(4).context("read page_label")?,
        evidence_type: row.get(5).context("read evidence_type")?,
        snippet: row.get(6).context("read snippet")?,
        source_path_redacted: row.get(7).context("read source_path_redacted")?,
        markdown_path_redacted: row.get(8).context("read markdown_path_redacted")?,
        image_path_redacted: row.get(9).context("read image_path_redacted")?,
        provenance: row.get(10).context("read provenance")?,
        producer_run_id: row.get(11).context("read producer_run_id")?,
        confidence: row.get(12).context("read confidence")?,
        status: row.get(13).context("read status")?,
        created_at: row.get(14).context("read created_at")?,
    })
}

pub(super) fn graph_snapshot_counts(
    path: &Path,
    workspace_id: &str,
) -> Result<KnowledgeGraphPersistReport> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let node_count = graph
        .connection()
        .cypher_builder("MATCH (n {workspace_id: $workspace_id}) RETURN count(n) AS count")
        .param("workspace_id", workspace_id)
        .run()
        .context("failed counting GraphQLite nodes")?
        .get(0)
        .and_then(|row| row.get::<i64>("count").ok())
        .unwrap_or_default() as usize;
    let relation_count = graph
        .connection()
        .cypher_builder(
            "MATCH (n {workspace_id: $workspace_id})-[r]->(m {workspace_id: $workspace_id}) RETURN count(r) AS count",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed counting GraphQLite relations")?
        .get(0)
        .and_then(|row| row.get::<i64>("count").ok())
        .unwrap_or_default() as usize;
    Ok(KnowledgeGraphPersistReport {
        node_count,
        relation_count,
    })
}

fn load_graph_canvas_nodes(graph: &Graph, workspace_id: &str) -> Result<Vec<BrainNodeRecord>> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n {workspace_id: $workspace_id})
             RETURN n.id AS node_id,
                    n.kind AS kind,
                    n.label AS label,
                    n.scope AS scope,
                    n.aliases_json AS aliases_json,
                    n.evidence_ids_json AS evidence_ids_json,
                    n.source_ids_json AS source_ids_json,
                    n.confidence AS confidence,
                    n.updated_at AS updated_at",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed querying GraphQLite graph canvas nodes")?;
    let mut nodes = rows
        .iter()
        .map(|row| {
            Ok(BrainNodeRecord {
                node_id: row_string(row, "node_id").context("read graph canvas node id")?,
                kind: parse_brain_node_kind(
                    &row_string(row, "kind").context("read graph canvas node kind")?,
                ),
                label: row_string(row, "label").context("read graph canvas node label")?,
                scope: parse_brain_scope(
                    &row_string(row, "scope").context("read graph canvas node scope")?,
                ),
                aliases: row_string_array(row, "aliases_json")
                    .context("read graph canvas node aliases")?,
                evidence_ids: row_string_array(row, "evidence_ids_json")
                    .context("read graph canvas node evidence refs")?,
                source_ids: row_string_array(row, "source_ids_json")
                    .context("read graph canvas node source refs")?,
                confidence: parse_optional_f32(
                    &row_string(row, "confidence").context("read graph canvas node confidence")?,
                ),
                updated_at: row_i64(row, "updated_at")
                    .context("read graph canvas node updated at")?
                    .max(0) as u64,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    Ok(nodes)
}

fn load_graph_canvas_relations(
    graph: &Graph,
    workspace_id: &str,
) -> Result<Vec<BrainRelationRecord>> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (source {workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
             RETURN r.relation_id AS relation_id,
                    r.kind AS kind,
                    source.id AS source_node_id,
                    target.id AS target_node_id,
                    r.label AS label,
                    r.evidence_ids_json AS evidence_ids_json,
                    r.confidence AS confidence,
                    r.updated_at AS updated_at",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed querying GraphQLite graph canvas relations")?;
    let mut relations = rows
        .iter()
        .map(graph_canvas_relation_from_row)
        .collect::<Result<Vec<_>>>()?;
    relations.sort_by(|left, right| left.relation_id.cmp(&right.relation_id));
    Ok(relations)
}

fn graph_canvas_relation_from_row(row: &graphqlite::Row) -> Result<BrainRelationRecord> {
    Ok(BrainRelationRecord {
        relation_id: row_string(row, "relation_id").context("read graph canvas relation id")?,
        kind: parse_brain_relation_kind(
            &row_string(row, "kind").context("read graph canvas relation kind")?,
        ),
        source_node_id: row_string(row, "source_node_id")
            .context("read graph canvas relation source node")?,
        target_node_id: row_string(row, "target_node_id")
            .context("read graph canvas relation target node")?,
        label: row_string(row, "label").context("read graph canvas relation label")?,
        evidence_ids: row_string_array(row, "evidence_ids_json")
            .context("read graph canvas relation evidence refs")?,
        confidence: parse_optional_f32(
            &row_string(row, "confidence").context("read graph canvas relation confidence")?,
        ),
        updated_at: row_i64(row, "updated_at")
            .context("read graph canvas relation updated at")?
            .max(0) as u64,
    })
}

fn load_graph_canvas_wiki_pages(graph: &Graph, workspace_id: &str) -> Result<Vec<WikiPage>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id,
                    path,
                    title,
                    body,
                    evidence_refs_json,
                    updated_at
             FROM wiki_pages
             WHERE workspace_id = ?1
               AND approval_status IN ('materialized', 'approved')
             ORDER BY path ASC, wiki_page_id ASC",
        )
        .context("failed preparing graph canvas wiki page query")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying graph canvas wiki pages")?;
    let mut wiki_pages = Vec::new();
    while let Some(row) = rows
        .next()
        .context("failed reading graph canvas wiki row")?
    {
        wiki_pages.push(WikiPage {
            page_id: row.get(0).context("read graph canvas wiki id")?,
            workspace_id: workspace_id.into(),
            path: row.get(1).context("read graph canvas wiki path")?,
            title: row.get(2).context("read graph canvas wiki title")?,
            body: row.get(3).context("read graph canvas wiki body")?,
            node_refs: Vec::new(),
            source_refs: Vec::new(),
            evidence_refs: serde_json::from_str::<Vec<String>>(
                &row.get::<_, String>(4)
                    .context("read graph canvas wiki evidence refs")?,
            )
            .unwrap_or_default(),
            updated_at: row
                .get::<_, i64>(5)
                .context("read graph canvas wiki updated at")?
                .max(0) as u64,
        });
    }
    Ok(wiki_pages)
}

fn parse_brain_node_kind(kind: &str) -> BrainNodeKind {
    match kind {
        "source" => BrainNodeKind::Source,
        "memory" => BrainNodeKind::Memory,
        "wiki_page" => BrainNodeKind::WikiPage,
        "person" => BrainNodeKind::Person,
        "company" => BrainNodeKind::Company,
        "project" => BrainNodeKind::Project,
        "product" => BrainNodeKind::Product,
        "team" => BrainNodeKind::Team,
        "event" => BrainNodeKind::Event,
        "decision" => BrainNodeKind::Decision,
        "task" => BrainNodeKind::Task,
        "claim" => BrainNodeKind::Claim,
        "topic" => BrainNodeKind::Topic,
        _ => BrainNodeKind::Concept,
    }
}

fn parse_brain_relation_kind(kind: &str) -> BrainRelationKind {
    match kind {
        "mentions" => BrainRelationKind::Mentions,
        "supports" => BrainRelationKind::Supports,
        "contradicts" => BrainRelationKind::Contradicts,
        "supersedes" => BrainRelationKind::Supersedes,
        "same_as" => BrainRelationKind::SameAs,
        "works_at" => BrainRelationKind::WorksAt,
        "founded" => BrainRelationKind::Founded,
        "invested_in" => BrainRelationKind::InvestedIn,
        "advises" => BrainRelationKind::Advises,
        "attended" => BrainRelationKind::Attended,
        "owns" => BrainRelationKind::Owns,
        "responsible_for" => BrainRelationKind::ResponsibleFor,
        "decided" => BrainRelationKind::Decided,
        "blocks" => BrainRelationKind::Blocks,
        "depends_on" => BrainRelationKind::DependsOn,
        "source_of" => BrainRelationKind::SourceOf,
        "derived_from" => BrainRelationKind::DerivedFrom,
        "cites" => BrainRelationKind::Cites,
        "links_to" => BrainRelationKind::LinksTo,
        _ => BrainRelationKind::RelatedTo,
    }
}

fn parse_brain_scope(scope: &str) -> BrainScope {
    match scope {
        "personal" => BrainScope::Personal,
        "team" => BrainScope::Team,
        "company" => BrainScope::Company,
        _ => BrainScope::Project,
    }
}

fn parse_optional_f32(value: &str) -> Option<f32> {
    if value.is_empty() {
        None
    } else {
        value.parse::<f32>().ok()
    }
}
