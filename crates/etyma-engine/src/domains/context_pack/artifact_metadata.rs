#[cfg(test)]
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
#[cfg(test)]
use etyma_engine_types::SourceId;
use etyma_engine_types::{
    ContextPackArtifactMetadataV0, ContextPackEvidenceMetadataV0, ContextPackSourceMetadataV0,
    EvidenceIndexItemV0, EvidenceIndexItemV1, EvidenceIndexV0, EvidenceIndexV1, SourcePackV0,
    SourceRecord,
};

use crate::brain_repo::BrainArtifactRepository;

#[cfg(test)]
pub(crate) fn build_context_pack_source_metadata(
    workspace_root: &Path,
    sources: &[SourceRecord],
) -> BTreeMap<SourceId, ContextPackSourceMetadataV0> {
    build_context_pack_artifact_metadata(workspace_root, sources).sources
}

pub(crate) fn build_context_pack_artifact_metadata(
    workspace_root: &Path,
    sources: &[SourceRecord],
) -> ContextPackArtifactMetadataV0 {
    let Ok(canonical_workspace_root) = workspace_root.canonicalize() else {
        return ContextPackArtifactMetadataV0::default();
    };
    let repo = BrainArtifactRepository::new(canonical_workspace_root.clone());
    let mut metadata = ContextPackArtifactMetadataV0::default();

    for source in sources {
        let mut source_pack_read_failed = false;
        let source_pack = match read_source_pack_v0(&repo, &source.source_id) {
            Ok(source_pack) => source_pack,
            Err(_error) => {
                source_pack_read_failed = true;
                metadata.warnings.push(context_artifact_warning(
                    "source_pack_unreadable",
                    format!(
                        "Source Pack for {} could not be read or decoded.",
                        source.source_id
                    ),
                    &source.source_id,
                    None,
                ));
                None
            }
        };
        if source_pack.is_none() && !source_pack_read_failed {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "source_pack_missing",
                    format!(
                        "Source Pack for {} is unavailable; Context Pack v0 may fall back to workspace source bytes.",
                        source.source_id
                    ),
                    &source.source_id,
                    None,
                ),
            );
        }
        let fallback_content = || {
            read_context_pack_source_bytes(
                &canonical_workspace_root,
                [&source.source_path, &source.markdown_path],
            )
        };

        let valid_source_pack = source_pack.as_ref().filter(|pack| {
            pack.schema_version == etyma_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
                && pack.source_id == source.source_id
                && pack.workspace_id == source.workspace_id
        });
        if let Some(pack) = source_pack.as_ref() {
            if pack.schema_version != etyma_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "source_pack_schema_mismatch",
                        format!(
                            "Source Pack for {} was ignored because schemaVersion {} is unsupported.",
                            source.source_id, pack.schema_version
                        ),
                        &source.source_id,
                        None,
                    ),
                );
            }
            if pack.source_id != source.source_id {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "source_pack_source_mismatch",
                        format!(
                            "Source Pack for {} was ignored because it declares sourceId {}.",
                            source.source_id, pack.source_id
                        ),
                        &source.source_id,
                        None,
                    ),
                );
            }
            if pack.workspace_id != source.workspace_id {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "source_pack_workspace_mismatch",
                        format!(
                            "Source Pack for {} was ignored because it declares workspaceId {}.",
                            source.source_id, pack.workspace_id
                        ),
                        &source.source_id,
                        None,
                    ),
                );
            }
        }

        let Some(source_metadata) = valid_source_pack
            .map(|pack| ContextPackSourceMetadataV0 {
                content_hash: pack.content_hash.clone(),
                provider_route: pack.provider_route.clone(),
                local_only: pack.local_only,
            })
            .or_else(|| {
                fallback_content().map(|content| ContextPackSourceMetadataV0 {
                    content_hash: format!("fnv64:{:016x}", fnv1a64(&content)),
                    provider_route: "unknown".into(),
                    local_only: false,
                })
            })
        else {
            continue;
        };

        if let Some(pack) = valid_source_pack {
            for warning in &pack.warnings {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning_from_source_warning(warning, &source.source_id),
                );
            }
        }

        metadata
            .sources
            .insert(source.source_id.clone(), source_metadata.clone());

        let evidence_index = match read_evidence_index_artifact(&repo, &source.source_id) {
            Ok(evidence_index) => evidence_index,
            Err(_error) => {
                metadata.warnings.push(context_artifact_warning(
                    "evidence_index_unreadable",
                    format!(
                        "Evidence Index for {} could not be read or decoded.",
                        source.source_id
                    ),
                    &source.source_id,
                    None,
                ));
                None
            }
        };

        let Some(evidence_index) = evidence_index else {
            if valid_source_pack.is_some() {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "evidence_index_missing",
                        format!(
                            "Evidence Index for {} is unavailable; Context Pack v0 may fall back to internal evidence refs.",
                            source.source_id
                        ),
                        &source.source_id,
                        None,
                    ),
                );
            }
            continue;
        };

        if evidence_index.schema_version()
            != etyma_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION
            && evidence_index.schema_version()
                != etyma_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION
        {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "evidence_index_schema_mismatch",
                    format!(
                        "Evidence Index for {} was ignored because schemaVersion {} is unsupported.",
                        source.source_id,
                        evidence_index.schema_version()
                    ),
                    &source.source_id,
                    None,
                ),
            );
            continue;
        }
        if evidence_index.source_id() != Some(source.source_id.as_str()) {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "evidence_index_source_mismatch",
                    format!(
                        "Evidence Index for {} was ignored because it declares sourceId {}.",
                        source.source_id,
                        evidence_index.source_id().unwrap_or("<missing>")
                    ),
                    &source.source_id,
                    None,
                ),
            );
            continue;
        }
        if evidence_index.workspace_id() != Some(source.workspace_id.as_str()) {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "evidence_index_workspace_mismatch",
                    format!(
                        "Evidence Index for {} was ignored because it declares workspaceId {}.",
                        source.source_id,
                        evidence_index.workspace_id().unwrap_or("<missing>")
                    ),
                    &source.source_id,
                    None,
                ),
            );
            continue;
        }
        if valid_source_pack.is_some()
            && (evidence_index.provider_route() != Some(source_metadata.provider_route.as_str())
                || evidence_index.local_only() != Some(source_metadata.local_only))
        {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "evidence_index_provider_mismatch",
                    format!(
                        "Evidence Index for {} was ignored because provider metadata does not match the selected Source Pack.",
                        source.source_id
                    ),
                    &source.source_id,
                    None,
                ),
            );
            continue;
        }
        if evidence_index.content_hash() != Some(source_metadata.content_hash.as_str()) {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning(
                    "evidence_index_stale_content_hash",
                    format!(
                        "Evidence Index for {} was ignored because contentHash {} does not match selected source hash {}.",
                        source.source_id,
                        evidence_index.content_hash().unwrap_or("<missing>"),
                        source_metadata.content_hash
                    ),
                    &source.source_id,
                    None,
                ),
            );
            continue;
        }

        for warning in evidence_index.warnings() {
            push_context_artifact_warning_once(
                &mut metadata.warnings,
                context_artifact_warning_from_source_warning(warning, &source.source_id),
            );
        }

        let source_evidence = metadata
            .evidence
            .entry(source.source_id.clone())
            .or_default();
        for evidence in evidence_index.evidence_items() {
            if evidence.source_id != source.source_id {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "evidence_item_source_mismatch",
                        format!(
                            "Evidence {} was ignored because it declares sourceId {}.",
                            evidence.evidence_ref, evidence.source_id
                        ),
                        &source.source_id,
                        Some(evidence.page),
                    ),
                );
                continue;
            }
            if evidence.content_hash != source_metadata.content_hash {
                push_context_artifact_warning_once(
                    &mut metadata.warnings,
                    context_artifact_warning(
                        "evidence_item_stale_content_hash",
                        format!(
                            "Evidence {} was ignored because contentHash {} does not match selected source hash {}.",
                            evidence.evidence_ref, evidence.content_hash, source_metadata.content_hash
                        ),
                        &source.source_id,
                        Some(evidence.page),
                    ),
                );
                continue;
            }
            source_evidence.insert(
                evidence.evidence_ref.clone(),
                ContextPackEvidenceMetadataV0 {
                    source_id: evidence.source_id,
                    page: evidence.page,
                    region: Some(evidence.region),
                    span: evidence.span,
                    quoted_text: evidence.quoted_text,
                    parse_confidence: evidence.parse_confidence,
                    content_hash: evidence.content_hash,
                    markdown_path: evidence.markdown_path,
                    image_path: evidence.image_path,
                    evidence_type: evidence.evidence_type,
                },
            );
        }
    }

    metadata
}

