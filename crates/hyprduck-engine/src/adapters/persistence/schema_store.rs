use anyhow::{anyhow, Context, Result};
use graphqlite::Graph;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::graph_snapshot_store::GRAPHQLITE_SCHEMA_VERSION;

pub(super) const KNOWLEDGE_DB_FILE_NAME: &str = "hyprduck.sqlite";
const KNOWLEDGE_SCHEMA_VERSION: i64 = 2;

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

pub(super) fn count_rows_for_workspace(
    path: &Path,
    table: &str,
    workspace_id: &str,
) -> Result<usize> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE workspace_id = ?1");
    graph
        .connection()
        .sqlite_connection()
        .query_row(&sql, [workspace_id], |row| row.get::<_, i64>(0))
        .with_context(|| format!("failed counting {table} rows"))?
        .try_into()
        .map_err(|_| anyhow!("negative {table} row count"))
}

pub(super) fn ensure_schema(path: &Path) -> Result<()> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
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
        VALUES ('knowledge_schema_version', '2')
        ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=unixepoch();
        INSERT INTO knowledge_meta (key, value)
        VALUES ('graphqlite_schema_version', '2')
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
            current_revision_event_id TEXT NOT NULL DEFAULT '',
            current_revision_version_id TEXT NOT NULL DEFAULT '',
            valid_from INTEGER NOT NULL DEFAULT 0,
            valid_to INTEGER NOT NULL DEFAULT 0,
            superseded_by TEXT NOT NULL DEFAULT '',
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
            source_refs_json TEXT NOT NULL DEFAULT '[]',
            node_refs_json TEXT NOT NULL DEFAULT '[]',
            relation_refs_json TEXT NOT NULL DEFAULT '[]',
            diff_json TEXT NOT NULL DEFAULT '{}',
            version_id TEXT NOT NULL DEFAULT '',
            created_by_event_id TEXT NOT NULL DEFAULT '',
            predecessor_revision INTEGER,
            superseded_by_event_id TEXT NOT NULL DEFAULT '',
            valid_from INTEGER NOT NULL DEFAULT 0,
            valid_to INTEGER NOT NULL DEFAULT 0,
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

        CREATE TABLE IF NOT EXISTS graph_evidence_record_index (
            workspace_id TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            record_kind TEXT NOT NULL,
            record_internal_id INTEGER NOT NULL,
            logical_record_id TEXT NOT NULL DEFAULT '',
            version_id TEXT NOT NULL DEFAULT '',
            created_by_event_id TEXT NOT NULL DEFAULT '',
            valid_from INTEGER NOT NULL DEFAULT 0,
            valid_to INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (workspace_id, evidence_id, record_kind, record_internal_id)
        ) WITHOUT ROWID;
        DROP INDEX IF EXISTS idx_graph_evidence_record_index_lookup;

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
    for column in [
        "path TEXT NOT NULL DEFAULT ''",
        "current_revision_event_id TEXT NOT NULL DEFAULT ''",
        "current_revision_version_id TEXT NOT NULL DEFAULT ''",
        "valid_from INTEGER NOT NULL DEFAULT 0",
        "valid_to INTEGER NOT NULL DEFAULT 0",
        "superseded_by TEXT NOT NULL DEFAULT ''",
    ] {
        let result = sqlite.execute(&format!("ALTER TABLE wiki_pages ADD COLUMN {column}"), []);
        if let Err(error) = result {
            let message = error.to_string();
            if !message.contains("duplicate column name") {
                return Err(error).context("failed migrating wiki_pages table");
            }
        }
    }
    for column in [
        "source_refs_json TEXT NOT NULL DEFAULT '[]'",
        "node_refs_json TEXT NOT NULL DEFAULT '[]'",
        "relation_refs_json TEXT NOT NULL DEFAULT '[]'",
        "version_id TEXT NOT NULL DEFAULT ''",
        "created_by_event_id TEXT NOT NULL DEFAULT ''",
        "predecessor_revision INTEGER",
        "superseded_by_event_id TEXT NOT NULL DEFAULT ''",
        "valid_from INTEGER NOT NULL DEFAULT 0",
        "valid_to INTEGER NOT NULL DEFAULT 0",
    ] {
        let result = sqlite.execute(
            &format!("ALTER TABLE wiki_revisions ADD COLUMN {column}"),
            [],
        );
        if let Err(error) = result {
            let message = error.to_string();
            if !message.contains("duplicate column name") {
                return Err(error).context("failed migrating wiki_revisions table");
            }
        }
    }
    for column in [
        "logical_record_id TEXT NOT NULL DEFAULT ''",
        "version_id TEXT NOT NULL DEFAULT ''",
        "created_by_event_id TEXT NOT NULL DEFAULT ''",
        "valid_from INTEGER NOT NULL DEFAULT 0",
        "valid_to INTEGER NOT NULL DEFAULT 0",
    ] {
        let result = sqlite.execute(
            &format!("ALTER TABLE graph_evidence_record_index ADD COLUMN {column}"),
            [],
        );
        if let Err(error) = result {
            let message = error.to_string();
            if !message.contains("duplicate column name") {
                return Err(error).context("failed migrating graph_evidence_record_index table");
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
        let result = sqlite.execute(&format!("ALTER TABLE import_jobs ADD COLUMN {column}"), []);
        if let Err(error) = result {
            let message = error.to_string();
            if !message.contains("duplicate column name") {
                return Err(error).context("failed migrating import_jobs table");
            }
        }
    }
    Ok(())
}

pub(super) fn schema_version(path: &Path) -> Result<i64> {
    let graph = Graph::open(path).context("GraphQLite failed to open knowledge DB")?;
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
