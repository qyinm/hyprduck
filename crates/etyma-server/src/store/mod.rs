use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrgRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: String,
    pub org_id: String,
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
        // Spike metadata: Org → Workspace hierarchy. Wipe local DB if schema conflicts.
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS orgs (
              id TEXT PRIMARY KEY NOT NULL,
              name TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspaces (
              id TEXT PRIMARY KEY NOT NULL,
              org_id TEXT NOT NULL REFERENCES orgs(id),
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
            CREATE INDEX IF NOT EXISTS idx_workspaces_org ON workspaces(org_id);
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

    pub fn create_org(&self, id: &str, name: &str) -> Result<OrgRow> {
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO orgs (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now],
        )
        .with_context(|| format!("failed inserting org {id}"))?;
        Ok(OrgRow {
            id: id.to_string(),
            name: name.to_string(),
        })
    }

    pub fn get_org(&self, id: &str) -> Result<Option<OrgRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, name FROM orgs WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(OrgRow {
                id: row.get(0)?,
                name: row.get(1)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_orgs(&self) -> Result<Vec<OrgRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, name FROM orgs ORDER BY created_at")?;
        let rows = stmt.query_map([], |row| {
            Ok(OrgRow {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn workspace_count_for_org(&self, org_id: &str) -> Result<usize> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspaces WHERE org_id = ?1",
            params![org_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Reject delete while workspaces remain (origin R3 / KTD5).
    pub fn delete_org(&self, org_id: &str) -> Result<()> {
        if self.get_org(org_id)?.is_none() {
            bail!("org not found: {org_id}");
        }
        let n = self.workspace_count_for_org(org_id)?;
        if n > 0 {
            bail!("org has workspaces: {org_id} ({n})");
        }
        let conn = self.lock()?;
        conn.execute("DELETE FROM orgs WHERE id = ?1", params![org_id])?;
        Ok(())
    }

    pub fn create_workspace(&self, org_id: &str, id: &str) -> Result<WorkspaceRow> {
        if self.get_org(org_id)?.is_none() {
            bail!("org not found: {org_id}");
        }
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO workspaces (id, org_id, created_at) VALUES (?1, ?2, ?3)",
            params![id, org_id, now],
        )
        .with_context(|| format!("failed inserting workspace {id}"))?;
        Ok(WorkspaceRow {
            id: id.to_string(),
            org_id: org_id.to_string(),
        })
    }

    pub fn get_workspace(&self, id: &str) -> Result<Option<WorkspaceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, org_id FROM workspaces WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(WorkspaceRow {
                id: row.get(0)?,
                org_id: row.get(1)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_workspaces(&self, org_id: &str) -> Result<Vec<WorkspaceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, org_id FROM workspaces WHERE org_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![org_id], |row| {
            Ok(WorkspaceRow {
                id: row.get(0)?,
                org_id: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
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

    pub fn source_count(&self, workspace_id: &str) -> Result<usize> {
        let conn = self.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sources WHERE workspace_id = ?1",
            params![workspace_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
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
    fn org_workspace_hierarchy_and_token() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.sqlite3")).unwrap();
        store.create_org("org1", "Acme").unwrap();
        let ws = store.create_workspace("org1", "ws1").unwrap();
        assert_eq!(ws.org_id, "org1");
        assert!(store.create_workspace("missing", "ws2").is_err());
        let token = store.mint_token("ws1", Some("test")).unwrap();
        assert!(token.starts_with("etyma_"));
        assert_eq!(store.resolve_token(&token).unwrap().as_deref(), Some("ws1"));
        assert_eq!(store.list_workspaces("org1").unwrap().len(), 1);
        assert!(store.delete_org("org1").is_err());
    }
}
