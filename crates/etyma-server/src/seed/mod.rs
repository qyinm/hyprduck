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

/// Seed document + synthetic issue rows into the workspace tenant (metadata DB only).
/// Idempotent: if the workspace already has sources, this is a no-op.
pub fn seed_multi_source_workspace(store: &Store, workspace_id: &str) -> StoreResult<usize> {
    store.require_workspace(workspace_id)?;
    let existing = store.source_count(workspace_id)?;
    if existing > 0 {
        return Ok(existing);
    }

    let doc = store.insert_source(
        workspace_id,
        "document",
        "auth-spec.md",
        DOCUMENT_BODY,
        None,
    )?;
    store.insert_evidence(
        workspace_id,
        &doc.id,
        "document",
        "Agents must present a workspace-scoped alpha-token before reading evidence.",
        "page:1",
    )?;

    let issue = store.insert_source(
        workspace_id,
        "issue",
        "ENG-42 alpha-token multi-tenant packs",
        ISSUE_BODY,
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
