use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
#[cfg(test)]
use hyprduck_engine_types::{
    BrainActor, BrainActorType, BrainEventCausality, BrainEventKind, ClaimRecord, EntityRecord,
    PolicyResult, StructuredExtractionArtifact, BRAIN_EVENT_SCHEMA_VERSION,
};
use hyprduck_engine_types::{
    BrainContextPack, BrainEvent, BrainNodeKind, BrainNodeRecord, BrainRelationKind,
    BrainRelationRecord, BrainRepoSnapshot, BrainScope, BrainSearchResult, BrainSearchResultKind,
    ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackSourceMetadataV0,
    ContextPackV1, EvidenceRef, GraphSnapshotSourceRecord, ImportJobRecord, KnowledgeProject,
    PageEvidenceV0, ReadNodeResponseData, ReadPageEvidenceResponseData, ReadSourceResponseData,
    SourceArtifactManifest, SourceFormat, SourceRecord, SourceStatus, WikiPage,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::artifact_store::{
    preserve_artifact_metadata_in_transaction, preserve_context_pack_exports_in_transaction,
};
use super::context_pack_store::{
    db_context_evidence_type, db_parse_confidence, evidence_snippet_from_ids,
    load_context_pack_evidence_row, load_context_pack_evidence_rows_for_source,
    load_context_pack_source_row, load_evidence_refs_by_ids, load_evidence_refs_for_source,
    load_wiki_page_by_path, load_wiki_page_for_source, source_record_from_context_row,
};
use super::graph_snapshot_store::persist_graph_snapshot_in_transaction;
pub(crate) use super::graph_snapshot_store::KnowledgeGraphPersistReport;
#[cfg(test)]
use super::graph_snapshot_store::GRAPHQLITE_SCHEMA_VERSION;
use super::import_job_store;
use super::row_decode::{
    non_empty_string, object_i64, object_optional_f32, object_string, object_string_array, row_i64,
    row_string, row_string_array,
};
use super::schema_store::{
    count_rows_for_workspace, ensure_schema, schema_version, validate_graphqlite_gate,
    GraphqliteGateReport, KnowledgeStoreHealth, KnowledgeStoreStateSummary, KNOWLEDGE_DB_FILE_NAME,
};
use super::search_store::{
    append_graph_neighbor_hits, append_source_page_fts_hits, append_wiki_fts_hits, db_best_snippet,
    db_float_score, db_match_score, db_search_terms, evidence_graph_neighbor_counts,
    fts_phrase_query, EvidenceQueryIntent, HybridRetrievalHit,
};
use super::source_manifest_store::persist_source_manifest_in_transaction;

#[derive(Debug, Clone)]
pub(crate) struct KnowledgeStore {
    path: PathBuf,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AgentWriteProposalRecord {
    pub(crate) proposal_id: String,
    pub(crate) workspace_id: String,
    pub(crate) content_type: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) actor_id: String,
    pub(crate) validation_status: String,
    pub(crate) requires_user_approval: bool,
    pub(crate) approval_reason: Option<String>,
    pub(crate) approval_status: String,
    pub(crate) proposal_json: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
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
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting agent proposal transaction")?;

        let result = (|| -> Result<()> {
            let evidence_refs_json = serde_json::to_string(&proposal.evidence_refs)
                .context("failed encoding agent proposal evidence refs")?;
            sqlite
                .execute(
                    "INSERT INTO agent_write_proposals (
                        proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                    ON CONFLICT(proposal_id) DO UPDATE SET
                        workspace_id=excluded.workspace_id,
                        content_type=excluded.content_type,
                        title=excluded.title,
                        body=excluded.body,
                        evidence_refs_json=excluded.evidence_refs_json,
                        actor_id=excluded.actor_id,
                        validation_status=excluded.validation_status,
                        requires_user_approval=excluded.requires_user_approval,
                        approval_reason=excluded.approval_reason,
                        approval_status=excluded.approval_status,
                        proposal_json=excluded.proposal_json,
                        updated_at=excluded.updated_at",
                    (
                        proposal.proposal_id.as_str(),
                        proposal.workspace_id.as_str(),
                        proposal.content_type.as_str(),
                        proposal.title.as_str(),
                        proposal.body.as_str(),
                        evidence_refs_json.as_str(),
                        proposal.actor_id.as_str(),
                        proposal.validation_status.as_str(),
                        if proposal.requires_user_approval {
                            1
                        } else {
                            0
                        },
                        proposal.approval_reason.as_deref(),
                        proposal.approval_status.as_str(),
                        proposal.proposal_json.as_str(),
                        proposal.created_at,
                        proposal.updated_at,
                    ),
                )
                .with_context(|| {
                    format!(
                        "failed inserting agent write proposal {}",
                        proposal.proposal_id
                    )
                })?;
            sqlite
                .execute(
                    "DELETE FROM agent_write_proposal_evidence_refs WHERE proposal_id = ?1",
                    [proposal.proposal_id.as_str()],
                )
                .with_context(|| {
                    format!(
                        "failed clearing agent write proposal evidence refs {}",
                        proposal.proposal_id
                    )
                })?;
            for evidence_ref in &proposal.evidence_refs {
                sqlite
                    .execute(
                        "INSERT INTO agent_write_proposal_evidence_refs (proposal_id, evidence_ref)
                         VALUES (?1, ?2)",
                        (proposal.proposal_id.as_str(), evidence_ref.as_str()),
                    )
                    .with_context(|| {
                        format!(
                            "failed inserting agent write proposal evidence ref {}",
                            proposal.proposal_id
                        )
                    })?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                sqlite
                    .execute_batch("COMMIT")
                    .context("failed committing agent proposal transaction")?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlite.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(crate) fn load_agent_write_proposal(
        &self,
        workspace_id: &str,
        proposal_id: &str,
    ) -> Result<Option<AgentWriteProposalRecord>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let mut statement = graph
            .connection()
            .sqlite_connection()
            .prepare(
                "SELECT proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                 FROM agent_write_proposals
                 WHERE workspace_id = ?1 AND proposal_id = ?2",
            )
            .context("failed preparing agent write proposal lookup")?;
        let mut rows = statement
            .query((workspace_id, proposal_id))
            .context("failed querying agent write proposal")?;
        if let Some(row) = rows
            .next()
            .context("failed reading agent write proposal row")?
        {
            let evidence_refs_json: String = row.get(5).context("read proposal evidence refs")?;
            let requires_user_approval: i64 =
                row.get(8).context("read proposal approval requirement")?;
            return Ok(Some(AgentWriteProposalRecord {
                proposal_id: row.get(0).context("read proposal id")?,
                workspace_id: row.get(1).context("read proposal workspace")?,
                content_type: row.get(2).context("read proposal content type")?,
                title: row.get(3).context("read proposal title")?,
                body: row.get(4).context("read proposal body")?,
                evidence_refs: decode_agent_write_proposal_evidence_refs(&evidence_refs_json)?,
                actor_id: row.get(6).context("read proposal actor")?,
                validation_status: row.get(7).context("read proposal validation status")?,
                requires_user_approval: requires_user_approval != 0,
                approval_reason: row.get(9).context("read proposal approval reason")?,
                approval_status: row.get(10).context("read proposal approval status")?,
                proposal_json: row.get(11).context("read proposal json")?,
                created_at: row.get(12).context("read proposal created at")?,
                updated_at: row.get(13).context("read proposal updated at")?,
            }));
        }
        Ok(None)
    }

    pub(crate) fn list_pending_agent_write_proposals(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<AgentWriteProposalRecord>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let mut statement = graph
            .connection()
            .sqlite_connection()
            .prepare(
                "SELECT proposal_id,
                        workspace_id,
                        content_type,
                        title,
                        body,
                        evidence_refs_json,
                        actor_id,
                        validation_status,
                        requires_user_approval,
                        approval_reason,
                        approval_status,
                        proposal_json,
                        created_at,
                        updated_at
                 FROM agent_write_proposals
                 WHERE workspace_id = ?1
                   AND approval_status IN ('pending', 'pending_user_approval')
                 ORDER BY created_at DESC, proposal_id DESC",
            )
            .context("failed preparing pending agent proposal list")?;
        let mut rows = statement
            .query([workspace_id])
            .context("failed querying pending agent proposals")?;
        let mut proposals = Vec::new();
        while let Some(row) = rows.next().context("failed reading agent proposal row")? {
            let evidence_refs_json: String = row.get(5).context("read proposal evidence refs")?;
            let requires_user_approval: i64 =
                row.get(8).context("read proposal approval requirement")?;
            proposals.push(AgentWriteProposalRecord {
                proposal_id: row.get(0).context("read proposal id")?,
                workspace_id: row.get(1).context("read proposal workspace")?,
                content_type: row.get(2).context("read proposal content type")?,
                title: row.get(3).context("read proposal title")?,
                body: row.get(4).context("read proposal body")?,
                evidence_refs: decode_agent_write_proposal_evidence_refs(&evidence_refs_json)?,
                actor_id: row.get(6).context("read proposal actor")?,
                validation_status: row.get(7).context("read proposal validation status")?,
                requires_user_approval: requires_user_approval != 0,
                approval_reason: row.get(9).context("read proposal approval reason")?,
                approval_status: row.get(10).context("read proposal approval status")?,
                proposal_json: row.get(11).context("read proposal json")?,
                created_at: row.get(12).context("read proposal created at")?,
                updated_at: row.get(13).context("read proposal updated at")?,
            });
        }
        Ok(proposals)
    }

    pub(crate) fn update_agent_write_proposal_status(
        &self,
        workspace_id: &str,
        proposal_id: &str,
        approval_status: &str,
        updated_at: i64,
    ) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        graph
            .connection()
            .sqlite_connection()
            .execute(
                "UPDATE agent_write_proposals
                 SET approval_status = ?3, updated_at = ?4
                 WHERE workspace_id = ?1 AND proposal_id = ?2",
                (workspace_id, proposal_id, approval_status, updated_at),
            )
            .with_context(|| format!("failed updating proposal status {proposal_id}"))?;
        Ok(())
    }

    pub(crate) fn record_agent_write_commit(
        &self,
        workspace_id: &str,
        proposal_id: &str,
        event: &BrainEvent,
        updated_at: i64,
    ) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sqlite = graph.connection().sqlite_connection();
        sqlite
            .execute_batch("BEGIN IMMEDIATE")
            .context("failed starting agent write commit audit transaction")?;

        let result = (|| -> Result<()> {
            let actor_json = serde_json::to_string(&event.actor)
                .context("failed encoding agent write event actor")?;
            let evidence_refs_json = serde_json::to_string(&event.evidence_refs)
                .context("failed encoding agent write event evidence refs")?;
            let operation_type = event
                .operation_type
                .clone()
                .unwrap_or_else(|| format!("{:?}", event.event_type).to_ascii_lowercase());
            let payload_json = if event.payload_json.trim().is_empty() {
                "{}"
            } else {
                event.payload_json.as_str()
            };
            sqlite
                .execute(
                    "INSERT INTO brain_events (
                        event_id,
                        workspace_id,
                        actor_json,
                        operation_type,
                        evidence_refs_json,
                        payload_json,
                        created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(event_id) DO UPDATE SET
                        workspace_id=excluded.workspace_id,
                        actor_json=excluded.actor_json,
                        operation_type=excluded.operation_type,
                        evidence_refs_json=excluded.evidence_refs_json,
                        payload_json=excluded.payload_json,
                        created_at=excluded.created_at",
                    (
                        event.event_id.as_str(),
                        event.workspace_id.as_str(),
                        actor_json.as_str(),
                        operation_type.as_str(),
                        evidence_refs_json.as_str(),
                        payload_json,
                        event.created_at as i64,
                    ),
                )
                .with_context(|| format!("failed inserting brain event row {}", event.event_id))?;
            sqlite
                .execute(
                    "UPDATE agent_write_proposals
                     SET approval_status = 'committed', updated_at = ?3
                     WHERE workspace_id = ?1 AND proposal_id = ?2",
                    (workspace_id, proposal_id, updated_at),
                )
                .with_context(|| format!("failed marking proposal {proposal_id} committed"))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                sqlite
                    .execute_batch("COMMIT")
                    .context("failed committing agent write audit transaction")?;
                Ok(())
            }
            Err(error) => {
                let _ = sqlite.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn load_brain_event_operation(
        &self,
        workspace_id: &str,
        event_id: &str,
    ) -> Result<Option<String>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let mut statement = graph
            .connection()
            .sqlite_connection()
            .prepare(
                "SELECT operation_type
                 FROM brain_events
                 WHERE workspace_id = ?1 AND event_id = ?2",
            )
            .context("failed preparing brain event lookup")?;
        let mut rows = statement
            .query((workspace_id, event_id))
            .context("failed querying brain event")?;
        if let Some(row) = rows.next().context("failed reading brain event row")? {
            return Ok(Some(row.get(0).context("read operation type")?));
        }
        Ok(None)
    }

    #[allow(dead_code)]
    pub(crate) fn hybrid_retrieve(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HybridRetrievalHit>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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
        Ok(context_pack)
    }

    #[allow(dead_code)]
    pub(crate) fn read_source_from_db(
        &self,
        workspace_id: &str,
        source_id: &str,
        include_local_paths: bool,
    ) -> Result<Option<ReadSourceResponseData>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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
    pub(crate) fn read_page_evidence_from_db(
        &self,
        workspace_id: &str,
        source_id: &str,
        page: Option<usize>,
        include_local_paths: bool,
    ) -> Result<Option<ReadPageEvidenceResponseData>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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
    pub(crate) fn read_wiki_page_from_db(
        &self,
        workspace_id: &str,
        path: &str,
    ) -> Result<Option<WikiPage>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        load_wiki_page_by_path(&graph, workspace_id, path)
    }

    #[allow(dead_code)]
    pub(crate) fn read_node_from_db(
        &self,
        workspace_id: &str,
        node_id: &str,
    ) -> Result<Option<ReadNodeResponseData>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let node_value = graph
            .get_all_nodes(None)
            .context("failed reading GraphQLite nodes")?
            .into_iter()
            .find(|node| {
                let graphqlite::Value::Object(properties) = node else {
                    return false;
                };
                matches!(properties.get("id"), Some(graphqlite::Value::String(id)) if id == node_id)
            });
        let Some(graphqlite::Value::Object(properties)) = node_value else {
            return Ok(None);
        };
        let row_workspace_id = object_string(&properties, "workspace_id");
        if row_workspace_id != workspace_id {
            return Ok(None);
        }
        let evidence_ids = object_string_array(&properties, "evidence_ids_json");
        let source_ids = object_string_array(&properties, "source_ids_json");
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
        let node = BrainNodeRecord {
            node_id: node_id.into(),
            kind: parse_brain_node_kind(&object_string(&properties, "kind")),
            label: object_string(&properties, "label"),
            scope: parse_brain_scope(&object_string(&properties, "scope")),
            aliases: object_string_array(&properties, "aliases_json"),
            evidence_ids,
            source_ids,
            confidence: object_optional_f32(&properties, "confidence"),
            updated_at: object_i64(&properties, "updated_at").max(0) as u64,
        };
        Ok(Some(ReadNodeResponseData {
            node,
            evidence,
            relations: Vec::new(),
        }))
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
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let nodes = load_graph_canvas_nodes(&graph, workspace_id)?;
        let relations = load_graph_canvas_relations(&graph, workspace_id)?;
        let wiki_pages = load_graph_canvas_wiki_pages(&graph, workspace_id)?;
        if nodes.is_empty() && relations.is_empty() && wiki_pages.is_empty() {
            return Ok(None);
        }
        Ok(Some((nodes, relations, wiki_pages)))
    }

    pub(crate) fn read_graph_snapshot_sources_from_db(
        &self,
        workspace_id: &str,
        include_local_paths: bool,
    ) -> Result<Vec<GraphSnapshotSourceRecord>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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

    pub(crate) fn search_brain_from_db(
        &self,
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

        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let mut results = Vec::new();
        for hit in self.hybrid_retrieve(workspace_id, query, limit)? {
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
    pub(crate) fn resolve_evidence_proof(
        &self,
        workspace_id: &str,
        evidence_id: &str,
    ) -> Result<RelationalEvidenceProof> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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

    pub(crate) fn graph_snapshot_counts(
        &self,
        workspace_id: &str,
    ) -> Result<KnowledgeGraphPersistReport> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
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
        .map(|row| {
            Ok(BrainRelationRecord {
                relation_id: row_string(row, "relation_id")
                    .context("read graph canvas relation id")?,
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
                    &row_string(row, "confidence")
                        .context("read graph canvas relation confidence")?,
                ),
                updated_at: row_i64(row, "updated_at")
                    .context("read graph canvas relation updated at")?
                    .max(0) as u64,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    relations.sort_by(|left, right| left.relation_id.cmp(&right.relation_id));
    Ok(relations)
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

fn decode_agent_write_proposal_evidence_refs(value: &str) -> Result<Vec<String>> {
    serde_json::from_str(value).context("failed decoding agent proposal evidence refs")
}

#[cfg(test)]
mod tests;
