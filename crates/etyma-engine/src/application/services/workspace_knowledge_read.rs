//! DB-primary workspace reads with artifact fallback.
//!
//! SQLite + GraphQLite is authoritative. BrainReader JSON artifacts remain
//! migration_input / empty-DB fallback only. Callers must not invent a third path.

use std::path::{Path, PathBuf};

use anyhow::Result;
use etyma_engine_types::BrainReadScope;

use crate::{resolve_brain_workspace_root, BrainReader, KnowledgeStore};

/// Open the workspace knowledge store for a read scope.
pub(crate) fn open_store_for_scope(scope: &BrainReadScope) -> Result<KnowledgeStore> {
    let root = resolve_brain_workspace_root(scope)?;
    KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))
}

/// Open workspace root + knowledge store together (avoids a second root resolve).
pub(crate) fn open_workspace_for_scope(
    scope: &BrainReadScope,
) -> Result<(PathBuf, KnowledgeStore)> {
    let root = resolve_brain_workspace_root(scope)?;
    let store = KnowledgeStore::open(KnowledgeStore::default_path_for_root(&root))?;
    Ok((root, store))
}

/// Prefer DB value; only open BrainReader when `db` returns `None`.
///
/// `db` receives the opened store and workspace root. Returning `Ok(None)` is a
/// deliberate miss (empty result / missing row) and triggers the reader path.
/// `Err` from `db` propagates and does not open BrainReader.
pub(crate) fn db_then_reader<T, FDb, FReader>(
    scope: &BrainReadScope,
    db: FDb,
    reader: FReader,
) -> Result<T>
where
    FDb: FnOnce(&KnowledgeStore, &Path) -> Result<Option<T>>,
    FReader: FnOnce(&BrainReader, &Path) -> Result<T>,
{
    let (root, store) = open_workspace_for_scope(scope)?;
    if let Some(value) = db(&store, &root)? {
        return Ok(value);
    }
    let brain_reader = BrainReader::open(scope)?;
    reader(&brain_reader, &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use etyma_engine_types::BrainReadScope;
    use tempfile::tempdir;

    #[test]
    fn open_store_for_scope_opens_default_knowledge_db() {
        let temp = tempdir().expect("temp dir");
        let scope = BrainReadScope {
            workspace_id: "workspace-default".into(),
            root_dir: Some(temp.path().display().to_string()),
        };
        let store = open_store_for_scope(&scope).expect("open store");
        let expected = KnowledgeStore::default_path_for_root(
            &resolve_brain_workspace_root(&scope).expect("root"),
        );
        assert_eq!(store.path(), expected.as_path());
    }
}
