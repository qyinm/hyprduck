use serde::{Deserialize, Serialize};

use crate::{BrainReadScope, SourceId, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadImportJobRequest {
    pub scope: BrainReadScope,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub source_id: Option<SourceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportJobRecord {
    pub job_id: String,
    pub workspace_id: WorkspaceId,
    pub source_id: SourceId,
    pub status: String,
    pub citation_ready: bool,
    pub graph_ready: bool,
    pub graph_status: String,
    pub graph_error_category: String,
    pub graph_error_message_redacted: String,
    pub graph_retryable: bool,
    pub graph_retry_attempt: u8,
    pub graph_max_retry_attempts: u8,
    #[serde(default)]
    pub graph_next_retry_at: Option<u64>,
    pub manual_retry_available: bool,
    pub source_markdown_path: String,
    pub source_document_path: String,
    pub source_manifest_path: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportLifecycleStatus {
    Imported,
    Parsing,
    Packaging,
    CitationReady,
    CitationReadyGraphPending,
    CitationReadyGraphSkipped,
    GraphRetryWaiting,
    ContextReady,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportLifecyclePhase {
    Imported,
    Parsing,
    Packaging,
    CitationReady,
    ContextMaterializing,
    GraphRetryWaiting,
    GraphPending,
    GraphSkipped,
    ContextReady,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLifecycleState {
    pub status: ImportLifecycleStatus,
    pub phase: ImportLifecyclePhase,
    pub citation_ready: bool,
    pub graph_ready: bool,
    pub retryable: bool,
    pub manual_retry_available: bool,
    pub terminal: bool,
}

impl ImportLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Parsing => "parsing",
            Self::Packaging => "packaging",
            Self::CitationReady => "citation_ready",
            Self::CitationReadyGraphPending => "citation_ready_graph_pending",
            Self::CitationReadyGraphSkipped => "citation_ready_graph_skipped",
            Self::GraphRetryWaiting => "graph_retry_waiting",
            Self::ContextReady => "context_ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::CitationReadyGraphPending
                | Self::CitationReadyGraphSkipped
                | Self::ContextReady
                | Self::Failed
                | Self::Cancelled
        )
    }

    pub fn from_persisted(value: &str) -> Self {
        match value {
            "imported" => Self::Imported,
            "parsing" => Self::Parsing,
            "packaging" => Self::Packaging,
            "citation_ready" => Self::CitationReady,
            "citation_ready_graph_pending" | "completed" => Self::CitationReadyGraphPending,
            "citation_ready_graph_skipped" => Self::CitationReadyGraphSkipped,
            "graph_retry_waiting" => Self::GraphRetryWaiting,
            "context_ready" => Self::ContextReady,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

impl ImportLifecyclePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Parsing => "parsing",
            Self::Packaging => "packaging",
            Self::CitationReady => "citation_ready",
            Self::ContextMaterializing => "context_materializing",
            Self::GraphRetryWaiting => "graph_retry_waiting",
            Self::GraphPending => "graph_pending",
            Self::GraphSkipped => "graph_skipped",
            Self::ContextReady => "context_ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_status_and_graph_status(status: ImportLifecycleStatus, graph_status: &str) -> Self {
        match status {
            ImportLifecycleStatus::Imported => Self::Imported,
            ImportLifecycleStatus::Parsing => Self::Parsing,
            ImportLifecycleStatus::Packaging => Self::Packaging,
            ImportLifecycleStatus::CitationReady => Self::CitationReady,
            ImportLifecycleStatus::CitationReadyGraphSkipped => Self::GraphSkipped,
            ImportLifecycleStatus::GraphRetryWaiting => Self::GraphRetryWaiting,
            ImportLifecycleStatus::ContextReady => Self::ContextReady,
            ImportLifecycleStatus::Failed => Self::Failed,
            ImportLifecycleStatus::Cancelled => Self::Cancelled,
            ImportLifecycleStatus::CitationReadyGraphPending => {
                if graph_status == "skipped" {
                    Self::GraphSkipped
                } else {
                    Self::GraphPending
                }
            }
        }
    }
}

impl ImportLifecycleState {
    pub fn from_persisted(
        persisted_status: &str,
        graph_status: &str,
        citation_ready: bool,
        graph_ready: bool,
        retryable: bool,
        manual_retry_available: bool,
    ) -> Self {
        let mut status = ImportLifecycleStatus::from_persisted(persisted_status);
        if matches!(status, ImportLifecycleStatus::Failed)
            && (persisted_status == "ingested" || (persisted_status == "partial" && citation_ready))
        {
            status = ImportLifecycleStatus::CitationReadyGraphPending;
        }
        if graph_ready || graph_status_is_ready(Some(graph_status)) {
            status = ImportLifecycleStatus::ContextReady;
        }
        Self {
            status,
            phase: ImportLifecyclePhase::from_status_and_graph_status(status, graph_status),
            citation_ready,
            graph_ready,
            retryable,
            manual_retry_available,
            terminal: status.is_terminal(),
        }
    }
}

pub fn graph_status_is_ready(status: Option<&str>) -> bool {
    matches!(status, Some("rebuilt" | "partially_applied" | "ready"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadImportJobResponseData {
    #[serde(default)]
    pub job: Option<ImportJobRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateImportJobGraphStatusRequest {
    pub scope: BrainReadScope,
    pub source_id: SourceId,
    pub status: String,
    pub graph_status: String,
    #[serde(default)]
    pub graph_error_category: Option<String>,
    #[serde(default)]
    pub graph_error_message_redacted: Option<String>,
    #[serde(default)]
    pub graph_retryable: bool,
    #[serde(default)]
    pub graph_retry_attempt: u8,
    #[serde(default)]
    pub graph_max_retry_attempts: u8,
    #[serde(default)]
    pub graph_next_retry_at: Option<u64>,
    #[serde(default)]
    pub manual_retry_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateImportJobGraphStatusResponseData {
    pub updated: bool,
}
