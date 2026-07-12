use crate::blob::{get_verified, BlobStore};
use crate::knowledge::{KnowledgeStore, SourceRow};
use std::sync::Arc;
use uuid::Uuid;

const LEASE_DURATION_SECONDS: i64 = 300;

pub fn spawn_upload_job(
    knowledge: KnowledgeStore,
    blobs: Arc<dyn BlobStore>,
    workspace_id: String,
    source: SourceRow,
    job_id: String,
) {
    tokio::spawn(async move {
        if let Err(error) =
            run_upload_job(&knowledge, blobs.as_ref(), &workspace_id, &source, &job_id).await
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

pub async fn run_upload_job(
    knowledge: &KnowledgeStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    source: &SourceRow,
    job_id: &str,
) -> Result<(), String> {
    let owner = format!("upload-http-{}", Uuid::now_v7().simple());
    let Some(claimed) = knowledge
        .claim_import_job(workspace_id, &owner, LEASE_DURATION_SECONDS)
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

    if claimed.id != job_id {
        let error = format!("claimed unexpected import job for source {}", source.id);
        let _ = knowledge
            .fail_import_job(
                workspace_id,
                &claimed.id,
                &owner,
                claimed.lease_token.as_deref().unwrap_or_default(),
                &error,
            )
            .await;
        return Err(error);
    }

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

    match soft_materialize_source(knowledge, blobs, source).await {
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

pub async fn soft_materialize_source(
    knowledge: &KnowledgeStore,
    blobs: &dyn BlobStore,
    source: &SourceRow,
) -> Result<usize, String> {
    let bytes = get_verified(blobs, &source.blob_key).map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("source blob is not valid UTF-8: {error}"))?;
    if text.trim().is_empty() {
        return Err("source blob is empty".into());
    }
    knowledge
        .upsert_evidence(
            &source.workspace_id,
            &format!("ev_{}_root", source.id),
            &source.id,
            &source.kind,
            text,
            "page:1",
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(1)
}
