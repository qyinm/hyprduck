#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use anyhow::anyhow;
#[cfg(test)]
use hyprduck_engine_types::{
    graph_status_is_ready, BrainReadScope, DocumentFormat, ImportLifecyclePhase as ImportJobPhase,
    ImportLifecycleStatus as ImportJobStatus,
};
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use serde_json::{Map, Value};

mod args;
pub(crate) mod automation_policy;
mod cache;
mod import_jobs;
mod policy;
mod protocol;
mod resources;
mod responses;
mod tool_catalog;
mod tool_dispatch;

pub use protocol::run_mcp_server;

#[cfg(test)]
use args::{
    import_document_format, read_scope, required_string_array, validate_mcp_proposal_id,
    validate_mcp_write_content_type,
};
#[cfg(test)]
use args::{PROPOSAL_ID_PATTERN, WRITE_CONTENT_TYPES};
pub(in crate::mcp) use import_jobs::ImportJobRegistry;
#[cfg(test)]
use import_jobs::ImportJobSnapshot;
#[cfg(test)]
use import_jobs::{
    classify_graph_failure, import_phase_from_parse_progress, record_graph_status_persist_result,
    sanitize_graph_error_message,
};
#[cfg(test)]
use policy::redact_local_path_text;
#[cfg(test)]
use policy::validate_import_source_path;
#[cfg(test)]
use policy::{IMPORT_ALLOWED_ROOTS_ENV, ROOT_DIR_ALLOWED_ROOTS_ENV, ROOT_DIR_ENV};
#[cfg(test)]
use resources::parse_resource_uri;
#[cfg(test)]
use responses::classify_mcp_error;
pub(in crate::mcp) use tool_catalog::tool_definitions;
#[cfg(test)]
use tool_dispatch::LOCAL_PATH_DISCLOSURE_ENV;
pub(in crate::mcp) use tool_dispatch::{
    call_tool, local_path_disclosure_for_tool, supports_local_path_disclosure,
};

#[cfg(test)]
mod tests;
