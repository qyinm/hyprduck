//! Read-side projections for the knowledge store facade.

use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
use etyma_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainScope,
    GraphSnapshotSourceRecord, PageEvidenceV0, ReadNodeResponseData, ReadPageEvidenceResponseData,
    ReadSourceResponseData, SourceFormat, SourceStatus, WikiPage,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use super::context_pack_store::{
    db_parse_confidence, load_context_pack_evidence_row,
    load_context_pack_evidence_rows_for_source, load_context_pack_source_row,
    load_evidence_refs_by_ids, load_evidence_refs_for_source, load_wiki_page_by_path,
    load_wiki_page_for_source, source_record_from_context_row,
};
use super::graph_snapshot_store::KnowledgeGraphPersistReport;
use super::row_decode::{non_empty_string, row_i64, row_string, row_string_array};
// (Retrieval policy moved to domains/retrieval/brain_search; see that module.
use crate::policy;

#[derive(Debug)]
struct NodeRelationRow {
    relation: BrainRelationRecord,
    source_ids: Vec<String>,
}

const READ_NODE_RELATION_LIMIT: usize = 64;
const READ_NODE_EVIDENCE_LIMIT: usize = 32;

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
    let Some(node) = load_graph_canvas_node_by_id(&graph, workspace_id, node_id)? else {
        return Ok(None);
    };
    let Some(node) = sanitize_read_node_agent_node(node) else {
        return Ok(None);
    };
    let evidence_ids = node.evidence_ids.clone();
    let source_ids = node.source_ids.clone();
    let limited_evidence_ids = evidence_ids
        .iter()
        .take(READ_NODE_EVIDENCE_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let mut evidence = load_evidence_refs_by_ids(&graph, workspace_id, &limited_evidence_ids)?;
    if evidence.is_empty() {
        for source_id in &source_ids {
            if evidence.len() >= READ_NODE_EVIDENCE_LIMIT {
                break;
            }
            let remaining = READ_NODE_EVIDENCE_LIMIT - evidence.len();
            evidence.extend(load_evidence_refs_for_source(
                &graph,
                workspace_id,
                source_id,
                Some(remaining as i64),
            )?);
        }
    }
    evidence.truncate(READ_NODE_EVIDENCE_LIMIT);
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
        "MATCH (source {logical_id: $node_id, workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_physical_node_id,
                source.logical_id AS source_logical_node_id,
                target.id AS target_physical_node_id,
                target.logical_id AS target_logical_node_id,
                r.source_logical_id AS relation_source_logical_id,
                r.target_logical_id AS relation_target_logical_id,
                source.valid_to AS source_valid_to,
                target.valid_to AS target_valid_to,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at,
                r.valid_from AS valid_from,
                r.valid_to AS valid_to,
                r.superseded_by AS superseded_by",
    )?;
    relation_rows.extend(load_node_relation_rows_from_db(
        graph,
        workspace_id,
        node_id,
        "MATCH (source {workspace_id: $workspace_id})-[r]->(target {logical_id: $node_id, workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_physical_node_id,
                source.logical_id AS source_logical_node_id,
                target.id AS target_physical_node_id,
                target.logical_id AS target_logical_node_id,
                r.source_logical_id AS relation_source_logical_id,
                r.target_logical_id AS relation_target_logical_id,
                source.valid_to AS source_valid_to,
                target.valid_to AS target_valid_to,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at,
                r.valid_from AS valid_from,
                r.valid_to AS valid_to,
                r.superseded_by AS superseded_by",
    )?);
    relation_rows.extend(load_node_relation_rows_from_db(
        graph,
        workspace_id,
        node_id,
        "MATCH (source {id: $node_id, workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_physical_node_id,
                source.logical_id AS source_logical_node_id,
                target.id AS target_physical_node_id,
                target.logical_id AS target_logical_node_id,
                r.source_logical_id AS relation_source_logical_id,
                r.target_logical_id AS relation_target_logical_id,
                source.valid_to AS source_valid_to,
                target.valid_to AS target_valid_to,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at,
                r.valid_from AS valid_from,
                r.valid_to AS valid_to,
                r.superseded_by AS superseded_by",
    )?);
    relation_rows.extend(load_node_relation_rows_from_db(
        graph,
        workspace_id,
        node_id,
        "MATCH (source {workspace_id: $workspace_id})-[r]->(target {id: $node_id, workspace_id: $workspace_id})
         RETURN r.relation_id AS relation_id,
                r.kind AS kind,
                source.id AS source_physical_node_id,
                source.logical_id AS source_logical_node_id,
                target.id AS target_physical_node_id,
                target.logical_id AS target_logical_node_id,
                r.source_logical_id AS relation_source_logical_id,
                r.target_logical_id AS relation_target_logical_id,
                source.valid_to AS source_valid_to,
                target.valid_to AS target_valid_to,
                r.label AS label,
                r.evidence_ids_json AS evidence_ids_json,
                r.source_ids_json AS source_ids_json,
                r.confidence AS confidence,
                r.updated_at AS updated_at,
                r.valid_from AS valid_from,
                r.valid_to AS valid_to,
                r.superseded_by AS superseded_by",
    )?);
    let (eligible_evidence_ids, eligible_source_ids) =
        load_node_relation_eligible_refs(graph, workspace_id, &relation_rows)?;
    let mut relations = relation_rows
        .into_iter()
        .filter(|row| read_node_relation_is_agent_safe(&row.relation))
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
    relations.truncate(READ_NODE_RELATION_LIMIT);
    Ok(relations)
}

