use anyhow::{Context, Result};
use graphqlite::Graph;
#[cfg(test)]
use hyprduck_engine_types::{
    BrainActor, BrainActorType, BrainEventCausality, BrainEventKind, BrainNodeKind,
    BrainRelationKind, BrainScope, ClaimRecord, EntityRecord, PolicyResult,
    StructuredExtractionArtifact, BRAIN_EVENT_SCHEMA_VERSION,
};
use hyprduck_engine_types::{
    BrainContextPack, BrainEvent, BrainNodeRecord, BrainRelationRecord, BrainRepoSnapshot,
    BrainSearchResult, ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0,
    ContextPackEvidenceV1, ContextPackGraphFollowUpArgumentsV1, ContextPackGraphFollowUpToolV1,
    ContextPackGraphFollowUpV1, ContextPackGraphHandleTypeV1, ContextPackGraphReadNodeArgumentsV1,
    ContextPackGraphReadPageEvidenceArgumentsV1, ContextPackGraphReadSourceArgumentsV1,
    ContextPackGraphReadWikiPageArgumentsV1, ContextPackGraphRecordKindV1,
    ContextPackGraphRecordV1, ContextPackGraphTrailV1, ContextPackSourceMetadataV0, ContextPackV1,
    EvidenceRef, GraphSnapshotSourceRecord, ImportJobRecord, KnowledgeProject,
    ReadNodeResponseData, ReadPageEvidenceResponseData, ReadSourceResponseData,
    SourceArtifactManifest, SourceFormat, SourceRecord, SourceStatus, WikiPage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
use super::agent_write_store::load_brain_event_operation;
pub(crate) use super::agent_write_store::AgentWriteProposalRecord;
use super::agent_write_store::{
    list_pending_agent_write_proposals, load_agent_write_proposal, persist_agent_write_proposal,
    record_agent_write_commit, update_agent_write_proposal_status,
};
use super::artifact_store::{
    preserve_artifact_metadata_in_transaction, preserve_context_pack_exports_in_transaction,
};
use super::context_pack_store::{
    db_context_evidence_type, db_parse_confidence, load_context_pack_evidence_row,
    load_context_pack_source_row,
};
use super::graph_snapshot_store::persist_graph_snapshot_in_transaction;
pub(crate) use super::graph_snapshot_store::KnowledgeGraphPersistReport;
#[cfg(test)]
use super::graph_snapshot_store::GRAPHQLITE_SCHEMA_VERSION;
use super::import_job_store;
use super::read_projection_store::{
    graph_snapshot_counts, hybrid_retrieve_from_db, read_graph_canvas_projection_from_db,
    read_graph_snapshot_sources_from_db, read_node_from_db, read_page_evidence_from_db,
    read_source_from_db, read_wiki_page_from_db, resolve_evidence_proof, search_brain_from_db,
    RelationalEvidenceProof,
};
use super::row_decode::{non_empty_string, row_string, row_string_array};
use super::schema_store::{
    count_rows_for_workspace, ensure_schema, schema_version, validate_graphqlite_gate,
    GraphqliteGateReport, KnowledgeStoreHealth, KnowledgeStoreStateSummary, KNOWLEDGE_DB_FILE_NAME,
};
#[cfg(test)]
use super::search_store::EvidenceQueryIntent;
use super::search_store::HybridRetrievalHit;
use super::source_manifest_store::persist_source_manifest_in_transaction;

#[derive(Debug, Clone)]
pub(crate) struct KnowledgeStore {
    path: PathBuf,
}

impl KnowledgeStore {
    pub(crate) fn default_path_for_root(root: &Path) -> PathBuf {
        root.join(KNOWLEDGE_DB_FILE_NAME)
    }

    pub(crate) fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        let store = Self { path };
        ensure_schema(&store.path)?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn health(&self) -> Result<KnowledgeStoreHealth> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let gate: GraphqliteGateReport = validate_graphqlite_gate(&graph)?;
        Ok(KnowledgeStoreHealth {
            db_path: self.path.display().to_string(),
            db_schema_version: schema_version(&self.path)?,
            graph_schema_version: gate.graph_schema_version,
            graphqlite_loaded: gate.loaded,
            graphqlite_transactional: gate.rollback_ready,
        })
    }

    pub(crate) fn state_summary(&self, workspace_id: &str) -> Result<KnowledgeStoreStateSummary> {
        let graph_counts = self.graph_snapshot_counts(workspace_id)?;
        Ok(KnowledgeStoreStateSummary {
            evidence_item_count: count_rows_for_workspace(
                &self.path,
                "evidence_items",
                workspace_id,
            )?,
            wiki_page_count: count_rows_for_workspace(&self.path, "wiki_pages", workspace_id)?,
            graph_node_count: graph_counts.node_count,
            graph_relation_count: graph_counts.relation_count,
        })
    }

    pub(crate) fn read_import_job(
        &self,
        workspace_id: &str,
        job_id: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<Option<ImportJobRecord>> {
        ensure_schema(&self.path)?;
        import_job_store::read_import_job(&self.path, workspace_id, job_id, source_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_import_job_graph_status_from_mcp(
        &self,
        workspace_id: &str,
        source_id: &str,
        status: &str,
        graph_status: &str,
        graph_error_category: Option<&str>,
        graph_error_message_redacted: Option<&str>,
        graph_retryable: bool,
        graph_retry_attempt: u8,
        graph_max_retry_attempts: u8,
        graph_next_retry_at: Option<u64>,
        manual_retry_available: bool,
    ) -> Result<bool> {
        ensure_schema(&self.path)?;
        import_job_store::update_import_job_graph_status_from_mcp(
            &self.path,
            workspace_id,
            source_id,
            status,
            graph_status,
            graph_error_category,
            graph_error_message_redacted,
            graph_retryable,
            graph_retry_attempt,
            graph_max_retry_attempts,
            graph_next_retry_at,
            manual_retry_available,
        )
    }

    pub(crate) fn preserve_artifact_metadata(
        &self,
        metadata: &ContextPackArtifactMetadataV0,
    ) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting artifact metadata preservation transaction")?;

        let result = preserve_artifact_metadata_in_transaction(&graph, metadata);
        match result {
            Ok(()) => {
                sqlite
                    .execute_batch("COMMIT")
                    .context("failed committing artifact metadata preservation")?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlite.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn preserve_context_pack_exports(
        &self,
        workspace_root: &Path,
        workspace_id: &str,
    ) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting context pack export preservation transaction")?;

        let result =
            preserve_context_pack_exports_in_transaction(&graph, workspace_root, workspace_id);
        match result {
            Ok(()) => {
                sqlite
                    .execute_batch("COMMIT")
                    .context("failed committing context pack export preservation")?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlite.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn persist_graph_snapshot(
        &self,
        snapshot: &BrainRepoSnapshot,
    ) -> Result<KnowledgeGraphPersistReport> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        graph
            .connection()
            .sqlite_connection()
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting GraphQLite graph snapshot transaction")?;

        let result = persist_graph_snapshot_in_transaction(&graph, snapshot);
        match result {
            Ok(report) => {
                graph
                    .connection()
                    .sqlite_connection()
                    .execute_batch("COMMIT")
                    .context("failed committing GraphQLite graph snapshot")?;
                Ok(report)
            }
            Err(error) => {
                let _ = graph
                    .connection()
                    .sqlite_connection()
                    .execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn persist_source_manifest(
        &self,
        project: &KnowledgeProject,
        manifest: &SourceArtifactManifest,
    ) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting source manifest transaction")?;

        let result = persist_source_manifest_in_transaction(&graph, project, manifest);
        match result {
            Ok(()) => {
                sqlite
                    .execute_batch("COMMIT")
                    .context("failed committing source manifest transaction")?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlite.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn persist_agent_write_proposal(
        &self,
        proposal: &AgentWriteProposalRecord,
    ) -> Result<()> {
        persist_agent_write_proposal(&self.path, proposal)
    }

    pub(crate) fn load_agent_write_proposal(
        &self,
        workspace_id: &str,
        proposal_id: &str,
    ) -> Result<Option<AgentWriteProposalRecord>> {
        load_agent_write_proposal(&self.path, workspace_id, proposal_id)
    }

    pub(crate) fn list_pending_agent_write_proposals(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<AgentWriteProposalRecord>> {
        list_pending_agent_write_proposals(&self.path, workspace_id)
    }

    pub(crate) fn update_agent_write_proposal_status(
        &self,
        workspace_id: &str,
        proposal_id: &str,
        approval_status: &str,
        updated_at: i64,
    ) -> Result<()> {
        update_agent_write_proposal_status(
            &self.path,
            workspace_id,
            proposal_id,
            approval_status,
            updated_at,
        )
    }

    pub(crate) fn record_agent_write_commit(
        &self,
        workspace_id: &str,
        proposal_id: &str,
        event: &BrainEvent,
        updated_at: i64,
    ) -> Result<()> {
        record_agent_write_commit(&self.path, workspace_id, proposal_id, event, updated_at)
    }

    #[cfg(test)]
    pub(crate) fn load_brain_event_operation(
        &self,
        workspace_id: &str,
        event_id: &str,
    ) -> Result<Option<String>> {
        load_brain_event_operation(&self.path, workspace_id, event_id)
    }

    #[allow(dead_code)]
    pub(crate) fn hybrid_retrieve(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HybridRetrievalHit>> {
        hybrid_retrieve_from_db(&self.path, workspace_id, query, limit)
    }

    #[allow(dead_code)]
    pub(crate) fn assemble_context_pack_v1_from_db(
        &self,
        workspace_id: &str,
        query: &str,
        budget: usize,
        pack_id: String,
        generated_at: String,
    ) -> Result<ContextPackV1> {
        let limit = budget.clamp(1, 24);
        let hits = self
            .hybrid_retrieve(workspace_id, query, limit)
            .context("failed retrieving DB-backed context pack evidence")?;
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;

        let mut evidence_rows = Vec::new();
        for hit in &hits {
            if let Some(row) =
                load_context_pack_evidence_row(&graph, workspace_id, &hit.evidence_id)?
            {
                evidence_rows.push(row);
            }
        }

        let source_ids = evidence_rows
            .iter()
            .map(|row| row.source_id.clone())
            .collect::<BTreeSet<_>>();
        let mut sources = Vec::new();
        let mut source_metadata = BTreeMap::new();
        for source_id in source_ids {
            let Some(source) = load_context_pack_source_row(&graph, workspace_id, &source_id)?
            else {
                continue;
            };
            source_metadata.insert(
                source.source_id.clone(),
                ContextPackSourceMetadataV0 {
                    content_hash: source.content_hash.clone(),
                    provider_route: source.provider_route.clone(),
                    local_only: source.provider_locality != "hosted",
                },
            );
            sources.push(SourceRecord {
                source_id: source.source_id,
                workspace_id: source.workspace_id,
                original_path: source.original_path_redacted,
                source_path: source.source_path_redacted,
                markdown_path: source.markdown_path_redacted,
                format: SourceFormat::from(source.format),
                status: SourceStatus::from(source.status),
                page_count: source.page_count.max(0) as usize,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: source.updated_at.max(0) as u64,
            });
        }

        let mut evidence_metadata: BTreeMap<
            String,
            BTreeMap<String, ContextPackEvidenceMetadataV0>,
        > = BTreeMap::new();
        let evidence = evidence_rows
            .into_iter()
            .filter_map(|row| {
                if !source_metadata.contains_key(&row.source_id) {
                    return None;
                }
                let page = row.page_index.unwrap_or(0).max(0) as usize + 1;
                let metadata = ContextPackEvidenceMetadataV0 {
                    source_id: row.source_id.clone(),
                    page,
                    region: None,
                    span: None,
                    quoted_text: row.snippet.clone(),
                    parse_confidence: db_parse_confidence(row.confidence),
                    content_hash: source_metadata
                        .get(&row.source_id)
                        .map(|metadata| metadata.content_hash.clone())
                        .unwrap_or_default(),
                    markdown_path: non_empty_string(row.markdown_path_redacted.clone()),
                    image_path: non_empty_string(row.image_path_redacted.clone()),
                    evidence_type: db_context_evidence_type(&row.evidence_type),
                };
                evidence_metadata
                    .entry(row.source_id.clone())
                    .or_default()
                    .insert(row.evidence_id.clone(), metadata);
                Some(EvidenceRef {
                    id: row.evidence_id,
                    page_label: row.page_label,
                    page_index: row.page_index.map(|page_index| page_index.max(0) as usize),
                    snippet: row.snippet,
                    source_path: non_empty_string(row.source_path_redacted),
                    source_id: Some(row.source_id),
                    markdown_path: non_empty_string(row.markdown_path_redacted),
                    image_path: non_empty_string(row.image_path_redacted),
                    provenance: non_empty_string(row.provenance),
                })
            })
            .collect::<Vec<_>>();

        let mut warnings = Vec::new();
        if evidence.is_empty() {
            warnings.push("No active DB evidence matched the Context Pack query.".into());
        }

        let pack = BrainContextPack {
            workspace_id: workspace_id.into(),
            query: query.into(),
            token_budget: budget,
            summary: format!(
                "Context assembled from {} DB evidence item(s) using SQLite FTS5 retrieval and GraphQLite graph expansion.",
                evidence.len()
            ),
            wiki_pages: Vec::new(),
            nodes: Vec::new(),
            sources,
            memories: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            relations: Vec::new(),
            evidence,
            recent_events: Vec::new(),
            warnings,
        };
        let artifact_metadata = ContextPackArtifactMetadataV0 {
            sources: source_metadata,
            evidence: evidence_metadata,
            warnings: Vec::new(),
        };
        let mut context_pack = ContextPackV1::from_brain_context_pack(
            &pack,
            pack_id,
            generated_at,
            &artifact_metadata,
        );
        context_pack.retrieval_trace.strategy = "sqlite-graphqlite-fts5-hybrid".into();
        attach_context_pack_graph_trails(&graph, workspace_id, &mut context_pack)?;
        Ok(context_pack)
    }

    #[allow(dead_code)]
    pub(crate) fn read_source_from_db(
        &self,
        workspace_id: &str,
        source_id: &str,
        include_local_paths: bool,
    ) -> Result<Option<ReadSourceResponseData>> {
        read_source_from_db(&self.path, workspace_id, source_id, include_local_paths)
    }

    #[allow(dead_code)]
    pub(crate) fn read_page_evidence_from_db(
        &self,
        workspace_id: &str,
        source_id: &str,
        page: Option<usize>,
        include_local_paths: bool,
    ) -> Result<Option<ReadPageEvidenceResponseData>> {
        read_page_evidence_from_db(
            &self.path,
            workspace_id,
            source_id,
            page,
            include_local_paths,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn read_wiki_page_from_db(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<Option<WikiPage>> {
        read_wiki_page_from_db(&self.path, workspace_id, path)
    }

    #[allow(dead_code)]
    pub(crate) fn read_node_from_db(
        &self,
        workspace_id: &str,
        node_id: &str,
    ) -> Result<Option<ReadNodeResponseData>> {
        read_node_from_db(&self.path, workspace_id, node_id)
    }

    #[allow(dead_code)]
    pub(crate) fn read_graph_canvas_projection_from_db(
        &self,
        workspace_id: &str,
    ) -> Result<
        Option<(
            Vec<BrainNodeRecord>,
            Vec<BrainRelationRecord>,
            Vec<WikiPage>,
        )>,
    > {
        read_graph_canvas_projection_from_db(&self.path, workspace_id)
    }

    pub(crate) fn read_graph_snapshot_sources_from_db(
        &self,
        workspace_id: &str,
        include_local_paths: bool,
    ) -> Result<Vec<GraphSnapshotSourceRecord>> {
        read_graph_snapshot_sources_from_db(&self.path, workspace_id, include_local_paths)
    }

    pub(crate) fn search_brain_from_db(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<BrainSearchResult>> {
        search_brain_from_db(&self.path, workspace_id, query, limit)
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_evidence_proof(
        &self,
        workspace_id: &str,
        evidence_id: &str,
    ) -> Result<RelationalEvidenceProof> {
        resolve_evidence_proof(&self.path, workspace_id, evidence_id)
    }

    pub(crate) fn graph_snapshot_counts(
        &self,
        workspace_id: &str,
    ) -> Result<KnowledgeGraphPersistReport> {
        graph_snapshot_counts(&self.path, workspace_id)
    }
}

const GRAPH_TRAIL_DIRECT_LIMIT: usize = 8;
const GRAPH_TRAIL_ADJACENT_LIMIT: usize = 8;
const GRAPH_TRAIL_FOLLOW_UP_LIMIT: usize = 8;

#[derive(Debug, Clone)]
struct GraphTrailNode {
    node_id: String,
    kind: String,
    label: String,
    aliases: Vec<String>,
    evidence_ids: Vec<String>,
    source_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct GraphTrailRelation {
    relation_id: String,
    label: String,
    evidence_ids: Vec<String>,
    source_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct GraphTrailRelationLink {
    relation: GraphTrailRelation,
    source: GraphTrailNode,
    target: GraphTrailNode,
}

fn attach_context_pack_graph_trails(
    graph: &Graph,
    workspace_id: &str,
    context_pack: &mut ContextPackV1,
) -> Result<()> {
    for evidence in &mut context_pack.selected_evidence {
        evidence.graph_trail = build_context_pack_graph_trail(graph, workspace_id, evidence)?;
    }
    Ok(())
}

fn build_context_pack_graph_trail(
    graph: &Graph,
    workspace_id: &str,
    evidence: &ContextPackEvidenceV1,
) -> Result<Option<ContextPackGraphTrailV1>> {
    let mut direct = Vec::new();
    let mut adjacent = Vec::new();
    let mut follow_up = Vec::new();
    let mut direct_keys = BTreeSet::new();
    let mut adjacent_keys = BTreeSet::new();
    let mut follow_up_keys = BTreeSet::new();

    let direct_nodes =
        load_graph_trail_nodes_for_evidence(graph, workspace_id, &evidence.evidence_ref)?
            .into_iter()
            .filter(|node| graph_trail_node_is_eligible(graph, workspace_id, node).unwrap_or(false))
            .collect::<Vec<_>>();
    for node in &direct_nodes {
        push_graph_record(
            &mut direct,
            &mut direct_keys,
            GRAPH_TRAIL_DIRECT_LIMIT,
            graph_record_kind_for_node(&node.kind),
            node.node_id.clone(),
            format!(
                "Selected evidence directly supports graph node '{}'.",
                node.label
            ),
        );
    }

    let mut direct_relation_ids = BTreeSet::new();
    let direct_relation_links =
        load_graph_trail_relations_for_evidence(graph, workspace_id, &evidence.evidence_ref)?
            .into_iter()
            .filter(|link| {
                graph_trail_relation_is_eligible(graph, workspace_id, &link.relation)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
    for link in &direct_relation_links {
        direct_relation_ids.insert(link.relation.relation_id.clone());
        push_graph_record(
            &mut direct,
            &mut direct_keys,
            GRAPH_TRAIL_DIRECT_LIMIT,
            ContextPackGraphRecordKindV1::Relation,
            link.relation.relation_id.clone(),
            format!(
                "Selected evidence directly supports relation '{}'.",
                link.relation.label
            ),
        );
        push_adjacent_node_from_relation_endpoint(
            graph,
            workspace_id,
            &link.source,
            &mut adjacent,
            &mut adjacent_keys,
        );
        push_adjacent_node_from_relation_endpoint(
            graph,
            workspace_id,
            &link.target,
            &mut adjacent,
            &mut adjacent_keys,
        );
    }

    for node in &direct_nodes {
        for link in load_graph_trail_relation_links_for_node(graph, workspace_id, &node.node_id)? {
            if direct_relation_ids.contains(&link.relation.relation_id)
                || !graph_trail_relation_is_eligible(graph, workspace_id, &link.relation)?
            {
                continue;
            }
            push_graph_record(
                &mut adjacent,
                &mut adjacent_keys,
                GRAPH_TRAIL_ADJACENT_LIMIT,
                ContextPackGraphRecordKindV1::Relation,
                link.relation.relation_id.clone(),
                format!(
                    "Relation '{}' is adjacent to directly supported node '{}'.",
                    link.relation.label, node.label
                ),
            );
            let neighbor = if link.source.node_id == node.node_id {
                &link.target
            } else {
                &link.source
            };
            push_adjacent_node_from_relation_endpoint(
                graph,
                workspace_id,
                neighbor,
                &mut adjacent,
                &mut adjacent_keys,
            );
        }
    }

    if direct.is_empty() && adjacent.is_empty() {
        return Ok(None);
    }

    push_context_pack_follow_up(
        &mut follow_up,
        &mut follow_up_keys,
        ContextPackGraphFollowUpV1 {
            tool: ContextPackGraphFollowUpToolV1::ReadSource,
            handle_type: ContextPackGraphHandleTypeV1::Source,
            arguments: ContextPackGraphFollowUpArgumentsV1::ReadSource(
                ContextPackGraphReadSourceArgumentsV1 {
                    source_id: evidence.source_id.clone(),
                },
            ),
            reason: "Read the source that contains this selected evidence.".into(),
        },
    );
    push_context_pack_follow_up(
        &mut follow_up,
        &mut follow_up_keys,
        ContextPackGraphFollowUpV1 {
            tool: ContextPackGraphFollowUpToolV1::ReadPageEvidence,
            handle_type: ContextPackGraphHandleTypeV1::PageEvidence,
            arguments: ContextPackGraphFollowUpArgumentsV1::ReadPageEvidence(
                ContextPackGraphReadPageEvidenceArgumentsV1 {
                    source_id: evidence.source_id.clone(),
                    page: evidence.page,
                },
            ),
            reason: "Read the page evidence behind this graph-linked selection.".into(),
        },
    );
    for node in direct_nodes.iter().chain(
        direct_relation_links
            .iter()
            .flat_map(|link| [&link.source, &link.target]),
    ) {
        push_follow_up_for_graph_node(node, &mut follow_up, &mut follow_up_keys);
    }

    Ok(Some(ContextPackGraphTrailV1 {
        direct,
        adjacent,
        follow_up,
        unavailable_reason: None,
    }))
}

fn push_adjacent_node_from_relation_endpoint(
    graph: &Graph,
    workspace_id: &str,
    node: &GraphTrailNode,
    adjacent: &mut Vec<ContextPackGraphRecordV1>,
    adjacent_keys: &mut BTreeSet<String>,
) {
    if !graph_trail_node_is_eligible(graph, workspace_id, node).unwrap_or(false) {
        return;
    }
    push_graph_record(
        adjacent,
        adjacent_keys,
        GRAPH_TRAIL_ADJACENT_LIMIT,
        graph_record_kind_for_node(&node.kind),
        node.node_id.clone(),
        format!(
            "Graph node '{}' is adjacent to directly supported relation context.",
            node.label
        ),
    );
}

fn push_graph_record(
    records: &mut Vec<ContextPackGraphRecordV1>,
    seen: &mut BTreeSet<String>,
    limit: usize,
    record_type: ContextPackGraphRecordKindV1,
    id: String,
    reason: String,
) {
    if records.len() >= limit {
        return;
    }
    let key = format!("{record_type:?}:{id}");
    if !seen.insert(key) {
        return;
    }
    records.push(ContextPackGraphRecordV1 {
        record_type,
        id,
        reason,
    });
}

fn push_follow_up_for_graph_node(
    node: &GraphTrailNode,
    follow_up: &mut Vec<ContextPackGraphFollowUpV1>,
    seen: &mut BTreeSet<String>,
) {
    match graph_record_kind_for_node(&node.kind) {
        ContextPackGraphRecordKindV1::Node => push_context_pack_follow_up(
            follow_up,
            seen,
            ContextPackGraphFollowUpV1 {
                tool: ContextPackGraphFollowUpToolV1::ReadNode,
                handle_type: ContextPackGraphHandleTypeV1::Node,
                arguments: ContextPackGraphFollowUpArgumentsV1::ReadNode(
                    ContextPackGraphReadNodeArgumentsV1 {
                        node_id: node.node_id.clone(),
                    },
                ),
                reason: format!("Inspect graph node '{}'.", node.label),
            },
        ),
        ContextPackGraphRecordKindV1::Source => {
            let source_id = node
                .source_ids
                .first()
                .cloned()
                .or_else(|| node.node_id.strip_prefix("source:").map(ToOwned::to_owned));
            if let Some(source_id) = source_id {
                push_context_pack_follow_up(
                    follow_up,
                    seen,
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadSource,
                        handle_type: ContextPackGraphHandleTypeV1::Source,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadSource(
                            ContextPackGraphReadSourceArgumentsV1 { source_id },
                        ),
                        reason: format!("Read source node '{}'.", node.label),
                    },
                );
            }
        }
        ContextPackGraphRecordKindV1::WikiPage => {
            if let Some(path) = node
                .aliases
                .iter()
                .find(|path| is_safe_agent_wiki_path(path))
            {
                push_context_pack_follow_up(
                    follow_up,
                    seen,
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadWikiPage,
                        handle_type: ContextPackGraphHandleTypeV1::WikiPage,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadWikiPage(
                            ContextPackGraphReadWikiPageArgumentsV1 { path: path.clone() },
                        ),
                        reason: format!("Read wiki page '{}'.", node.label),
                    },
                );
            }
        }
        ContextPackGraphRecordKindV1::Claim
        | ContextPackGraphRecordKindV1::Relation
        | ContextPackGraphRecordKindV1::Evidence => {}
    }
}

fn push_context_pack_follow_up(
    follow_up: &mut Vec<ContextPackGraphFollowUpV1>,
    seen: &mut BTreeSet<String>,
    item: ContextPackGraphFollowUpV1,
) {
    if follow_up.len() >= GRAPH_TRAIL_FOLLOW_UP_LIMIT {
        return;
    }
    let key = format!(
        "{:?}:{:?}:{:?}",
        item.tool, item.handle_type, item.arguments
    );
    if seen.insert(key) {
        follow_up.push(item);
    }
}

fn load_graph_trail_nodes_for_evidence(
    graph: &Graph,
    workspace_id: &str,
    evidence_id: &str,
) -> Result<Vec<GraphTrailNode>> {
    let rows = graph
        .connection()
        .cypher_builder(
            "MATCH (n {workspace_id: $workspace_id})
             RETURN n.id AS node_id,
                    n.kind AS kind,
                    n.label AS label,
                    n.aliases_json AS aliases_json,
                    n.evidence_ids_json AS evidence_ids_json,
                    n.source_ids_json AS source_ids_json,
                    n.status AS status",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed querying GraphQLite graph trail nodes")?;
    let mut nodes = Vec::new();
    for row in &rows {
        if !graph_record_status_is_active(&row_string(row, "status")?) {
            continue;
        }
        let evidence_ids = row_string_array(row, "evidence_ids_json")?;
        if !evidence_ids.iter().any(|value| value == evidence_id) {
            continue;
        }
        nodes.push(GraphTrailNode {
            node_id: row_string(row, "node_id")?,
            kind: row_string(row, "kind")?,
            label: row_string(row, "label")?,
            aliases: row_string_array(row, "aliases_json")?,
            evidence_ids,
            source_ids: row_string_array(row, "source_ids_json")?,
        });
    }
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    nodes.truncate(GRAPH_TRAIL_DIRECT_LIMIT);
    Ok(nodes)
}

fn load_graph_trail_relations_for_evidence(
    graph: &Graph,
    workspace_id: &str,
    evidence_id: &str,
) -> Result<Vec<GraphTrailRelationLink>> {
    let mut links = load_graph_trail_relation_links(
        graph,
        workspace_id,
        "MATCH (source {workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
         RETURN source.id AS source_node_id,
                source.kind AS source_kind,
                source.label AS source_label,
                source.aliases_json AS source_aliases_json,
                source.evidence_ids_json AS source_evidence_ids_json,
                source.source_ids_json AS source_source_ids_json,
                target.id AS target_node_id,
                target.kind AS target_kind,
                target.label AS target_label,
                target.aliases_json AS target_aliases_json,
                target.evidence_ids_json AS target_evidence_ids_json,
                target.source_ids_json AS target_source_ids_json,
                r.relation_id AS relation_id,
                r.label AS relation_label,
                r.evidence_ids_json AS relation_evidence_ids_json,
                r.source_ids_json AS relation_source_ids_json,
                r.status AS relation_status",
        None,
    )?;
    links.retain(|link| {
        link.relation
            .evidence_ids
            .iter()
            .any(|value| value == evidence_id)
    });
    links.truncate(GRAPH_TRAIL_DIRECT_LIMIT);
    Ok(links)
}

fn load_graph_trail_relation_links_for_node(
    graph: &Graph,
    workspace_id: &str,
    node_id: &str,
) -> Result<Vec<GraphTrailRelationLink>> {
    let mut links = load_graph_trail_relation_links(
        graph,
        workspace_id,
        "MATCH (source {id: $node_id, workspace_id: $workspace_id})-[r]->(target {workspace_id: $workspace_id})
         RETURN source.id AS source_node_id,
                source.kind AS source_kind,
                source.label AS source_label,
                source.aliases_json AS source_aliases_json,
                source.evidence_ids_json AS source_evidence_ids_json,
                source.source_ids_json AS source_source_ids_json,
                target.id AS target_node_id,
                target.kind AS target_kind,
                target.label AS target_label,
                target.aliases_json AS target_aliases_json,
                target.evidence_ids_json AS target_evidence_ids_json,
                target.source_ids_json AS target_source_ids_json,
                r.relation_id AS relation_id,
                r.label AS relation_label,
                r.evidence_ids_json AS relation_evidence_ids_json,
                r.source_ids_json AS relation_source_ids_json,
                r.status AS relation_status",
        Some(node_id),
    )?;
    links.extend(load_graph_trail_relation_links(
        graph,
        workspace_id,
        "MATCH (source {workspace_id: $workspace_id})-[r]->(target {id: $node_id, workspace_id: $workspace_id})
         RETURN source.id AS source_node_id,
                source.kind AS source_kind,
                source.label AS source_label,
                source.aliases_json AS source_aliases_json,
                source.evidence_ids_json AS source_evidence_ids_json,
                source.source_ids_json AS source_source_ids_json,
                target.id AS target_node_id,
                target.kind AS target_kind,
                target.label AS target_label,
                target.aliases_json AS target_aliases_json,
                target.evidence_ids_json AS target_evidence_ids_json,
                target.source_ids_json AS target_source_ids_json,
                r.relation_id AS relation_id,
                r.label AS relation_label,
                r.evidence_ids_json AS relation_evidence_ids_json,
                r.source_ids_json AS relation_source_ids_json,
                r.status AS relation_status",
        Some(node_id),
    )?);
    links.sort_by(|left, right| left.relation.relation_id.cmp(&right.relation.relation_id));
    links.dedup_by(|left, right| left.relation.relation_id == right.relation.relation_id);
    links.truncate(GRAPH_TRAIL_ADJACENT_LIMIT);
    Ok(links)
}

fn load_graph_trail_relation_links(
    graph: &Graph,
    workspace_id: &str,
    cypher: &str,
    node_id: Option<&str>,
) -> Result<Vec<GraphTrailRelationLink>> {
    let mut builder = graph
        .connection()
        .cypher_builder(cypher)
        .param("workspace_id", workspace_id);
    if let Some(node_id) = node_id {
        builder = builder.param("node_id", node_id);
    }
    let rows = builder
        .run()
        .context("failed querying GraphQLite graph trail relations")?;
    let mut links = Vec::new();
    for row in &rows {
        if !graph_record_status_is_active(&row_string(row, "relation_status")?) {
            continue;
        }
        links.push(GraphTrailRelationLink {
            relation: GraphTrailRelation {
                relation_id: row_string(row, "relation_id")?,
                label: row_string(row, "relation_label")?,
                evidence_ids: row_string_array(row, "relation_evidence_ids_json")?,
                source_ids: row_string_array(row, "relation_source_ids_json")?,
            },
            source: graph_trail_node_from_relation_row(row, "source")?,
            target: graph_trail_node_from_relation_row(row, "target")?,
        });
    }
    Ok(links)
}

fn graph_trail_node_from_relation_row(
    row: &graphqlite::Row,
    prefix: &str,
) -> Result<GraphTrailNode> {
    Ok(GraphTrailNode {
        node_id: row_string(row, &format!("{prefix}_node_id"))?,
        kind: row_string(row, &format!("{prefix}_kind"))?,
        label: row_string(row, &format!("{prefix}_label"))?,
        aliases: row_string_array(row, &format!("{prefix}_aliases_json"))?,
        evidence_ids: row_string_array(row, &format!("{prefix}_evidence_ids_json"))?,
        source_ids: row_string_array(row, &format!("{prefix}_source_ids_json"))?,
    })
}

fn graph_trail_node_is_eligible(
    graph: &Graph,
    workspace_id: &str,
    node: &GraphTrailNode,
) -> Result<bool> {
    graph_record_refs_are_eligible(graph, workspace_id, &node.evidence_ids, &node.source_ids)
}

fn graph_trail_relation_is_eligible(
    graph: &Graph,
    workspace_id: &str,
    relation: &GraphTrailRelation,
) -> Result<bool> {
    graph_record_refs_are_eligible(
        graph,
        workspace_id,
        &relation.evidence_ids,
        &relation.source_ids,
    )
}

fn graph_record_refs_are_eligible(
    graph: &Graph,
    workspace_id: &str,
    evidence_ids: &[String],
    source_ids: &[String],
) -> Result<bool> {
    if !evidence_ids.is_empty() {
        for evidence_id in evidence_ids {
            let Some(evidence_row) =
                load_context_pack_evidence_row(graph, workspace_id, evidence_id)?
            else {
                continue;
            };
            if load_context_pack_source_row(graph, workspace_id, &evidence_row.source_id)?.is_some()
            {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    for source_id in source_ids {
        if load_context_pack_source_row(graph, workspace_id, source_id)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn graph_record_kind_for_node(kind: &str) -> ContextPackGraphRecordKindV1 {
    match kind {
        "claim" => ContextPackGraphRecordKindV1::Claim,
        "wiki_page" => ContextPackGraphRecordKindV1::WikiPage,
        "source" => ContextPackGraphRecordKindV1::Source,
        _ => ContextPackGraphRecordKindV1::Node,
    }
}

fn graph_record_status_is_active(status: &str) -> bool {
    status.is_empty() || status == "active"
}

fn is_safe_agent_wiki_path(path: &str) -> bool {
    if !path.starts_with("wiki/") || path.contains("docs/private") {
        return false;
    }
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests;
