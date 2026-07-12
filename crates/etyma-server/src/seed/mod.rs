use crate::blob::BlobStore;
use crate::ingest::ingest_source;
use crate::knowledge::KnowledgeStore;
use crate::store::{Store, StoreError, StoreResult};

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
pub async fn seed_multi_source_workspace(
    store: &Store,
    knowledge: &KnowledgeStore,
    blobs: &dyn BlobStore,
    workspace_id: &str,
) -> StoreResult<usize> {
    store.require_workspace(workspace_id).await?;
    let existing = knowledge
        .source_count(workspace_id)
        .await
        .map_err(StoreError::from)?;
    if existing > 0 {
        return Ok(existing);
    }

    let doc = ingest_source(
        store,
        knowledge,
        blobs,
        workspace_id,
        "document",
        "auth-spec.md",
        DOCUMENT_BODY.as_bytes(),
        "text/markdown",
        None,
    )
    .await?;
    knowledge
        .insert_evidence(
            workspace_id,
            &doc.id,
            "document",
            "Agents must present a workspace-scoped alpha-token before reading evidence.",
            "page:1",
        )
        .await
        .map_err(StoreError::from)?;

    let issue = ingest_source(
        store,
        knowledge,
        blobs,
        workspace_id,
        "issue",
        "ENG-42 alpha-token multi-tenant packs",
        ISSUE_BODY.as_bytes(),
        "text/markdown",
        Some("ENG-42"),
    )
    .await?;
    knowledge
        .insert_evidence(
            workspace_id,
            &issue.id,
            "issue",
            "We need alpha-token binding so packs never leak across workspaces.",
            "issue:ENG-42",
        )
        .await
        .map_err(StoreError::from)?;
    knowledge
        .source_count(workspace_id)
        .await
        .map_err(StoreError::from)
}