fn read_node_relation_is_agent_safe(relation: &BrainRelationRecord) -> bool {
    policy::is_agent_text_safe(&relation.relation_id)
        && policy::is_agent_text_safe(&relation.label)
        && policy::is_agent_text_safe(&relation.source_node_id)
        && policy::is_agent_text_safe(&relation.target_node_id)
}

fn sanitize_read_node_agent_node(mut node: BrainNodeRecord) -> Option<BrainNodeRecord> {
    if !policy::is_agent_text_safe(&node.node_id) || !policy::is_agent_text_safe(&node.label) {
        return None;
    }
    node.aliases
        .retain(|alias| policy::is_agent_text_safe(alias));
    Some(node)
}

fn load_node_relation_rows_from_db(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
    cypher: &str,
) -> Result<Vec<NodeRelationRow>> {
    let cypher =
        format!("{cypher}\n ORDER BY r.relation_id ASC\n LIMIT {READ_NODE_RELATION_LIMIT}");
    let rows = graph
        .connection()
        .cypher_builder(&cypher)
        .param("workspace_id", workspace_id)
        .param("node_id", node_id)
        .run()
        .context("failed querying GraphQLite node relations")?;
    let mut relations = Vec::new();
    for row in &rows {
        if !row_live_by_valid_to(row, "source_valid_to")?
            || !row_live_by_valid_to(row, "target_valid_to")?
        {
            continue;
        }
        let relation = graph_canvas_relation_from_row(row)?;
        if !graph_relation_is_live(&relation) {
            continue;
        }
        relations.push(NodeRelationRow {
            relation,
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
    let node_count = load_graph_canvas_nodes(&graph, workspace_id)?.len();
    let relation_count = load_graph_canvas_relations(&graph, workspace_id)?.len();
    Ok(KnowledgeGraphPersistReport {
        node_count,
        relation_count,
    })
}

pub(crate) fn load_graph_canvas_nodes(
    graph: &Graph,
    workspace_id: &str,
) -> Result<Vec<BrainNodeRecord>> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n {workspace_id: $workspace_id})
             RETURN n.id AS physical_node_id,
                    n.logical_id AS logical_node_id,
                    n.kind AS kind,
                    n.label AS label,
                    n.scope AS scope,
                    n.aliases_json AS aliases_json,
                    n.evidence_ids_json AS evidence_ids_json,
                    n.source_ids_json AS source_ids_json,
                    n.confidence AS confidence,
                    n.updated_at AS updated_at,
                    n.valid_from AS valid_from,
                    n.valid_to AS valid_to,
                    n.superseded_by AS superseded_by",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed querying GraphQLite graph canvas nodes")?;
    let mut nodes = rows
        .iter()
        .map(graph_canvas_node_from_row)
        .collect::<Result<Vec<_>>>()?;
    nodes.retain(graph_node_is_live);
    nodes.sort_by(graph_canvas_node_projection_order);
    nodes.dedup_by(|left, right| left.node_id == right.node_id);
    Ok(nodes)
}

