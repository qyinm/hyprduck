use anyhow::{anyhow, Context, Result};
use graphqlite::{Graph, PropertyValue};
#[cfg(test)]
use hyprduck_engine_types::{
    BrainActor, BrainActorType, BrainEvent, BrainEventCausality, BrainEventKind, ClaimRecord,
    EntityRecord, PolicyResult, StructuredExtractionArtifact, BRAIN_EVENT_SCHEMA_VERSION,
};
use hyprduck_engine_types::{
    BrainContextPack, BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord,
    BrainRepoSnapshot, BrainScope, BrainSearchResult, BrainSearchResultKind,
    ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackParseConfidence,
    ContextPackSourceMetadataV0, ContextPackV1, EvidenceRef, EvidenceType, KnowledgeProject,
    PageEvidenceV0, ReadNodeResponseData, ReadPageEvidenceResponseData, ReadSourceResponseData,
    SourceArtifactManifest, SourceFormat, SourceRecord, SourceStatus, WikiPage,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const KNOWLEDGE_DB_FILE_NAME: &str = "hyprduck.sqlite";
const KNOWLEDGE_SCHEMA_VERSION: i64 = 1;
const GRAPHQLITE_SCHEMA_VERSION: i64 = 1;

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
pub(crate) struct GraphqliteGateReport {
    pub(crate) loaded: bool,
    pub(crate) cypher_ready: bool,
    pub(crate) rollback_ready: bool,
    pub(crate) graph_schema_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KnowledgeGraphPersistReport {
    pub(crate) node_count: usize,
    pub(crate) relation_count: usize,
}

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

#[derive(Debug, Clone)]
struct ContextPackEvidenceRow {
    evidence_id: String,
    source_id: String,
    page_index: Option<i64>,
    page_label: String,
    evidence_type: String,
    snippet: String,
    source_path_redacted: String,
    markdown_path_redacted: String,
    image_path_redacted: String,
    provenance: String,
    confidence: Option<f64>,
}

#[derive(Debug, Clone)]
struct ContextPackSourceRow {
    source_id: String,
    workspace_id: String,
    original_path_redacted: String,
    source_path_redacted: String,
    markdown_path_redacted: String,
    format: String,
    status: String,
    page_count: i64,
    provider_route: String,
    provider_locality: String,
    content_hash: String,
    updated_at: i64,
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
                "INSERT INTO import_jobs (job_id, workspace_id, source_id, status, citation_ready, graph_ready, created_at, updated_at, error_message)
                 VALUES ({job_id}, {workspace_id}, {source_id}, {status}, {citation_ready}, 0, {created_at}, {updated_at}, NULL)
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
                citation_ready = if success_count > 0 { 1 } else { 0 },
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
                    markdown_path = sql_optional_literal(page.markdown_path.as_deref()),
                    image_path = sql_optional_literal(page.image_path.as_deref()),
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

            for evidence in unique_project_evidence(project) {
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
    ) -> Result<Option<ReadSourceResponseData>> {
        let graph = Graph::open(&self.path).context("GraphQLite failed to open knowledge DB")?;
        let Some(source) = load_context_pack_source_row(&graph, workspace_id, source_id)? else {
            return Ok(None);
        };
        let evidence = load_evidence_refs_for_source(&graph, workspace_id, source_id, None)?;
        let wiki_page = load_wiki_page_for_source(&graph, workspace_id, source_id)?;
        Ok(Some(ReadSourceResponseData {
            source: source_record_from_context_row(source),
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
            source: source_record_from_context_row(source),
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

    #[cfg(test)]
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

fn db_search_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect()
}

fn db_match_score(terms: &[String], haystack: &str) -> Option<usize> {
    let lower = haystack.to_lowercase();
    let matched = terms.iter().filter(|term| lower.contains(*term)).count();
    (matched > 0).then(|| matched * 100)
}

fn db_best_snippet(text: &str, terms: &[String]) -> String {
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

fn db_float_score(score: f64) -> usize {
    (score.max(0.0) * 1000.0).round() as usize
}

fn evidence_snippet_from_ids(evidence_ids: &[String]) -> String {
    if evidence_ids.is_empty() {
        String::new()
    } else {
        format!("Evidence refs: {}", evidence_ids.join(", "))
    }
}

fn load_context_pack_evidence_row(
    graph: &Graph,
    workspace_id: &str,
    evidence_id: &str,
) -> Result<Option<ContextPackEvidenceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                evidence_id,
                source_id,
                page_index,
                page_label,
                evidence_type,
                snippet,
                source_path_redacted,
                markdown_path_redacted,
                image_path_redacted,
                provenance,
                confidence
             FROM evidence_items
             WHERE workspace_id = ?1
               AND evidence_id = ?2
               AND status = 'active'",
        )
        .context("failed preparing context pack evidence row query")?;
    let mut rows = statement
        .query((workspace_id, evidence_id))
        .context("failed querying context pack evidence row")?;
    let Some(row) = rows
        .next()
        .context("failed reading context pack evidence row")?
    else {
        return Ok(None);
    };
    Ok(Some(ContextPackEvidenceRow {
        evidence_id: row.get(0).context("read context evidence id")?,
        source_id: row.get(1).context("read context evidence source")?,
        page_index: row.get(2).context("read context evidence page index")?,
        page_label: row.get(3).context("read context evidence page label")?,
        evidence_type: row.get(4).context("read context evidence type")?,
        snippet: row.get(5).context("read context evidence snippet")?,
        source_path_redacted: row.get(6).context("read context evidence source path")?,
        markdown_path_redacted: row.get(7).context("read context evidence markdown path")?,
        image_path_redacted: row.get(8).context("read context evidence image path")?,
        provenance: row.get(9).context("read context evidence provenance")?,
        confidence: row.get(10).context("read context evidence confidence")?,
    }))
}

fn load_context_pack_source_row(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
) -> Result<Option<ContextPackSourceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                source_id,
                workspace_id,
                original_path_redacted,
                source_path_redacted,
                markdown_path_redacted,
                format,
                status,
                page_count,
                provider_route,
                provider_locality,
                content_hash,
                updated_at
             FROM sources
             WHERE workspace_id = ?1
               AND source_id = ?2
               AND status NOT IN ('failed', 'stale', 'hash_mismatched', 'unapproved')",
        )
        .context("failed preparing context pack source row query")?;
    let mut rows = statement
        .query((workspace_id, source_id))
        .context("failed querying context pack source row")?;
    let Some(row) = rows
        .next()
        .context("failed reading context pack source row")?
    else {
        return Ok(None);
    };
    Ok(Some(ContextPackSourceRow {
        source_id: row.get(0).context("read context source id")?,
        workspace_id: row.get(1).context("read context source workspace")?,
        original_path_redacted: row.get(2).context("read context source original path")?,
        source_path_redacted: row.get(3).context("read context source source path")?,
        markdown_path_redacted: row.get(4).context("read context source markdown path")?,
        format: row.get(5).context("read context source format")?,
        status: row.get(6).context("read context source status")?,
        page_count: row.get(7).context("read context source page count")?,
        provider_route: row.get(8).context("read context source provider route")?,
        provider_locality: row
            .get(9)
            .context("read context source provider locality")?,
        content_hash: row.get(10).context("read context source content hash")?,
        updated_at: row.get(11).context("read context source updated at")?,
    }))
}

fn load_context_pack_evidence_rows_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    page_index: Option<i64>,
) -> Result<Vec<ContextPackEvidenceRow>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT
                evidence_id,
                source_id,
                page_index,
                page_label,
                evidence_type,
                snippet,
                source_path_redacted,
                markdown_path_redacted,
                image_path_redacted,
                provenance,
                confidence
             FROM evidence_items
             WHERE workspace_id = ?1
               AND source_id = ?2
               AND (?3 IS NULL OR page_index = ?3)
               AND status = 'active'
             ORDER BY page_index ASC, evidence_id ASC",
        )
        .context("failed preparing source evidence rows query")?;
    let mut rows = statement
        .query((workspace_id, source_id, page_index))
        .context("failed querying source evidence rows")?;
    let mut evidence_rows = Vec::new();
    while let Some(row) = rows.next().context("failed reading source evidence row")? {
        evidence_rows.push(ContextPackEvidenceRow {
            evidence_id: row.get(0).context("read source evidence id")?,
            source_id: row.get(1).context("read source evidence source")?,
            page_index: row.get(2).context("read source evidence page index")?,
            page_label: row.get(3).context("read source evidence page label")?,
            evidence_type: row.get(4).context("read source evidence type")?,
            snippet: row.get(5).context("read source evidence snippet")?,
            source_path_redacted: row.get(6).context("read source evidence source path")?,
            markdown_path_redacted: row.get(7).context("read source evidence markdown path")?,
            image_path_redacted: row.get(8).context("read source evidence image path")?,
            provenance: row.get(9).context("read source evidence provenance")?,
            confidence: row.get(10).context("read source evidence confidence")?,
        });
    }
    Ok(evidence_rows)
}

