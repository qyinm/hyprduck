use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use hyprduck_knowledge::{EvidenceRef, SourceRecord, SourceStatus};

use crate::{BrainContextPack, DocumentFormat, IngestStatus, SourceId, WorkspaceId};

pub const CONTEXT_PACK_V0_SCHEMA_VERSION: &str = "hyprduck.context_pack.v0";
pub const CONTEXT_PACK_V1_SCHEMA_VERSION: &str = "hyprduck.context_pack.v1";
pub const SOURCE_PACK_V0_SCHEMA_VERSION: &str = "hyprduck.source_pack.v0";
pub const EVIDENCE_INDEX_V0_SCHEMA_VERSION: &str = "hyprduck.evidence_index.v0";
pub const EVIDENCE_INDEX_V1_SCHEMA_VERSION: &str = "hyprduck.evidence_index.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    Text,
    Table,
    ImageRegion,
    Ocr,
    Caption,
    Summary,
    Claim,
    Relationship,
    Unknown,
}

impl EvidenceType {
    pub fn legacy_default() -> Self {
        Self::Text
    }

    pub fn as_trace_key(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Table => "table",
            Self::ImageRegion => "image_region",
            Self::Ocr => "ocr",
            Self::Caption => "caption",
            Self::Summary => "summary",
            Self::Claim => "claim",
            Self::Relationship => "relationship",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackStaleness {
    Current,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackParseConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackFindingStatus {
    DerivedSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackWarningSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSourceV0 {
    pub source_id: SourceId,
    pub original_filename: String,
    pub content_hash: String,
    pub page_count: usize,
    pub ingestion_status: String,
    pub staleness: ContextPackStaleness,
    pub provider_route: String,
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackEvidenceV0 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub selection_reason: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackEvidenceV1 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub selection_reason: String,
    pub content_hash: String,
    pub evidence_type: EvidenceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_trail: Option<ContextPackGraphTrailV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackGraphRecordKindV1 {
    Node,
    Relation,
    Claim,
    WikiPage,
    Source,
    Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackGraphRecordV1 {
    #[serde(rename = "type")]
    pub record_type: ContextPackGraphRecordKindV1,
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackGraphFollowUpToolV1 {
    ReadNode,
    ReadSource,
    ReadPageEvidence,
    ReadWikiPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPackGraphHandleTypeV1 {
    Node,
    Source,
    PageEvidence,
    WikiPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPackGraphReadNodeArgumentsV1 {
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPackGraphReadSourceArgumentsV1 {
    pub source_id: SourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPackGraphReadPageEvidenceArgumentsV1 {
    pub source_id: SourceId,
    pub page: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextPackGraphReadWikiPageArgumentsV1 {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContextPackGraphFollowUpArgumentsV1 {
    ReadNode(ContextPackGraphReadNodeArgumentsV1),
    ReadSource(ContextPackGraphReadSourceArgumentsV1),
    ReadPageEvidence(ContextPackGraphReadPageEvidenceArgumentsV1),
    ReadWikiPage(ContextPackGraphReadWikiPageArgumentsV1),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackGraphFollowUpV1 {
    pub tool: ContextPackGraphFollowUpToolV1,
    pub handle_type: ContextPackGraphHandleTypeV1,
    pub arguments: ContextPackGraphFollowUpArgumentsV1,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackGraphTrailV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct: Vec<ContextPackGraphRecordV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjacent: Vec<ContextPackGraphRecordV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up: Vec<ContextPackGraphFollowUpV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackFindingV0 {
    pub finding_id: String,
    pub statement: String,
    pub status: ContextPackFindingStatus,
    pub statement_confidence: ContextPackParseConfidence,
    pub derived_from: Vec<String>,
    pub relevance_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackPageRefV0 {
    pub source_id: SourceId,
    pub page: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackWarningV0 {
    #[serde(rename = "type")]
    pub warning_type: String,
    pub severity: ContextPackWarningSeverity,
    pub message: String,
    #[serde(default)]
    pub page_refs: Vec<ContextPackPageRefV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackRetrievalTraceV0 {
    pub strategy: String,
    pub chunks_considered: usize,
    pub chunks_selected: usize,
    pub budget_requested: usize,
    pub budget_used: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackEvidenceTypeTraceV1 {
    #[serde(default)]
    pub considered: BTreeMap<String, usize>,
    #[serde(default)]
    pub selected: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackRetrievalTraceV1 {
    pub strategy: String,
    pub chunks_considered: usize,
    pub chunks_selected: usize,
    pub budget_requested: usize,
    pub budget_used: usize,
    pub evidence_type_trace: ContextPackEvidenceTypeTraceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSuggestedNextReadV0 {
    pub source_id: SourceId,
    pub page: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackPageV0 {
    pub page: usize,
    pub label: String,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub plain_text_path: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackWarningV0 {
    #[serde(rename = "type")]
    pub warning_type: String,
    pub severity: ContextPackWarningSeverity,
    pub message: String,
    #[serde(default)]
    pub page: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePackV0 {
    pub schema_version: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub original_filename: String,
    pub original_path: String,
    pub source_path: String,
    pub markdown_path: String,
    pub artifact_root: String,
    pub content_hash: String,
    pub format: DocumentFormat,
    pub page_count: usize,
    pub ingestion_status: IngestStatus,
    pub provider_route: String,
    pub local_only: bool,
    pub pages: Vec<SourcePackPageV0>,
    pub warnings: Vec<SourcePackWarningV0>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndexItemV0 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    pub region: String,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub content_hash: String,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndexV0 {
    pub schema_version: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub content_hash: String,
    pub provider_route: String,
    pub local_only: bool,
    pub evidence: Vec<EvidenceIndexItemV0>,
    pub warnings: Vec<SourcePackWarningV0>,
    pub generated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndexItemV1 {
    pub evidence_ref: String,
    pub source_id: SourceId,
    pub page: usize,
    pub region: String,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub content_hash: String,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    pub evidence_type: EvidenceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceIndexV1 {
    pub schema_version: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub content_hash: String,
    pub provider_route: String,
    pub local_only: bool,
    pub evidence: Vec<EvidenceIndexItemV1>,
    pub warnings: Vec<SourcePackWarningV0>,
    pub generated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackV0 {
    pub schema_version: String,
    pub pack_id: String,
    pub workspace_id: WorkspaceId,
    pub query: String,
    pub generated_at: String,
    pub source_set: Vec<ContextPackSourceV0>,
    pub selected_evidence: Vec<ContextPackEvidenceV0>,
    pub findings: Vec<ContextPackFindingV0>,
    pub warnings: Vec<ContextPackWarningV0>,
    pub retrieval_trace: ContextPackRetrievalTraceV0,
    pub suggested_next_reads: Vec<ContextPackSuggestedNextReadV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackV1 {
    pub schema_version: String,
    pub pack_id: String,
    pub workspace_id: WorkspaceId,
    pub query: String,
    pub generated_at: String,
    pub source_set: Vec<ContextPackSourceV0>,
    pub selected_evidence: Vec<ContextPackEvidenceV1>,
    pub findings: Vec<ContextPackFindingV0>,
    pub warnings: Vec<ContextPackWarningV0>,
    pub retrieval_trace: ContextPackRetrievalTraceV1,
    pub suggested_next_reads: Vec<ContextPackSuggestedNextReadV0>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackSourceMetadataV0 {
    pub content_hash: String,
    pub provider_route: String,
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackEvidenceMetadataV0 {
    pub source_id: SourceId,
    pub page: usize,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub span: Option<String>,
    pub quoted_text: String,
    pub parse_confidence: ContextPackParseConfidence,
    pub content_hash: String,
    #[serde(default)]
    pub markdown_path: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default = "EvidenceType::legacy_default")]
    pub evidence_type: EvidenceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackArtifactMetadataV0 {
    #[serde(default)]
    pub sources: BTreeMap<SourceId, ContextPackSourceMetadataV0>,
    #[serde(default)]
    pub evidence: BTreeMap<SourceId, BTreeMap<String, ContextPackEvidenceMetadataV0>>,
    #[serde(default)]
    pub warnings: Vec<ContextPackWarningV0>,
}

impl ContextPackArtifactMetadataV0 {
    pub fn from_sources(sources: BTreeMap<SourceId, ContextPackSourceMetadataV0>) -> Self {
        Self {
            sources,
            evidence: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

impl ContextPackV0 {
    pub fn from_brain_context_pack(
        pack: &BrainContextPack,
        pack_id: impl Into<String>,
        generated_at: impl Into<String>,
        artifact_metadata: &ContextPackArtifactMetadataV0,
    ) -> Self {
        let source_metadata = &artifact_metadata.sources;
        let source_set = pack
            .sources
            .iter()
            .filter(|source| source_metadata.contains_key(&source.source_id))
            .map(|source| ContextPackSourceV0::from_source_record(source, source_metadata))
            .collect::<Vec<_>>();
        let source_set_ids = source_set
            .iter()
            .map(|source| source.source_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let selected_evidence_pairs = pack
            .evidence
            .iter()
            .filter_map(|evidence| {
                ContextPackEvidenceV0::from_evidence_ref(
                    evidence,
                    &artifact_metadata.evidence,
                    &source_set_ids,
                )
                .map(|selected| (evidence.id.clone(), selected))
            })
            .collect::<Vec<_>>();
        let selected_evidence = selected_evidence_pairs
            .iter()
            .map(|(_internal_id, evidence)| evidence.clone())
            .collect::<Vec<_>>();
        let selected_evidence_by_internal_id = selected_evidence_pairs
            .iter()
            .map(|(internal_id, evidence)| (internal_id.clone(), evidence.evidence_ref.clone()))
            .collect::<BTreeMap<_, _>>();
        let selected_evidence_ids = selected_evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let findings = pack
            .claims
            .iter()
            .filter_map(|claim| {
                let derived_from = claim
                    .evidence_refs
                    .iter()
                    .filter_map(|evidence_ref| {
                        if selected_evidence_ids.contains(evidence_ref) {
                            Some(evidence_ref.clone())
                        } else {
                            selected_evidence_by_internal_id.get(evidence_ref).cloned()
                        }
                    })
                    .collect::<Vec<_>>();
                if derived_from.is_empty() {
                    return None;
                }
                Some(ContextPackFindingV0 {
                    finding_id: claim.claim_id.clone(),
                    statement: claim.statement.clone(),
                    status: ContextPackFindingStatus::DerivedSummary,
                    statement_confidence: ContextPackParseConfidence::Unknown,
                    derived_from,
                    relevance_reason: "Selected from the internal context pack for this query."
                        .into(),
                })
            })
            .collect();

        let omitted_evidence = pack
            .evidence
            .iter()
            .filter(|evidence| match evidence.source_id.as_ref() {
                Some(source_id) => {
                    !source_metadata.contains_key(source_id)
                        || !source_set_ids.contains(source_id)
                        || resolve_context_pack_evidence_metadata(
                            evidence,
                            &artifact_metadata.evidence,
                            &source_set_ids,
                        )
                        .is_none()
                }
                None => true,
            })
            .collect::<Vec<_>>();

        let mut suggested_next_reads = Vec::new();
        let mut suggested_next_read_keys = std::collections::BTreeSet::new();
        for evidence in &selected_evidence {
            if evidence.parse_confidence == ContextPackParseConfidence::Low
                && suggested_next_read_keys.insert((evidence.source_id.clone(), evidence.page))
            {
                suggested_next_reads.push(ContextPackSuggestedNextReadV0 {
                    source_id: evidence.source_id.clone(),
                    page: evidence.page,
                    reason: "Review this page because selected evidence has low parse confidence."
                        .into(),
                });
            }
        }
        for evidence in &omitted_evidence {
            let Some(source_id) = evidence.source_id.clone() else {
                continue;
            };
            let page = evidence.page_index.unwrap_or(0) + 1;
            if suggested_next_read_keys.insert((source_id.clone(), page)) {
                suggested_next_reads.push(ContextPackSuggestedNextReadV0 {
                    source_id,
                    page,
                    reason: format!(
                        "Review this page because evidence {} could not be selected for the Context Pack.",
                        evidence.id
                    ),
                });
            }
        }

        let warnings = pack
            .warnings
            .iter()
            .map(|warning| ContextPackWarningV0 {
                warning_type: context_pack_internal_warning_type(warning).into(),
                severity: ContextPackWarningSeverity::Medium,
                message: warning.clone(),
                page_refs: Vec::new(),
            })
            .chain(omitted_evidence.iter().map(|evidence| ContextPackWarningV0 {
                warning_type: "evidence_missing_content_hash".into(),
                severity: ContextPackWarningSeverity::High,
                message: format!(
                    "Evidence {} was omitted from Context Pack v0 because its source content hash or provider route is unavailable.",
                    evidence.id
                ),
                page_refs: evidence.source_id.clone().map_or_else(Vec::new, |source_id| {
                    vec![ContextPackPageRefV0 {
                        source_id,
                        page: evidence.page_index.unwrap_or(0) + 1,
                    }]
                }),
            }))
            .chain(artifact_metadata.warnings.iter().cloned())
            .chain(selected_evidence.iter().filter_map(|evidence| {
                if evidence.parse_confidence != ContextPackParseConfidence::Low {
                    return None;
                }
                Some(ContextPackWarningV0 {
                    warning_type: "low_parse_confidence".into(),
                    severity: ContextPackWarningSeverity::Medium,
                    message: format!(
                        "Evidence {} has low parse confidence; verify the source page before relying on it.",
                        evidence.evidence_ref
                    ),
                    page_refs: vec![ContextPackPageRefV0 {
                        source_id: evidence.source_id.clone(),
                        page: evidence.page,
                    }],
                })
            }))
            .collect();

        Self {
            schema_version: CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
            pack_id: pack_id.into(),
            workspace_id: pack.workspace_id.clone(),
            query: pack.query.clone(),
            generated_at: generated_at.into(),
            source_set,
            selected_evidence,
            findings,
            warnings,
            retrieval_trace: ContextPackRetrievalTraceV0 {
                strategy: "internal-brain-context-pack".into(),
                chunks_considered: pack.evidence.len(),
                chunks_selected: selected_evidence_ids.len(),
                budget_requested: pack.token_budget,
                budget_used: pack.summary.len(),
            },
            suggested_next_reads,
        }
    }
}

impl ContextPackV1 {
    pub fn from_brain_context_pack(
        pack: &BrainContextPack,
        pack_id: impl Into<String>,
        generated_at: impl Into<String>,
        artifact_metadata: &ContextPackArtifactMetadataV0,
    ) -> Self {
        let v0 =
            ContextPackV0::from_brain_context_pack(pack, pack_id, generated_at, artifact_metadata);
        let selected_evidence = v0
            .selected_evidence
            .iter()
            .map(|evidence| ContextPackEvidenceV1::from_v0(evidence, artifact_metadata))
            .collect::<Vec<_>>();
        let evidence_type_trace =
            ContextPackEvidenceTypeTraceV1::from_pack(pack, &selected_evidence, artifact_metadata);

        Self {
            schema_version: CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
            pack_id: v0.pack_id,
            workspace_id: v0.workspace_id,
            query: v0.query,
            generated_at: v0.generated_at,
            source_set: v0.source_set,
            selected_evidence,
            findings: v0.findings,
            warnings: v0.warnings,
            retrieval_trace: ContextPackRetrievalTraceV1 {
                strategy: v0.retrieval_trace.strategy,
                chunks_considered: v0.retrieval_trace.chunks_considered,
                chunks_selected: v0.retrieval_trace.chunks_selected,
                budget_requested: v0.retrieval_trace.budget_requested,
                budget_used: v0.retrieval_trace.budget_used,
                evidence_type_trace,
            },
            suggested_next_reads: v0.suggested_next_reads,
        }
    }
}

fn context_pack_internal_warning_type(warning: &str) -> &'static str {
    let normalized = warning.to_ascii_lowercase();
    if normalized.contains("budget") && normalized.contains("truncat") {
        "budget_truncated"
    } else {
        "internal_context_warning"
    }
}

impl ContextPackSourceV0 {
    fn from_source_record(
        source: &SourceRecord,
        metadata: &BTreeMap<SourceId, ContextPackSourceMetadataV0>,
    ) -> Self {
        let metadata = metadata.get(&source.source_id).unwrap_or_else(|| {
            panic!(
                "source metadata missing for Context Pack v0 source {}",
                source.source_id
            )
        });
        Self {
            source_id: source.source_id.clone(),
            original_filename: std::path::Path::new(&source.original_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(source.original_path.as_str())
                .into(),
            content_hash: metadata.content_hash.clone(),
            page_count: source.page_count,
            ingestion_status: source.status.to_string(),
            staleness: if source.status == SourceStatus::stale() {
                ContextPackStaleness::Stale
            } else {
                ContextPackStaleness::Current
            },
            provider_route: metadata.provider_route.clone(),
            local_only: metadata.local_only,
        }
    }
}

impl ContextPackEvidenceV0 {
    fn from_evidence_ref(
        evidence: &EvidenceRef,
        evidence_metadata: &BTreeMap<SourceId, BTreeMap<String, ContextPackEvidenceMetadataV0>>,
        source_set_ids: &std::collections::BTreeSet<SourceId>,
    ) -> Option<Self> {
        let source_id = evidence.source_id.clone()?;
        if !source_set_ids.contains(&source_id) {
            return None;
        }

        if let Some((evidence_ref, metadata)) =
            resolve_context_pack_evidence_metadata(evidence, evidence_metadata, source_set_ids)
        {
            return Some(Self {
                evidence_ref,
                source_id: metadata.source_id.clone(),
                page: metadata.page,
                region: metadata.region.clone(),
                span: metadata.span.clone(),
                quoted_text: metadata.quoted_text.clone(),
                parse_confidence: metadata.parse_confidence.clone(),
                selection_reason: "Selected from the source evidence index for this query.".into(),
                content_hash: metadata.content_hash.clone(),
            });
        }
        None
    }
}

impl ContextPackEvidenceV1 {
    fn from_v0(
        evidence: &ContextPackEvidenceV0,
        artifact_metadata: &ContextPackArtifactMetadataV0,
    ) -> Self {
        let evidence_type = artifact_metadata
            .evidence
            .get(&evidence.source_id)
            .and_then(|source_evidence| source_evidence.get(&evidence.evidence_ref))
            .map(|metadata| metadata.evidence_type)
            .unwrap_or_else(EvidenceType::legacy_default);

        Self {
            evidence_ref: evidence.evidence_ref.clone(),
            source_id: evidence.source_id.clone(),
            page: evidence.page,
            region: evidence.region.clone(),
            span: evidence.span.clone(),
            quoted_text: evidence.quoted_text.clone(),
            parse_confidence: evidence.parse_confidence.clone(),
            selection_reason: evidence.selection_reason.clone(),
            content_hash: evidence.content_hash.clone(),
            evidence_type,
            graph_trail: None,
        }
    }
}

impl ContextPackEvidenceTypeTraceV1 {
    fn from_pack(
        pack: &BrainContextPack,
        selected_evidence: &[ContextPackEvidenceV1],
        artifact_metadata: &ContextPackArtifactMetadataV0,
    ) -> Self {
        let mut considered = BTreeMap::new();
        for evidence in &pack.evidence {
            let evidence_type = evidence
                .source_id
                .as_ref()
                .and_then(|source_id| artifact_metadata.evidence.get(source_id))
                .and_then(|source_evidence| {
                    source_evidence.get(&evidence.id).or_else(|| {
                        let page = context_pack_evidence_page(evidence)?;
                        source_evidence.values().find(|metadata| {
                            metadata.page == page
                                && evidence
                                    .source_id
                                    .as_ref()
                                    .is_some_and(|source_id| metadata.source_id == *source_id)
                        })
                    })
                })
                .map(|metadata| metadata.evidence_type)
                .unwrap_or_else(EvidenceType::legacy_default);
            *considered
                .entry(evidence_type.as_trace_key().to_string())
                .or_insert(0) += 1;
        }

        let mut selected = BTreeMap::new();
        for evidence in selected_evidence {
            *selected
                .entry(evidence.evidence_type.as_trace_key().to_string())
                .or_insert(0) += 1;
        }

        Self {
            considered,
            selected,
        }
    }
}

fn resolve_context_pack_evidence_metadata<'a>(
    evidence: &EvidenceRef,
    evidence_metadata: &'a BTreeMap<SourceId, BTreeMap<String, ContextPackEvidenceMetadataV0>>,
    source_set_ids: &std::collections::BTreeSet<SourceId>,
) -> Option<(String, &'a ContextPackEvidenceMetadataV0)> {
    let source_id = evidence.source_id.as_ref()?;
    if !source_set_ids.contains(source_id) {
        return None;
    }
    let source_evidence = evidence_metadata.get(source_id)?;
    if let Some(metadata) = source_evidence.get(&evidence.id) {
        return Some((evidence.id.clone(), metadata));
    }
    let page = context_pack_evidence_page(evidence)?;
    source_evidence
        .iter()
        .find(|(_evidence_ref, metadata)| metadata.page == page && metadata.source_id == *source_id)
        .map(|(evidence_ref, metadata)| (evidence_ref.clone(), metadata))
}

fn context_pack_evidence_page(evidence: &EvidenceRef) -> Option<usize> {
    evidence
        .page_index
        .map(|page_index| page_index + 1)
        .or_else(|| parse_page_number_from_label(&evidence.page_label))
}

fn parse_page_number_from_label(label: &str) -> Option<usize> {
    let normalized = label.to_ascii_lowercase();
    let page_offset = normalized.rfind("page")?;
    let after_page = &normalized[page_offset + "page".len()..];
    let digits = after_page
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}