pub(crate) fn read_source_pack_v0(
    repo: &BrainArtifactRepository,
    source_id: &str,
) -> Result<Option<SourcePackV0>> {
    read_optional_source_artifact(repo, source_id, "source_pack.json")
}

#[allow(dead_code)]
pub(crate) fn read_evidence_index_v0(
    repo: &BrainArtifactRepository,
    source_id: &str,
) -> Result<Option<EvidenceIndexV0>> {
    read_optional_source_artifact(repo, source_id, "evidence_index.json")
}

pub(crate) enum EvidenceIndexArtifact {
    V0(EvidenceIndexV0),
    V1(EvidenceIndexV1),
    Unsupported { schema_version: String },
}

impl EvidenceIndexArtifact {
    pub(crate) fn schema_version(&self) -> &str {
        match self {
            Self::V0(index) => &index.schema_version,
            Self::V1(index) => &index.schema_version,
            Self::Unsupported { schema_version } => schema_version,
        }
    }

    pub(crate) fn workspace_id(&self) -> Option<&str> {
        match self {
            Self::V0(index) => Some(&index.workspace_id),
            Self::V1(index) => Some(&index.workspace_id),
            Self::Unsupported { .. } => None,
        }
    }

    pub(crate) fn source_id(&self) -> Option<&str> {
        match self {
            Self::V0(index) => Some(&index.source_id),
            Self::V1(index) => Some(&index.source_id),
            Self::Unsupported { .. } => None,
        }
    }