fn load_evidence_refs_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
    page_index: Option<i64>,
) -> Result<Vec<EvidenceRef>> {
    Ok(
        load_context_pack_evidence_rows_for_source(graph, workspace_id, source_id, page_index)?
            .into_iter()
            .map(|row| EvidenceRef {
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
            .collect(),
    )
}

fn load_evidence_refs_by_ids(
    graph: &Graph,
    workspace_id: &str,
    evidence_ids: &[String],
) -> Result<Vec<EvidenceRef>> {
    let mut evidence = Vec::new();
    for evidence_id in evidence_ids {
        if let Some(row) = load_context_pack_evidence_row(graph, workspace_id, evidence_id)? {
            evidence.push(EvidenceRef {
                id: row.evidence_id,
                page_label: row.page_label,
                page_index: row.page_index.map(|page_index| page_index.max(0) as usize),
                snippet: row.snippet,
                source_path: non_empty_string(row.source_path_redacted),
                source_id: Some(row.source_id),
                markdown_path: non_empty_string(row.markdown_path_redacted),
                image_path: non_empty_string(row.image_path_redacted),
                provenance: non_empty_string(row.provenance),
            });
        }
    }
    Ok(evidence)
}

fn load_wiki_page_for_source(
    graph: &Graph,
    workspace_id: &str,
    source_id: &str,
) -> Result<Option<WikiPage>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, path, title, body, evidence_refs_json, updated_at
             FROM wiki_pages
             WHERE workspace_id = ?1
               AND approval_status IN ('materialized', 'approved')
               AND evidence_refs_json <> '[]'
             ORDER BY updated_at DESC",
        )
        .context("failed preparing wiki page source query")?;
    let mut rows = statement
        .query([workspace_id])
        .context("failed querying wiki page source rows")?;
    while let Some(row) = rows.next().context("failed reading wiki page source row")? {
        let evidence_refs_json: String = row.get(4).context("read wiki evidence refs")?;
        let evidence_refs =
            serde_json::from_str::<Vec<String>>(&evidence_refs_json).unwrap_or_default();
        if !wiki_evidence_refs_source(graph, workspace_id, &evidence_refs, source_id)? {
            continue;
        }
        return Ok(Some(WikiPage {
            page_id: row.get(0).context("read wiki id")?,
            workspace_id: workspace_id.into(),
            path: row.get(1).context("read wiki path")?,
            title: row.get(2).context("read wiki title")?,
            body: row.get(3).context("read wiki body")?,
            node_refs: Vec::new(),
            source_refs: vec![source_id.into()],
            evidence_refs,
            updated_at: row.get::<_, i64>(5).context("read wiki updated at")?.max(0) as u64,
        }));
    }
    Ok(None)
}

fn load_wiki_page_by_path(
    graph: &Graph,
    workspace_id: &str,
    path: &str,
) -> Result<Option<WikiPage>> {
    let sqlite = graph.connection().sqlite_connection();
    let mut statement = sqlite
        .prepare(
            "SELECT wiki_page_id, title, body, evidence_refs_json, updated_at
             FROM wiki_pages
             WHERE workspace_id = ?1
               AND path = ?2
               AND approval_status IN ('materialized', 'approved')
             LIMIT 1",
        )
        .context("failed preparing wiki page by path query")?;
    let mut rows = statement
        .query((workspace_id, path))
        .context("failed querying wiki page by path")?;
    let Some(row) = rows.next().context("failed reading wiki page by path")? else {
        return Ok(None);
    };
    Ok(Some(WikiPage {
        page_id: row.get(0).context("read wiki id")?,
        workspace_id: workspace_id.into(),
        path: path.into(),
        title: row.get(1).context("read wiki title")?,
        body: row.get(2).context("read wiki body")?,
        node_refs: Vec::new(),
        source_refs: Vec::new(),
        evidence_refs: serde_json::from_str::<Vec<String>>(
            &row.get::<_, String>(3).context("read wiki evidence refs")?,
        )
        .unwrap_or_default(),
        updated_at: row.get::<_, i64>(4).context("read wiki updated at")?.max(0) as u64,
    }))
}

