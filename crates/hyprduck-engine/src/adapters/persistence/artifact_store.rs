//! Internal helpers extracted from the engine facade module.

use anyhow::{Context, Result};
use graphqlite::Graph;
use hyprduck_engine_types::{
    ContextPackArtifactMetadataV0, ContextPackParseConfidence, EvidenceType,
};
use std::fs;
use std::path::{Path, PathBuf};

use crate::policy::redact_path_for_agent;

pub(super) fn preserve_artifact_metadata_in_transaction(
    graph: &Graph,
    metadata: &ContextPackArtifactMetadataV0,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for (source_id, source_metadata) in &metadata.sources {
        let source_warnings = metadata
            .warnings
            .iter()
            .filter(|warning| {
                warning
                    .page_refs
                    .iter()
                    .any(|page_ref| page_ref.source_id == *source_id)
            })
            .collect::<Vec<_>>();
        let warnings_json = if source_warnings.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&source_warnings)
                    .context("failed encoding artifact warnings")?,
            )
        };
        sqlite
            .execute(
                "UPDATE sources
                 SET provider_route = ?2,
                     provider_locality = ?3,
                     content_hash = ?4,
                     parse_warnings_json = COALESCE(?5, parse_warnings_json),
                     updated_at = unixepoch()
                 WHERE source_id = ?1",
                (
                    source_id.as_str(),
                    source_metadata.provider_route.as_str(),
                    if source_metadata.local_only {
                        "local"
                    } else {
                        "hosted"
                    },
                    source_metadata.content_hash.as_str(),
                    warnings_json.as_deref(),
                ),
            )
            .with_context(|| format!("failed preserving source metadata for {source_id}"))?;
    }

    for (source_id, evidence_by_ref) in &metadata.evidence {
        for (evidence_id, evidence_metadata) in evidence_by_ref {
            let evidence_type = db_evidence_type(evidence_metadata.evidence_type);
            let span_json = optional_string_json("span", evidence_metadata.span.as_deref())?;
            let region_json = optional_string_json("region", evidence_metadata.region.as_deref())?;
            let confidence = parse_confidence_score(&evidence_metadata.parse_confidence);
            let markdown_path_redacted = evidence_metadata
                .markdown_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
            let image_path_redacted = evidence_metadata
                .image_path
                .as_deref()
                .map(redact_path_for_agent)
                .unwrap_or_default();
            sqlite
                .execute(
                    "UPDATE evidence_items
                     SET evidence_type = ?3,
                         snippet = ?4,
                         markdown_path_redacted = ?5,
                         image_path_redacted = ?6,
                         span_json = ?7,
                         region_json = ?8,
                         confidence = ?9
                     WHERE source_id = ?1 AND evidence_id = ?2",
                    (
                        source_id.as_str(),
                        evidence_id.as_str(),
                        evidence_type.as_str(),
                        evidence_metadata.quoted_text.as_str(),
                        markdown_path_redacted.as_str(),
                        image_path_redacted.as_str(),
                        span_json.as_str(),
                        region_json.as_str(),
                        confidence,
                    ),
                )
                .with_context(|| {
                    format!("failed preserving evidence metadata for {evidence_id}")
                })?;
            sqlite
                .execute(
                    "UPDATE evidence_fts
                     SET evidence_type = ?3,
                         text = ?4
                     WHERE source_id = ?1 AND evidence_id = ?2",
                    (
                        source_id.as_str(),
                        evidence_id.as_str(),
                        evidence_type.as_str(),
                        evidence_metadata.quoted_text.as_str(),
                    ),
                )
                .with_context(|| {
                    format!("failed preserving evidence FTS metadata for {evidence_id}")
                })?;
        }
    }

    Ok(())
}

pub(super) fn preserve_context_pack_exports_in_transaction(
    graph: &Graph,
    workspace_root: &Path,
    workspace_id: &str,
) -> Result<()> {
    let sqlite = graph.connection().sqlite_connection();
    for export in discover_context_pack_exports(workspace_root)? {
        let payload = fs::read_to_string(&export.path)
            .with_context(|| format!("failed reading context pack {}", export.path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&payload)
            .with_context(|| format!("failed decoding context pack {}", export.path.display()))?;
        let export_workspace_id = value
            .get("workspaceId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if export_workspace_id != workspace_id {
            continue;
        }
        let Some(pack_id) = value.get("packId").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let schema_version = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let query = value
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let generated_at = value
            .get("generatedAt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        sqlite
            .execute(
                "INSERT INTO context_pack_exports (
                    pack_id,
                    workspace_id,
                    query,
                    export_path,
                    schema_version,
                    payload_json,
                    generated_at,
                    is_latest,
                    preserved_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())
                 ON CONFLICT(pack_id, export_path) DO UPDATE SET
                    workspace_id=excluded.workspace_id,
                    query=excluded.query,
                    schema_version=excluded.schema_version,
                    payload_json=excluded.payload_json,
                    generated_at=excluded.generated_at,
                    is_latest=excluded.is_latest,
                    preserved_at=excluded.preserved_at",
                (
                    pack_id,
                    workspace_id,
                    query,
                    export.relative_path.as_str(),
                    schema_version,
                    payload.as_str(),
                    generated_at,
                    if export.is_latest { 1 } else { 0 },
                ),
            )
            .with_context(|| format!("failed preserving context pack export {pack_id}"))?;
    }

    Ok(())
}

#[derive(Debug)]
struct ContextPackExportCandidate {
    path: PathBuf,
    relative_path: String,
    is_latest: bool,
}

fn discover_context_pack_exports(workspace_root: &Path) -> Result<Vec<ContextPackExportCandidate>> {
    let mut exports = Vec::new();
    let latest_path = workspace_root.join("context_pack.json");
    if latest_path.exists() {
        exports.push(ContextPackExportCandidate {
            path: latest_path,
            relative_path: "context_pack.json".into(),
            is_latest: true,
        });
    }

    let history_dir = workspace_root.join("context_packs");
    if history_dir.exists() {
        for entry in fs::read_dir(&history_dir)
            .with_context(|| format!("failed reading {}", history_dir.display()))?
        {
            let entry = entry.context("failed reading context pack history entry")?;
            let file_type = entry
                .file_type()
                .context("failed reading context pack history file type")?;
            if !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Some(file_name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            exports.push(ContextPackExportCandidate {
                path,
                relative_path: format!("context_packs/{}", file_name),
                is_latest: false,
            });
        }
    }

    Ok(exports)
}

fn db_evidence_type(evidence_type: EvidenceType) -> String {
    format!("{}_evidence", evidence_type.as_trace_key())
}

fn parse_confidence_score(confidence: &ContextPackParseConfidence) -> Option<f64> {
    match confidence {
        ContextPackParseConfidence::High => Some(1.0),
        ContextPackParseConfidence::Medium => Some(0.66),
        ContextPackParseConfidence::Low => Some(0.33),
        ContextPackParseConfidence::Unknown => None,
    }
}

fn optional_string_json(key: &str, value: Option<&str>) -> Result<String> {
    let value = value
        .map(|value| serde_json::json!({ key: value }))
        .unwrap_or_else(|| serde_json::json!({}));
    serde_json::to_string(&value).context("failed encoding optional metadata JSON")
}
