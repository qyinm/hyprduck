use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use duckdocs_engine_types::SourceArtifactManifest;
use serde::{Deserialize, Serialize};

const SOURCE_INDEX_DIR: &str = "source-index";
const SOURCE_CHUNKS_PATH: &str = "source-index/source-chunks.jsonl";
const SOURCE_CHUNKS_MANIFEST_PATH: &str = "source-index/source-chunks-manifest.json";
const MAX_CHUNK_LINES: usize = 80;
const MAX_CHUNK_CHARS: usize = 6_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceChunk {
    pub chunk_id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub source_path: String,
    pub markdown_path: String,
    pub source_title: String,
    pub heading_path: Vec<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub text_hash: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIndexManifest {
    pub schema_version: u32,
    pub updated_at: u64,
    pub source_count: usize,
    pub chunk_count: usize,
    pub sources: Vec<SourceIndexManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIndexManifestEntry {
    pub workspace_id: String,
    pub source_id: String,
    pub source_title: String,
    pub chunk_count: usize,
    pub updated_at: u64,
}

pub fn chunk_source_markdown(
    manifest: &SourceArtifactManifest,
    markdown: &str,
) -> Vec<SourceChunk> {
    let mut chunks = Vec::new();
    let mut heading_path = Vec::new();
    let mut buffer = Vec::new();
    let mut chunk_start = 1usize;

    for (line_index, line) in markdown.lines().enumerate() {
        let line_number = line_index + 1;
        if let Some((level, heading)) = markdown_heading(line) {
            flush_chunk(
                manifest,
                &mut chunks,
                &heading_path,
                &mut buffer,
                chunk_start,
                line_number.saturating_sub(1),
            );
            heading_path.truncate(level.saturating_sub(1));
            heading_path.push(heading);
            chunk_start = line_number;
        }

        if buffer.is_empty() {
            chunk_start = line_number;
        }
        buffer.push(line.to_string());

        let char_count = buffer.iter().map(|entry| entry.len()).sum::<usize>();
        if buffer.len() >= MAX_CHUNK_LINES || char_count >= MAX_CHUNK_CHARS {
            flush_chunk(
                manifest,
                &mut chunks,
                &heading_path,
                &mut buffer,
                chunk_start,
                line_number,
            );
            chunk_start = line_number + 1;
        }
    }

    flush_chunk(
        manifest,
        &mut chunks,
        &heading_path,
        &mut buffer,
        chunk_start,
        markdown.lines().count(),
    );
    chunks
}

pub fn upsert_source_chunks(
    workspace_root: &Path,
    manifest: &SourceArtifactManifest,
    chunks: &[SourceChunk],
) -> Result<()> {
    let index_dir = workspace_root.join(SOURCE_INDEX_DIR);
    fs::create_dir_all(&index_dir)
        .with_context(|| format!("failed creating {}", index_dir.display()))?;

    let path = workspace_root.join(SOURCE_CHUNKS_PATH);
    let mut existing = read_source_chunks(&path)?;
    existing.retain(|chunk| chunk.source_id != manifest.source_id);
    existing.extend(chunks.iter().cloned());
    existing.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then(left.line_start.cmp(&right.line_start))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });

    let encoded = existing
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed encoding source chunks")?
        .join("\n");
    write_file_atomic(&path, format!("{encoded}\n").as_bytes())?;
    write_source_index_manifest(workspace_root, &existing)
}

pub fn read_source_chunks(path: &Path) -> Result<Vec<SourceChunk>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("failed decoding source chunk"))
        .collect()
}

fn flush_chunk(
    manifest: &SourceArtifactManifest,
    chunks: &mut Vec<SourceChunk>,
    heading_path: &[String],
    buffer: &mut Vec<String>,
    line_start: usize,
    line_end: usize,
) {
    let text = buffer.join("\n").trim().to_string();
    buffer.clear();
    if text.is_empty() {
        return;
    }

    let text_hash = format!("{:016x}", fnv1a_hash(text.as_bytes()));
    let chunk_id = format!(
        "chunk-{}-{:04}-{}",
        sanitize_chunk_id(&manifest.source_id),
        chunks.len() + 1,
        text_hash
    );
    chunks.push(SourceChunk {
        chunk_id,
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        source_title: source_title(manifest),
        heading_path: heading_path.to_vec(),
        line_start,
        line_end: line_end.max(line_start),
        text_hash,
        text,
    });
}