fn wiki_evidence_refs_source(
    graph: &Graph,
    workspace_id: &str,
    evidence_refs: &[String],
    source_id: &str,
) -> Result<bool> {
    for evidence_ref in evidence_refs {
        if load_context_pack_evidence_row(graph, workspace_id, evidence_ref)?
            .is_some_and(|row| row.source_id == source_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn source_record_from_context_row(source: ContextPackSourceRow) -> SourceRecord {
    SourceRecord {
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
    }
}

fn db_parse_confidence(confidence: Option<f64>) -> ContextPackParseConfidence {
    match confidence {
        Some(value) if value >= 0.8 => ContextPackParseConfidence::High,
        Some(value) if value >= 0.5 => ContextPackParseConfidence::Medium,
        Some(_) => ContextPackParseConfidence::Low,
        None => ContextPackParseConfidence::Unknown,
    }
}

fn db_context_evidence_type(evidence_type: &str) -> EvidenceType {
    match evidence_type {
        "text_evidence" => EvidenceType::Text,
        "table_evidence" => EvidenceType::Table,
        "image_region_evidence" => EvidenceType::ImageRegion,
        "ocr_evidence" => EvidenceType::Ocr,
        "caption_evidence" => EvidenceType::Caption,
        "summary_evidence" | "wiki_evidence" => EvidenceType::Summary,
        "claim_evidence" => EvidenceType::Claim,
        "relationship_evidence" => EvidenceType::Relationship,
        _ => EvidenceType::Unknown,
    }
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

fn object_string(properties: &HashMap<String, graphqlite::Value>, key: &str) -> String {
    match properties.get(key) {
        Some(graphqlite::Value::String(value)) => value.clone(),
        Some(graphqlite::Value::Integer(value)) => value.to_string(),
        Some(graphqlite::Value::Float(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn object_i64(properties: &HashMap<String, graphqlite::Value>, key: &str) -> i64 {
    match properties.get(key) {
        Some(graphqlite::Value::Integer(value)) => *value,
        Some(graphqlite::Value::Float(value)) => *value as i64,
        Some(graphqlite::Value::String(value)) => value.parse::<i64>().unwrap_or_default(),
        _ => 0,
    }
}

fn object_optional_f32(properties: &HashMap<String, graphqlite::Value>, key: &str) -> Option<f32> {
    match properties.get(key) {
        Some(graphqlite::Value::Float(value)) => Some(*value as f32),
        Some(graphqlite::Value::Integer(value)) => Some(*value as f32),
        Some(graphqlite::Value::String(value)) => parse_optional_f32(value),
        _ => None,
    }
}

fn object_string_array(properties: &HashMap<String, graphqlite::Value>, key: &str) -> Vec<String> {
    match properties.get(key) {
        Some(graphqlite::Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                graphqlite::Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        Some(graphqlite::Value::String(value)) => {
            serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn non_empty_string(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn unique_project_evidence(project: &KnowledgeProject) -> Vec<EvidenceRef> {
    let mut evidence_by_id = std::collections::BTreeMap::new();
    for evidence in project
        .details_by_node_id
        .values()
        .flat_map(|detail| detail.evidence.iter())
        .chain(
            project
                .edge_details_by_id
                .values()
                .flat_map(|detail| detail.evidence.iter()),
        )
    {
        evidence_by_id
            .entry(evidence.id.clone())
            .or_insert_with(|| evidence.clone());
    }
    evidence_by_id.into_values().collect()
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

fn redact_path_for_agent(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<redacted>".into())
}

#[allow(dead_code)]
fn evidence_graph_neighbor_counts(
    graph: &Graph,
    workspace_id: &str,
) -> Result<std::collections::BTreeMap<String, i64>> {
    let mut counts = std::collections::BTreeMap::new();
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
fn append_graph_neighbor_hits(
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
             RETURN n.id AS node_id, n.evidence_ids_json AS evidence_ids_json",
        )
        .param("workspace_id", workspace_id)
        .run()
        .context("failed finding GraphQLite retrieval seed nodes")?;
    for row in &seed_rows {
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

#[allow(dead_code)]
fn row_string(row: &graphqlite::Row, column: &str) -> Result<String> {
    match row.get_value(column) {
        Some(graphqlite::Value::String(value)) => Ok(value.clone()),
        Some(graphqlite::Value::Integer(value)) => Ok(value.to_string()),
        Some(graphqlite::Value::Float(value)) => Ok(value.to_string()),
        Some(graphqlite::Value::Bool(value)) => Ok(value.to_string()),
        Some(graphqlite::Value::Null) | None => Ok(String::new()),
        Some(other) => Err(anyhow!("expected scalar column {column}, got {other:?}")),
    }
}

#[allow(dead_code)]
fn row_i64(row: &graphqlite::Row, column: &str) -> Result<i64> {
    match row.get_value(column) {
        Some(graphqlite::Value::Integer(value)) => Ok(*value),
        Some(graphqlite::Value::Float(value)) => Ok(*value as i64),
        Some(graphqlite::Value::String(value)) if value.trim().is_empty() => Ok(0),
        Some(graphqlite::Value::String(value)) => value
            .parse::<i64>()
            .with_context(|| format!("failed parsing integer column {column}")),
        Some(graphqlite::Value::Null) | None => Ok(0),
        Some(other) => Err(anyhow!("expected integer column {column}, got {other:?}")),
    }
}

#[allow(dead_code)]
fn row_string_array(row: &graphqlite::Row, column: &str) -> Result<Vec<String>> {
    match row.get_value(column) {
        Some(graphqlite::Value::Array(values)) => Ok(values
            .iter()
            .filter_map(|value| match value {
                graphqlite::Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect()),
        Some(graphqlite::Value::String(value)) => {
            Ok(serde_json::from_str::<Vec<String>>(value).unwrap_or_default())
        }
        Some(graphqlite::Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(anyhow!(
            "expected string array column {column}, got {other:?}"
        )),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct EvidenceQueryIntent {
    wants_table: bool,
    wants_visual: bool,
    wants_summary: bool,
    wants_claim: bool,
    wants_relationship: bool,
}

impl EvidenceQueryIntent {
    fn from_query(query: &str) -> Self {
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

    fn boost(self, evidence_type: &str) -> f64 {
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
fn append_source_page_fts_hits(
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
fn append_wiki_fts_hits(
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
             JOIN wiki_pages wp ON wp.wiki_page_id = w.wiki_page_id
             WHERE w.workspace_id = ?1 AND wiki_fts MATCH ?2
               AND wp.approval_status IN ('materialized', 'approved')
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
fn fts_phrase_query(query: &str) -> String {
    query
        .replace('"', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn persist_graph_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<KnowledgeGraphPersistReport> {
    persist_snapshot_sources_in_transaction(graph, snapshot)?;
    persist_source_pages_snapshot_in_transaction(graph, snapshot)?;
    persist_evidence_snapshot_in_transaction(graph, snapshot)?;
    validate_snapshot_evidence_refs(snapshot)?;
    persist_wiki_pages_snapshot_in_transaction(graph, snapshot)?;
    persist_brain_events_snapshot_in_transaction(graph, snapshot)?;
    graph
        .connection()
        .cypher_builder("MATCH (n {workspace_id: $workspace_id}) DETACH DELETE n")
        .param("workspace_id", snapshot.workspace_id.as_str())
        .run()
        .context("failed clearing GraphQLite workspace graph")?;

    let graph_nodes = current_graph_nodes(snapshot);
    for node in &graph_nodes {
        let metadata = node_graph_metadata(snapshot, node);
        graph
            .upsert_node(
                &node.node_id,
                node_graph_properties(&snapshot.workspace_id, node, &metadata),
                brain_node_label(node.kind),
            )
            .with_context(|| format!("failed upserting GraphQLite node {}", node.node_id))?;
    }
    for relation in &snapshot.relations {
        let metadata = relation_graph_metadata(snapshot, relation);
        graph
            .upsert_edge(
                &relation.source_node_id,
                &relation.target_node_id,
                relation_graph_properties(&snapshot.workspace_id, relation, &metadata),
                brain_relation_type(relation.kind),
            )
            .with_context(|| {
                format!(
                    "failed upserting GraphQLite relation {}",
                    relation.relation_id
                )
            })?;
    }

    mark_import_jobs_graph_ready_in_transaction(graph, snapshot)?;

    let report = KnowledgeGraphPersistReport {
        node_count: graph_nodes.len(),
        relation_count: snapshot.relations.len(),
    };
    persist_graph_checkpoint_metadata_in_transaction(graph, snapshot, &report)?;

    Ok(report)
}

fn persist_graph_checkpoint_metadata_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
    report: &KnowledgeGraphPersistReport,
) -> Result<()> {
    if report.node_count == 0 && report.relation_count == 0 {
        return Ok(());
    }
    let sqlite = graph.connection().sqlite_connection();
    let actor_json = serde_json::json!({
        "actorType": "system",
        "actorId": "hyprduck-knowledge-store"
    })
    .to_string();
    let checkpoint_id = graph_checkpoint_id(snapshot, report);
    let checksum = graph_checkpoint_checksum(snapshot, report)?;
    let evidence_ref_count = snapshot
        .nodes
        .iter()
        .flat_map(|node| node.evidence_ids.iter())
        .chain(
            snapshot
                .relations
                .iter()
                .flat_map(|relation| relation.evidence_ids.iter()),
        )
        .chain(
            snapshot
                .wiki_pages
                .iter()
                .flat_map(|wiki| wiki.evidence_refs.iter()),
        )
        .chain(
            snapshot
                .claims
                .iter()
                .flat_map(|claim| claim.evidence_refs.iter()),
        )
        .collect::<BTreeSet<_>>()
        .len();
    sqlite
        .execute(
            "INSERT INTO graph_checkpoints (
                checkpoint_id,
                workspace_id,
                reason,
                actor_json,
                related_event_id,
                graph_schema_version,
                graphqlite_extension_version,
                node_count,
                edge_count,
                evidence_ref_count,
                checksum,
                storage_ref,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(checkpoint_id) DO UPDATE SET
                reason=excluded.reason,
                actor_json=excluded.actor_json,
                related_event_id=excluded.related_event_id,
                graph_schema_version=excluded.graph_schema_version,
                graphqlite_extension_version=excluded.graphqlite_extension_version,
                node_count=excluded.node_count,
                edge_count=excluded.edge_count,
                evidence_ref_count=excluded.evidence_ref_count,
                checksum=excluded.checksum,
                storage_ref=excluded.storage_ref,
                created_at=excluded.created_at",
            (
                checkpoint_id.as_str(),
                snapshot.workspace_id.as_str(),
                "graph_snapshot_commit",
                actor_json.as_str(),
                snapshot.events.last().map(|event| event.event_id.as_str()),
                GRAPHQLITE_SCHEMA_VERSION,
                graphqlite_extension_version(),
                report.node_count as i64,
                report.relation_count as i64,
                evidence_ref_count as i64,
                checksum.as_str(),
                "hyprduck.sqlite:graphqlite",
                snapshot.generated_at as i64,
            ),
        )
        .context("failed storing graph checkpoint metadata")?;
    Ok(())
}

fn graph_checkpoint_id(
    snapshot: &BrainRepoSnapshot,
    report: &KnowledgeGraphPersistReport,
) -> String {
    format!(
        "graph-checkpoint-{}-{}-{}-{}",
        snapshot.workspace_id, snapshot.generated_at, report.node_count, report.relation_count
    )
}

fn graph_checkpoint_checksum(
    snapshot: &BrainRepoSnapshot,
    report: &KnowledgeGraphPersistReport,
) -> Result<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    snapshot.workspace_id.hash(&mut hasher);
    snapshot.generated_at.hash(&mut hasher);
    report.node_count.hash(&mut hasher);
    report.relation_count.hash(&mut hasher);
    serde_json::to_string(&snapshot.nodes)
        .context("failed encoding checkpoint nodes")?
        .hash(&mut hasher);
    serde_json::to_string(&snapshot.relations)
        .context("failed encoding checkpoint relations")?
        .hash(&mut hasher);
    serde_json::to_string(&snapshot.wiki_pages)
        .context("failed encoding checkpoint wiki pages")?
        .hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn graphqlite_extension_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn mark_import_jobs_graph_ready_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let mut source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<BTreeSet<_>>();
    source_ids.extend(
        snapshot
            .evidence
            .iter()
            .filter_map(|evidence| evidence.source_id.clone()),
    );

    let sqlite = graph.connection().sqlite_connection();
    for source_id in source_ids {
        sqlite.execute_batch(&format!(
            "UPDATE import_jobs
             SET graph_ready = 1,
                 updated_at = {updated_at}
             WHERE workspace_id = {workspace_id}
               AND source_id = {source_id}
               AND citation_ready = 1;",
            updated_at = snapshot.generated_at,
            workspace_id = sql_literal(&snapshot.workspace_id),
            source_id = sql_literal(&source_id),
        ))?;
    }

    Ok(())
}

fn preserve_artifact_metadata_in_transaction(
    graph: &Graph,
    metadata: &ContextPackArtifactMetadataV0,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for (source_id, source_metadata) in &metadata.sources {
        let source_warnings = metadata
            .warnings
            .iter()
            .filter(|warning| {
                warning
                    .page_refs
                    .iter()
                    .any(|page_ref| page_ref.source_id == *source_id)
            })
            .collect::<Vec<_>>();
        let warnings_json = if source_warnings.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&source_warnings)
                    .context("failed encoding artifact warnings")?,
            )
        };
        sqlite
            .execute(
                "UPDATE sources
                 SET provider_route = ?2,
                     provider_locality = ?3,
                     content_hash = ?4,
                     parse_warnings_json = COALESCE(?5, parse_warnings_json),
                     updated_at = unixepoch()
                 WHERE source_id = ?1",
                (
                    source_id.as_str(),
                    source_metadata.provider_route.as_str(),
                    if source_metadata.local_only {
                        "local"
                    } else {
                        "hosted"
                    },
                    source_metadata.content_hash.as_str(),
                    warnings_json.as_deref(),
                ),
            )
            .with_context(|| format!("failed preserving source metadata for {source_id}"))?;
    }

    for (source_id, evidence_by_ref) in &metadata.evidence {
        for (evidence_id, evidence_metadata) in evidence_by_ref {
            let evidence_type = db_evidence_type(evidence_metadata.evidence_type);
            let span_json = optional_string_json("span", evidence_metadata.span.as_deref())?;
            let region_json = optional_string_json("region", evidence_metadata.region.as_deref())?;
            let confidence = parse_confidence_score(&evidence_metadata.parse_confidence);
            let markdown_path_redacted = evidence_metadata
                .markdown_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
            let image_path_redacted = evidence_metadata
                .image_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
            sqlite
                .execute(
                    "UPDATE evidence_items
                     SET evidence_type = ?3,
                         snippet = ?4,
                         markdown_path_redacted = ?5,
                         image_path_redacted = ?6,
                         span_json = ?7,
                         region_json = ?8,
                         confidence = ?9
                     WHERE source_id = ?1 AND evidence_id = ?2",
                    (
                        source_id.as_str(),
                        evidence_id.as_str(),
                        evidence_type.as_str(),
                        evidence_metadata.quoted_text.as_str(),
                        markdown_path_redacted.as_str(),
                        image_path_redacted.as_str(),
                        span_json.as_str(),
                        region_json.as_str(),
                        confidence,
                    ),
                )
                .with_context(|| {
                    format!("failed preserving evidence metadata for {evidence_id}")
                })?;
            sqlite
                .execute(
                    "UPDATE evidence_fts
                     SET evidence_type = ?3,
                         text = ?4
                     WHERE source_id = ?1 AND evidence_id = ?2",
                    (
                        source_id.as_str(),
                        evidence_id.as_str(),
                        evidence_type.as_str(),
                        evidence_metadata.quoted_text.as_str(),
                    ),
                )
                .with_context(|| {
                    format!("failed preserving evidence FTS metadata for {evidence_id}")
                })?;
        }
    }

    Ok(())
}

fn preserve_context_pack_exports_in_transaction(
    graph: &Graph,
    workspace_root: &Path,
    workspace_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for export in discover_context_pack_exports(workspace_root)? {
        let payload = fs::read_to_string(&export.path)
            .with_context(|| format!("failed reading context pack {}", export.path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&payload)
            .with_context(|| format!("failed decoding context pack {}", export.path.display()))?;
        let export_workspace_id = value
            .get("workspaceId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if export_workspace_id != workspace_id {
            continue;
        }
        let Some(pack_id) = value.get("packId").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let schema_version = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let query = value
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let generated_at = value
            .get("generatedAt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        sqlite
            .execute(
                "INSERT INTO context_pack_exports (
                    pack_id,
                    workspace_id,
                    query,
                    export_path,
                    schema_version,
                    payload_json,
                    generated_at,
                    is_latest,
                    preserved_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())
                 ON CONFLICT(pack_id, export_path) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    query=excluded.query,
                    schema_version=excluded.schema_version,
                    payload_json=excluded.payload_json,
                    generated_at=excluded.generated_at,
                    is_latest=excluded.is_latest,
                    preserved_at=excluded.preserved_at",
                (
                    pack_id,
                    workspace_id,
                    query,
                    export.relative_path.as_str(),
                    schema_version,
                    payload.as_str(),
                    generated_at,
                    if export.is_latest { 1 } else { 0 },
                ),
            )
            .with_context(|| format!("failed preserving context pack export {pack_id}"))?;
    }

    Ok(())
}

#[derive(Debug)]
struct ContextPackExportCandidate {
    path: PathBuf,
    relative_path: String,
    is_latest: bool,
}

fn discover_context_pack_exports(workspace_root: &Path) -> Result<Vec<ContextPackExportCandidate>> {
    let mut exports = Vec::new();
    let latest_path = workspace_root.join("context_pack.json");
    if latest_path.exists() {
        exports.push(ContextPackExportCandidate {
            path: latest_path,
            relative_path: "context_pack.json".into(),
            is_latest: true,
        });
    }

    let history_dir = workspace_root.join("context_packs");
    if history_dir.exists() {
        for entry in fs::read_dir(&history_dir)
            .with_context(|| format!("failed reading {}", history_dir.display()))?
        {
            let entry = entry.context("failed reading context pack history entry")?;
            let file_type = entry
                .file_type()
                .context("failed reading context pack history file type")?;
            if !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Some(file_name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            exports.push(ContextPackExportCandidate {
                path,
                relative_path: format!("context_packs/{}", file_name),
                is_latest: false,
            });
        }
    }

    Ok(exports)
}

fn db_evidence_type(evidence_type: EvidenceType) -> String {
    format!("{}_evidence", evidence_type.as_trace_key())
}

fn parse_confidence_score(confidence: &ContextPackParseConfidence) -> Option<f64> {
    match confidence {
        ContextPackParseConfidence::High => Some(1.0),
        ContextPackParseConfidence::Medium => Some(0.66),
        ContextPackParseConfidence::Low => Some(0.33),
        ContextPackParseConfidence::Unknown => None,
    }
}

fn optional_string_json(key: &str, value: Option<&str>) -> Result<String> {
    let value = value
        .map(|value| serde_json::json!({ key: value }))
        .unwrap_or_else(|| serde_json::json!({}));
    serde_json::to_string(&value).context("failed encoding optional metadata JSON")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphRecordMetadata {
    source_ids: Vec<String>,
    producer_run_ids: Vec<String>,
    status: String,
}

fn node_graph_metadata(
    snapshot: &BrainRepoSnapshot,
    node: &BrainNodeRecord,
) -> GraphRecordMetadata {
    let mut source_ids = node.source_ids.clone();
    if node.kind == BrainNodeKind::Source && source_ids.is_empty() {
        if let Some(source_id) = node.node_id.strip_prefix("source:") {
            source_ids.push(source_id.to_string());
        }
    }
    source_ids.sort();
    source_ids.dedup();

    let status = snapshot
        .claims
        .iter()
        .find(|claim| claim.claim_id == node.node_id)
        .map(|claim| claim.status.clone())
        .unwrap_or_else(|| "active".into());
    let producer_run_ids = producer_run_ids_for_refs(snapshot, &node.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status,
    }
}

fn relation_graph_metadata(
    snapshot: &BrainRepoSnapshot,
    relation: &BrainRelationRecord,
) -> GraphRecordMetadata {
    let evidence_source_ids = snapshot
        .evidence
        .iter()
        .filter(|evidence| relation.evidence_ids.contains(&evidence.id))
        .filter_map(|evidence| evidence.source_id.clone());
    let endpoint_source_ids = [&relation.source_node_id, &relation.target_node_id]
        .into_iter()
        .filter_map(|node_id| node_id.strip_prefix("source:").map(ToOwned::to_owned));
    let mut source_ids = evidence_source_ids
        .chain(endpoint_source_ids)
        .collect::<Vec<_>>();
    source_ids.sort();
    source_ids.dedup();
    let producer_run_ids = producer_run_ids_for_refs(snapshot, &relation.evidence_ids, &source_ids);

    GraphRecordMetadata {
        source_ids,
        producer_run_ids,
        status: "active".into(),
    }
}

fn producer_run_ids_for_refs(
    snapshot: &BrainRepoSnapshot,
    evidence_ids: &[String],
    source_ids: &[String],
) -> Vec<String> {
    let mut producer_run_ids = snapshot
        .extractions
        .iter()
        .filter(|extraction| {
            source_ids.contains(&extraction.source_id)
                || extraction
                    .source_refs
                    .iter()
                    .any(|source_id| source_ids.contains(source_id))
                || extraction
                    .evidence_refs
                    .iter()
                    .any(|evidence| evidence_ids.contains(&evidence.id))
        })
        .map(|extraction| extraction.artifact_id.clone())
        .collect::<Vec<_>>();
    producer_run_ids.sort();
    producer_run_ids.dedup();
    producer_run_ids
}

fn current_graph_nodes(snapshot: &BrainRepoSnapshot) -> Vec<BrainNodeRecord> {
    let mut nodes_by_id = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.node_id.clone(), node))
        .collect::<std::collections::BTreeMap<_, _>>();

    for source in &snapshot.sources {
        let node_id = format!("source:{}", source.source_id);
        nodes_by_id
            .entry(node_id.clone())
            .or_insert(BrainNodeRecord {
                node_id,
                kind: BrainNodeKind::Source,
                label: source.source_id.clone(),
                scope: BrainScope::Project,
                aliases: Vec::new(),
                evidence_ids: Vec::new(),
                source_ids: vec![source.source_id.clone()],
                confidence: None,
                updated_at: source.updated_at,
            });
    }
    for wiki_page in &snapshot.wiki_pages {
        nodes_by_id
            .entry(wiki_page.page_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: wiki_page.page_id.clone(),
                kind: BrainNodeKind::WikiPage,
                label: wiki_page.title.clone(),
                scope: BrainScope::Project,
                aliases: vec![wiki_page.path.clone()],
                evidence_ids: wiki_page.evidence_refs.clone(),
                source_ids: wiki_page.source_refs.clone(),
                confidence: None,
                updated_at: wiki_page.updated_at,
            });
    }
    for entity in &snapshot.entities {
        nodes_by_id
            .entry(entity.entity_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: entity.entity_id.clone(),
                kind: entity.kind,
                label: entity.name.clone(),
                scope: BrainScope::Project,
                aliases: entity.aliases.clone(),
                evidence_ids: entity.evidence_refs.clone(),
                source_ids: entity.source_refs.clone(),
                confidence: None,
                updated_at: entity.updated_at,
            });
    }
    for claim in &snapshot.claims {
        nodes_by_id
            .entry(claim.claim_id.clone())
            .or_insert(BrainNodeRecord {
                node_id: claim.claim_id.clone(),
                kind: BrainNodeKind::Claim,
                label: claim.statement.clone(),
                scope: BrainScope::Project,
                aliases: claim.topic_refs.clone(),
                evidence_ids: claim.evidence_refs.clone(),
                source_ids: claim.source_refs.clone(),
                confidence: None,
                updated_at: claim.updated_at,
            });
    }

    nodes_by_id.into_values().collect()
}

fn persist_snapshot_sources_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for source in &snapshot.sources {
        sqlite
            .execute(
                "INSERT INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 0, ?10)
                ON CONFLICT(source_id) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    title=excluded.title,
                    original_path=excluded.original_path,
                    source_path=excluded.source_path,
                    markdown_path=excluded.markdown_path,
                    format=excluded.format,
                    status=excluded.status,
                    page_count=excluded.page_count,
                    updated_at=excluded.updated_at",
                (
                    source.source_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source.source_id.as_str(),
                    source.original_path.as_str(),
                    source.source_path.as_str(),
                    source.markdown_path.as_str(),
                    format!("{:?}", source.format).to_ascii_lowercase(),
                    format!("{:?}", source.status).to_ascii_lowercase(),
                    source.page_count as i64,
                    source.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting source row {}", source.source_id))?;
    }
    for evidence in &snapshot.evidence {
        let Some(source_id) = evidence.source_id.as_deref() else {
            continue;
        };
        sqlite
            .execute(
                "INSERT OR IGNORE INTO sources (
                    source_id,
                    workspace_id,
                    project_id,
                    title,
                    original_path,
                    source_path,
                    markdown_path,
                    format,
                    status,
                    page_count,
                    success_count,
                    failed_count,
                    updated_at
                ) VALUES (?1, ?2, '', ?1, '', '', '', 'unknown', 'unknown', 0, 0, 0, ?3)",
                (
                    source_id,
                    snapshot.workspace_id.as_str(),
                    snapshot.generated_at as i64,
                ),
            )
            .with_context(|| format!("failed upserting evidence source row {source_id}"))?;
    }
    Ok(())
}

#[derive(Debug, Default)]
struct SourcePageSnapshotRow {
    page_label: String,
    markdown_path_redacted: String,
    image_path_redacted: String,
    snippets: Vec<String>,
}

fn persist_source_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    let source_ids = snapshot
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .chain(
            snapshot
                .evidence
                .iter()
                .filter_map(|evidence| evidence.source_id.clone()),
        )
        .collect::<BTreeSet<_>>();
    for source_id in &source_ids {
        sqlite
            .execute("DELETE FROM source_pages WHERE source_id = ?1", [source_id])
            .with_context(|| format!("failed clearing source pages for {source_id}"))?;
        sqlite
            .execute(
                "DELETE FROM source_page_fts WHERE source_id = ?1",
                [source_id],
            )
            .with_context(|| format!("failed clearing source page FTS for {source_id}"))?;
    }

    let mut pages = BTreeMap::<(String, usize), SourcePageSnapshotRow>::new();
    for evidence in &snapshot.evidence {
        let (Some(source_id), Some(page_index)) =
            (evidence.source_id.as_ref(), evidence.page_index)
        else {
            continue;
        };
        let row = pages
            .entry((source_id.clone(), page_index))
            .or_insert_with(|| SourcePageSnapshotRow {
                page_label: evidence.page_label.clone(),
                markdown_path_redacted: evidence
                    .markdown_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                image_path_redacted: evidence
                    .image_path
                    .as_deref()
                    .map(redact_path_for_agent)
                    .unwrap_or_default(),
                snippets: Vec::new(),
            });
        if row.page_label.is_empty() {
            row.page_label = evidence.page_label.clone();
        }
        if !evidence.snippet.trim().is_empty() {
            row.snippets.push(evidence.snippet.clone());
        }
    }

    for ((source_id, page_index), row) in pages {
        let plain_text = row.snippets.join("\n\n");
        sqlite
            .execute(
                "INSERT INTO source_pages (
                    source_id,
                    page_index,
                    page_label,
                    markdown_path_redacted,
                    image_path_redacted,
                    plain_text,
                    parse_warnings_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]')",
                (
                    source_id.as_str(),
                    page_index as i64,
                    row.page_label.as_str(),
                    row.markdown_path_redacted.as_str(),
                    row.image_path_redacted.as_str(),
                    plain_text.as_str(),
                ),
            )
            .with_context(|| {
                format!("failed inserting migrated source page {source_id}:{page_index}")
            })?;
        sqlite
            .execute(
                "INSERT INTO source_page_fts (source_id, page_index, page_label, text)
                 VALUES (?1, ?2, ?3, ?4)",
                (
                    source_id.as_str(),
                    page_index as i64,
                    row.page_label.as_str(),
                    plain_text.as_str(),
                ),
            )
            .with_context(|| format!("failed indexing source page {source_id}:{page_index}"))?;
    }

    Ok(())
}

fn persist_evidence_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM evidence_fts WHERE evidence_id IN (SELECT evidence_id FROM evidence_items WHERE workspace_id = ?1)",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing evidence FTS rows")?;
    sqlite
        .execute(
            "DELETE FROM evidence_items WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing relational evidence rows")?;

    for evidence in &snapshot.evidence {
        let source_id = evidence.source_id.as_deref().unwrap_or_default();
        let page_index = evidence
            .page_index
            .map(|value| value.to_string())
            .unwrap_or_default();
        sqlite
            .execute(
                "INSERT INTO evidence_items (
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
                    status
                ) VALUES (?1, ?2, ?3, NULLIF(?4, ''), ?5, 'text_evidence', ?6, ?7, ?8, ?9, ?10, 'active')",
                (
                    evidence.id.as_str(),
                    snapshot.workspace_id.as_str(),
                    source_id,
                    page_index.as_str(),
                    evidence.page_label.as_str(),
                    evidence.snippet.as_str(),
                    evidence.source_path.as_deref().unwrap_or_default(),
                    evidence.markdown_path.as_deref().unwrap_or_default(),
                    evidence.image_path.as_deref().unwrap_or_default(),
                    evidence.provenance.as_deref().unwrap_or_default(),
                ),
            )
            .with_context(|| format!("failed inserting evidence row {}", evidence.id))?;
        sqlite
            .execute(
                "INSERT INTO evidence_fts (evidence_id, source_id, evidence_type, text)
                 VALUES (?1, ?2, 'text_evidence', ?3)",
                (evidence.id.as_str(), source_id, evidence.snippet.as_str()),
            )
            .with_context(|| format!("failed indexing evidence row {}", evidence.id))?;
    }

    Ok(())
}

fn persist_wiki_pages_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM wiki_pages WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing wiki page rows")?;
    sqlite
        .execute(
            "DELETE FROM wiki_revisions WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing wiki revision rows")?;
    sqlite
        .execute(
            "DELETE FROM wiki_fts WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing wiki FTS rows")?;
    for page in &snapshot.wiki_pages {
        let evidence_refs_json = serde_json::to_string(&page.evidence_refs)
            .context("failed encoding wiki evidence refs")?;
        let revision = 1_i64;
        let approval_status = "materialized";
        let diff_json = "{}";
        sqlite
            .execute(
                "INSERT INTO wiki_pages (
                    wiki_page_id,
                    workspace_id,
                    path,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    revision,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    page.page_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    page.path.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    revision,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed inserting wiki page row {}", page.page_id))?;
        sqlite
            .execute(
                "INSERT INTO wiki_revisions (
                    wiki_page_id,
                    revision,
                    workspace_id,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    diff_json,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    page.page_id.as_str(),
                    revision,
                    snapshot.workspace_id.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    approval_status,
                    evidence_refs_json.as_str(),
                    diff_json,
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed inserting wiki revision row {}", page.page_id))?;
        for section in wiki_sections_from_page(page) {
            let section_evidence_refs_json = serde_json::to_string(&section.evidence_refs)
                .context("failed encoding wiki section evidence refs")?;
            sqlite
                .execute(
                    "INSERT INTO wiki_sections (
                        wiki_page_id,
                        revision,
                        section_index,
                        heading,
                        body,
                        evidence_refs_json,
                        updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (
                        page.page_id.as_str(),
                        revision,
                        section.index,
                        section.heading.as_str(),
                        section.body.as_str(),
                        section_evidence_refs_json.as_str(),
                        page.updated_at as i64,
                    ),
                )
                .with_context(|| format!("failed inserting wiki section row {}", page.page_id))?;
            sqlite
                .execute(
                    "INSERT INTO wiki_fts (workspace_id, wiki_page_id, revision, section_index, title, text)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (
                        snapshot.workspace_id.as_str(),
                        page.page_id.as_str(),
                        revision,
                        section.index,
                        page.title.as_str(),
                        section.body.as_str(),
                    ),
                )
                .with_context(|| format!("failed indexing wiki section {}", page.page_id))?;
        }
    }
    Ok(())
}

struct WikiSectionRow {
    index: i64,
    heading: String,
    body: String,
    evidence_refs: Vec<String>,
}

fn wiki_sections_from_page(page: &WikiPage) -> Vec<WikiSectionRow> {
    let mut sections = Vec::new();
    let mut current_heading = page.title.clone();
    let mut current_body = String::new();

    for line in page.body.lines() {
        if let Some(heading) = markdown_heading_text(line) {
            if !current_body.trim().is_empty() || !sections.is_empty() {
                sections.push(WikiSectionRow {
                    index: sections.len() as i64,
                    heading: current_heading,
                    body: current_body.trim().to_owned(),
                    evidence_refs: page.evidence_refs.clone(),
                });
                current_body.clear();
            }
            current_heading = heading.to_owned();
        } else {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line);
        }
    }

    if !current_body.trim().is_empty() || sections.is_empty() {
        sections.push(WikiSectionRow {
            index: sections.len() as i64,
            heading: current_heading,
            body: current_body.trim().to_owned(),
            evidence_refs: page.evidence_refs.clone(),
        });
    }

    sections
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|value| *value == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed.get(hashes..)?.trim_start();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn persist_brain_events_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for event in &snapshot.events {
        let actor_json =
            serde_json::to_string(&event.actor).context("failed encoding brain event actor")?;
        let evidence_refs_json = serde_json::to_string(&event.evidence_refs)
            .context("failed encoding brain event evidence refs")?;
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
    }
    Ok(())
}

fn decode_agent_write_proposal_evidence_refs(value: &str) -> Result<Vec<String>> {
    serde_json::from_str(value).context("failed decoding agent proposal evidence refs")
}

fn validate_snapshot_evidence_refs(snapshot: &BrainRepoSnapshot) -> Result<()> {
    let evidence_ids = snapshot
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for node in &snapshot.nodes {
        validate_record_evidence_refs(
            &format!("node {}", node.node_id),
            &node.evidence_ids,
            &evidence_ids,
        )?;
    }
    for relation in &snapshot.relations {
        validate_record_evidence_refs(
            &format!("relation {}", relation.relation_id),
            &relation.evidence_ids,
            &evidence_ids,
        )?;
    }
    for wiki_page in &snapshot.wiki_pages {
        validate_record_evidence_refs(
            &format!("wiki page {}", wiki_page.page_id),
            &wiki_page.evidence_refs,
            &evidence_ids,
        )?;
    }
    for claim in &snapshot.claims {
        validate_record_evidence_refs(
            &format!("claim {}", claim.claim_id),
            &claim.evidence_refs,
            &evidence_ids,
        )?;
    }
    for memory in &snapshot.memories {
        validate_record_evidence_refs(
            &format!("memory {}", memory.memory_id),
            &memory.evidence_refs,
            &evidence_ids,
        )?;
    }
    Ok(())
}

fn validate_record_evidence_refs(
    record_label: &str,
    refs: &[String],
    evidence_ids: &std::collections::BTreeSet<&str>,
) -> Result<()> {
    for evidence_ref in refs {
        if !evidence_ids.contains(evidence_ref.as_str()) {
            return Err(anyhow!(
                "{} references missing relational evidence row {}",
                record_label,
                evidence_ref
            ));
        }
    }
    Ok(())
}

fn node_graph_properties(
    workspace_id: &str,
    node: &BrainNodeRecord,
    metadata: &GraphRecordMetadata,
) -> Vec<(String, PropertyValue)> {
    vec![
        (
            "workspace_id".into(),
            PropertyValue::Text(workspace_id.into()),
        ),
        (
            "kind".into(),
            PropertyValue::Text(brain_node_kind_slug(node.kind).into()),
        ),
        ("label".into(), PropertyValue::Text(node.label.clone())),
        (
            "scope".into(),
            PropertyValue::Text(brain_scope_slug(node.scope).into()),
        ),
        (
            "aliases_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&node.aliases).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "evidence_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&node.evidence_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "source_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.source_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "producer_run_id".into(),
            PropertyValue::Text(
                metadata
                    .producer_run_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            ),
        ),
        (
            "producer_run_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.producer_run_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "confidence".into(),
            PropertyValue::Text(
                node.confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
        ),
        (
            "status".into(),
            PropertyValue::Text(metadata.status.clone()),
        ),
        (
            "updated_at".into(),
            PropertyValue::Integer(node.updated_at as i64),
        ),
    ]
}

fn relation_graph_properties(
    workspace_id: &str,
    relation: &BrainRelationRecord,
    metadata: &GraphRecordMetadata,
) -> Vec<(String, PropertyValue)> {
    vec![
        (
            "workspace_id".into(),
            PropertyValue::Text(workspace_id.into()),
        ),
        (
            "relation_id".into(),
            PropertyValue::Text(relation.relation_id.clone()),
        ),
        (
            "kind".into(),
            PropertyValue::Text(brain_relation_kind_slug(relation.kind).into()),
        ),
        ("label".into(), PropertyValue::Text(relation.label.clone())),
        (
            "evidence_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&relation.evidence_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "source_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.source_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "producer_run_id".into(),
            PropertyValue::Text(
                metadata
                    .producer_run_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            ),
        ),
        (
            "producer_run_ids_json".into(),
            PropertyValue::Text(
                serde_json::to_string(&metadata.producer_run_ids).unwrap_or_else(|_| "[]".into()),
            ),
        ),
        (
            "confidence".into(),
            PropertyValue::Text(
                relation
                    .confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
        ),
        (
            "status".into(),
            PropertyValue::Text(metadata.status.clone()),
        ),
        (
            "updated_at".into(),
            PropertyValue::Integer(relation.updated_at as i64),
        ),
    ]
}

fn brain_node_label(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "Source",
        BrainNodeKind::Memory => "Memory",
        BrainNodeKind::WikiPage => "WikiPage",
        BrainNodeKind::Person => "Person",
        BrainNodeKind::Company => "Company",
        BrainNodeKind::Project => "Project",
        BrainNodeKind::Product => "Product",
        BrainNodeKind::Team => "Team",
        BrainNodeKind::Event => "Event",
        BrainNodeKind::Decision => "Decision",
        BrainNodeKind::Task => "Task",
        BrainNodeKind::Claim => "Claim",
        BrainNodeKind::Topic => "Topic",
        BrainNodeKind::Concept => "Concept",
    }
}

fn brain_node_kind_slug(kind: BrainNodeKind) -> &'static str {
    match kind {
        BrainNodeKind::Source => "source",
        BrainNodeKind::Memory => "memory",
        BrainNodeKind::WikiPage => "wiki_page",
        BrainNodeKind::Person => "person",
        BrainNodeKind::Company => "company",
        BrainNodeKind::Project => "project",
        BrainNodeKind::Product => "product",
        BrainNodeKind::Team => "team",
        BrainNodeKind::Event => "event",
        BrainNodeKind::Decision => "decision",
        BrainNodeKind::Task => "task",
        BrainNodeKind::Claim => "claim",
        BrainNodeKind::Topic => "topic",
        BrainNodeKind::Concept => "concept",
    }
}

fn brain_scope_slug(scope: BrainScope) -> &'static str {
    match scope {
        BrainScope::Personal => "personal",
        BrainScope::Project => "project",
        BrainScope::Team => "team",
        BrainScope::Company => "company",
    }
}

fn brain_relation_type(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "MENTIONS",
        BrainRelationKind::Supports => "SUPPORTS",
        BrainRelationKind::Contradicts => "CONTRADICTS",
        BrainRelationKind::Supersedes => "SUPERSEDES",
        BrainRelationKind::SameAs => "SAME_AS",
        BrainRelationKind::WorksAt => "WORKS_AT",
        BrainRelationKind::Founded => "FOUNDED",
        BrainRelationKind::InvestedIn => "INVESTED_IN",
        BrainRelationKind::Advises => "ADVISES",
        BrainRelationKind::Attended => "ATTENDED",
        BrainRelationKind::Owns => "OWNS",
        BrainRelationKind::ResponsibleFor => "RESPONSIBLE_FOR",
        BrainRelationKind::Decided => "DECIDED",
        BrainRelationKind::Blocks => "BLOCKS",
        BrainRelationKind::DependsOn => "DEPENDS_ON",
        BrainRelationKind::SourceOf => "SOURCE_OF",
        BrainRelationKind::DerivedFrom => "DERIVED_FROM",
        BrainRelationKind::Cites => "CITES",
        BrainRelationKind::LinksTo => "LINKS_TO",
        BrainRelationKind::RelatedTo => "RELATED_TO",
    }
}

fn brain_relation_kind_slug(kind: BrainRelationKind) -> &'static str {
    match kind {
        BrainRelationKind::Mentions => "mentions",
        BrainRelationKind::Supports => "supports",
        BrainRelationKind::Contradicts => "contradicts",
        BrainRelationKind::Supersedes => "supersedes",
        BrainRelationKind::SameAs => "same_as",
        BrainRelationKind::WorksAt => "works_at",
        BrainRelationKind::Founded => "founded",
        BrainRelationKind::InvestedIn => "invested_in",
        BrainRelationKind::Advises => "advises",
        BrainRelationKind::Attended => "attended",
        BrainRelationKind::Owns => "owns",
        BrainRelationKind::ResponsibleFor => "responsible_for",
        BrainRelationKind::Decided => "decided",
        BrainRelationKind::Blocks => "blocks",
        BrainRelationKind::DependsOn => "depends_on",
        BrainRelationKind::SourceOf => "source_of",
        BrainRelationKind::DerivedFrom => "derived_from",
        BrainRelationKind::Cites => "cites",
        BrainRelationKind::LinksTo => "links_to",
        BrainRelationKind::RelatedTo => "related_to",
    }
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
                original_path: "/tmp/source-a.pdf".into(),
                source_path: "sources/source-a.pdf".into(),
                markdown_path: "sources/source-a.md".into(),
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
                    source_path: Some("sources/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("sources/source-a.md".into()),
                    image_path: None,
                    provenance: Some("test".into()),
                },
                EvidenceRef {
                    id: "evidence-b".into(),
                    page_label: "p2".into(),
                    page_index: Some(1),
                    snippet: "Beta neighbor evidence.".into(),
                    source_path: Some("sources/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("sources/source-a.md".into()),
                    image_path: None,
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
                    source_path: Some("sources/source-a.pdf".into()),
                    source_id: Some("source-a".into()),
                    markdown_path: Some("sources/source-a.md".into()),
                    image_path: None,
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
        assert!(context_pack
            .retrieval_trace
            .evidence_type_trace
            .selected
            .get("text")
            .is_some_and(|count| *count >= 1));
        let source_response = store
            .read_source_from_db("workspace-default", "source-a")
            .expect("read source from DB")
            .expect("source response");
        assert_eq!(source_response.source.source_id, "source-a");
        assert_eq!(source_response.evidence.len(), 2);
        assert_eq!(
            source_response
                .wiki_page
                .as_ref()
                .map(|page| page.page_id.as_str()),
            Some("wiki-alpha")
        );
        let page_response = store
            .read_page_evidence_from_db("workspace-default", "source-a", Some(1))
            .expect("read page evidence from DB")
            .expect("page evidence response");
        assert_eq!(page_response.source.source_id, "source-a");
        assert_eq!(page_response.evidence.len(), 1);
        assert_eq!(page_response.evidence[0].evidence_ref, "evidence-a");
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
            .starts_with("graph-checkpoint-workspace-default-10-6-3"));
        assert_eq!(row.1, "graph_snapshot_commit");
        assert!(row.2.contains("hyprduck-knowledge-store"));
        assert_eq!(row.3, "event-a");
        assert_eq!(row.4, GRAPHQLITE_SCHEMA_VERSION);
        assert_eq!(row.5, env!("CARGO_PKG_VERSION"));
        assert_eq!(row.6, 6);
        assert_eq!(row.7, 3);
        assert_eq!(row.8, 2);
        assert_eq!(row.9.len(), 16);
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
