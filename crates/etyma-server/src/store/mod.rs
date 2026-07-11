//! Postgres control-plane store.

use crate::blob::BlobError;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::fmt;
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

#[derive(Debug)]
pub enum StoreError {
    NotFound { entity: &'static str, id: String },
    Conflict(String),
    Integrity(String),
    Internal(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict(message) | Self::Integrity(message) | Self::Internal(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<BlobError> for StoreError {
    fn from(error: BlobError) -> Self {
        match error {
            BlobError::NotFound { key } => Self::NotFound {
                entity: "blob",
                id: key,
            },
            BlobError::Integrity { expected, actual } => Self::Integrity(format!(
                "blob integrity mismatch: expected {expected}, got {actual}"
            )),
            BlobError::InvalidKey(message) => {
                Self::Integrity(format!("invalid blob key: {message}"))
            }
            BlobError::Io(message) => Self::Internal(format!("blob io: {message}")),
        }
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_org(&self, id: &str, name: &str) -> StoreResult<OrgRow> {
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(name)
            .bind(unix_now())
            .execute(&self.pool)
            .await
            .map_err(|error| map_pg_write(error, &format!("org {id}")))?;
        Ok(OrgRow {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }

    pub async fn get_org(&self, id: &str) -> StoreResult<Option<OrgRow>> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM control.orgs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(id, name)| OrgRow { id, name }))
    }

    pub async fn require_org(&self, id: &str) -> StoreResult<OrgRow> {
        self.get_org(id).await?.ok_or_else(|| StoreError::NotFound {
            entity: "org",
            id: id.to_owned(),
        })
    }

    pub async fn list_orgs(&self) -> StoreResult<Vec<OrgRow>> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM control.orgs ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(id, name)| OrgRow { id, name })
                .collect()
        })
        .map_err(pg_err)
    }

    pub async fn workspace_count_for_org(&self, org_id: &str) -> StoreResult<usize> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM control.workspaces WHERE org_id = $1",
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(count as usize)
    }

    pub async fn delete_org(&self, org_id: &str) -> StoreResult<()> {
        self.require_org(org_id).await?;
        let count = self.workspace_count_for_org(org_id).await?;
        if count > 0 {
            return Err(StoreError::Conflict(format!(
                "org has workspaces: {org_id} ({count})"
            )));
        }
        sqlx::query("DELETE FROM control.orgs WHERE id = $1")
            .bind(org_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    pub async fn create_workspace(&self, org_id: &str, id: &str) -> StoreResult<WorkspaceRow> {
        self.require_org(org_id).await?;
        sqlx::query("INSERT INTO control.workspaces (id, org_id, created_at) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org_id)
            .bind(unix_now())
            .execute(&self.pool)
            .await
            .map_err(|error| map_pg_write(error, &format!("workspace {id}")))?;
        Ok(WorkspaceRow {
            id: id.to_owned(),
            org_id: org_id.to_owned(),
        })
    }

    pub async fn get_workspace(&self, id: &str) -> StoreResult<Option<WorkspaceRow>> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT id, org_id FROM control.workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(id, org_id)| WorkspaceRow { id, org_id }))
    }

    pub async fn require_workspace(&self, id: &str) -> StoreResult<WorkspaceRow> {
        self.get_workspace(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "workspace",
                id: id.to_owned(),
            })
    }

    pub async fn list_workspaces(&self, org_id: &str) -> StoreResult<Vec<WorkspaceRow>> {
        self.require_org(org_id).await?;
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, org_id FROM control.workspaces WHERE org_id = $1 ORDER BY created_at, id",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(id, org_id)| WorkspaceRow { id, org_id })
                .collect()
        })
        .map_err(pg_err)
    }

    pub async fn mint_token(&self, workspace_id: &str, label: Option<&str>) -> StoreResult<String> {
        self.require_workspace(workspace_id).await?;
        let raw = format!("etyma_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO control.api_tokens (token_hash, workspace_id, label, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(hash_token(&raw))
        .bind(workspace_id)
        .bind(label)
        .bind(unix_now())
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(raw)
    }

    pub async fn resolve_token(&self, raw_token: &str) -> StoreResult<Option<String>> {
        sqlx::query_scalar::<_, String>(
            "SELECT workspace_id FROM control.api_tokens WHERE token_hash = $1",
        )
        .bind(hash_token(raw_token))
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)
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
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn pg_err(error: sqlx::Error) -> StoreError {
    tracing::warn!(%error, "control database operation failed");
    StoreError::Internal("control database operation failed".into())
}

fn map_pg_write(error: sqlx::Error, entity: &str) -> StoreError {
    if let Some(database_error) = error.as_database_error() {
        return match database_error.code().as_deref() {
            Some("23503") => StoreError::NotFound {
                entity: "parent",
                id: entity.to_owned(),
            },
            Some("23505") => StoreError::Conflict(format!("{entity} already exists")),
            _ => pg_err(error),
        };
    }
    pg_err(error)
}
