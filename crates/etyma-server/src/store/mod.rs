use rusqlite::{params, Connection, ErrorCode};
use sha2::{Digest, Sha256};
use std::fmt;
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

#[derive(Debug)]
pub enum StoreError {
    NotFound { entity: &'static str, id: String },
    Conflict(String),
    Internal(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict(msg) => write!(f, "{msg}"),
            Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

pub type StoreResult<T> = Result<T, StoreError>;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> StoreResult<Self> {
        let conn = Connection::open(path).map_err(|e| {
            StoreError::Internal(format!("failed opening server db {}: {e}", path.display()))
        })?;
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
        )
        .map_err(db_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| StoreError::Internal("server db mutex poisoned".into()))
    }

    pub fn create_org(&self, id: &str, name: &str) -> StoreResult<OrgRow> {
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO orgs (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![id, name, now],
        )
        .map_err(|e| map_write(e, &format!("org {id}")))?;
        Ok(OrgRow {
            id: id.to_string(),
            name: name.to_string(),
        })
    }

    pub fn get_org(&self, id: &str) -> StoreResult<Option<OrgRow>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, name FROM orgs WHERE id = ?1")
            .map_err(db_err)?;
        let mut rows = stmt.query(params![id]).map_err(db_err)?;
        if let Some(row) = rows.next().map_err(db_err)? {
            Ok(Some(OrgRow {
                id: row.get(0).map_err(db_err)?,
                name: row.get(1).map_err(db_err)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn require_org(&self, id: &str) -> StoreResult<OrgRow> {
        self.get_org(id)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "org",
                id: id.to_string(),
            })
    }

    pub fn list_orgs(&self) -> StoreResult<Vec<OrgRow>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, name FROM orgs ORDER BY created_at")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub fn workspace_count_for_org(&self, org_id: &str) -> StoreResult<usize> {
        let conn = self.lock()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workspaces WHERE org_id = ?1",
                params![org_id],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        Ok(count as usize)
    }

    /// Reject delete while workspaces remain.
    pub fn delete_org(&self, org_id: &str) -> StoreResult<()> {
        self.require_org(org_id)?;
        let n = self.workspace_count_for_org(org_id)?;
        if n > 0 {
            return Err(StoreError::Conflict(format!(
                "org has workspaces: {org_id} ({n})"
            )));
        }
        let conn = self.lock()?;
        conn.execute("DELETE FROM orgs WHERE id = ?1", params![org_id])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn create_workspace(&self, org_id: &str, id: &str) -> StoreResult<WorkspaceRow> {
        self.require_org(org_id)?;
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO workspaces (id, org_id, created_at) VALUES (?1, ?2, ?3)",
            params![id, org_id, now],
        )
        .map_err(|e| map_write(e, &format!("workspace {id}")))?;
        Ok(WorkspaceRow {
            id: id.to_string(),
            org_id: org_id.to_string(),
        })
    }

    pub fn get_workspace(&self, id: &str) -> StoreResult<Option<WorkspaceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, org_id FROM workspaces WHERE id = ?1")
            .map_err(db_err)?;
        let mut rows = stmt.query(params![id]).map_err(db_err)?;
        if let Some(row) = rows.next().map_err(db_err)? {
            Ok(Some(WorkspaceRow {
                id: row.get(0).map_err(db_err)?,
                org_id: row.get(1).map_err(db_err)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn require_workspace(&self, id: &str) -> StoreResult<WorkspaceRow> {
        self.get_workspace(id)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "workspace",
                id: id.to_string(),
            })
    }

    pub fn list_workspaces(&self, org_id: &str) -> StoreResult<Vec<WorkspaceRow>> {
        self.require_org(org_id)?;
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, org_id FROM workspaces WHERE org_id = ?1 ORDER BY created_at")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![org_id], |row| {
                Ok(WorkspaceRow {
                    id: row.get(0)?,
                    org_id: row.get(1)?,
                })
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub fn mint_token(&self, workspace_id: &str, label: Option<&str>) -> StoreResult<String> {
        self.require_workspace(workspace_id)?;
        let raw = format!("etyma_{}", Uuid::now_v7().simple());
        let hash = hash_token(&raw);
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO api_tokens (token_hash, workspace_id, label, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![hash, workspace_id, label, now],
        )
        .map_err(db_err)?;
        Ok(raw)
    }

    pub fn resolve_token(&self, raw_token: &str) -> StoreResult<Option<String>> {
        let hash = hash_token(raw_token);
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT workspace_id FROM api_tokens WHERE token_hash = ?1")
            .map_err(db_err)?;
        let mut rows = stmt.query(params![hash]).map_err(db_err)?;
        if let Some(row) = rows.next().map_err(db_err)? {
            Ok(Some(row.get(0).map_err(db_err)?))
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
    ) -> StoreResult<SourceRow> {
        let id = format!("src_{}", Uuid::now_v7().simple());
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sources (id, workspace_id, kind, title, body, external_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, workspace_id, kind, title, body, external_id, now],
        )
        .map_err(db_err)?;
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
    ) -> StoreResult<EvidenceRow> {
        let id = format!("ev_{}", Uuid::now_v7().simple());
        let now = unix_now();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO evidence (id, workspace_id, source_id, source_kind, quote, locator, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, workspace_id, source_id, source_kind, quote, locator, now],
        )
        .map_err(db_err)?;
        Ok(EvidenceRow {
            id,
            workspace_id: workspace_id.to_string(),
            source_id: source_id.to_string(),
            source_kind: source_kind.to_string(),
            quote: quote.to_string(),
            locator: locator.to_string(),
        })
    }

    pub fn list_sources(&self, workspace_id: &str) -> StoreResult<Vec<SourceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
            "SELECT id, workspace_id, kind, title, body, external_id FROM sources WHERE workspace_id = ?1 ORDER BY created_at",
        )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![workspace_id], |row| {
                Ok(SourceRow {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    kind: row.get(2)?,
                    title: row.get(3)?,
                    body: row.get(4)?,
                    external_id: row.get(5)?,
                })
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub fn list_evidence(&self, workspace_id: &str) -> StoreResult<Vec<EvidenceRow>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
            "SELECT id, workspace_id, source_id, source_kind, quote, locator FROM evidence WHERE workspace_id = ?1 ORDER BY created_at",
        )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![workspace_id], |row| {
                Ok(EvidenceRow {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    source_id: row.get(2)?,
                    source_kind: row.get(3)?,
                    quote: row.get(4)?,
                    locator: row.get(5)?,
                })
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub fn source_count(&self, workspace_id: &str) -> StoreResult<usize> {
        let conn = self.lock()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sources WHERE workspace_id = ?1",
                params![workspace_id],
                |row| row.get(0),
            )
            .map_err(db_err)?;
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

fn db_err(err: rusqlite::Error) -> StoreError {
    StoreError::Internal(err.to_string())
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(e, _) => matches!(e.code, ErrorCode::ConstraintViolation),
        _ => false,
    }
}

fn map_write(err: rusqlite::Error, what: &str) -> StoreError {
    if is_unique_violation(&err) {
        StoreError::Conflict(format!("{what} already exists"))
    } else {
        db_err(err)
    }
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
        assert!(matches!(
            store.create_workspace("missing", "ws2"),
            Err(StoreError::NotFound { entity: "org", .. })
        ));
        let token = store.mint_token("ws1", Some("test")).unwrap();
        assert!(token.starts_with("etyma_"));
        assert_eq!(store.resolve_token(&token).unwrap().as_deref(), Some("ws1"));
        assert_eq!(store.list_workspaces("org1").unwrap().len(), 1);
        assert!(matches!(
            store.delete_org("org1"),
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            store.create_org("org1", "Dup"),
            Err(StoreError::Conflict(_))
        ));
    }
}
