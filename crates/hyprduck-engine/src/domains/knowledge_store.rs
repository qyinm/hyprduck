use anyhow::{anyhow, Context, Result};
use graphqlite::{Graph, PropertyValue};
#[cfg(test)]
use hyprduck_engine_types::EvidenceRef;
#[cfg(test)]
use hyprduck_engine_types::{
    BrainActor, BrainActorType, BrainEvent, BrainEventCausality, BrainEventKind, PolicyResult,
    BRAIN_EVENT_SCHEMA_VERSION,
};
use hyprduck_engine_types::{
    BrainNodeKind, BrainNodeRecord, BrainRelationKind, BrainRelationRecord, BrainRepoSnapshot,
    BrainScope,
};
use serde::{Deserialize, Serialize};
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
    persist_evidence_snapshot_in_transaction(graph, snapshot)?;
    persist_brain_events_snapshot_in_transaction(graph, snapshot)?;
    validate_snapshot_evidence_refs(snapshot)?;
    graph
        .connection()
        .cypher_builder("MATCH (n {workspace_id: $workspace_id}) DETACH DELETE n")
        .param("workspace_id", snapshot.workspace_id.as_str())
        .run()
        .context("failed clearing GraphQLite workspace graph")?;

    for node in &snapshot.nodes {
        graph
            .upsert_node(
                &node.node_id,
                node_graph_properties(&snapshot.workspace_id, node),
                brain_node_label(node.kind),
            )
            .with_context(|| format!("failed upserting GraphQLite node {}", node.node_id))?;
    }
    for relation in &snapshot.relations {
        graph
            .upsert_edge(
                &relation.source_node_id,
                &relation.target_node_id,
                relation_graph_properties(&snapshot.workspace_id, relation),
                brain_relation_type(relation.kind),
            )
            .with_context(|| {
                format!(
                    "failed upserting GraphQLite relation {}",
                    relation.relation_id
                )
            })?;
    }

    Ok(KnowledgeGraphPersistReport {
        node_count: snapshot.nodes.len(),
        relation_count: snapshot.relations.len(),
    })
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

fn persist_brain_events_snapshot_in_transaction(
    graph: &Graph,
    snapshot: &BrainRepoSnapshot,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    sqlite
        .execute(
            "DELETE FROM brain_events WHERE workspace_id = ?1",
            [snapshot.workspace_id.as_str()],
        )
        .context("failed clearing relational brain event rows")?;

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
                serde_json::to_string(&node.source_ids).unwrap_or_else(|_| "[]".into()),
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
            "updated_at".into(),
            PropertyValue::Integer(node.updated_at as i64),
        ),
    ]
}

fn relation_graph_properties(
    workspace_id: &str,
    relation: &BrainRelationRecord,
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
            "confidence".into(),
            PropertyValue::Text(
                relation
                    .confidence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
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
            sources: Vec::new(),
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
            relations: vec![BrainRelationRecord {
                relation_id: "rel-a".into(),
                kind: BrainRelationKind::RelatedTo,
                source_node_id: "node-a".into(),
                target_node_id: "node-b".into(),
                label: "relates".into(),
                evidence_ids: vec!["evidence-a".into()],
                confidence: Some(0.8),
                updated_at: 10,
            }],
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
            wiki_pages: Vec::new(),
            entities: Vec::new(),
            claims: Vec::new(),
            extractions: Vec::new(),
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
                node_count: 2,
                relation_count: 1,
            }
        );
        assert_eq!(
            store
                .graph_snapshot_counts("workspace-default")
                .expect("graph counts"),
            report
        );
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
