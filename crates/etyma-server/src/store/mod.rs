use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: String,
    pub engine_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub external_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvidenceRow {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub source_kind: String,
    pub quote: String,
    pub locator: String,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed opening server db {}", path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS workspaces (
              id TEXT PRIMARY KEY NOT NULL,
              engine_root TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS api_tokens (
              token_hash TEXT PRIMARY KEY NOT NULL,
              workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
              label TEXT,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sources (
              id TEXT PRIMARY KEY NOT NULL,
              workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
              kind TEXT NOT NULL,
              title TEXT NOT NULL,
              body TEXT NOT NULL,
              external_id TEXT,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS evidence (
              id TEXT PRIMARY KEY NOT NULL,
              workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
              source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
              source_kind TEXT NOT NULL,
              quote TEXT NOT NULL,
              locator TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sources_workspace ON sources(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_evidence_workspace ON evidence(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_tokens_workspace ON api_tokens(workspace_id);
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow!("server db mutex poisoned"))
    }

    pub fn create_workspace(&self, id: &str, engine_root: &Path) -> Result<WorkspaceRow> {
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO workspaces (id, engine_root, created_at) VALUES (?1, ?2, ?3)",
            params![id, engine_root.to_string_lossy(), now],
        )
        .with_context(|| format!("failed inserting workspace {id}"))?;
        Ok(WorkspaceRow {
            id: id.to_string(),
            engine_root: engine_root.to_path_buf(),
        })
    }

    pub fn get_workspace(&self, id: &str) -> Result<Option<WorkspaceRow>> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT id, engine_root FROM workspaces WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(WorkspaceRow {
                id: row.get(0)?,
                engine_root: PathBuf::from(row.get::<_, String>(1)?),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn mint_token(&self, workspace_id: &str, label: Option<&str>) -> Result<String> {
        if self.get_workspace(workspace_id)?.is_none() {
            bail!("workspace not found: {workspace_id}");
        }
        let raw = format!("etyma_{}", Uuid::now_v7().simple());
        let hash = hash_token(&raw);
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO api_tokens (token_hash, workspace_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![hash, workspace_id, label, now],
        )?;
        Ok(raw)
    }

    pub fn resolve_token(&self, raw_token: &str) -> Result<Option<String>> {
        let hash = hash_token(raw_token);
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT workspace_id FROM api_tokens WHERE token_hash = ?1")?;
        let mut rows = stmt.query(params![hash])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn insert_source(
        &self,
        workspace_id: &str,
        kind: &str,
        title: &str,
        body: &str,
        external_id: Option<&str>,
    ) -> Result<SourceRow> {
        let id = format!("src_{}", Uuid::now_v7().simple());
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sources (id, workspace_id, kind, title, body, external_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, workspace_id, kind, title, body, external_id, now],
        )?;
        Ok(SourceRow {
            id,
            workspace_id: workspace_id.to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            external_id: external_id.map(str::to_string),
        })
    }

    pub fn insert_evidence(
        &self,
        workspace_id: &str,
        source_id: &str,
        source_kind: &str,
        quote: &str,
        locator: &str,
    ) -> Result<EvidenceRow> {
        let id = format!("ev_{}", Uuid::now_v7().simple());
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO evidence (id, workspace_id, source_id, source_kind, quote, locator, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, workspace_id, source_id, source_kind, quote, locator, now],
        )?;
        Ok(EvidenceRow {
            id,
            workspace_id: workspace_id.to_string(),
            source_id: source_id.to_string(),
            source_kind: source_kind.to_string(),
            quote: quote.to_string(),
            locator: locator.to_string(),
        })
    }

    pub fn list_sources(&self, workspace_id: &str) -> Result<Vec<SourceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, kind, title, body, external_id FROM sources WHERE workspace_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            Ok(SourceRow {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                kind: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                external_id: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_evidence(&self, workspace_id: &str) -> Result<Vec<EvidenceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, source_id, source_kind, quote, locator FROM evidence WHERE workspace_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![workspace_id], |row| {
            Ok(EvidenceRow {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                source_id: row.get(2)?,
                source_kind: row.get(3)?,
                quote: row.get(4)?,
                locator: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_source(&self, workspace_id: &str, source_id: &str) -> Result<Option<SourceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, kind, title, body, external_id FROM sources WHERE workspace_id = ?1 AND id = ?2",
        )?;
        let mut rows = stmt.query(params![workspace_id, source_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(SourceRow {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                kind: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                external_id: row.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }
}

pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mint_and_resolve_token() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.sqlite3")).unwrap();
        store
            .create_workspace("ws1", &dir.path().join("ws1"))
            .unwrap();
        let token = store.mint_token("ws1", Some("test")).unwrap();
        assert!(token.starts_with("etyma_"));
        assert_eq!(store.resolve_token(&token).unwrap().as_deref(), Some("ws1"));
        assert!(store.resolve_token("etyma_bogus").unwrap().is_none());
    }
}
