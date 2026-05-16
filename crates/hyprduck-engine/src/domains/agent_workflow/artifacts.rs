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
            "additionalProperties": false,
            "required": ["materializedGraph"],
            "properties": {
                "materializedGraph": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "generatedAt",
                        "sources",
                        "evidence",
                        "nodes",
                        "edges",
                        "claims",
                        "memories",
                        "wikiPages",
                        "entities",
                        "extractions"
                    ],
                    "properties": {
                        "generatedAt": { "type": ["integer", "null"] },
                        "sources": { "type": "array", "items": source_record_schema() },
                        "evidence": { "type": "array", "items": evidence_ref_schema() },
                        "nodes": { "type": "array", "items": brain_node_record_schema() },
                        "edges": { "type": "array", "items": brain_relation_record_schema() },
                        "claims": { "type": "array", "items": claim_record_schema() },
                        "memories": { "type": "array", "items": memory_record_schema() },
                        "wikiPages": { "type": "array", "items": wiki_page_schema() },
                        "entities": { "type": "array", "items": entity_record_schema() },
                        "extractions": {
                            "type": "array",
                            "items": empty_object_schema(),
                            "maxItems": 0
                        }
                    }
                }
            }
        })),
        strict: Some(true),
    }
}

pub(crate) fn provider_workspace_linking_response_schema(
) -> async_openai::types::chat::ResponseFormatJsonSchema {
    async_openai::types::chat::ResponseFormatJsonSchema {
        name: "workspace_graph_linking".into(),
        description: Some(
            "Cross-source relation records for a HyprDuck workspace linking run.".into(),
        ),
        schema: Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["materializedGraph"],
            "properties": {
                "materializedGraph": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "generatedAt",
                        "edges",
                        "claims",
                        "memories",
                        "wikiPages"
                    ],
                    "properties": {
                        "generatedAt": { "type": ["integer", "null"] },
                        "edges": { "type": "array", "items": brain_relation_record_schema() },
                        "claims": { "type": "array", "items": claim_record_schema() },
                        "memories": { "type": "array", "items": memory_record_schema() },
                        "wikiPages": { "type": "array", "items": wiki_page_schema() }
                    }
                }
            }
        })),
        strict: Some(true),
    }
}

fn string_array_schema() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn empty_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [],
        "properties": {}
    })
}

fn nullable_string_schema() -> Value {
    json!({ "type": ["string", "null"] })
}

fn nullable_integer_schema() -> Value {
    json!({ "type": ["integer", "null"] })
}

fn nullable_number_schema() -> Value {
    json!({ "type": ["number", "null"] })
}

fn brain_scope_schema() -> Value {
    json!({ "type": "string", "enum": ["project"] })
}

fn brain_node_kind_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            "source",
            "memory",
            "wiki_page",
            "person",
            "company",
            "project",
            "product",
            "team",
            "event",
            "decision",
            "task",
            "claim",
            "topic",
            "concept"
        ]
    })
}

fn brain_relation_kind_schema() -> Value {
    json!({
        "type": "string",
        "enum": [
            "mentions",
            "supports",
            "contradicts",
            "supersedes",
            "same_as",
            "works_at",
            "founded",
            "invested_in",
            "advises",
            "attended",
            "owns",
            "responsible_for",
            "decided",
            "blocks",
            "depends_on",
            "source_of",
            "derived_from",
            "related_to"
        ]
    })
}

fn source_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "sourceId",
            "workspaceId",
            "originalPath",
            "sourcePath",
            "markdownPath",
            "format",
            "status",
            "pageCount",
            "description",
            "userContext",
            "ingestInstruction",
            "updatedAt"
        ],
        "properties": {
            "sourceId": { "type": "string" },
            "workspaceId": { "type": "string" },
            "originalPath": { "type": "string" },
            "sourcePath": { "type": "string" },
            "markdownPath": { "type": "string" },
            "format": { "type": "string" },
            "status": { "type": "string" },
            "pageCount": { "type": "integer" },
            "description": { "type": "string" },
            "userContext": { "type": "string" },
            "ingestInstruction": { "type": "string" },
            "updatedAt": { "type": "integer" }
        }
    })
}

fn evidence_ref_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "id",
            "pageLabel",
            "pageIndex",
            "snippet",
            "sourcePath",
            "sourceId",
            "markdownPath",
            "imagePath",
            "provenance"
        ],
        "properties": {
            "id": { "type": "string" },
            "pageLabel": { "type": "string" },
            "pageIndex": nullable_integer_schema(),
            "snippet": { "type": "string" },
            "sourcePath": nullable_string_schema(),
            "sourceId": nullable_string_schema(),
            "markdownPath": nullable_string_schema(),
            "imagePath": nullable_string_schema(),
            "provenance": nullable_string_schema()
        }
    })
}

fn brain_node_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "nodeId",
            "kind",
            "label",
            "scope",
            "aliases",
            "evidenceIds",
            "sourceIds",
            "confidence",
            "updatedAt"
        ],
        "properties": {
            "nodeId": { "type": "string" },
            "kind": brain_node_kind_schema(),
            "label": { "type": "string" },
            "scope": brain_scope_schema(),
            "aliases": string_array_schema(),
            "evidenceIds": string_array_schema(),
            "sourceIds": string_array_schema(),
            "confidence": nullable_number_schema(),
            "updatedAt": { "type": "integer" }
        }
    })
}

