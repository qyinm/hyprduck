use std::path::Path;

use crate::*;

pub(crate) fn write_provider_graph_run_artifacts(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    manifest: &SourceArtifactManifest,
    status: &str,
    prompt: Option<&str>,
    provider_response: Option<&str>,
    error_message: Option<String>,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("provider-response.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": manifest.source_id,
            "status": status,
            "prompt": prompt,
            "providerResponse": provider_response,
            "errorMessage": error_message,
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

pub(crate) fn write_provider_graph_run_validation_report(
    workspace_root: &Path,
    run_id: &str,
    workspace_id: &str,
    source_id: &str,
    status: &str,
    parsed_count: usize,
    error_message: Option<String>,
) -> Result<()> {
    write_json_pretty(
        &workspace_root
            .join("runs")
            .join(run_id)
            .join("validation-report.json"),
        &json!({
            "runId": run_id,
            "workspaceId": workspace_id,
            "sourceId": source_id,
            "status": status,
            "parsedProposalCount": parsed_count,
            "errorMessage": error_message,
            "createdAt": unix_timestamp_seconds(),
        }),
    )
}

pub(crate) fn provider_workspace_rebuild_response_schema(
) -> async_openai::types::chat::ResponseFormatJsonSchema {
    async_openai::types::chat::ResponseFormatJsonSchema {
        name: "workspace_graph_rebuild".into(),
        description: Some(
            "Complete materialized graph state for a HyprDuck workspace rebuild.".into(),
        ),
        schema: Some(json!({
            "type": "object",
            "additionalProperties": true,
            "required": ["materializedGraph"],
            "properties": {
                "materializedGraph": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["sources", "evidence", "nodes", "edges"],
                    "properties": {
                        "generatedAt": { "type": ["integer", "null"] },
                        "sources": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "evidence": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "nodes": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "edges": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "claims": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "memories": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "wikiPages": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "entities": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
                        "extractions": { "type": "array", "items": { "type": "object", "additionalProperties": true } }
                    }
                }
            }
        })),
        strict: Some(false),
    }
}