    pub(crate) fn content_hash(&self) -> Option<&str> {
        match self {
            Self::V0(index) => Some(&index.content_hash),
            Self::V1(index) => Some(&index.content_hash),
            Self::Unsupported { .. } => None,
        }
    }

    pub(crate) fn provider_route(&self) -> Option<&str> {
        match self {
            Self::V0(index) => Some(&index.provider_route),
            Self::V1(index) => Some(&index.provider_route),
            Self::Unsupported { .. } => None,
        }
    }

    pub(crate) fn local_only(&self) -> Option<bool> {
        match self {
            Self::V0(index) => Some(index.local_only),
            Self::V1(index) => Some(index.local_only),
            Self::Unsupported { .. } => None,
        }
    }

    fn warnings(&self) -> &[etyma_engine_types::SourcePackWarningV0] {
        match self {
            Self::V0(index) => &index.warnings,
            Self::V1(index) => &index.warnings,
            Self::Unsupported { .. } => &[],
        }
    }

    fn evidence_items(&self) -> Vec<EvidenceIndexItemForContext> {
        match self {
            Self::V0(index) => index
                .evidence
                .iter()
                .map(EvidenceIndexItemForContext::from_v0)
                .collect(),
            Self::V1(index) => index
                .evidence
                .iter()
                .map(EvidenceIndexItemForContext::from_v1)
                .collect(),
            Self::Unsupported { .. } => Vec::new(),
        }
    }
}

struct EvidenceIndexItemForContext {
    evidence_ref: String,
    source_id: String,
    page: usize,
    region: String,
    span: Option<String>,
    quoted_text: String,
    parse_confidence: etyma_engine_types::ContextPackParseConfidence,
    content_hash: String,
    markdown_path: Option<String>,
    image_path: Option<String>,
    evidence_type: etyma_engine_types::EvidenceType,
}

impl EvidenceIndexItemForContext {
    fn from_v0(item: &EvidenceIndexItemV0) -> Self {
        Self {
            evidence_ref: item.evidence_ref.clone(),
            source_id: item.source_id.clone(),
            page: item.page,
            region: item.region.clone(),
            span: item.span.clone(),
            quoted_text: item.quoted_text.clone(),
            parse_confidence: item.parse_confidence.clone(),
            content_hash: item.content_hash.clone(),
            markdown_path: item.markdown_path.clone(),
            image_path: item.image_path.clone(),
            evidence_type: etyma_engine_types::EvidenceType::legacy_default(),
        }
    }

