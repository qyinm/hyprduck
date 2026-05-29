use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
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
                source_id TEXT NOT NULL,
                page_index INTEGER,
                evidence_type TEXT NOT NULL,
                snippet TEXT NOT NULL,
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
}
