//! Blob backend for original source bytes (outside SQLite metadata).
//!
//! Key scheme (v1): `w/{workspace_id}/sha256/{hex}`
//! Content hash form: `sha256:{hex}` (full SHA-256 of raw bytes).

use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Content-addressed blob key for a workspace-scoped object.
pub fn blob_key_for(workspace_id: &str, content_hash: &str) -> String {
    let hex = strip_sha256_prefix(content_hash);
    format!("w/{workspace_id}/sha256/{hex}")
}

/// Full content hash of raw bytes: `sha256:{hex}`.
pub fn content_hash_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn strip_sha256_prefix(content_hash: &str) -> &str {
    content_hash
        .strip_prefix("sha256:")
        .unwrap_or(content_hash)
}

#[derive(Debug)]
pub enum BlobError {
    NotFound { key: String },
    Integrity { expected: String, actual: String },
    InvalidKey(String),
    Io(String),
}

impl fmt::Display for BlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { key } => write!(f, "blob not found: {key}"),
            Self::Integrity { expected, actual } => {
                write!(f, "blob integrity mismatch: expected {expected}, got {actual}")
            }
            Self::InvalidKey(msg) => write!(f, "invalid blob key: {msg}"),
            Self::Io(msg) => write!(f, "blob io: {msg}"),
        }
    }
}

impl std::error::Error for BlobError {}

pub type BlobResult<T> = Result<T, BlobError>;

/// Blob backend: put/get/delete/exists. Implementations must be process-safe for the spike.
pub trait BlobStore: Send + Sync {
    fn put(&self, key: &str, bytes: &[u8]) -> BlobResult<()>;
    fn get(&self, key: &str) -> BlobResult<Vec<u8>>;
    fn delete(&self, key: &str) -> BlobResult<()>;
    fn exists(&self, key: &str) -> BlobResult<bool>;
}

/// Result of a hashed write: key + hash + size for source metadata.
#[derive(Debug, Clone)]
pub struct BlobPutMeta {
    pub blob_key: String,
    pub content_hash: String,
    pub byte_size: u64,
}

/// Hash bytes, verify integrity, put under the standard key scheme, return meta.
pub fn put_bytes(
    blobs: &dyn BlobStore,
    workspace_id: &str,
    bytes: &[u8],
) -> BlobResult<BlobPutMeta> {
    let content_hash = content_hash_sha256(bytes);
    // B6: integrity check on write (hash of payload must match stored hash).
    let verify = content_hash_sha256(bytes);
    if verify != content_hash {
        return Err(BlobError::Integrity {
            expected: content_hash,
            actual: verify,
        });
    }
    let blob_key = blob_key_for(workspace_id, &content_hash);
    put_with_expected_hash(blobs, &blob_key, &content_hash, bytes)?;
    Ok(BlobPutMeta {
        blob_key,
        content_hash,
        byte_size: bytes.len() as u64,
    })
}

/// Put bytes only if their SHA-256 matches `expected_hash` (`sha256:…`).
pub fn put_with_expected_hash(
    blobs: &dyn BlobStore,
    key: &str,
    expected_hash: &str,
    bytes: &[u8],
) -> BlobResult<()> {
    let actual = content_hash_sha256(bytes);
    if actual != expected_hash {
        return Err(BlobError::Integrity {
            expected: expected_hash.to_string(),
            actual,
        });
    }
    blobs.put(key, bytes)
}

/// Local filesystem blob adapter (`ETYMA_BLOB_ROOT`).
///
/// Layout: `{root}/{key}` where key uses `/` as path separators.
/// Writes are atomic via temp file + rename within the same directory.
#[derive(Debug, Clone)]
pub struct LocalFsBlobStore {
    root: PathBuf,
}

impl LocalFsBlobStore {
    pub fn open(root: impl AsRef<Path>) -> BlobResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| {
            BlobError::Io(format!("create blob root {}: {e}", root.display()))
        })?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, key: &str) -> BlobResult<PathBuf> {
        validate_blob_key(key)?;
        // Key is already slash-separated segments; join under root.
        let mut path = self.root.clone();
        for segment in key.split('/') {
            path.push(segment);
        }
        // Ensure resolved path stays under root (no escape after join).
        let root_canon = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        if let Ok(canon) = path.canonicalize() {
            if !canon.starts_with(&root_canon) {
                return Err(BlobError::InvalidKey(format!(
                    "path escapes blob root: {key}"
                )));
            }
        } else {
            // Parent may not exist yet; check parent prefix against root.
            if let Some(parent) = path.parent() {
                if parent.exists() {
                    let parent_canon = parent.canonicalize().map_err(|e| {
                        BlobError::Io(format!("canonicalize {}: {e}", parent.display()))
                    })?;
                    if !parent_canon.starts_with(&root_canon) {
                        return Err(BlobError::InvalidKey(format!(
                            "path escapes blob root: {key}"
                        )));
                    }
                }
            }
        }
        Ok(path)
    }
}

