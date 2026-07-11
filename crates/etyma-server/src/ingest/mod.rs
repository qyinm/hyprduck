//! Source ingest: write original bytes to the blob backend, then insert metadata.
//!
//! # Atomicity
//!
//! Blob put and Postgres meta insert are **not** a single transaction. If meta insert
//! fails after a successful put, an orphan blob object may remain under the blob root.
//! Recovery requires orphan-object cleanup or re-ingest after fixing the failure.
//! GC of orphan blobs is out of scope.

use crate::blob::{blob_key_for, put_bytes, BlobStore};
use crate::knowledge::{KnowledgeStore, SourceRow};
use crate::store::{Store, StoreResult};

/// Write original bytes to the blob backend, then insert source metadata.
pub async fn ingest_source(
    store: &Store,
    knowledge: &KnowledgeStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    kind: &str,
    title: &str,
    bytes: &[u8],
    content_type: &str,
    external_id: Option<&str>,
) -> StoreResult<SourceRow> {
    store.require_workspace(workspace_id).await?;
    let meta = put_bytes(blobs, workspace_id, bytes)?;
    // Defensive: meta key must match workspace content-address scheme.
    let expected_key = blob_key_for(workspace_id, &meta.content_hash);
    if meta.blob_key != expected_key {
        return Err(crate::store::StoreError::Integrity(format!(
            "blob key {} does not match content hash {}",
            meta.blob_key, meta.content_hash
        )));
    }
    knowledge
        .insert_source(workspace_id, kind, title, &meta, content_type, external_id)
        .await
        .map_err(Into::into)
}
