use crate::blob::BlobStore;
use crate::graph::GraphStore;
use crate::knowledge::{KnowledgeStore, SourceRow};
use crate::materialize::materialize_source;
use std::sync::Arc;
use tokio::time::{self, Duration, MissedTickBehavior};
use uuid::Uuid;

const LEASE_DURATION_SECONDS: i64 = 300;
const RECOVERY_INTERVAL: Duration = Duration::from_secs(1);
const RECOVERY_BATCH_LIMIT: usize = 16;

pub fn spawn_upload_job(
    knowledge: KnowledgeStore,
    graph: GraphStore,
    blobs: Arc<dyn BlobStore>,
    workspace_id: String,
    source: SourceRow,
    job_id: String,
) {
    tokio::spawn(async move {
        if let Err(error) = run_upload_job(
            &knowledge,
            &graph,
            blobs.as_ref(),
            &workspace_id,
            &source,
            &job_id,
        )
        .await
        {
            tracing::warn!(
                workspace_id,
                source_id = %source.id,
                job_id,
                error = %error,
                "upload job worker failed"
            );
        }
    });
}

pub fn spawn_upload_recovery_loop(
    knowledge: KnowledgeStore,
    graph: GraphStore,
    blobs: Arc<dyn BlobStore>,
) {
    tokio::spawn(async move {
        let mut interval = time::interval(RECOVERY_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            if let Err(error) =
                recover_upload_jobs_once(&knowledge, &graph, blobs.clone(), RECOVERY_BATCH_LIMIT)
                    .await
            {
                tracing::warn!(error = %error, "upload job recovery scan failed");
            }
            interval.tick().await;
        }
    });
}

pub async fn recover_upload_jobs_once(
    knowledge: &KnowledgeStore,
    graph: &GraphStore,
    blobs: Arc<dyn BlobStore>,
    limit: usize,
) -> Result<usize, String> {
    let dispatched = knowledge
        .dispatchable_upload_jobs(limit)
        .await
        .map_err(|error| error.to_string())?;
    let count = dispatched.len();
    for job in dispatched {
        let source = job.clone().source();
        spawn_upload_job(
            knowledge.clone(),
            graph.clone(),
            blobs.clone(),
            job.workspace_id.clone(),
            source,
            job.job_id,
        );
    }
    Ok(count)
}

