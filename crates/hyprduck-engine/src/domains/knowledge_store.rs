use anyhow::{anyhow, Context, Result};
use graphqlite::{Graph, PropertyValue};
#[cfg(test)]
use hyprduck_engine_types::{
    BrainActor, BrainActorType, BrainEvent, BrainEventCausality, BrainEventKind, ClaimRecord,
    EntityRecord, PolicyResult, SourceFormat, SourceRecord, SourceStatus,
    StructuredExtractionArtifact, WikiPage, BRAIN_EVENT_SCHEMA_VERSION,
};
use hyprduck_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot,
    BrainScope, ContextPackArtifactMetadataV0, ContextPackParseConfidence, EvidenceRef,
    EvidenceType, KnowledgeProject, SourceArtifactManifest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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
                let parse_warnings_json = serde_json::to_string(
                    &page
                        .error_message
                        .as_ref()
                        .map(|message| vec![message.clone()])
                        .unwrap_or_default(),
                )?;
                sqlite.execute_batch(&format!(
                    "INSERT INTO source_pages (source_id, page_index, page_label, markdown_path_redacted, image_path_redacted, plain_text, parse_warnings_json)
                     VALUES ({source_id}, {page_index}, {page_label}, {markdown_path}, {image_path}, '', {parse_warnings_json});",
                    source_id = sql_literal(&manifest.source_id),
                    page_index = page.index,
                    page_label = sql_literal(&page.label),
                    markdown_path = sql_optional_literal(page.markdown_path.as_deref()),
                    image_path = sql_optional_literal(page.image_path.as_deref()),
                    parse_warnings_json = sql_literal(&parse_warnings_json),
                ))?;
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
                 WHERE e.workspace_id = ?1 AND evidence_fts MATCH ?2
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
            let typed_evidence_boost = if evidence_type == "text_evidence" {
                0.05
            } else {
                0.0
            };
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
        if hits.is_empty() {
            let mut fallback_statement = graph
                .connection()
                .sqlite_connection()
                .prepare(
                    "SELECT evidence_id, source_id, evidence_type, snippet
                     FROM evidence_items
                     WHERE snippet LIKE '%' || ?1 || '%'
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
                let typed_evidence_boost = if evidence_type == "text_evidence" {
                    0.05
                } else {
                    0.0
                };
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

            CREATE TABLE IF NOT EXISTS wiki_pages (
                wiki_page_id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                approval_status TEXT NOT NULL,
                evidence_refs_json TEXT NOT NULL DEFAULT '[]',
                revision INTEGER NOT NULL DEFAULT 1,
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
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
    persist_wiki_pages_snapshot_in_transaction(graph, snapshot)?;
    persist_brain_events_snapshot_in_transaction(graph, snapshot)?;
    validate_snapshot_evidence_refs(snapshot)?;
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

    Ok(KnowledgeGraphPersistReport {
        node_count: graph_nodes.len(),
        relation_count: snapshot.relations.len(),
    })
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
    for page in &snapshot.wiki_pages {
        let evidence_refs_json = serde_json::to_string(&page.evidence_refs)
            .context("failed encoding wiki evidence refs")?;
        sqlite
            .execute(
                "INSERT INTO wiki_pages (
                    wiki_page_id,
                    workspace_id,
                    title,
                    body,
                    approval_status,
                    evidence_refs_json,
                    revision,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'materialized', ?5, 1, ?6)",
                (
                    page.page_id.as_str(),
                    snapshot.workspace_id.as_str(),
                    page.title.as_str(),
                    page.body.as_str(),
                    evidence_refs_json.as_str(),
                    page.updated_at as i64,
                ),
            )
            .with_context(|| format!("failed inserting wiki page row {}", page.page_id))?;
    }
    Ok(())
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
                page_count: 1,
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
                    evidence_ids: Vec::new(),
                    source_ids: Vec::new(),
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
            evidence: vec![EvidenceRef {
                id: "evidence-a".into(),
                page_label: "p1".into(),
                page_index: Some(0),
                snippet: "Alpha relates to beta.".into(),
                source_path: Some("sources/source-a.pdf".into()),
                source_id: Some("source-a".into()),
                markdown_path: Some("sources/source-a.md".into()),
                image_path: None,
                provenance: Some("test".into()),
            }],
            memories: Vec::new(),
            wiki_pages: vec![WikiPage {
                page_id: "wiki-alpha".into(),
                workspace_id: "workspace-default".into(),
                path: "wiki/alpha".into(),
                title: "Alpha Wiki".into(),
                body: "Alpha wiki body.".into(),
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
        assert_graph_edge_metadata(&store, "claim-alpha", "source:source-a", "CITES");
        assert_relational_proof_ignores_graph_metadata_tamper(&store);
        let hits = store
            .hybrid_retrieve("workspace-default", "Alpha", 5)
            .expect("hybrid retrieve");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence_id, "evidence-a");
        assert_eq!(hits[0].graph_neighbor_count, 1);
        assert_eq!(
            brain_event_count(&store, "workspace-default").expect("brain event count"),
            1
        );
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