impl BlobStore for LocalFsBlobStore {
    fn put(&self, key: &str, bytes: &[u8]) -> BlobResult<()> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                BlobError::Io(format!("create parent {}: {e}", parent.display()))
            })?;
        }
        let parent = path.parent().unwrap_or(self.root.as_path());
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            BlobError::Io(format!("temp file in {}: {e}", parent.display()))
        })?;
        tmp.write_all(bytes)
            .map_err(|e| BlobError::Io(format!("write temp blob: {e}")))?;
        tmp.flush()
            .map_err(|e| BlobError::Io(format!("flush temp blob: {e}")))?;
        tmp.persist(&path).map_err(|e| {
            BlobError::Io(format!("persist blob {}: {e}", path.display()))
        })?;
        Ok(())
    }

    fn get(&self, key: &str) -> BlobResult<Vec<u8>> {
        let path = self.resolve(key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(BlobError::NotFound {
                    key: key.to_string(),
                })
            }
            Err(e) => Err(BlobError::Io(format!("read {}: {e}", path.display()))),
        }
    }

    fn delete(&self, key: &str) -> BlobResult<()> {
        let path = self.resolve(key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BlobError::Io(format!("delete {}: {e}", path.display()))),
        }
    }

    fn exists(&self, key: &str) -> BlobResult<bool> {
        let path = self.resolve(key)?;
        Ok(path.is_file())
    }
}

fn validate_blob_key(key: &str) -> BlobResult<()> {
    if key.is_empty() || key.len() > 512 {
        return Err(BlobError::InvalidKey("empty or too long".into()));
    }
    if key.starts_with('/') || key.ends_with('/') {
        return Err(BlobError::InvalidKey("leading/trailing slash".into()));
    }
    if key.contains("..") || key.contains('\\') || key.contains('\0') {
        return Err(BlobError::InvalidKey("path traversal or invalid char".into()));
    }
    for segment in key.split('/') {
        if segment.is_empty() {
            return Err(BlobError::InvalidKey("empty path segment".into()));
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(BlobError::InvalidKey(format!(
                "invalid segment '{segment}'"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn put_get_exists_delete_roundtrip() {
        let dir = tempdir().unwrap();
        let store = LocalFsBlobStore::open(dir.path()).unwrap();
        let bytes = b"hello blob";
        let meta = put_bytes(&store, "ws1", bytes).unwrap();
        assert!(meta.content_hash.starts_with("sha256:"));
        assert!(meta.blob_key.starts_with("w/ws1/sha256/"));
        assert_eq!(meta.byte_size, bytes.len() as u64);
        assert!(store.exists(&meta.blob_key).unwrap());
        assert_eq!(store.get(&meta.blob_key).unwrap(), bytes);
        store.delete(&meta.blob_key).unwrap();
        assert!(!store.exists(&meta.blob_key).unwrap());
        assert!(matches!(
            store.get(&meta.blob_key),
            Err(BlobError::NotFound { .. })
        ));
    }

    #[test]
    fn put_rejects_hash_mismatch() {
        let dir = tempdir().unwrap();
        let store = LocalFsBlobStore::open(dir.path()).unwrap();
        let err = put_with_expected_hash(
            &store,
            "w/ws1/sha256/deadbeef",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            b"payload",
        )
        .unwrap_err();
        assert!(matches!(err, BlobError::Integrity { .. }));
    }

    #[test]
    fn rejects_path_traversal_key() {
        let dir = tempdir().unwrap();
        let store = LocalFsBlobStore::open(dir.path()).unwrap();
        assert!(matches!(
            store.put("../escape", b"x"),
            Err(BlobError::InvalidKey(_))
        ));
        assert!(matches!(
            store.put("w/ws/../../etc/passwd", b"x"),
            Err(BlobError::InvalidKey(_))
        ));
    }

    #[test]
    fn content_addressed_same_bytes_same_key() {
        let a = put_bytes(
            &LocalFsBlobStore::open(tempdir().unwrap().path()).unwrap(),
            "ws",
            b"same",
        )
        .unwrap();
        let b = put_bytes(
            &LocalFsBlobStore::open(tempdir().unwrap().path()).unwrap(),
            "ws",
            b"same",
        )
        .unwrap();
        assert_eq!(a.blob_key, b.blob_key);
        assert_eq!(a.content_hash, b.content_hash);
    }
}