pub async fn run_upload_job(
    knowledge: &KnowledgeStore,
    graph: &GraphStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    source: &SourceRow,
    job_id: &str,
) -> Result<(), String> {
    let owner = format!("upload-http-{}", Uuid::now_v7().simple());
    let Some(claimed) = knowledge
        .claim_import_job_by_id(workspace_id, job_id, &owner, LEASE_DURATION_SECONDS)
        .await
        .map_err(|error| error.to_string())?
    else {
        tracing::info!(
            workspace_id,
            source_id = %source.id,
            job_id,
            "upload job already claimed by another worker"
        );
        return Ok(());
    };

    if claimed.source_id.as_deref() != Some(source.id.as_str()) {
        let error = format!("claimed import job source mismatch for {}", source.id);
        knowledge
            .fail_import_job(
                workspace_id,
                job_id,
                &owner,
                claimed.lease_token.as_deref().unwrap_or_default(),
                &error,
            )
            .await
            .map_err(|fail_error| fail_error.to_string())?;
        return Err(error);
    }

    let lease_token = claimed
        .lease_token
        .as_deref()
        .ok_or_else(|| "claimed import job missing lease token".to_string())?;

    match materialize_source(knowledge, graph, blobs, source).await {
        Ok(_) => {
            knowledge
                .succeed_import_job(workspace_id, job_id, &owner, lease_token)
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Err(error) => {
            knowledge
                .fail_import_job(workspace_id, job_id, &owner, lease_token, &error)
                .await
                .map_err(|fail_error| fail_error.to_string())?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::{put_bytes, LocalFsBlobStore};
    use crate::db::connect_and_migrate;
    use crate::knowledge::ImportJobStatus;

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
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(&org_id)
            .bind("Import job test")
            .bind(1_i64)
            .execute(pool)
            .await
            .expect("insert org");
        sqlx::query("INSERT INTO control.workspaces (id, org_id, created_at) VALUES ($1, $2, $3)")
            .bind(&workspace_id)
            .bind(&org_id)
            .bind(1_i64)
            .execute(pool)
            .await
            .expect("insert workspace");
        workspace_id
    }

    async fn wait_for_terminal_job(
        store: &KnowledgeStore,
        workspace_id: &str,
        job_id: &str,
    ) -> crate::knowledge::ImportJobRow {
        for _ in 0..100 {
            let job = store
                .get_import_job(workspace_id, job_id)
                .await
                .expect("read job")
                .expect("job exists");
            if matches!(
                job.status,
                ImportJobStatus::Succeeded | ImportJobStatus::Failed
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("upload job did not finish");
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn recovery_scan_dispatches_queued_and_expired_running_upload_jobs_only() {
        let pool = connect_and_migrate(&require_database_url())
            .await
            .expect("connect and migrate");
        let suffix = Uuid::now_v7().simple().to_string();
        let workspace = create_workspace(&pool, &format!("{suffix}_recover")).await;
        let store = KnowledgeStore::new(pool.clone());
        let graph = GraphStore::new(pool.clone());
        let blob_dir = tempfile::tempdir().expect("temp blob dir");
        let blobs = Arc::new(LocalFsBlobStore::open(blob_dir.path()).expect("open blob store"));

        let queued_blob =
            put_bytes(blobs.as_ref(), &workspace, b"queued evidence").expect("store queued blob");
        let expired_blob =
            put_bytes(blobs.as_ref(), &workspace, b"expired evidence").expect("store expired blob");
        let connector_blob = put_bytes(blobs.as_ref(), &workspace, b"connector evidence")
            .expect("store connector blob");

        let queued_source = store
            .insert_source(
                &workspace,
                "document",
                "Queued source",
                &queued_blob,
                "text/plain",
                None,
            )
            .await
            .expect("insert queued source");
        let expired_source = store
            .insert_source(
                &workspace,
                "document",
                "Expired source",
                &expired_blob,
                "text/plain",
                None,
            )
            .await
            .expect("insert expired source");
        let connector_source = store
            .insert_source(
                &workspace,
                "document",
                "Connector source",
                &connector_blob,
                "text/plain",
                None,
            )
            .await
            .expect("insert connector source");

        let queued_job = store
            .enqueue_upload_job(&workspace, &queued_source.id)
            .await
            .expect("enqueue queued job");
        let expired_job = store
            .enqueue_upload_job(&workspace, &expired_source.id)
            .await
            .expect("enqueue expired job");
        let connector_job = store
            .create_import_job(&workspace, Some(&connector_source.id), "connector", 3, 0)
            .await
            .expect("enqueue connector job");

        let expired_claim = store
            .claim_import_job_by_id(&workspace, &expired_job.id, "worker-a", 60)
            .await
            .expect("claim expired job")
            .expect("expired job claimed");
        assert_eq!(expired_claim.status, ImportJobStatus::Running);
        sqlx::query("UPDATE knowledge.import_jobs SET lease_expires_at = 0 WHERE id = $1")
            .bind(&expired_job.id)
            .execute(&pool)
            .await
            .expect("expire running upload job");

        let dispatched = recover_upload_jobs_once(&store, &graph, blobs.clone(), 16)
            .await
            .expect("recover upload jobs");
        assert_eq!(dispatched, 2);

        let queued_done = wait_for_terminal_job(&store, &workspace, &queued_job.id).await;
        let expired_done = wait_for_terminal_job(&store, &workspace, &expired_job.id).await;
        let connector_persisted = store
            .get_import_job(&workspace, &connector_job.id)
            .await
            .expect("read connector job")
            .expect("connector job exists");

        assert_eq!(queued_done.status, ImportJobStatus::Succeeded);
        assert_eq!(queued_done.attempts, 1);
        assert_eq!(expired_done.status, ImportJobStatus::Succeeded);
        assert_eq!(expired_done.attempts, 2);
        assert_eq!(connector_persisted.status, ImportJobStatus::Queued);
        assert_eq!(connector_persisted.attempts, 0);
    }
}