fn markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|char| *char == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = trimmed[level..].trim();
    if rest.is_empty() {
        return None;
    }
    Some((level, rest.trim_matches('#').trim().to_string()))
}

fn write_source_index_manifest(workspace_root: &Path, chunks: &[SourceChunk]) -> Result<()> {
    let mut by_source = BTreeMap::<String, SourceIndexManifestEntry>::new();
    for chunk in chunks {
        let entry =
            by_source
                .entry(chunk.source_id.clone())
                .or_insert_with(|| SourceIndexManifestEntry {
                    workspace_id: chunk.workspace_id.clone(),
                    source_id: chunk.source_id.clone(),
                    source_title: chunk.source_title.clone(),
                    chunk_count: 0,
                    updated_at: 0,
                });
        entry.chunk_count += 1;
    }
    let manifest = SourceIndexManifest {
        schema_version: 1,
        updated_at: unix_timestamp_seconds(),
        source_count: by_source.len(),
        chunk_count: chunks.len(),
        sources: by_source.into_values().collect(),
    };
    let path = workspace_root.join(SOURCE_CHUNKS_MANIFEST_PATH);
    let bytes = serde_json::to_vec_pretty(&manifest).context("failed encoding source index")?;
    write_file_atomic(&path, &bytes)
}

fn source_title(manifest: &SourceArtifactManifest) -> String {
    if !manifest.output_name.trim().is_empty() {
        return manifest.output_name.clone();
    }
    Path::new(&manifest.source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&manifest.source_id)
        .to_string()
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating {}", parent.display()))?;
    }
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, bytes)
        .with_context(|| format!("failed writing {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed replacing {} with {}",
            path.display(),
            tmp_path.display()
        )
    })
}

fn sanitize_chunk_id(value: &str) -> String {
    value
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
                char
            } else {
                '-'
            }
        })
        .collect()
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn unix_timestamp_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdocs_engine_types::{DocumentFormat, IngestStatus};
    use tempfile::tempdir;

    #[test]
    fn chunks_markdown_by_heading_and_line_window() {
        let manifest = test_manifest();
        let markdown =
            "# Sparse Matrix\n\nUse triples.\n\n## Transposing a Matrix\nrowTerms\nstartingPos\n";

        let chunks = chunk_source_markdown(&manifest, markdown);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path, vec!["Sparse Matrix"]);
        assert_eq!(
            chunks[1].heading_path,
            vec!["Sparse Matrix", "Transposing a Matrix"]
        );
        assert!(chunks[1].text.contains("rowTerms"));
        assert!(chunks[1].text.contains("startingPos"));
    }

    #[test]
    fn upsert_source_chunks_is_idempotent_by_source_id() {
        let dir = tempdir().unwrap();
        let manifest = test_manifest();
        let first = chunk_source_markdown(&manifest, "# One\nrowTerms");
        let second = chunk_source_markdown(&manifest, "# Two\nstartingPos");

        upsert_source_chunks(dir.path(), &manifest, &first).unwrap();
        upsert_source_chunks(dir.path(), &manifest, &second).unwrap();

        let chunks = read_source_chunks(&dir.path().join(SOURCE_CHUNKS_PATH)).unwrap();
        assert_eq!(chunks.len(), second.len());
        assert!(chunks
            .iter()
            .any(|chunk| chunk.text.contains("startingPos")));
        assert!(!chunks.iter().any(|chunk| chunk.text.contains("rowTerms")));
    }

    fn test_manifest() -> SourceArtifactManifest {
        SourceArtifactManifest {
            workspace_id: "default".into(),
            source_id: "source-test".into(),
            original_path: "/tmp/source.pdf".into(),
            source_path: "/tmp/source.pdf".into(),
            markdown_path: "/tmp/source.md".into(),
            artifact_root: "/tmp/artifact".into(),
            manifest_path: "/tmp/artifact/source-manifest.json".into(),
            format: DocumentFormat::Pdf,
            output_name: "source".into(),
            status: IngestStatus::Ingested,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            pages: Vec::new(),
            created_at: 1,
            updated_at: 1,
        }
    }
}
