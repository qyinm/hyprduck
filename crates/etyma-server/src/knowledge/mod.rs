use crate::blob::BlobPutMeta;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SourceRow {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub title: String,
    pub blob_key: String,
    pub content_hash: String,
    pub byte_size: i64,
    pub content_type: String,
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EvidenceRow {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub source_kind: String,
    pub quote: String,
    pub locator: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
pub enum ImportJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ImportJobRow {
    pub id: String,
    pub workspace_id: String,
    pub source_id: Option<String>,
    pub kind: String,
    pub status: ImportJobStatus,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_error: Option<String>,
    pub available_at: i64,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug)]
pub enum KnowledgeError {
    NotFound { entity: &'static str, id: String },
    Conflict(String),
    Internal(String),
}

impl fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict(message) | Self::Internal(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for KnowledgeError {}

impl From<KnowledgeError> for crate::store::StoreError {
    fn from(error: KnowledgeError) -> Self {
        match error {
            KnowledgeError::NotFound { entity, id } => Self::NotFound { entity, id },
            KnowledgeError::Conflict(message) => Self::Conflict(message),
            KnowledgeError::Internal(message) => Self::Internal(message),
        }
    }
}

pub type KnowledgeResult<T> = Result<T, KnowledgeError>;

#[derive(Clone)]
pub struct KnowledgeStore {
    pool: PgPool,
}

impl KnowledgeStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn insert_source(
        &self,
        workspace_id: &str,
        kind: &str,
        title: &str,
        blob: &BlobPutMeta,
        content_type: &str,
        external_id: Option<&str>,
    ) -> KnowledgeResult<SourceRow> {
        let id = format!("src_{}", Uuid::now_v7().simple());
        let now = unix_now();
        sqlx::query_as::<_, SourceRow>(
            r#"
            INSERT INTO knowledge.sources (
              id, workspace_id, kind, title, blob_key, content_hash, byte_size,
              content_type, external_id, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
            RETURNING id, workspace_id, kind, title, blob_key, content_hash,
                      byte_size, content_type, external_id
            "#,
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(kind)
        .bind(title)
        .bind(&blob.blob_key)
        .bind(&blob.content_hash)
        .bind(blob.byte_size as i64)
        .bind(content_type)
        .bind(external_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_write(error, "source"))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_evidence(
        &self,
        workspace_id: &str,
        id: &str,
        source_id: &str,
        source_kind: &str,
        quote: &str,
        locator: &str,
    ) -> KnowledgeResult<EvidenceRow> {
        let now = unix_now();
        let content_hash = hash_text(quote);
        sqlx::query_as::<_, EvidenceRow>(
            r#"
            INSERT INTO knowledge.evidence (
              id, workspace_id, source_id, source_kind, quote, locator,
              content_hash, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
            ON CONFLICT (id) DO UPDATE SET
              source_kind = EXCLUDED.source_kind,
              quote = EXCLUDED.quote,
              locator = EXCLUDED.locator,
              content_hash = EXCLUDED.content_hash,
              updated_at = EXCLUDED.updated_at
            WHERE knowledge.evidence.workspace_id = EXCLUDED.workspace_id
              AND knowledge.evidence.source_id = EXCLUDED.source_id
            RETURNING id, workspace_id, source_id, source_kind, quote, locator,
                      content_hash
            "#,
        )
        .bind(id)
        .bind(workspace_id)
        .bind(source_id)
        .bind(source_kind)
        .bind(quote)
        .bind(locator)
        .bind(content_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_write(error, "evidence"))?
        .ok_or_else(|| KnowledgeError::Conflict(format!("evidence identity conflict: {id}")))
    }

    pub async fn insert_evidence(
        &self,
        workspace_id: &str,
        source_id: &str,
        source_kind: &str,
        quote: &str,
        locator: &str,
    ) -> KnowledgeResult<EvidenceRow> {
        let id = format!("ev_{}", Uuid::now_v7().simple());
        self.upsert_evidence(workspace_id, &id, source_id, source_kind, quote, locator)
            .await
    }

    pub async fn list_sources(&self, workspace_id: &str) -> KnowledgeResult<Vec<SourceRow>> {
        sqlx::query_as::<_, SourceRow>(
            r#"
            SELECT id, workspace_id, kind, title, blob_key, content_hash,
                   byte_size, content_type, external_id
            FROM knowledge.sources
            WHERE workspace_id = $1
            ORDER BY created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)
    }

    pub async fn list_evidence(&self, workspace_id: &str) -> KnowledgeResult<Vec<EvidenceRow>> {
        sqlx::query_as::<_, EvidenceRow>(
            r#"
            SELECT id, workspace_id, source_id, source_kind, quote, locator,
                   content_hash
            FROM knowledge.evidence
            WHERE workspace_id = $1
            ORDER BY created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_read)
    }

    pub async fn source_count(&self, workspace_id: &str) -> KnowledgeResult<usize> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge.sources WHERE workspace_id = $1",
        )
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_read)?;
        Ok(count as usize)
    }

    pub async fn create_import_job(
        &self,
        workspace_id: &str,
        source_id: Option<&str>,
        kind: &str,
        max_attempts: i32,
        available_at: i64,
    ) -> KnowledgeResult<ImportJobRow> {
        let id = format!("job_{}", Uuid::now_v7().simple());
        let now = unix_now();
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            INSERT INTO knowledge.import_jobs (
              id, workspace_id, source_id, kind, status, attempts, max_attempts,
              available_at, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'queued', 0, $5, $6, $7, $7)
            RETURNING id, workspace_id, source_id, kind, status, attempts,
                      max_attempts, last_error, available_at, lease_owner,
                      lease_expires_at, created_at, updated_at
            "#,
        )
        .bind(&id)
        .bind(workspace_id)
        .bind(source_id)
        .bind(kind)
        .bind(max_attempts)
        .bind(available_at)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| map_write(error, "import job"))
    }

    pub async fn claim_import_job(
        &self,
        workspace_id: &str,
        lease_owner: &str,
        now: i64,
        lease_expires_at: i64,
    ) -> KnowledgeResult<Option<ImportJobRow>> {
        let mut transaction = self.pool.begin().await.map_err(map_read)?;
        sqlx::query(
            r#"
            UPDATE knowledge.import_jobs
            SET status = 'failed',
                last_error = 'maximum attempts exhausted after lease expiry',
                lease_owner = NULL,
                lease_expires_at = NULL,
                updated_at = $2
            WHERE workspace_id = $1
              AND status = 'running'
              AND lease_expires_at <= $2
              AND attempts >= max_attempts
            "#,
        )
        .bind(workspace_id)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(map_read)?;

        let claimed = sqlx::query_as::<_, ImportJobRow>(
            r#"
            WITH candidate AS (
              SELECT id
              FROM knowledge.import_jobs
              WHERE workspace_id = $1
                AND attempts < max_attempts
                AND (
                  (status = 'queued' AND available_at <= $3)
                  OR (status = 'running' AND lease_expires_at <= $3)
                )
              ORDER BY available_at, created_at, id
              FOR UPDATE SKIP LOCKED
              LIMIT 1
            )
            UPDATE knowledge.import_jobs AS jobs
            SET status = 'running',
                attempts = jobs.attempts + 1,
                last_error = NULL,
                lease_owner = $2,
                lease_expires_at = $4,
                updated_at = $3
            FROM candidate
            WHERE jobs.id = candidate.id
            RETURNING jobs.id, jobs.workspace_id, jobs.source_id, jobs.kind,
                      jobs.status, jobs.attempts, jobs.max_attempts,
                      jobs.last_error, jobs.available_at, jobs.lease_owner,
                      jobs.lease_expires_at, jobs.created_at, jobs.updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(lease_owner)
        .bind(now)
        .bind(lease_expires_at)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_read)?;
        transaction.commit().await.map_err(map_read)?;
        Ok(claimed)
    }

    pub async fn succeed_import_job(
        &self,
        workspace_id: &str,
        job_id: &str,
        lease_owner: &str,
    ) -> KnowledgeResult<ImportJobRow> {
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            UPDATE knowledge.import_jobs
            SET status = 'succeeded', lease_owner = NULL,
                lease_expires_at = NULL, last_error = NULL,
                updated_at = $4
            WHERE workspace_id = $1 AND id = $2
              AND status = 'running' AND lease_owner = $3
            RETURNING id, workspace_id, source_id, kind, status, attempts,
                      max_attempts, last_error, available_at, lease_owner,
                      lease_expires_at, created_at, updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(job_id)
        .bind(lease_owner)
        .bind(unix_now())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_read)?
        .ok_or_else(|| KnowledgeError::Conflict(format!("import job lease conflict: {job_id}")))
    }

    pub async fn retry_import_job(
        &self,
        workspace_id: &str,
        job_id: &str,
        lease_owner: &str,
        error: &str,
        available_at: i64,
    ) -> KnowledgeResult<ImportJobRow> {
        let error = bounded_error(error);
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            UPDATE knowledge.import_jobs
            SET status = CASE
                  WHEN attempts >= max_attempts THEN 'failed'
                  ELSE 'queued'
                END,
                lease_owner = NULL,
                lease_expires_at = NULL, last_error = $4,
                available_at = $5, updated_at = $6
            WHERE workspace_id = $1 AND id = $2
              AND status = 'running' AND lease_owner = $3
            RETURNING id, workspace_id, source_id, kind, status, attempts,
                      max_attempts, last_error, available_at, lease_owner,
                      lease_expires_at, created_at, updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(job_id)
        .bind(lease_owner)
        .bind(error)
        .bind(available_at)
        .bind(unix_now())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_read)?
        .ok_or_else(|| KnowledgeError::Conflict(format!("import job lease conflict: {job_id}")))
    }

    pub async fn fail_import_job(
        &self,
        workspace_id: &str,
        job_id: &str,
        lease_owner: &str,
        error: &str,
    ) -> KnowledgeResult<ImportJobRow> {
        let error = bounded_error(error);
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            UPDATE knowledge.import_jobs
            SET status = 'failed', lease_owner = NULL,
                lease_expires_at = NULL, last_error = $4,
                updated_at = $5
            WHERE workspace_id = $1 AND id = $2
              AND status = 'running' AND lease_owner = $3
            RETURNING id, workspace_id, source_id, kind, status, attempts,
                      max_attempts, last_error, available_at, lease_owner,
                      lease_expires_at, created_at, updated_at
            "#,
        )
        .bind(workspace_id)
        .bind(job_id)
        .bind(lease_owner)
        .bind(error)
        .bind(unix_now())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_read)?
        .ok_or_else(|| KnowledgeError::Conflict(format!("import job lease conflict: {job_id}")))
    }

    pub async fn get_import_job(
        &self,
        workspace_id: &str,
        job_id: &str,
    ) -> KnowledgeResult<Option<ImportJobRow>> {
        sqlx::query_as::<_, ImportJobRow>(
            r#"
            SELECT id, workspace_id, source_id, kind, status, attempts,
                   max_attempts, last_error, available_at, lease_owner,
                   lease_expires_at, created_at, updated_at
            FROM knowledge.import_jobs
            WHERE workspace_id = $1 AND id = $2
            "#,
        )
        .bind(workspace_id)
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_read)
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn bounded_error(error: &str) -> String {
    error.chars().take(4096).collect()
}