fn load_graph_canvas_node_by_id(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
) -> Result<Option<BrainNodeRecord>> {
    Ok(load_graph_canvas_nodes(graph, workspace_id)?
        .into_iter()
        .find(|node| node.node_id == node_id))
}

fn graph_canvas_node_from_row(row: &graphqlite::Row) -> Result<BrainNodeRecord> {
    let logical_node_id =
        row_string(row, "logical_node_id").context("read graph canvas node logical id")?;
    let physical_node_id =
        row_string(row, "physical_node_id").context("read graph canvas node physical id")?;
    Ok(BrainNodeRecord {
        node_id: public_graph_id(&logical_node_id, &physical_node_id).into(),
        kind: parse_brain_node_kind(
            &row_string(row, "kind").context("read graph canvas node kind")?,
        ),
        label: row_string(row, "label").context("read graph canvas node label")?,
        scope: parse_brain_scope(
            &row_string(row, "scope").context("read graph canvas node scope")?,
        ),
        aliases: row_string_array(row, "aliases_json").context("read graph canvas node aliases")?,
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
        valid_from: row_i64(row, "valid_from")
            .context("read graph canvas node valid from")?
            .max(0) as u64,
        valid_to: positive_i64_as_u64(row_i64(row, "valid_to")?),
        superseded_by: non_empty_string(
            row_string(row, "superseded_by").context("read graph canvas node superseded by")?,
        ),
    })
}

pub(crate) fn load_graph_canvas_relations(
    graph: &Graph,
    workspace_id: &str,
) -> Result<Vec<BrainRelationRecord>> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (source {workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
             RETURN r.relation_id AS relation_id,
                    r.kind AS kind,
                    source.id AS source_physical_node_id,
                    source.logical_id AS source_logical_node_id,
                    target.id AS target_physical_node_id,
                    target.logical_id AS target_logical_node_id,
                    r.source_logical_id AS relation_source_logical_id,
                    r.target_logical_id AS relation_target_logical_id,
                    source.valid_to AS source_valid_to,
                    target.valid_to AS target_valid_to,
                    r.label AS label,
                    r.evidence_ids_json AS evidence_ids_json,
                    r.confidence AS confidence,
                    r.updated_at AS updated_at,
                    r.valid_from AS valid_from,
                    r.valid_to AS valid_to,
                    r.superseded_by AS superseded_by",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed querying GraphQLite graph canvas relations")?;
    let mut relations = Vec::new();
    for row in &rows {
        if !row_live_by_valid_to(row, "source_valid_to")?
            || !row_live_by_valid_to(row, "target_valid_to")?
        {
            continue;
        }
        let relation = graph_canvas_relation_from_row(row)?;
        if graph_relation_is_live(&relation) {
            relations.push(relation);
        }
    }
    relations.sort_by(graph_canvas_relation_projection_order);
    relations.dedup_by(|left, right| left.relation_id == right.relation_id);
    Ok(relations)
}

