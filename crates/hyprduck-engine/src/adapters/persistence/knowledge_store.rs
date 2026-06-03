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
    unique_project_evidence,
};
pub(crate) use super::graph_snapshot_store::KnowledgeGraphPersistReport;
use super::graph_snapshot_store::{
    persist_graph_snapshot_in_transaction, GRAPHQLITE_SCHEMA_VERSION,
};
use super::import_job_store;
use super::row_decode::{
    non_empty_string, object_i64, object_optional_f32, object_string, object_string_array, row_i64,
    row_string, row_string_array,
};
use super::search_store::{
    append_graph_neighbor_hits, append_source_page_fts_hits, append_wiki_fts_hits, db_best_snippet,
    db_float_score, db_match_score, db_search_terms, evidence_graph_neighbor_counts,
    fts_phrase_query, EvidenceQueryIntent, HybridRetrievalHit,
};
use crate::policy::redact_path_for_agent;

const KNOWLEDGE_DB_FILE_NAME: &str = "hyprduck.sqlite";
const KNOWLEDGE_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone)]
pub(crate) struct KnowledgeStore {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KnowledgeStoreHealth {
    pub(crate) db_path: String,
    pub(crate) db_schema_version: i64,
    pub(crate) graph_schema_version: i64,
    pub(crate) graphqlite_loaded: bool,
    pub(crate) graphqlite_transactional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KnowledgeStoreStateSummary {
    pub(crate) evidence_item_count: usize,
    pub(crate) wiki_page_count: usize,
    pub(crate) graph_node_count: usize,
    pub(crate) graph_relation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GraphqliteGateReport {
    pub(crate) loaded: bool,
    pub(crate) cypher_ready: bool,
    pub(crate) rollback_ready: bool,
    pub(crate) graph_schema_version: i64,
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
        store.ensure_schema()?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn health(&self) -> Result<KnowledgeStoreHealth> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let gate = validate_graphqlite_gate(&graph)?;
        Ok(KnowledgeStoreHealth {
            db_path: self.path.display().to_string(),
            db_schema_version: self.schema_version()?,
            graph_schema_version: gate.graph_schema_version,
            graphqlite_loaded: gate.loaded,
            graphqlite_transactional: gate.rollback_ready,
        })
    }

    pub(crate) fn state_summary(&self, workspace_id: &str) -> Result<KnowledgeStoreStateSummary> {
        let graph_counts = self.graph_snapshot_counts(workspace_id)?;
        Ok(KnowledgeStoreStateSummary {
            evidence_item_count: self.count_rows_for_workspace("evidence_items", workspace_id)?,
            wiki_page_count: self.count_rows_for_workspace("wiki_pages", workspace_id)?,
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
        self.ensure_schema()?;
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
        self.ensure_schema()?;
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

        let result = (|| -> Result<()> {
            let manifest_json = serde_json::to_string(manifest)
                .context("failed encoding source manifest for knowledge DB")?;
            let format = json_string_slug(&manifest.format)?;
            let status = json_string_slug(&manifest.status)?;
            let original_path_redacted = redact_path_for_agent(&manifest.original_path);
            let source_path_redacted = redact_path_for_agent(&manifest.source_path);
            let markdown_path_redacted = redact_path_for_agent(&manifest.markdown_path);
            let failed_count = manifest
                .pages
                .iter()
                .filter(|page| page.error_message.is_some())
                .count();
            let success_count = manifest.pages.len().saturating_sub(failed_count);
            let project_evidence = unique_project_evidence(project);
            let source_evidence_count = project_evidence
                .iter()
                .filter(|evidence| {
                    evidence.source_id.as_deref().unwrap_or(&manifest.source_id)
                        == manifest.source_id
                })
                .count();
            let citation_ready = success_count > 0 || source_evidence_count > 0;
            let warnings_json = serde_json::to_string(
                &manifest
                    .pages
                    .iter()
                    .filter_map(|page| {
                        page.error_message.as_ref().map(|message| {
                            serde_json::json!({
                                "pageIndex": page.index,
                                "pageLabel": page.label,
                                "message": message
                            })
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .context("failed encoding source warnings for knowledge DB")?;
            sqlite.execute_batch(&format!(
                "INSERT INTO import_jobs (job_id, workspace_id, source_id, status, citation_ready, graph_ready, graph_status, graph_error_category, graph_error_message_redacted, graph_retryable, graph_retry_attempt, graph_max_retry_attempts, graph_next_retry_at, manual_retry_available, created_at, updated_at, error_message)
                 VALUES ({job_id}, {workspace_id}, {source_id}, {status}, {citation_ready}, 0, '', '', '', 0, 0, 2, NULL, 0, {created_at}, {updated_at}, NULL)
                 ON CONFLICT(job_id) DO UPDATE SET
                   workspace_id=excluded.workspace_id,
                   source_id=excluded.source_id,
                   status=excluded.status,
                   citation_ready=excluded.citation_ready,
                   updated_at=excluded.updated_at,
                   error_message=excluded.error_message;
                 INSERT INTO sources (source_id, workspace_id, project_id, title, original_path, source_path, markdown_path, original_path_redacted, source_path_redacted, markdown_path_redacted, format, status, page_count, success_count, failed_count, parse_warnings_json, manifest_path, manifest_json, created_at, updated_at)
                 VALUES ({source_id}, {workspace_id}, {project_id}, {title}, {original_path}, {source_path}, {markdown_path}, {original_path_redacted}, {source_path_redacted}, {markdown_path_redacted}, {format}, {status}, {page_count}, {success_count}, {failed_count}, {warnings_json}, {manifest_path}, {manifest_json}, {created_at}, {updated_at})
                 ON CONFLICT(source_id) DO UPDATE SET
                   workspace_id=excluded.workspace_id,
                   project_id=excluded.project_id,
                   title=excluded.title,
                   original_path=excluded.original_path,
                   source_path=excluded.source_path,
                   markdown_path=excluded.markdown_path,
                   original_path_redacted=excluded.original_path_redacted,
                   source_path_redacted=excluded.source_path_redacted,
                   markdown_path_redacted=excluded.markdown_path_redacted,
                   format=excluded.format,
                   status=excluded.status,
                   page_count=excluded.page_count,
                   success_count=excluded.success_count,
                   failed_count=excluded.failed_count,
                   parse_warnings_json=excluded.parse_warnings_json,
                   manifest_path=excluded.manifest_path,
                   manifest_json=excluded.manifest_json,
                   updated_at=excluded.updated_at;
                 DELETE FROM source_pages WHERE source_id = {source_id};
                 DELETE FROM source_page_fts WHERE source_id = {source_id};
                 DELETE FROM evidence_fts WHERE source_id = {source_id};
                 DELETE FROM evidence_items WHERE source_id = {source_id};",
                job_id = sql_literal(&format!("import:{}", manifest.source_id)),
                workspace_id = sql_literal(&manifest.workspace_id),
                source_id = sql_literal(&manifest.source_id),
                status = sql_literal(&status),
                citation_ready = if citation_ready { 1 } else { 0 },
                created_at = manifest.created_at,
                updated_at = manifest.updated_at,
                project_id = sql_literal(&project.summary.project_id),
                title = sql_literal(&project.summary.title),
                original_path = sql_literal(&manifest.original_path),
                source_path = sql_literal(&manifest.source_path),
                markdown_path = sql_literal(&manifest.markdown_path),
                original_path_redacted = sql_literal(&original_path_redacted),
                source_path_redacted = sql_literal(&source_path_redacted),
                markdown_path_redacted = sql_literal(&markdown_path_redacted),
                format = sql_literal(&format),
                page_count = manifest.pages.len(),
                success_count = success_count,
                failed_count = failed_count,
                warnings_json = sql_literal(&warnings_json),
                manifest_path = sql_literal(&manifest.manifest_path),
                manifest_json = sql_literal(&manifest_json),
            ))?;

            for page in &manifest.pages {
                let markdown_path_redacted =
                    page.markdown_path.as_deref().map(redact_path_for_agent);
                let image_path_redacted = page.image_path.as_deref().map(redact_path_for_agent);
                let plain_text = page
                    .plain_text_path
                    .as_deref()
                    .and_then(|path| fs::read_to_string(path).ok())
                    .unwrap_or_default();
                let parse_warnings_json = serde_json::to_string(
                    &page
                        .error_message
                        .as_ref()
                        .map(|message| vec![message.clone()])
                        .unwrap_or_default(),
                )?;
                sqlite.execute_batch(&format!(
                    "INSERT INTO source_pages (source_id, page_index, page_label, markdown_path_redacted, image_path_redacted, plain_text, parse_warnings_json)
                     VALUES ({source_id}, {page_index}, {page_label}, {markdown_path}, {image_path}, {plain_text}, {parse_warnings_json});",
                    source_id = sql_literal(&manifest.source_id),
                    page_index = page.index,
                    page_label = sql_literal(&page.label),
                    markdown_path = sql_optional_literal(markdown_path_redacted.as_deref()),
                    image_path = sql_optional_literal(image_path_redacted.as_deref()),
                    plain_text = sql_literal(&plain_text),
                    parse_warnings_json = sql_literal(&parse_warnings_json),
                ))?;
                if !plain_text.trim().is_empty() {
                    sqlite.execute_batch(&format!(
                        "INSERT INTO source_page_fts (source_id, page_index, page_label, text)
                         VALUES ({source_id}, {page_index}, {page_label}, {plain_text});",
                        source_id = sql_literal(&manifest.source_id),
                        page_index = page.index,
                        page_label = sql_literal(&page.label),
                        plain_text = sql_literal(&plain_text),
                    ))?;
                }
            }

            for evidence in project_evidence {
                let source_id = evidence.source_id.as_deref().unwrap_or(&manifest.source_id);
                if source_id != manifest.source_id {
                    continue;
                }
                let page_index = evidence
                    .page_index
                    .map(|page| page.to_string())
                    .unwrap_or_else(|| "NULL".into());
                let provenance = evidence.provenance.clone().unwrap_or_default();
                sqlite.execute_batch(&format!(
                    "INSERT INTO evidence_items (evidence_id, workspace_id, source_id, page_index, page_label, evidence_type, snippet, source_path_redacted, markdown_path_redacted, image_path_redacted, provenance, span_json, region_json, status)
                     VALUES ({evidence_id}, {workspace_id}, {source_id}, {page_index}, {page_label}, 'text', {snippet}, {source_path}, {markdown_path}, {image_path}, {provenance}, '{{}}', '{{}}', 'active')
                     ON CONFLICT(evidence_id) DO UPDATE SET
                       workspace_id=excluded.workspace_id,
                       source_id=excluded.source_id,
                       page_index=excluded.page_index,
                       page_label=excluded.page_label,
                       evidence_type=excluded.evidence_type,
                       snippet=excluded.snippet,
                       source_path_redacted=excluded.source_path_redacted,
                       markdown_path_redacted=excluded.markdown_path_redacted,
                       image_path_redacted=excluded.image_path_redacted,
                       provenance=excluded.provenance,
                       status=excluded.status;
                     INSERT INTO evidence_fts (evidence_id, source_id, evidence_type, text)
                     VALUES ({evidence_id}, {source_id}, 'text', {snippet});",
                    evidence_id = sql_literal(&evidence.id),
                    workspace_id = sql_literal(&manifest.workspace_id),
                    source_id = sql_literal(source_id),
                    page_index = page_index,
                    page_label = sql_literal(&evidence.page_label),
                    snippet = sql_literal(&evidence.snippet),
                    source_path = sql_literal(&redact_path_for_agent(evidence.source_path.as_deref().unwrap_or(&manifest.source_path))),
                    markdown_path = sql_literal(&redact_path_for_agent(evidence.markdown_path.as_deref().unwrap_or(&manifest.markdown_path))),
                    image_path = sql_literal(&redact_path_for_agent(evidence.image_path.as_deref().unwrap_or(""))),
                    provenance = sql_literal(&provenance),
                ))?;
            }
            Ok(())
        })();
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

    fn count_rows_for_workspace(&self, table: &str, workspace_id: &str) -> Result<usize> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE workspace_id = ?1");
        graph
            .connection()
            .sqlite_connection()
            .query_row(&sql, [workspace_id], |row| row.get::<_, i64>(0))
            .with_context(|| format!("failed counting {table} rows"))?
            .try_into()
            .map_err(|_| anyhow!("negative {table} row count"))
    }

    fn ensure_schema(&self) -> Result<()> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let conn = graph.connection();
        conn.sqlite_connection()
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS knowledge_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            INSERT INTO knowledge_meta (key, value)
            VALUES ('knowledge_schema_version', '1')
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=unixepoch();
            INSERT INTO knowledge_meta (key, value)
            VALUES ('graphqlite_schema_version', '1')
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=unixepoch();

            CREATE TABLE IF NOT EXISTS import_jobs (
                job_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                source_id TEXT,
                status TEXT NOT NULL,
                citation_ready INTEGER NOT NULL DEFAULT 0,
                graph_ready INTEGER NOT NULL DEFAULT 0,
                graph_status TEXT NOT NULL DEFAULT '',
                graph_error_category TEXT NOT NULL DEFAULT '',
                graph_error_message_redacted TEXT NOT NULL DEFAULT '',
                graph_retryable INTEGER NOT NULL DEFAULT 0,
                graph_retry_attempt INTEGER NOT NULL DEFAULT 0,
                graph_max_retry_attempts INTEGER NOT NULL DEFAULT 0,
                graph_next_retry_at INTEGER,
                manual_retry_available INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                error_message TEXT
            );

            CREATE TABLE IF NOT EXISTS sources (
                source_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                project_id TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                original_path TEXT NOT NULL DEFAULT '',
                source_path TEXT NOT NULL DEFAULT '',
                markdown_path TEXT NOT NULL DEFAULT '',
                original_path_redacted TEXT NOT NULL DEFAULT '',
                source_path_redacted TEXT NOT NULL DEFAULT '',
                markdown_path_redacted TEXT NOT NULL DEFAULT '',
                format TEXT NOT NULL,
                status TEXT NOT NULL,
                page_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                failed_count INTEGER NOT NULL DEFAULT 0,
                provider_route TEXT NOT NULL DEFAULT '',
                provider_locality TEXT NOT NULL DEFAULT '',
                content_hash TEXT NOT NULL DEFAULT '',
                parse_warnings_json TEXT NOT NULL DEFAULT '[]',
                manifest_path TEXT NOT NULL DEFAULT '',
                manifest_base64 TEXT NOT NULL DEFAULT '',
                manifest_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_knowledge_sources_workspace_updated_at
                ON sources(workspace_id, updated_at DESC);

            CREATE TABLE IF NOT EXISTS source_pages (
                source_id TEXT NOT NULL,
                page_index INTEGER NOT NULL,
                page_label TEXT NOT NULL,
                markdown_path_redacted TEXT,
                image_path_redacted TEXT,
                plain_text TEXT NOT NULL DEFAULT '',
                parse_warnings_json TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (source_id, page_index),
                FOREIGN KEY (source_id) REFERENCES sources(source_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS evidence_items (
                evidence_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL DEFAULT '',
                source_id TEXT NOT NULL,
                page_index INTEGER,
                page_label TEXT NOT NULL DEFAULT '',
                evidence_type TEXT NOT NULL,
                snippet TEXT NOT NULL,
                source_path_redacted TEXT NOT NULL DEFAULT '',
                markdown_path_redacted TEXT NOT NULL DEFAULT '',
                image_path_redacted TEXT NOT NULL DEFAULT '',
                provenance TEXT NOT NULL DEFAULT '',
                span_json TEXT NOT NULL DEFAULT '{}',
                region_json TEXT NOT NULL DEFAULT '{}',
                producer_run_id TEXT NOT NULL DEFAULT '',
                confidence REAL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                FOREIGN KEY (source_id) REFERENCES sources(source_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_evidence_source_page
                ON evidence_items(source_id, page_index);

            CREATE VIRTUAL TABLE IF NOT EXISTS evidence_fts USING fts5(
                evidence_id UNINDEXED,
                source_id UNINDEXED,
                evidence_type UNINDEXED,
                text
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS source_page_fts USING fts5(
                source_id UNINDEXED,
                page_index UNINDEXED,
                page_label UNINDEXED,
                text
            );

            CREATE TABLE IF NOT EXISTS wiki_pages (
                wiki_page_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                path TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                approval_status TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                revision INTEGER NOT NULL DEFAULT 1,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_wiki_pages_workspace_updated_at
                ON wiki_pages(workspace_id, updated_at DESC);

            CREATE TABLE IF NOT EXISTS wiki_revisions (
                wiki_page_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                workspace_id TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                approval_status TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                diff_json TEXT NOT NULL DEFAULT '{}',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (wiki_page_id, revision),
                FOREIGN KEY (wiki_page_id) REFERENCES wiki_pages(wiki_page_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS wiki_sections (
                wiki_page_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                section_index INTEGER NOT NULL,
                heading TEXT NOT NULL,
                body TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (wiki_page_id, revision, section_index),
                FOREIGN KEY (wiki_page_id, revision) REFERENCES wiki_revisions(wiki_page_id, revision) ON DELETE CASCADE
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS wiki_fts USING fts5(
                workspace_id UNINDEXED,
                wiki_page_id UNINDEXED,
                revision UNINDEXED,
                section_index UNINDEXED,
                title,
                text
            );

            CREATE TABLE IF NOT EXISTS brain_events (
                event_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                actor_json TEXT NOT NULL,
                operation_type TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                payload_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE IF NOT EXISTS agent_write_proposals (
                proposal_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                content_type TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                actor_id TEXT NOT NULL,
                validation_status TEXT NOT NULL,
                requires_user_approval INTEGER NOT NULL DEFAULT 0,
                approval_reason TEXT,
                approval_status TEXT NOT NULL,
                proposal_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_agent_write_proposals_workspace_status
                ON agent_write_proposals(workspace_id, approval_status, updated_at DESC);

            CREATE TABLE IF NOT EXISTS agent_write_proposal_evidence_refs (
                proposal_id TEXT NOT NULL,
                evidence_ref TEXT NOT NULL,
                PRIMARY KEY (proposal_id, evidence_ref),
                FOREIGN KEY (proposal_id) REFERENCES agent_write_proposals(proposal_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_agent_write_proposal_evidence_refs_ref
                ON agent_write_proposal_evidence_refs(evidence_ref);

            CREATE TABLE IF NOT EXISTS graph_checkpoints (
                checkpoint_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                actor_json TEXT NOT NULL,
                related_event_id TEXT,
                graph_schema_version INTEGER NOT NULL,
                graphqlite_extension_version TEXT NOT NULL,
                node_count INTEGER NOT NULL,
                edge_count INTEGER NOT NULL,
                evidence_ref_count INTEGER NOT NULL,
                checksum TEXT NOT NULL,
                storage_ref TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE IF NOT EXISTS context_pack_exports (
                pack_id TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                query TEXT NOT NULL DEFAULT '',
                export_path TEXT NOT NULL,
                schema_version TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                generated_at TEXT NOT NULL DEFAULT '',
                is_latest INTEGER NOT NULL DEFAULT 0,
                preserved_at INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (pack_id, export_path)
            );
            CREATE INDEX IF NOT EXISTS idx_context_pack_exports_workspace_latest
                ON context_pack_exports(workspace_id, is_latest, preserved_at DESC);
            ",
            )
            .context("failed creating knowledge store schema")?;
        let sqlite = conn.sqlite_connection();
        for column in [
            "title TEXT NOT NULL DEFAULT ''",
            "original_path TEXT NOT NULL DEFAULT ''",
            "source_path TEXT NOT NULL DEFAULT ''",
            "markdown_path TEXT NOT NULL DEFAULT ''",
            "original_path_redacted TEXT NOT NULL DEFAULT ''",
            "source_path_redacted TEXT NOT NULL DEFAULT ''",
            "markdown_path_redacted TEXT NOT NULL DEFAULT ''",
            "page_count INTEGER NOT NULL DEFAULT 0",
            "success_count INTEGER NOT NULL DEFAULT 0",
            "failed_count INTEGER NOT NULL DEFAULT 0",
            "provider_route TEXT NOT NULL DEFAULT ''",
            "provider_locality TEXT NOT NULL DEFAULT ''",
            "content_hash TEXT NOT NULL DEFAULT ''",
            "parse_warnings_json TEXT NOT NULL DEFAULT '[]'",
            "manifest_path TEXT NOT NULL DEFAULT ''",
            "manifest_base64 TEXT NOT NULL DEFAULT ''",
            "manifest_json TEXT NOT NULL DEFAULT '{}'",
            "created_at INTEGER NOT NULL DEFAULT 0",
        ] {
            let result = sqlite.execute(&format!("ALTER TABLE sources ADD COLUMN {column}"), []);
            if let Err(error) = result {
                let message = error.to_string();
                if !message.contains("duplicate column name") {
                    return Err(error).context("failed migrating sources table");
                }
            }
        }
        for column in [
            "workspace_id TEXT NOT NULL DEFAULT ''",
            "page_label TEXT NOT NULL DEFAULT ''",
            "source_path_redacted TEXT NOT NULL DEFAULT ''",
            "markdown_path_redacted TEXT NOT NULL DEFAULT ''",
            "image_path_redacted TEXT NOT NULL DEFAULT ''",
            "provenance TEXT NOT NULL DEFAULT ''",
        ] {
            let result = sqlite.execute(
                &format!("ALTER TABLE evidence_items ADD COLUMN {column}"),
                [],
            );
            if let Err(error) = result {
                let message = error.to_string();
                if !message.contains("duplicate column name") {
                    return Err(error).context("failed migrating evidence_items table");
                }
            }
        }
        for column in ["path TEXT NOT NULL DEFAULT ''"] {
            let result = sqlite.execute(&format!("ALTER TABLE wiki_pages ADD COLUMN {column}"), []);
            if let Err(error) = result {
                let message = error.to_string();
                if !message.contains("duplicate column name") {
                    return Err(error).context("failed migrating wiki_pages table");
                }
            }
        }
        for column in [
            "graph_status TEXT NOT NULL DEFAULT ''",
            "graph_error_category TEXT NOT NULL DEFAULT ''",
            "graph_error_message_redacted TEXT NOT NULL DEFAULT ''",
            "graph_retryable INTEGER NOT NULL DEFAULT 0",
            "graph_retry_attempt INTEGER NOT NULL DEFAULT 0",
            "graph_max_retry_attempts INTEGER NOT NULL DEFAULT 0",
            "graph_next_retry_at INTEGER",
            "manual_retry_available INTEGER NOT NULL DEFAULT 0",
        ] {
            let result =
                sqlite.execute(&format!("ALTER TABLE import_jobs ADD COLUMN {column}"), []);
            if let Err(error) = result {
                let message = error.to_string();
                if !message.contains("duplicate column name") {
                    return Err(error).context("failed migrating import_jobs table");
                }
            }
        }
        Ok(())
    }

    fn schema_version(&self) -> Result<i64> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let result = graph.connection().cypher(&format!(
            "RETURN {} AS schema_version",
            KNOWLEDGE_SCHEMA_VERSION
        ))?;
        result
            .get(0)
            .ok_or_else(|| anyhow!("knowledge schema version query returned no rows"))?
            .get::<i64>("schema_version")
            .context("failed reading knowledge schema version")
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

fn json_string_slug<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value).context("failed encoding slug value")?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("slug value did not encode as a JSON string"))
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn sql_optional_literal(value: Option<&str>) -> String {
    value.map(sql_literal).unwrap_or_else(|| "NULL".into())
}

fn decode_agent_write_proposal_evidence_refs(value: &str) -> Result<Vec<String>> {
    serde_json::from_str(value).context("failed decoding agent proposal evidence refs")
}

pub(crate) fn validate_graphqlite_gate(graph: &Graph) -> Result<GraphqliteGateReport> {
    graph
        .connection()
        .cypher("CREATE (n:HyprDuckGate {id: 'gate-load'})")
        .context("GraphQLite failed basic Cypher create")?;
    let result = graph
        .connection()
        .cypher("MATCH (n:HyprDuckGate {id: 'gate-load'}) RETURN n.id")
        .context("GraphQLite failed basic Cypher match")?;
    let loaded = result
        .get(0)
        .and_then(|row| row.get::<String>("n.id").ok())
        .is_some_and(|value| value == "gate-load");
    graph
        .connection()
        .cypher("MATCH (n:HyprDuckGate {id: 'gate-load'}) DELETE n")
        .context("GraphQLite failed basic Cypher cleanup")?;

    graph
        .connection()
        .sqlite_connection()
        .execute_batch("BEGIN")
        .context("GraphQLite transaction begin failed")?;
    graph
        .connection()
        .cypher("CREATE (n:HyprDuckGate {id: 'gate-rollback'})")
        .context("GraphQLite failed transaction create")?;
    graph
        .connection()
        .sqlite_connection()
        .execute_batch("ROLLBACK")
        .context("GraphQLite rollback failed")?;
    let result = graph
        .connection()
        .cypher("MATCH (n:HyprDuckGate {id: 'gate-rollback'}) RETURN n.id")
        .context("GraphQLite failed post-rollback query")?;

    Ok(GraphqliteGateReport {
        loaded,
        cypher_ready: loaded,
        rollback_ready: result.is_empty(),
        graph_schema_version: GRAPHQLITE_SCHEMA_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_store_creates_canonical_schema_and_graphqlite_gate() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");

        assert!(store.path().ends_with("hyprduck.sqlite"));

        let health = store.health().expect("health");
        assert_eq!(health.db_schema_version, 1);
        assert_eq!(health.graph_schema_version, 1);
        assert!(health.graphqlite_loaded);
        assert!(health.graphqlite_transactional);
    }

    #[test]
    fn graphqlite_gate_rejects_non_transactional_graph_mutations() {
        let temp = tempfile::tempdir().expect("temp dir");
        let graph = Graph::open(temp.path().join("hyprduck.sqlite")).expect("open graph");
        let report = validate_graphqlite_gate(&graph).expect("gate");

        assert!(report.loaded);
        assert!(report.cypher_ready);
        assert!(report.rollback_ready);
    }

    #[test]
    fn evidence_query_intent_boosts_requested_evidence_types() {
        let table_intent = EvidenceQueryIntent::from_query("show the financial table");
        assert!(table_intent.boost("table_evidence") > table_intent.boost("text_evidence"));

        let visual_intent = EvidenceQueryIntent::from_query("inspect the figure caption");
        assert!(
            visual_intent.boost("image_region_evidence") > visual_intent.boost("text_evidence")
        );

        let relationship_intent = EvidenceQueryIntent::from_query("which claims are connected");
        assert!(
            relationship_intent.boost("relationship_evidence")
                > relationship_intent.boost("text_evidence")
        );
    }

    #[test]
    fn graph_snapshot_is_persisted_as_current_graphqlite_workspace_graph() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        let snapshot = BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: 10,
            sources: vec![SourceRecord {
                source_id: "source-a".into(),
                workspace_id: "workspace-default".into(),
                original_path: "/Users/hyprduck/private/original-source-a.pdf".into(),
                source_path: "/Users/hyprduck/private/source-a.pdf".into(),
                markdown_path: "/Users/hyprduck/private/source-a.md".into(),
                format: SourceFormat::pdf(),
                status: SourceStatus::ingested(),
                page_count: 2,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 10,
            }],
            nodes: vec![
                BrainNodeRecord {
                    node_id: "node-a".into(),
                    kind: BrainNodeKind::Concept,
                    label: "Alpha".into(),
                    scope: BrainScope::Project,
                    aliases: Vec::new(),
                    evidence_ids: vec!["evidence-a".into()],
                    source_ids: vec!["source-a".into()],
                    confidence: Some(0.9),
                    updated_at: 10,
                },
                BrainNodeRecord {
                    node_id: "node-b".into(),
                    kind: BrainNodeKind::Concept,
                    label: "Beta".into(),
                    scope: BrainScope::Project,
                    aliases: Vec::new(),
                    evidence_ids: vec!["evidence-b".into()],
                    source_ids: vec!["source-a".into()],
                    confidence: None,
                    updated_at: 10,
                },
            ],
            relations: vec![
                BrainRelationRecord {
                    relation_id: "rel-a".into(),
                    kind: BrainRelationKind::RelatedTo,
                    source_node_id: "node-a".into(),
                    target_node_id: "node-b".into(),
                    label: "relates".into(),
                    evidence_ids: vec!["evidence-a".into()],
                    confidence: Some(0.8),
                    updated_at: 10,
                },
                BrainRelationRecord {
                    relation_id: "rel-cites".into(),
                    kind: BrainRelationKind::Cites,
                    source_node_id: "claim-alpha".into(),
                    target_node_id: "source:source-a".into(),
                    label: "cites".into(),
                    evidence_ids: vec!["evidence-a".into()],
                    confidence: Some(0.8),
                    updated_at: 10,
                },
                BrainRelationRecord {
                    relation_id: "rel-links".into(),
                    kind: BrainRelationKind::LinksTo,
                    source_node_id: "wiki-alpha".into(),
                    target_node_id: "entity-alpha".into(),
                    label: "links".into(),
                    evidence_ids: vec!["evidence-a".into()],
                    confidence: Some(0.8),
                    updated_at: 10,
                },
            ],
            evidence: vec![
                EvidenceRef {
                    id: "evidence-a".into(),
                    page_label: "p1".into(),
                    page_index: Some(0),
                    snippet: "Alpha relates to beta.".into(),
                    source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                    image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                    provenance: Some("test".into()),
                },
                EvidenceRef {
                    id: "evidence-b".into(),
                    page_label: "p2".into(),
                    page_index: Some(1),
                    snippet: "Beta neighbor evidence.".into(),
                    source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                    image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                    provenance: Some("test".into()),
                },
            ],
            memories: Vec::new(),
            wiki_pages: vec![WikiPage {
                page_id: "wiki-alpha".into(),
                workspace_id: "workspace-default".into(),
                path: "wiki/alpha".into(),
                title: "Alpha Wiki".into(),
                body: "# Overview\nAlpha wiki body.\n## Evidence\nAlpha cites source evidence."
                    .into(),
                node_refs: Vec::new(),
                source_refs: vec!["source-a".into()],
                evidence_refs: vec!["evidence-a".into()],
                updated_at: 10,
            }],
            entities: vec![EntityRecord {
                entity_id: "entity-alpha".into(),
                workspace_id: "workspace-default".into(),
                kind: BrainNodeKind::Company,
                name: "Alpha Inc".into(),
                aliases: vec!["Alpha".into()],
                source_refs: vec!["source-a".into()],
                evidence_refs: vec!["evidence-a".into()],
                updated_at: 10,
            }],
            claims: vec![ClaimRecord {
                claim_id: "claim-alpha".into(),
                workspace_id: "workspace-default".into(),
                statement: "Alpha relates to beta.".into(),
                topic_refs: vec!["Alpha".into()],
                source_refs: vec!["source-a".into()],
                evidence_refs: vec!["evidence-a".into()],
                status: "active".into(),
                updated_at: 10,
            }],
            extractions: vec![StructuredExtractionArtifact {
                artifact_id: "provider-run-alpha".into(),
                workspace_id: "workspace-default".into(),
                source_id: "source-a".into(),
                extractor: "test-extractor".into(),
                extractor_model: Some("test-model".into()),
                source_refs: vec!["source-a".into()],
                page_refs: Vec::new(),
                entities: Vec::new(),
                topics: Vec::new(),
                claims: Vec::new(),
                relations: Vec::new(),
                memories: Vec::new(),
                evidence_refs: vec![EvidenceRef {
                    id: "evidence-a".into(),
                    page_label: "p1".into(),
                    page_index: Some(0),
                    snippet: "Alpha relates to beta.".into(),
                    source_path: Some("/Users/hyprduck/private/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("/Users/hyprduck/private/source-a.md".into()),
                    image_path: Some("/Users/hyprduck/private/source-a.png".into()),
                    provenance: Some("test extraction".into()),
                }],
                confidence: Some(0.7),
                provenance: "test".into(),
                created_at: 10,
            }],
            events: vec![test_brain_event(
                "event-a",
                "workspace-default",
                &["evidence-a"],
            )],
        };

        let report = store
            .persist_graph_snapshot(&snapshot)
            .expect("persist graph snapshot");

        assert_eq!(
            report,
            KnowledgeGraphPersistReport {
                node_count: 6,
                relation_count: 3,
            }
        );
        assert_eq!(
            store
                .graph_snapshot_counts("workspace-default")
                .expect("graph counts"),
            report
        );
        assert_eq!(
            store
                .state_summary("workspace-default")
                .expect("state summary"),
            KnowledgeStoreStateSummary {
                evidence_item_count: 2,
                wiki_page_count: 1,
                graph_node_count: 6,
                graph_relation_count: 3,
            }
        );
        assert_graph_node_metadata(&store, "node-a");
        assert_graph_wiki_page_node(&store, "wiki-alpha");
        assert_wiki_relational_content(&store, "wiki-alpha");
        assert_source_page_fts_content(&store);
        assert_graph_edge_metadata(&store, "claim-alpha", "source:source-a", "CITES");
        assert_relational_proof_ignores_graph_metadata_tamper(&store);
        let hits = store
            .hybrid_retrieve("workspace-default", "Alpha", 5)
            .expect("hybrid retrieve");
        assert_eq!(hits.len(), 2);
        let alpha_hit = hits
            .iter()
            .find(|hit| hit.evidence_id == "evidence-a")
            .expect("alpha evidence hit");
        assert_eq!(alpha_hit.graph_neighbor_count, 1);
        let beta_neighbor_hit = hits
            .iter()
            .find(|hit| hit.evidence_id == "evidence-b")
            .expect("graph neighbor evidence hit");
        assert_eq!(beta_neighbor_hit.snippet, "Beta neighbor evidence.");
        update_source_context_pack_metadata(&store, "source-a");
        let context_pack = store
            .assemble_context_pack_v1_from_db(
                "workspace-default",
                "Alpha",
                5,
                "ctx_db_alpha".into(),
                "2026-05-29T09:53:26Z".into(),
            )
            .expect("assemble DB context pack v1");
        assert_eq!(
            context_pack.schema_version,
            hyprduck_engine_types::CONTEXT_PACK_V1_SCHEMA_VERSION
        );
        assert_eq!(
            context_pack.retrieval_trace.strategy,
            "sqlite-graphqlite-fts5-hybrid"
        );
        assert!(context_pack
            .selected_evidence
            .iter()
            .any(|evidence| evidence.evidence_ref == "evidence-a"));
        let context_pack_json =
            serde_json::to_string(&context_pack).expect("serialize context pack");
        assert!(!context_pack_json.contains("/Users/hyprduck/private"));
        assert!(context_pack
            .retrieval_trace
            .evidence_type_trace
            .selected
            .get("text")
            .is_some_and(|count| *count >= 1));
        let source_response = store
            .read_source_from_db("workspace-default", "source-a", false)
            .expect("read source from DB")
            .expect("source response");
        assert_eq!(source_response.source.source_id, "source-a");
        assert_eq!(
            source_response.source.original_path,
            "original-source-a.pdf"
        );
        assert_eq!(source_response.source.source_path, "source-a.pdf");
        assert_eq!(source_response.source.markdown_path, "source-a.md");
        assert_eq!(source_response.evidence.len(), 2);
        let source_response_json =
            serde_json::to_string(&source_response).expect("serialize source response");
        assert!(!source_response_json.contains("/Users/hyprduck/private"));
        assert!(source_response
            .evidence
            .iter()
            .all(|evidence| evidence.source_path.as_deref() == Some("source-a.pdf")));
        assert!(source_response.evidence.iter().all(|evidence| {
            evidence.markdown_path.as_deref() == Some("source-a.md")
                && evidence.image_path.as_deref() == Some("source-a.png")
        }));
        assert_eq!(
            source_response
                .wiki_page
                .as_ref()
                .map(|page| page.page_id.as_str()),
            Some("wiki-alpha")
        );
        let page_response = store
            .read_page_evidence_from_db("workspace-default", "source-a", Some(1), false)
            .expect("read page evidence from DB")
            .expect("page evidence response");
        assert_eq!(page_response.source.source_id, "source-a");
        assert_eq!(page_response.evidence.len(), 1);
        assert_eq!(page_response.evidence[0].evidence_ref, "evidence-a");
        let page_response_json =
            serde_json::to_string(&page_response).expect("serialize page evidence response");
        assert!(!page_response_json.contains("/Users/hyprduck/private"));
        assert_eq!(
            page_response.evidence[0].markdown_path.as_deref(),
            Some("source-a.md")
        );
        assert_eq!(
            page_response.evidence[0].image_path.as_deref(),
            Some("source-a.png")
        );
        let wiki_page = store
            .read_wiki_page_from_db("workspace-default", "wiki/alpha")
            .expect("read wiki page from DB")
            .expect("wiki page");
        assert_eq!(wiki_page.page_id, "wiki-alpha");
        let _ = store
            .read_node_from_db("workspace-default", "node-a")
            .expect("read node from DB");
        let (graph_nodes, graph_relations, graph_wiki_pages) = store
            .read_graph_canvas_projection_from_db("workspace-default")
            .expect("read graph canvas projection")
            .expect("graph canvas projection");
        assert!(graph_nodes.iter().any(|node| node.node_id == "node-a"));
        assert!(graph_nodes
            .iter()
            .any(|node| node.node_id == "source:source-a"));
        assert!(graph_relations
            .iter()
            .any(|relation| relation.relation_id == "rel-a"));
        assert_eq!(graph_wiki_pages.len(), 1);
        assert_eq!(graph_wiki_pages[0].page_id, "wiki-alpha");
        update_evidence_status(&store, "evidence-b", "failed");
        let filtered_hits = store
            .hybrid_retrieve("workspace-default", "Alpha", 5)
            .expect("filtered hybrid retrieve");
        assert!(filtered_hits
            .iter()
            .all(|hit| hit.evidence_id != "evidence-b"));
        let wiki_hits = store
            .hybrid_retrieve("workspace-default", "source evidence", 5)
            .expect("wiki hybrid retrieve");
        assert_eq!(wiki_hits.len(), 1);
        assert_eq!(wiki_hits[0].evidence_id, "evidence-a");
        assert_eq!(wiki_hits[0].source_id, "wiki-alpha");
        assert_eq!(wiki_hits[0].evidence_type, "wiki_evidence");
        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("brain event count"),
            1
        );
        assert_eq!(
            graph_checkpoint_count(&store, "workspace-default").expect("checkpoint count"),
            1
        );
        assert_graph_checkpoint_metadata(&store, "workspace-default");
    }

    fn assert_wiki_relational_content(store: &KnowledgeStore, wiki_page_id: &str) {
        let graph = Graph::open(store.path()).expect("open graph");
        let sqlite = graph.connection().sqlite_connection();
        let page = sqlite
            .query_row(
                "SELECT path, approval_status, revision, evidence_refs_json
                 FROM wiki_pages
                 WHERE wiki_page_id = ?1",
                [wiki_page_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .expect("wiki page row");
        assert_eq!(page.0, "wiki/alpha");
        assert_eq!(page.1, "materialized");
        assert_eq!(page.2, 1);
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&page.3).expect("wiki evidence refs"),
            vec!["evidence-a"]
        );

        let revision = sqlite
            .query_row(
                "SELECT approval_status, diff_json, body
                 FROM wiki_revisions
                 WHERE wiki_page_id = ?1 AND revision = 1",
                [wiki_page_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("wiki revision row");
        assert_eq!(revision.0, "materialized");
        assert_eq!(revision.1, "{}");
        assert!(revision.2.contains("Alpha wiki body."));

        let sections = sqlite
            .query_row(
                "SELECT count(*) FROM wiki_sections WHERE wiki_page_id = ?1 AND revision = 1",
                [wiki_page_id],
                |row| row.get::<_, i64>(0),
            )
            .expect("wiki section count");
        assert_eq!(sections, 2);

        let fts_hits = sqlite
            .query_row(
                "SELECT count(*) FROM wiki_fts WHERE wiki_fts MATCH 'source'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("wiki fts count");
        assert_eq!(fts_hits, 1);
    }

    fn update_evidence_status(store: &KnowledgeStore, evidence_id: &str, status: &str) {
        let graph = Graph::open(store.path()).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .execute(
                "UPDATE evidence_items SET status = ?2 WHERE evidence_id = ?1",
                (evidence_id, status),
            )
            .expect("update evidence status");
    }

    fn update_source_context_pack_metadata(store: &KnowledgeStore, source_id: &str) {
        let graph = Graph::open(store.path()).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .execute(
                "UPDATE sources
                 SET provider_route = 'test-local',
                     provider_locality = 'local',
                     content_hash = 'sha256:test-context-pack'
                 WHERE source_id = ?1",
                [source_id],
            )
            .expect("update source context metadata");
    }

    fn assert_source_page_fts_content(store: &KnowledgeStore) {
        let graph = Graph::open(store.path()).expect("open graph");
        let sqlite = graph.connection().sqlite_connection();
        let fts_hits = sqlite
            .query_row(
                "SELECT count(*) FROM source_page_fts WHERE source_page_fts MATCH 'relates'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("source page fts count");
        assert_eq!(fts_hits, 1);
    }

    #[test]
    fn graph_snapshot_rejects_missing_relational_evidence_refs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        let snapshot = BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: 10,
            sources: Vec::new(),
            nodes: vec![BrainNodeRecord {
                node_id: "node-a".into(),
                kind: BrainNodeKind::Concept,
                label: "Alpha".into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec!["missing-evidence".into()],
                source_ids: Vec::new(),
                confidence: None,
                updated_at: 10,
            }],
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: vec![test_brain_event(
                "event-invalid",
                "workspace-default",
                &["missing-evidence"],
            )],
        };

        let error = store
            .persist_graph_snapshot(&snapshot)
            .expect_err("missing evidence ref should fail graph publish");

        assert!(error
            .to_string()
            .contains("references missing relational evidence row missing-evidence"));
        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("brain event count"),
            0
        );
    }

    #[test]
    fn wiki_content_rejects_missing_evidence_before_durable_rows_commit() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        let snapshot = BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: 10,
            sources: Vec::new(),
            nodes: Vec::new(),
            relations: Vec::new(),
            evidence: Vec::new(),
            memories: Vec::new(),
            wiki_pages: vec![WikiPage {
                page_id: "wiki-missing-evidence".into(),
                workspace_id: "workspace-default".into(),
                path: "wiki/missing-evidence".into(),
                title: "Missing Evidence Wiki".into(),
                body: "This durable wiki content cites evidence that is not relationally present."
                    .into(),
                node_refs: Vec::new(),
                source_refs: Vec::new(),
                evidence_refs: vec!["missing-evidence".into()],
                updated_at: 10,
            }],
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: Vec::new(),
        };

        let error = store
            .persist_graph_snapshot(&snapshot)
            .expect_err("wiki evidence ref validation should fail before commit");

        assert!(error
            .to_string()
            .contains("wiki page wiki-missing-evidence references missing relational evidence row missing-evidence"));
        assert_eq!(
            wiki_page_count(&store, "workspace-default").expect("wiki page count"),
            0
        );
        assert_eq!(
            wiki_revision_count(&store, "workspace-default").expect("wiki revision count"),
            0
        );
    }

    #[test]
    fn graph_snapshot_appends_brain_events_in_graph_transaction() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");

        let first_snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
        store
            .persist_graph_snapshot(&first_snapshot)
            .expect("persist first graph snapshot");
        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("first event count"),
            1
        );

        let second_snapshot = single_event_snapshot("event-b", "source-b", "evidence-b", "Beta");
        store
            .persist_graph_snapshot(&second_snapshot)
            .expect("persist second graph snapshot");

        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("second event count"),
            2
        );
        assert_eq!(
            store
                .graph_snapshot_counts("workspace-default")
                .expect("graph counts"),
            KnowledgeGraphPersistReport {
                node_count: 2,
                relation_count: 0,
            }
        );
    }

    #[test]
    fn graph_snapshot_marks_citation_ready_import_job_graph_ready_after_commit() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        insert_citation_ready_import_job(&store, "source-a");

        let snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
        store
            .persist_graph_snapshot(&snapshot)
            .expect("persist graph snapshot");

        assert_eq!(import_job_readiness(&store, "source-a"), (1, 1));
        assert_eq!(
            import_job_status(&store, "source-a"),
            "context_ready",
            "graph-ready commits should keep import lifecycle status consistent"
        );
    }

    #[test]
    fn import_job_graph_pending_state_round_trips_for_source_retry() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        insert_citation_ready_import_job(&store, "source-a");

        assert!(store
            .update_import_job_graph_status_from_mcp(
                "workspace-default",
                "source-a",
                "citation_ready_graph_pending",
                "pending",
                Some("db_busy"),
                Some("database is busy"),
                true,
                1,
                2,
                Some(123),
                true,
            )
            .expect("update graph pending state"));

        let job = store
            .read_import_job("workspace-default", None, Some("source-a"))
            .expect("read import job")
            .expect("job should exist");
        assert_eq!(job.status, "citation_ready_graph_pending");
        assert!(job.citation_ready);
        assert!(!job.graph_ready);
        assert_eq!(job.graph_status, "pending");
        assert_eq!(job.graph_error_category, "db_busy");
        assert_eq!(job.graph_error_message_redacted, "database is busy");
        assert!(job.graph_retryable);
        assert_eq!(job.graph_retry_attempt, 1);
        assert_eq!(job.graph_max_retry_attempts, 2);
        assert_eq!(job.graph_next_retry_at, Some(123));
        assert!(job.manual_retry_available);
    }

    #[test]
    fn graphqlite_mutation_failure_rolls_back_relational_graph_audit_writes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(temp.path()))
            .expect("open knowledge store");
        insert_citation_ready_import_job(&store, "source-a");
        let graph = Graph::open(&store.path).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .execute_batch(
                "CREATE TRIGGER fail_graph_node_insert
                 BEFORE INSERT ON nodes
                 BEGIN
                   SELECT RAISE(FAIL, 'forced GraphQLite node failure');
                 END;",
            )
            .expect("install GraphQLite failure trigger");

        let snapshot = single_event_snapshot("event-a", "source-a", "evidence-a", "Alpha");
        let error = store
            .persist_graph_snapshot(&snapshot)
            .expect_err("GraphQLite node mutation should fail");

        assert!(error
            .to_string()
            .contains("failed upserting GraphQLite node"));
        assert_eq!(
            evidence_item_count(&store, "workspace-default").expect("evidence count"),
            0
        );
        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("brain event count"),
            0
        );
        assert_eq!(
            store
                .graph_snapshot_counts("workspace-default")
                .expect("graph counts"),
            KnowledgeGraphPersistReport {
                node_count: 0,
                relation_count: 0,
            }
        );
        assert_eq!(import_job_readiness(&store, "source-a"), (1, 0));
    }

    fn insert_citation_ready_import_job(store: &KnowledgeStore, source_id: &str) {
        let graph = Graph::open(&store.path).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .execute_batch(&format!(
                "INSERT INTO import_jobs
                   (job_id, workspace_id, source_id, status, citation_ready, graph_ready, created_at, updated_at, error_message)
                 VALUES ({job_id}, 'workspace-default', {source_id}, 'completed', 1, 0, 1, 1, NULL);",
                job_id = sql_literal(&format!("import:{source_id}")),
                source_id = sql_literal(source_id),
            ))
            .expect("insert citation-ready import job");
    }

    fn import_job_readiness(store: &KnowledgeStore, source_id: &str) -> (i64, i64) {
        let graph = Graph::open(&store.path).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT citation_ready, graph_ready FROM import_jobs WHERE source_id = ?1",
                [source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read import job readiness")
    }

    fn import_job_status(store: &KnowledgeStore, source_id: &str) -> String {
        let graph = Graph::open(&store.path).expect("open graph");
        graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT status FROM import_jobs WHERE source_id = ?1",
                [source_id],
                |row| row.get(0),
            )
            .expect("read import job status")
    }

    fn brain_event_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
        let graph = Graph::open(&store.path).context("open graph")?;
        let count = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT COUNT(*) FROM brain_events WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .context("query brain event count")?;
        Ok(count)
    }

    fn evidence_item_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
        let graph = Graph::open(&store.path).context("open graph")?;
        let count = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT COUNT(*) FROM evidence_items WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .context("query evidence item count")?;
        Ok(count)
    }

    fn graph_checkpoint_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
        let graph = Graph::open(&store.path).context("open graph")?;
        let count = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT COUNT(*) FROM graph_checkpoints WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .context("query graph checkpoint count")?;
        Ok(count)
    }

    fn assert_graph_checkpoint_metadata(store: &KnowledgeStore, workspace_id: &str) {
        let graph = Graph::open(&store.path).expect("open graph");
        let row = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT checkpoint_id,
                        reason,
                        actor_json,
                        related_event_id,
                        graph_schema_version,
                        graphqlite_extension_version,
                        node_count,
                        edge_count,
                        evidence_ref_count,
                        checksum,
                        storage_ref
                 FROM graph_checkpoints
                 WHERE workspace_id = ?1",
                [workspace_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .expect("graph checkpoint metadata row");
        assert!(row
            .0
            .starts_with("graph-checkpoint-workspace-default-event-a-"));
        assert_eq!(row.1, "graph_snapshot_commit");
        assert!(row.2.contains("hyprduck-knowledge-store"));
        assert_eq!(row.3, "event-a");
        assert_eq!(row.4, GRAPHQLITE_SCHEMA_VERSION);
        assert_eq!(row.5, env!("CARGO_PKG_VERSION"));
        assert_eq!(row.6, 6);
        assert_eq!(row.7, 3);
        assert_eq!(row.8, 2);
        assert_eq!(row.9.len(), 64);
        assert_eq!(row.10, "hyprduck.sqlite:graphqlite");
    }

    fn wiki_page_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
        let graph = Graph::open(&store.path).context("open graph")?;
        let count = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT COUNT(*) FROM wiki_pages WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .context("query wiki page count")?;
        Ok(count)
    }

    fn wiki_revision_count(store: &KnowledgeStore, workspace_id: &str) -> Result<i64> {
        let graph = Graph::open(&store.path).context("open graph")?;
        let count = graph
            .connection()
            .sqlite_connection()
            .query_row(
                "SELECT COUNT(*) FROM wiki_revisions WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .context("query wiki revision count")?;
        Ok(count)
    }

    fn assert_graph_node_metadata(store: &KnowledgeStore, node_id: &str) {
        let graph = Graph::open(&store.path).expect("open graph");
        let rows = graph
            .connection()
            .cypher_builder(
                "MATCH (n {id: $node_id})
                 RETURN n.evidence_ids_json AS evidence_ids_json,
                        n.source_ids_json AS source_ids_json,
                        n.producer_run_id AS producer_run_id,
                        n.producer_run_ids_json AS producer_run_ids_json,
                        n.confidence AS confidence,
                        n.status AS status,
                        n.updated_at AS updated_at",
            )
            .param("node_id", node_id)
            .run()
            .expect("query graph node metadata");
        let row = rows.get(0).expect("graph node metadata row");
        assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
        assert_string_array(row, "source_ids_json", &["source-a"]);
        assert_eq!(
            row.get::<String>("producer_run_id")
                .expect("producer run id"),
            "provider-run-alpha"
        );
        assert_string_array(row, "producer_run_ids_json", &["provider-run-alpha"]);
        assert_eq!(row.get::<f64>("confidence").expect("confidence"), 0.9);
        assert_eq!(row.get::<String>("status").expect("status"), "active");
        assert_eq!(row.get::<i64>("updated_at").expect("updated at"), 10);
    }

    fn assert_graph_wiki_page_node(store: &KnowledgeStore, node_id: &str) {
        let graph = Graph::open(&store.path).expect("open graph");
        let rows = graph
            .connection()
            .cypher_builder(
                "MATCH (n:WikiPage {id: $node_id})
                 RETURN n.kind AS kind,
                        n.label AS label,
                        n.aliases_json AS aliases_json,
                        n.evidence_ids_json AS evidence_ids_json,
                        n.source_ids_json AS source_ids_json,
                        n.status AS status",
            )
            .param("node_id", node_id)
            .run()
            .expect("query wiki page graph node");
        let row = rows.get(0).expect("wiki page graph node row");
        assert_eq!(row.get::<String>("kind").expect("kind"), "wiki_page");
        assert_eq!(row.get::<String>("label").expect("label"), "Alpha Wiki");
        assert_string_array(row, "aliases_json", &["wiki/alpha"]);
        assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
        assert_string_array(row, "source_ids_json", &["source-a"]);
        assert_eq!(row.get::<String>("status").expect("status"), "active");
    }

    fn assert_graph_edge_metadata(
        store: &KnowledgeStore,
        source_node_id: &str,
        target_node_id: &str,
        relation_type: &str,
    ) {
        let graph = Graph::open(&store.path).expect("open graph");
        let rows = graph
            .connection()
            .cypher_builder(&format!(
                "MATCH (a {{id: $source_node_id}})-[r:{relation_type}]->(b {{id: $target_node_id}})
                 RETURN r.evidence_ids_json AS evidence_ids_json,
                        r.source_ids_json AS source_ids_json,
                        r.producer_run_id AS producer_run_id,
                        r.producer_run_ids_json AS producer_run_ids_json,
                        r.confidence AS confidence,
                        r.status AS status,
                        r.updated_at AS updated_at"
            ))
            .param("source_node_id", source_node_id)
            .param("target_node_id", target_node_id)
            .run()
            .expect("query graph edge metadata");
        let row = rows.get(0).expect("graph edge metadata row");
        assert_string_array(row, "evidence_ids_json", &["evidence-a"]);
        assert_string_array(row, "source_ids_json", &["source-a"]);
        assert_eq!(
            row.get::<String>("producer_run_id")
                .expect("producer run id"),
            "provider-run-alpha"
        );
        assert_string_array(row, "producer_run_ids_json", &["provider-run-alpha"]);
        assert_eq!(row.get::<f64>("confidence").expect("confidence"), 0.8);
        assert_eq!(row.get::<String>("status").expect("status"), "active");
        assert_eq!(row.get::<i64>("updated_at").expect("updated at"), 10);
    }

    fn assert_relational_proof_ignores_graph_metadata_tamper(store: &KnowledgeStore) {
        let graph = Graph::open(&store.path).expect("open graph");
        graph
            .upsert_node(
                "node-a",
                [
                    ("workspace_id", "workspace-default"),
                    ("evidence_ids_json", "[\"graph-only-evidence\"]"),
                    ("source_ids_json", "[\"graph-only-source\"]"),
                ],
                "Concept",
            )
            .expect("tamper GraphQLite node metadata");

        let proof = store
            .resolve_evidence_proof("workspace-default", "evidence-a")
            .expect("resolve relational evidence proof");
        assert_eq!(proof.evidence_id, "evidence-a");
        assert_eq!(proof.source_id, "source-a");
        assert_eq!(proof.page_index, Some(0));
        assert_eq!(proof.page_label, "p1");
        assert_eq!(proof.evidence_type, "text_evidence");
        assert_eq!(proof.snippet, "Alpha relates to beta.");
        assert_eq!(proof.status, "active");

        let error = store
            .resolve_evidence_proof("workspace-default", "graph-only-evidence")
            .expect_err("graph-only evidence ref must not resolve proof");
        assert!(error
            .to_string()
            .contains("missing relational evidence row graph-only-evidence"));
    }

    fn assert_string_array(row: &graphqlite::Row, column: &str, expected: &[&str]) {
        let values = match row.get_value(column).expect("array column exists") {
            graphqlite::Value::Array(values) => values
                .iter()
                .map(|value| match value {
                    graphqlite::Value::String(value) => value.clone(),
                    other => panic!("unexpected array value for {column}: {other:?}"),
                })
                .collect::<Vec<_>>(),
            other => panic!("unexpected value for {column}: {other:?}"),
        };
        assert_eq!(values, expected);
    }

    fn single_event_snapshot(
        event_id: &str,
        source_id: &str,
        evidence_id: &str,
        label: &str,
    ) -> BrainRepoSnapshot {
        BrainRepoSnapshot {
            workspace_id: "workspace-default".into(),
            generated_at: 10,
            sources: vec![SourceRecord {
                source_id: source_id.into(),
                workspace_id: "workspace-default".into(),
                original_path: format!("/tmp/{source_id}.pdf"),
                source_path: format!("sources/{source_id}.pdf"),
                markdown_path: format!("sources/{source_id}.md"),
                format: SourceFormat::pdf(),
                status: SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 10,
            }],
            nodes: vec![BrainNodeRecord {
                node_id: format!("node-{source_id}"),
                kind: BrainNodeKind::Concept,
                label: label.into(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: vec![evidence_id.into()],
                source_ids: vec![source_id.into()],
                confidence: Some(0.9),
                updated_at: 10,
            }],
            relations: Vec::new(),
            evidence: vec![EvidenceRef {
                id: evidence_id.into(),
                page_label: "p1".into(),
                page_index: Some(0),
                snippet: format!("{label} evidence."),
                source_path: Some(format!("sources/{source_id}.pdf")),
                source_id: Some(source_id.into()),
                markdown_path: Some(format!("sources/{source_id}.md")),
                image_path: None,
                provenance: Some("test".into()),
            }],
            memories: Vec::new(),
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
            events: vec![test_brain_event(
                event_id,
                "workspace-default",
                &[evidence_id],
            )],
        }
    }

    fn test_brain_event(event_id: &str, workspace_id: &str, evidence_refs: &[&str]) -> BrainEvent {
        BrainEvent {
            event_id: event_id.into(),
            schema_version: BRAIN_EVENT_SCHEMA_VERSION,
            workspace_id: workspace_id.into(),
            scope: BrainScope::Project,
            event_type: BrainEventKind::GraphMaterialized,
            operation_type: Some("graph_materialized".into()),
            actor: BrainActor {
                actor_type: BrainActorType::Agent,
                actor_id: "test-agent".into(),
            },
            source_refs: Vec::new(),
            source_markdown_refs: Vec::new(),
            node_refs: Vec::new(),
            relation_refs: Vec::new(),
            claim_refs: Vec::new(),
            memory_refs: Vec::new(),
            target_node_ids: Vec::new(),
            target_edge_ids: Vec::new(),
            target_claim_ids: Vec::new(),
            target_memory_ids: Vec::new(),
            evidence_refs: evidence_refs.iter().map(|value| (*value).into()).collect(),
            payload_json: "{}".into(),
            causality: BrainEventCausality::default(),
            confidence: None,
            policy_result: PolicyResult::materialized(),
            created_at: 10,
        }
    }
}