    fn from_v1(item: &EvidenceIndexItemV1) -> Self {
        Self {
            evidence_ref: item.evidence_ref.clone(),
            source_id: item.source_id.clone(),
            page: item.page,
            region: item.region.clone(),
            span: item.span.clone(),
            quoted_text: item.quoted_text.clone(),
            parse_confidence: item.parse_confidence.clone(),
            content_hash: item.content_hash.clone(),
            markdown_path: item.markdown_path.clone(),
            image_path: item.image_path.clone(),
            evidence_type: item.evidence_type,
        }
    }
}

pub(crate) fn read_evidence_index_artifact(
    repo: &BrainArtifactRepository,
    source_id: &str,
) -> Result<Option<EvidenceIndexArtifact>> {
    let Some(path) = source_artifact_relative_path(source_id, "evidence_index.json") else {
        return Ok(None);
    };
    if !repo.root().join(&path).exists() {
        return Ok(None);
    }

    let value: serde_json::Value = repo.read_json_artifact(&path)?;
    let schema_version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if schema_version == etyma_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION {
        return Ok(Some(EvidenceIndexArtifact::V1(serde_json::from_value(
            value,
        )?)));
    }
    if schema_version != etyma_engine_types::EVIDENCE_INDEX_V0_SCHEMA_VERSION {
        return Ok(Some(EvidenceIndexArtifact::Unsupported {
            schema_version: if schema_version.is_empty() {
                "<missing>".into()
            } else {
                schema_version.into()
            },
        }));
    }

    Ok(Some(EvidenceIndexArtifact::V0(serde_json::from_value(
        value,
    )?)))
}

fn context_artifact_warning(
    warning_type: impl Into<String>,
    message: impl Into<String>,
    source_id: &str,
    page: Option<usize>,
) -> etyma_engine_types::ContextPackWarningV0 {
    etyma_engine_types::ContextPackWarningV0 {
        warning_type: warning_type.into(),
        severity: etyma_engine_types::ContextPackWarningSeverity::High,
        message: message.into(),
        page_refs: page.map_or_else(Vec::new, |page| {
            vec![etyma_engine_types::ContextPackPageRefV0 {
                source_id: source_id.to_string(),
                page,
            }]
        }),
    }
}

fn context_artifact_warning_from_source_warning(
    warning: &etyma_engine_types::SourcePackWarningV0,
    source_id: &str,
) -> etyma_engine_types::ContextPackWarningV0 {
    etyma_engine_types::ContextPackWarningV0 {
        warning_type: warning.warning_type.clone(),
        severity: warning.severity.clone(),
        message: warning.message.clone(),
        page_refs: warning.page.map_or_else(Vec::new, |page| {
            vec![etyma_engine_types::ContextPackPageRefV0 {
                source_id: source_id.to_string(),
                page,
            }]
        }),
    }
}

fn push_context_artifact_warning_once(
    warnings: &mut Vec<etyma_engine_types::ContextPackWarningV0>,
    warning: etyma_engine_types::ContextPackWarningV0,
) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

fn read_optional_source_artifact<T: serde::de::DeserializeOwned>(
    repo: &BrainArtifactRepository,
    source_id: &str,
    filename: &str,
) -> Result<Option<T>> {
    let Some(path) = source_artifact_relative_path(source_id, filename) else {
        return Ok(None);
    };
    if !repo.root().join(&path).exists() {
        return Ok(None);
    }
    match repo.read_json_artifact(&path) {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.to_string().contains("No such file")
                || error.to_string().contains("not found") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn source_artifact_relative_path(source_id: &str, filename: &str) -> Option<String> {
    let source_id_path = Path::new(source_id);
    if source_id_path.components().count() != 1
        || !source_id_path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(format!("artifacts/{source_id}/{filename}"))
}

fn read_context_pack_source_bytes<'a>(
    canonical_workspace_root: &Path,
    candidate_paths: impl IntoIterator<Item = &'a String>,
) -> Option<Vec<u8>> {
    for candidate in candidate_paths {
        let path = Path::new(candidate);
        if !path.exists() {
            continue;
        }
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        if !canonical_path.starts_with(canonical_workspace_root) {
            continue;
        }
        if let Ok(content) = fs::read(&canonical_path) {
            return Some(content);
        }
    }
    None
}

pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