fn brain_relation_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "relationId",
            "kind",
            "sourceNodeId",
            "targetNodeId",
            "label",
            "evidenceIds",
            "confidence",
            "updatedAt"
        ],
        "properties": {
            "relationId": { "type": "string" },
            "kind": brain_relation_kind_schema(),
            "sourceNodeId": { "type": "string" },
            "targetNodeId": { "type": "string" },
            "label": { "type": "string" },
            "evidenceIds": string_array_schema(),
            "confidence": nullable_number_schema(),
            "updatedAt": { "type": "integer" }
        }
    })
}

fn claim_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "claimId",
            "workspaceId",
            "statement",
            "topicRefs",
            "sourceRefs",
            "evidenceRefs",
            "status",
            "updatedAt"
        ],
        "properties": {
            "claimId": { "type": "string" },
            "workspaceId": { "type": "string" },
            "statement": { "type": "string" },
            "topicRefs": string_array_schema(),
            "sourceRefs": string_array_schema(),
            "evidenceRefs": string_array_schema(),
            "status": { "type": "string" },
            "updatedAt": { "type": "integer" }
        }
    })
}

fn memory_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "memoryId",
            "workspaceId",
            "scope",
            "title",
            "body",
            "sourceRefs",
            "evidenceRefs",
            "createdAt",
            "updatedAt"
        ],
        "properties": {
            "memoryId": { "type": "string" },
            "workspaceId": { "type": "string" },
            "scope": brain_scope_schema(),
            "title": { "type": "string" },
            "body": { "type": "string" },
            "sourceRefs": string_array_schema(),
            "evidenceRefs": string_array_schema(),
            "createdAt": { "type": "integer" },
            "updatedAt": { "type": "integer" }
        }
    })
}

fn wiki_page_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "pageId",
            "workspaceId",
            "path",
            "title",
            "body",
            "nodeRefs",
            "sourceRefs",
            "evidenceRefs",
            "updatedAt"
        ],
        "properties": {
            "pageId": { "type": "string" },
            "workspaceId": { "type": "string" },
            "path": { "type": "string" },
            "title": { "type": "string" },
            "body": { "type": "string" },
            "nodeRefs": string_array_schema(),
            "sourceRefs": string_array_schema(),
            "evidenceRefs": string_array_schema(),
            "updatedAt": { "type": "integer" }
        }
    })
}

fn entity_record_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "entityId",
            "workspaceId",
            "kind",
            "name",
            "aliases",
            "sourceRefs",
            "evidenceRefs",
            "updatedAt"
        ],
        "properties": {
            "entityId": { "type": "string" },
            "workspaceId": { "type": "string" },
            "kind": brain_node_kind_schema(),
            "name": { "type": "string" },
            "aliases": string_array_schema(),
            "sourceRefs": string_array_schema(),
            "evidenceRefs": string_array_schema(),
            "updatedAt": { "type": "integer" }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_workspace_rebuild_schema_is_strict_canonical_graph_shape() {
        let schema = provider_workspace_rebuild_response_schema();
        let encoded = serde_json::to_value(schema).expect("encode schema");

        assert_eq!(encoded["strict"], true);
        assert_eq!(encoded["schema"]["additionalProperties"], false);
        assert_eq!(
            encoded["schema"]["properties"]["materializedGraph"]["additionalProperties"],
            false
        );
        let node_properties = &encoded["schema"]["properties"]["materializedGraph"]["properties"]
            ["nodes"]["items"]["properties"];
        assert!(node_properties.get("kind").is_some());
        assert!(node_properties.get("label").is_some());
        assert!(node_properties.get("type").is_none());
        assert!(node_properties.get("name").is_none());

        let edge_properties = &encoded["schema"]["properties"]["materializedGraph"]["properties"]
            ["edges"]["items"]["properties"];
        assert!(edge_properties.get("relationId").is_some());
        assert!(edge_properties.get("sourceNodeId").is_some());
        assert!(edge_properties.get("targetNodeId").is_some());
        assert!(edge_properties.get("edgeId").is_none());
        assert!(edge_properties.get("fromId").is_none());
        assert!(edge_properties.get("toId").is_none());
        assert_eq!(
            encoded["schema"]["properties"]["materializedGraph"]["properties"]["nodes"]["items"]
                ["properties"]["scope"]["enum"],
            json!(["project"])
        );
        assert!(
            encoded["schema"]["properties"]["materializedGraph"]["properties"]["extractions"]
                ["items"]
                .is_object()
        );
    }

    #[test]
    fn provider_workspace_linking_schema_is_relation_only() {
        let schema = provider_workspace_linking_response_schema();
        let encoded = serde_json::to_value(schema).expect("encode schema");
        let properties = &encoded["schema"]["properties"]["materializedGraph"]["properties"];

        assert_eq!(encoded["strict"], true);
        assert!(properties.get("edges").is_some());
        assert!(properties.get("claims").is_some());
        assert!(properties.get("memories").is_some());
        assert!(properties.get("wikiPages").is_some());
        assert!(properties.get("nodes").is_none());
        assert!(properties.get("sources").is_none());
        assert!(properties.get("evidence").is_none());
        assert!(properties.get("entities").is_none());
        assert!(properties.get("extractions").is_none());
    }
}
