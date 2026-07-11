use crate::blob::BlobStore;
use crate::ingest::ingest_source;
use crate::store::{Store, StoreResult};

pub const FIXTURE_TERM: &str = "alpha-token";

const DOCUMENT_BODY: &str = r#"# Auth Spec

Workspace isolation depends on the alpha-token boundary.

Agents must present a workspace-scoped alpha-token before reading evidence.
"#;

const ISSUE_BODY: &str = r#"# ENG-42 Track alpha-token multi-tenant packs

We need alpha-token binding so packs never leak across workspaces.

Acceptance: token for workspace A cannot read workspace B.
"#;

/// Seed document + synthetic issue rows into the workspace tenant.
/// Uses [`ingest_source`] (blob first, then metadata). Idempotent when sources already exist.
///
/// Coarse idempotency: any existing source count skips re-seed (partial seed is not repaired).
pub fn seed_multi_source_workspace(
    store: &Store,
    blobs: &dyn BlobStore,
    workspace_id: &str,
) -> StoreResult<usize> {
    store.require_workspace(workspace_id)?;
    let existing = store.source_count(workspace_id)?;
    if existing > 0 {
        return Ok(existing);
    }

    let doc = ingest_source(
        store,
        blobs,
        workspace_id,
        "document",
        "auth-spec.md",
        DOCUMENT_BODY.as_bytes(),
        "text/markdown",
        None,
    )?;
    store.insert_evidence(
        workspace_id,
        &doc.id,
        "document",
        "Agents must present a workspace-scoped alpha-token before reading evidence.",
        "page:1",
    )?;

    let issue = ingest_source(
        store,
        blobs,
        workspace_id,
        "issue",
        "ENG-42 alpha-token multi-tenant packs",
        ISSUE_BODY.as_bytes(),
        "text/markdown",
        Some("ENG-42"),
    )?;
    store.insert_evidence(
        workspace_id,
        &issue.id,
        "issue",
        "We need alpha-token binding so packs never leak across workspaces.",
        "issue:ENG-42",
    )?;
    store.source_count(workspace_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob::{BlobStore, LocalFsBlobStore};
    use tempfile::tempdir;

    #[test]
    fn seed_writes_blob_then_meta() {
        let dir = tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite3")).unwrap();
        let blobs = LocalFsBlobStore::open(dir.path().join("blobs")).unwrap();
        store.create_org("org1", "Test").unwrap();
        store.create_workspace("org1", "ws").unwrap();
        let n = seed_multi_source_workspace(&store, &blobs, "ws").unwrap();
        assert_eq!(n, 2);
        let sources = store.list_sources("ws").unwrap();
        assert_eq!(sources.len(), 2);
        for src in &sources {
            assert!(!src.blob_key.is_empty());
            assert!(src.content_hash.starts_with("sha256:"));
            assert!(src.byte_size > 0);
            assert!(blobs.exists(&src.blob_key).unwrap());
            let bytes = blobs.get(&src.blob_key).unwrap();
            assert_eq!(bytes.len() as i64, src.byte_size);
            assert_eq!(
                crate::blob::content_hash_sha256(&bytes),
                src.content_hash
            );
        }
        // idempotent
        assert_eq!(seed_multi_source_workspace(&store, &blobs, "ws").unwrap(), 2);
    }
}