fn map_read(error: sqlx::Error) -> KnowledgeError {
    tracing::warn!(%error, "knowledge database read failed");
    KnowledgeError::Internal("knowledge database read failed".into())
}

fn map_write(error: sqlx::Error, entity: &str) -> KnowledgeError {
    if let Some(database_error) = error.as_database_error() {
        return match database_error.code().as_deref() {
            Some("23503") => KnowledgeError::Conflict(format!("invalid {entity} relationship")),
            Some("23505") => KnowledgeError::Conflict(format!("{entity} already exists")),
            _ => {
                tracing::warn!(%error, entity, "knowledge database write failed");
                KnowledgeError::Internal("knowledge database write failed".into())
            }
        };
    }
    tracing::warn!(%error, entity, "knowledge database write failed");
    KnowledgeError::Internal("knowledge database write failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn require_database_url() -> String {
        std::env::var("ETYMA_DATABASE_URL")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests")
    }

    async fn create_workspace(pool: &sqlx::PgPool, suffix: &str) -> String {
        let org_id = format!("org_{suffix}");
        let workspace_id = format!("ws_{suffix}");
        let now = 1_i64;
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(&org_id)
            .bind("Knowledge test")
            .bind(now)
            .execute(pool)
            .await
            .expect("insert org");
        sqlx::query("INSERT INTO control.workspaces (id, org_id, created_at) VALUES ($1, $2, $3)")
            .bind(&workspace_id)
            .bind(&org_id)
            .bind(now)
            .execute(pool)
            .await
            .expect("insert workspace");
        workspace_id
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn source_and_evidence_rows_are_workspace_scoped() {
        let pool = crate::db::connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace_a = create_workspace(&pool, &format!("{suffix}_a")).await;
        let workspace_b = create_workspace(&pool, &format!("{suffix}_b")).await;
        let evidence_id = format!("ev_{suffix}_materialized");
        let cross_evidence_id = format!("ev_{suffix}_cross_workspace");
        let store = KnowledgeStore::new(pool);
        let blob = BlobPutMeta {
            blob_key: format!("w/{workspace_a}/sha256/abc"),
            content_hash: "sha256:abc".into(),
            byte_size: 12,
        };

        let source = store
            .insert_source(
                &workspace_a,
                "document",
                "Spec",
                &blob,
                "text/plain",
                Some("DOC-1"),
            )
            .await
            .expect("insert source");
        let cross_workspace_job = store
            .create_import_job(&workspace_b, Some(&source.id), "upload", 3, 1)
            .await;
        assert!(cross_workspace_job.is_err());
        store
            .upsert_evidence(
                &workspace_a,
                &evidence_id,
                &source.id,
                "document",
                "Durable quoted evidence",
                "page:1",
            )
            .await
            .expect("insert evidence");
        store
            .upsert_evidence(
                &workspace_a,
                &evidence_id,
                &source.id,
                "document",
                "Updated durable quote",
                "page:2",
            )
            .await
            .expect("update evidence");

        let sources_a = store.list_sources(&workspace_a).await.expect("sources A");
        let sources_b = store.list_sources(&workspace_b).await.expect("sources B");
        let evidence_a = store.list_evidence(&workspace_a).await.expect("evidence A");
        assert_eq!(sources_a.len(), 1);
        assert!(sources_b.is_empty());
        assert_eq!(evidence_a.len(), 1);
        assert_eq!(evidence_a[0].quote, "Updated durable quote");
        assert_eq!(evidence_a[0].locator, "page:2");

        let cross_workspace = store
            .upsert_evidence(
                &workspace_b,
                &cross_evidence_id,
                &source.id,
                "document",
                "Must be rejected",
                "page:1",
            )
            .await;
        assert!(cross_workspace.is_err());
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn import_job_claim_is_concurrent_and_lease_safe() {
        let database_url = require_database_url();
        let pool = crate::db::connect_and_migrate(&database_url)
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace = create_workspace(&pool, &format!("{suffix}_jobs")).await;
        let store = KnowledgeStore::new(pool);
        let created = store
            .create_import_job(&workspace, None, "upload", 3, 10)
            .await
            .expect("create job");

        let store_a = store.clone();
        let store_b = store.clone();
        let workspace_a = workspace.clone();
        let workspace_b = workspace.clone();
        let (claim_a, claim_b) = tokio::join!(
            store_a.claim_import_job(&workspace_a, "worker-a", 10, 20),
            store_b.claim_import_job(&workspace_b, "worker-b", 10, 20),
        );
        let claims = [claim_a.expect("claim A"), claim_b.expect("claim B")];
        assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
        let first = claims.into_iter().flatten().next().expect("one claim");
        assert_eq!(first.id, created.id);
        assert_eq!(first.attempts, 1);

        let reclaimed = store
            .claim_import_job(&workspace, "worker-c", 21, 31)
            .await
            .expect("reclaim")
            .expect("expired lease is claimable");
        assert_eq!(reclaimed.id, created.id);
        assert_eq!(reclaimed.attempts, 2);

        let first_worker = first.lease_owner.expect("claimed worker");
        let stale = store.succeed_import_job(&workspace, &created.id, &first_worker);
        assert!(stale.await.is_err());
        store
            .succeed_import_job(&workspace, &created.id, "worker-c")
            .await
            .expect("active worker completes");

        let reconnected = KnowledgeStore::new(
            crate::db::connect_and_migrate(&database_url)
                .await
                .expect("reconnect"),
        );
        let persisted = reconnected
            .get_import_job(&workspace, &created.id)
            .await
            .expect("read persisted job")
            .expect("job exists");
        assert_eq!(persisted.status, ImportJobStatus::Succeeded);
        assert_eq!(persisted.attempts, 2);
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn import_job_retry_and_terminal_error_are_durable() {
        let database_url = require_database_url();
        let pool = crate::db::connect_and_migrate(&database_url)
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace = create_workspace(&pool, &format!("{suffix}_retry")).await;
        let store = KnowledgeStore::new(pool);
        let job = store
            .create_import_job(&workspace, None, "connector", 3, 100)
            .await
            .expect("create job");
        store
            .claim_import_job(&workspace, "worker-a", 100, 110)
            .await
            .expect("claim")
            .expect("job claimed");

        let retried = store
            .retry_import_job(&workspace, &job.id, "worker-a", "temporary", 120)
            .await
            .expect("retry job");
        assert_eq!(retried.status, ImportJobStatus::Queued);
        assert_eq!(retried.last_error.as_deref(), Some("temporary"));
        assert_eq!(retried.available_at, 120);
        assert!(retried.lease_owner.is_none());
        assert!(store
            .claim_import_job(&workspace, "worker-b", 119, 130)
            .await
            .expect("early claim")
            .is_none());
        store
            .claim_import_job(&workspace, "worker-b", 120, 130)
            .await
            .expect("second claim")
            .expect("retry became available");

        let oversized = "x".repeat(5000);
        let failed = store
            .fail_import_job(&workspace, &job.id, "worker-b", &oversized)
            .await
            .expect("terminal failure");
        assert_eq!(failed.status, ImportJobStatus::Failed);
        assert_eq!(
            failed.last_error.as_ref().map(|e| e.chars().count()),
            Some(4096)
        );
        assert!(failed.lease_owner.is_none());

        let reconnected = KnowledgeStore::new(
            crate::db::connect_and_migrate(&database_url)
                .await
                .expect("reconnect"),
        );
        let persisted = reconnected
            .get_import_job(&workspace, &job.id)
            .await
            .expect("read job")
            .expect("job exists");
        assert_eq!(persisted.status, ImportJobStatus::Failed);
        assert_eq!(persisted.attempts, 2);
        assert_eq!(
            persisted.last_error.as_ref().map(|e| e.chars().count()),
            Some(4096)
        );
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn import_job_expired_final_attempt_becomes_failed() {
        let pool = crate::db::connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace = create_workspace(&pool, &format!("{suffix}_exhausted")).await;
        let store = KnowledgeStore::new(pool);
        let job = store
            .create_import_job(&workspace, None, "upload", 1, 10)
            .await
            .expect("create job");
        store
            .claim_import_job(&workspace, "worker-a", 10, 20)
            .await
            .expect("claim")
            .expect("job claimed");

        assert!(store
            .claim_import_job(&workspace, "worker-b", 21, 31)
            .await
            .expect("claim after final lease")
            .is_none());
        let exhausted = store
            .get_import_job(&workspace, &job.id)
            .await
            .expect("read exhausted job")
            .expect("job exists");
        assert_eq!(exhausted.status, ImportJobStatus::Failed);
        assert_eq!(
            exhausted.last_error.as_deref(),
            Some("maximum attempts exhausted after lease expiry")
        );
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn import_job_retry_on_final_attempt_becomes_failed() {
        let pool = crate::db::connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let workspace = create_workspace(&pool, &format!("{suffix}_final_retry")).await;
        let store = KnowledgeStore::new(pool);
        let job = store
            .create_import_job(&workspace, None, "upload", 1, 10)
            .await
            .expect("create job");
        store
            .claim_import_job(&workspace, "worker-a", 10, 20)
            .await
            .expect("claim")
            .expect("job claimed");

        let exhausted = store
            .retry_import_job(&workspace, &job.id, "worker-a", "still broken", 30)
            .await
            .expect("record final retry failure");
        assert_eq!(exhausted.status, ImportJobStatus::Failed);
        assert_eq!(exhausted.last_error.as_deref(), Some("still broken"));
        assert!(exhausted.lease_owner.is_none());
    }
}
