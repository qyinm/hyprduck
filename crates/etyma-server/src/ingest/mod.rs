//! Source ingest: write original bytes to the blob backend, then insert metadata.
//!
//! # Atomicity (spike)
//!
//! Blob put and SQLite meta insert are **not** a single transaction. If meta insert
//! fails after a successful put, an orphan blob object may remain under the blob root.
//! Spike recovery: wipe the data dir (DB + blobs) or re-seed after fixing the failure.
//! GC of orphan blobs is out of scope.

use crate::blob::{blob_key_for, put_bytes, BlobStore};
use crate::store::{SourceRow, Store, StoreResult};

/// Write original bytes to the blob backend, then insert source metadata.
pub fn ingest_source(
    store: &Store,
    blobs: &dyn BlobStore,
    workspace_id: &str,
    kind: &str,
    title: &str,
    bytes: &[u8],
    content_type: &str,
    external_id: Option<&str>,
) -> StoreResult<SourceRow> {
    store.require_workspace(workspace_id)?;
    let meta = put_bytes(blobs, workspace_id, bytes)?;
    // Defensive: meta key must match workspace content-address scheme.
    let expected_key = blob_key_for(workspace_id, &meta.content_hash);
    if meta.blob_key != expected_key {
        return Err(crate::store::StoreError::Integrity(format!(
            "blob key {} does not match content hash {}",
            meta.blob_key, meta.content_hash
        )));
    }
    store.insert_source(
        workspace_id,
        kind,
        title,
        &meta,
        content_type,
        external_id,
    )
}