fn graph_canvas_node_projection_order(
    left: &BrainNodeRecord,
    right: &BrainNodeRecord,
) -> std::cmp::Ordering {
    left.node_id
        .cmp(&right.node_id)
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| right.valid_from.cmp(&left.valid_from))
        .then_with(|| right.evidence_ids.len().cmp(&left.evidence_ids.len()))
        .then_with(|| left.label.cmp(&right.label))
}

fn graph_canvas_relation_projection_order(
    left: &BrainRelationRecord,
    right: &BrainRelationRecord,
) -> std::cmp::Ordering {
    left.relation_id
        .cmp(&right.relation_id)
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| right.valid_from.cmp(&left.valid_from))
        .then_with(|| right.evidence_ids.len().cmp(&left.evidence_ids.len()))
        .then_with(|| left.source_node_id.cmp(&right.source_node_id))
        .then_with(|| left.target_node_id.cmp(&right.target_node_id))
        .then_with(|| left.label.cmp(&right.label))
}

fn graph_canvas_relation_from_row(row: &graphqlite::Row) -> Result<BrainRelationRecord> {
    let source_relation_logical_id = row_string(row, "relation_source_logical_id")
        .context("read graph canvas relation stored source")?;
    let source_logical_node_id =
        row_string(row, "source_logical_node_id").context("read graph canvas source logical id")?;
    let source_physical_node_id = row_string(row, "source_physical_node_id")
        .context("read graph canvas source physical id")?;
    let target_relation_logical_id = row_string(row, "relation_target_logical_id")
        .context("read graph canvas relation stored target")?;
    let target_logical_node_id =
        row_string(row, "target_logical_node_id").context("read graph canvas target logical id")?;
    let target_physical_node_id = row_string(row, "target_physical_node_id")
        .context("read graph canvas target physical id")?;
    Ok(BrainRelationRecord {
        relation_id: row_string(row, "relation_id").context("read graph canvas relation id")?,
        kind: parse_brain_relation_kind(
            &row_string(row, "kind").context("read graph canvas relation kind")?,
        ),
        source_node_id: public_graph_id(
            public_graph_id(&source_relation_logical_id, &source_logical_node_id),
            &source_physical_node_id,
        )
        .into(),
        target_node_id: public_graph_id(
            public_graph_id(&target_relation_logical_id, &target_logical_node_id),
            &target_physical_node_id,
        )
        .into(),
        label: row_string(row, "label").context("read graph canvas relation label")?,
        evidence_ids: row_string_array(row, "evidence_ids_json")
            .context("read graph canvas relation evidence refs")?,
        confidence: parse_optional_f32(
            &row_string(row, "confidence").context("read graph canvas relation confidence")?,
        ),
        updated_at: row_i64(row, "updated_at")
            .context("read graph canvas relation updated at")?
            .max(0) as u64,
        valid_from: row_i64(row, "valid_from")
            .context("read graph canvas relation valid from")?
            .max(0) as u64,
        valid_to: positive_i64_as_u64(row_i64(row, "valid_to")?),
        superseded_by: non_empty_string(
            row_string(row, "superseded_by").context("read graph canvas relation superseded by")?,
        ),
    })
}

fn public_graph_id<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    if preferred.trim().is_empty() {
        fallback
    } else {
        preferred
    }
}

fn positive_i64_as_u64(value: i64) -> Option<u64> {
    (value > 0).then_some(value as u64)
}

fn row_live_by_valid_to(row: &graphqlite::Row, column: &str) -> Result<bool> {
    Ok(row_i64(row, column).with_context(|| format!("read {column}"))? <= 0)
}

fn graph_node_is_live(node: &BrainNodeRecord) -> bool {
    node.valid_to.is_none()
}

fn graph_relation_is_live(relation: &BrainRelationRecord) -> bool {
    relation.valid_to.is_none()
}

pub(crate) fn load_graph_canvas_wiki_pages(
    graph: &Graph,
    workspace_id: &str,
) -> Result<Vec<WikiPage>> {
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
               AND valid_to <= 0
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
