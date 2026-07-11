use std::collections::BTreeMap;

use serde_json::Value;
use uuid::Uuid;

use super::*;

#[test]
fn graph_snapshot_read_contract_schema_requires_materialized_state_fields() {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/graph-snapshot-read.schema.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display())),
    )
    .unwrap();

    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .expect("schema must define top-level required fields");
    for field in [
        "snapshotId",
        "sourceIngestId",
        "sourceOfTruthPath",
        "latestReadableSnapshotPath",
        "createdAt",
        "materializedAt",
        "materializedPaths",
        "nodes",
        "edges",
        "claims",
        "memoryRefs",
    ] {
        assert!(
            required.iter().any(|value| value.as_str() == Some(field)),
            "schema must require {field}"
        );
    }

    assert_eq!(
        schema
            .pointer("/properties/snapshotId/type")
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        schema
            .pointer("/properties/sourceIngestId/type")
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        schema
            .pointer("/properties/sourceOfTruthPath/const")
            .and_then(Value::as_str),
        Some("events/brain_events.jsonl")
    );
    assert_eq!(
        schema
            .pointer("/properties/latestReadableSnapshotPath/const")
            .and_then(Value::as_str),
        Some("state/latest-readable-snapshot.json")
    );
    assert_eq!(
        schema
            .pointer("/properties/materializedPaths/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/stringArray")
    );
    assert_eq!(
        schema
            .pointer("/properties/nodes/items/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/node")
    );
    assert_eq!(
        schema
            .pointer("/properties/edges/items/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/edge")
    );
    assert_eq!(
        schema
            .pointer("/properties/claims/items/$ref")
            .and_then(Value::as_str),
        Some("#/$defs/claim")
    );
    assert_eq!(
        schema
            .pointer("/properties/memoryRefs/items/type")
            .and_then(Value::as_str),
        Some("string")
    );
}

#[test]
fn agent_chat_contract_schemas_require_agent_chat_fields() {
    let request_schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/agent-chat-request.schema.json");
    let response_schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/agent-chat-response.schema.json");
    let request_schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&request_schema_path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", request_schema_path.display())
        }),
    )
    .unwrap();
    let response_schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&response_schema_path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", response_schema_path.display())
        }),
    )
    .unwrap();

    assert_eq!(
        request_schema["properties"]["schemaVersion"]["const"],
        AGENT_CHAT_SCHEMA_VERSION
    );
    assert_eq!(
        response_schema["properties"]["schemaVersion"]["const"],
        AGENT_CHAT_SCHEMA_VERSION
    );

    let request_required = request_schema["required"]
        .as_array()
        .expect("request schema must define required fields");
    for field in [
        "schemaVersion",
        "conversationId",
        "scope",
        "mode",
        "question",
    ] {
        assert!(
            request_required
                .iter()
                .any(|value| value.as_str() == Some(field)),
            "request schema must require {field}"
        );
    }

    let response_required = response_schema["required"]
        .as_array()
        .expect("response schema must define required fields");
    for field in [
        "schemaVersion",
        "conversationId",
        "answerMode",
        "assistantMessage",
        "answer",
        "contextPackId",
        "citations",
        "retrievalTrace",
        "provider",
        "warnings",
    ] {
        assert!(
            response_required
                .iter()
                .any(|value| value.as_str() == Some(field)),
            "response schema must require {field}"
        );
    }
}

#[test]
fn parse_request_round_trip() {
    let request = EngineRequest::Parse(ParseRequest {
        version: "1".into(),
        input: ParseInput {
            path: "/tmp/sample.pdf".into(),
            format: DocumentFormat::Pdf,
        },
        template: "General".into(),
        options: ParseOptions::default(),
        output: Some(ParseOutputTarget {
            root_dir: Some("/tmp/out".into()),
            name: Some("sample".into()),
            workspace_id: Some("default".into()),
            source_id: None,
        }),
    });

    let json = serde_json::to_string(&request).unwrap();
    let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, request);
}

#[test]
fn runtime_request_envelope_round_trip() {
    let request = EngineRuntimeRequest {
        id: Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap(),
        request: EngineRequest::LoadConfig(LoadConfigRequest {}),
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"id\""));
    assert!(json.contains("\"command\":\"load_config\""));
    let decoded: EngineRuntimeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, request);
}

#[test]
fn runtime_response_envelope_round_trip() {
    let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
    let response = EngineRuntimeResponse::new(
        id,
        EngineSuccess::new(
            EngineCommand::LoadConfig,
            serde_json::json!({"ready": true}),
        ),
    );

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"id\""));
    assert!(json.contains("\"type\":\"response\""));
    assert!(json.contains("\"command\":\"load_config\""));

    let decoded: EngineRuntimeResponse<serde_json::Value> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn runtime_event_envelope_round_trip() {
    let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
    let event = EngineRuntimeEvent::parse(id, ParseEvent::Queued);

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"event\""));
    assert!(json.contains("\"event\":{\"type\":\"queued\"}"));

    let decoded: EngineRuntimeEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn parse_success_round_trip() {
    let response = EngineSuccess::new(
        EngineCommand::Parse,
        ParseResponseData {
            result: ParseResult {
                version: "1".into(),
                markdown: "# sample".into(),
                pages: vec![ParsedPage {
                    index: 0,
                    markdown: Some("# page".into()),
                    plain_text: Some("page".into()),
                    svg: None,
                    image_asset_path: Some("images/page_1.png".into()),
                    error_message: None,
                }],
                assets: vec![OutputAsset {
                    relative_path: "images/page_1.png".into(),
                    mime_type: "image/png".into(),
                    base64: "cG5n".into(),
                }],
                metadata: ParseMetadata {
                    engine_id: "stub".into(),
                    duration_ms: 5,
                    page_count: 1,
                },
                success_count: 1,
                failed_count: 0,
            },
            saved_output_path: Some("/tmp/out/sample.md".into()),
            source_manifest: None,
        },
    );

    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<ParseResponseData> = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn config_success_round_trip() {
    let response = EngineSuccess::new(
        EngineCommand::LoadConfig,
        EngineConfigPayload {
            provider: "open_router".into(),
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: "key".into(),
            base_url: None,
            prompt_template: "General".into(),
            provider_options: vec![ProviderOption {
                id: "open_router".into(),
                label: "OpenRouter".into(),
                requires_api_key: true,
                supports_base_url: true,
            }],
            model_options: vec!["openai/gpt-4.1-mini".into()],
            prompt_template_options: vec!["General".into()],
        },
    );

    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<EngineConfigPayload> = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn load_project_round_trip() {
    let response = EngineSuccess::new(
        EngineCommand::LoadProject,
        LoadProjectResponseData {
            project: None,
            workspace_id: Some("default".into()),
            sources: Vec::new(),
        },
    );

    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<LoadProjectResponseData> = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::LoadProject);
    assert!(decoded.data.project.is_none());
    assert_eq!(decoded.data.workspace_id.as_deref(), Some("default"));
}

#[test]
fn source_artifact_contract_round_trip() {
    let manifest = SourceArtifactManifest {
        workspace_id: "default".into(),
        source_id: "source-123".into(),
        original_path: "/tmp/input.pdf".into(),
        source_path: "/tmp/Etyma/default/sources/source-123/input.pdf".into(),
        markdown_path: "/tmp/Etyma/default/artifacts/source-123/input.md".into(),
        artifact_root: "/tmp/Etyma/default/artifacts/source-123".into(),
        manifest_path: "/tmp/Etyma/default/artifacts/source-123/source-manifest.json".into(),
        format: DocumentFormat::Pdf,
        output_name: "input".into(),
        status: IngestStatus::Ingested,
        description: "Project brief".into(),
        user_context: "Used for planning".into(),
        ingest_instruction: "Extract decisions".into(),
        pages: vec![PageArtifact {
            index: 0,
            label: "Page 1".into(),
            image_path: Some(
                "/tmp/Etyma/default/artifacts/source-123/images/page_1.png".into(),
            ),
            markdown_path: Some(
                "/tmp/Etyma/default/artifacts/source-123/pages/page_1.md".into(),
            ),
            plain_text_path: None,
            error_message: None,
        }],
        created_at: 1,
        updated_at: 2,
    };

    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains("\"status\":\"ingested\""));
    assert!(json.contains("\"format\":\"pdf\""));
    assert!(json.contains("\"description\":\"Project brief\""));
    let decoded: SourceArtifactManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, manifest);
}

#[test]
fn answer_project_round_trip() {
    let request = EngineRequest::AnswerProject(AnswerProjectRequest {
        project_id: "project-123".into(),
        node_id: Some("concept-a".into()),
        question: "What does this concept cover?".into(),
    });
    let json = serde_json::to_string(&request).unwrap();
    let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, request);

    let response = EngineSuccess::new(
        EngineCommand::AnswerProject,
        AnswerProjectResponseData {
            answer: AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some("Grounded answer".into()),
                explanation: "Based on visible evidence.".into(),
                citations: vec![],
                related_node_ids: vec!["concept-b".into()],
                suggested_actions: vec![],
            },
        },
    );
    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<AnswerProjectResponseData> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::AnswerProject);
    assert_eq!(decoded.data.answer.status, AnswerStatus::Grounded);
}

#[test]
fn agent_chat_ask_round_trip() {
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: Some("/tmp/Etyma".into()),
    };
    let request = EngineRequest::AgentChatAsk(AgentChatAskRequest {
        schema_version: AGENT_CHAT_SCHEMA_VERSION.into(),
        conversation_id: "chat-1".into(),
        assistant_message_id: Some("msg-2".into()),
        scope,
        mode: AgentChatScopeMode::GraphContext,
        selected_node_id: Some("concept-a".into()),
        source_ids: vec!["source-1".into()],
        question: "What should an agent cite?".into(),
        history: vec![AgentChatMessage {
            id: "msg-1".into(),
            role: AgentChatMessageRole::User,
            text: "Summarize the docs.".into(),
            created_at: 1,
        }],
        budget: Some(4096),
        persist_context_pack: true,
    });
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"command\":\"agent_chat_ask\""));
    assert!(json.contains("\"schemaVersion\":\"etyma.agent_chat.v1\""));
    assert!(json.contains("\"mode\":\"graph_context\""));
    let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, request);

    let response = EngineSuccess::new(
        EngineCommand::AgentChatAsk,
        AgentChatAskResponseData {
            schema_version: AGENT_CHAT_SCHEMA_VERSION.into(),
            conversation_id: "chat-1".into(),
            answer_mode: AgentChatAnswerMode::Evidence,
            assistant_message: AgentChatMessage {
                id: "msg-2".into(),
                role: AgentChatMessageRole::Assistant,
                text: "Use evidence refs.".into(),
                created_at: 2,
            },
            answer: AnswerResponse {
                status: AnswerStatus::Grounded,
                text: Some("Use evidence refs.".into()),
                explanation: "Generated by the agent chat path.".into(),
                citations: Vec::new(),
                related_node_ids: Vec::new(),
                suggested_actions: Vec::new(),
            },
            context_pack_id: "ctx_1".into(),
            persisted_context_pack_path: Some("/tmp/Etyma/context_pack.json".into()),
            citations: vec![ContextPackEvidenceV1 {
                evidence_ref: "ev-1".into(),
                source_id: "source-1".into(),
                page: 1,
                region: None,
                span: None,
                quoted_text: "Agents should cite source/page/evidence refs.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                selection_reason: "Matches the question.".into(),
                content_hash: "hash".into(),
                evidence_type: EvidenceType::Text,
                graph_trail: None,
            }],
            retrieval_trace: ContextPackRetrievalTraceV1 {
                strategy: "keyword".into(),
                chunks_considered: 3,
                chunks_selected: 1,
                budget_requested: 4096,
                budget_used: 120,
                evidence_type_trace: ContextPackEvidenceTypeTraceV1::default(),
            },
            provider: AgentChatProviderSummary {
                id: "open_router".into(),
                label: "OpenRouter".into(),
                model_id: "openai/gpt-4.1-mini".into(),
                hosted: true,
            },
            warnings: Vec::new(),
        },
    );
    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<AgentChatAskResponseData> = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::AgentChatAsk);
    assert_eq!(decoded.data.schema_version, AGENT_CHAT_SCHEMA_VERSION);
    assert_eq!(decoded.data.answer_mode, AgentChatAnswerMode::Evidence);
    assert_eq!(decoded.data.citations[0].evidence_ref, "ev-1");
}

#[test]
fn agent_chat_stream_event_round_trip() {
    let event = AgentChatStreamEvent::Status {
        status: AgentChatStreamStatus::RetrievingContext,
        message: "Retrieving context...".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"status\""));
    assert!(json.contains("\"status\":\"retrieving_context\""));
    let decoded: AgentChatStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, event);

    let id = Uuid::parse_str("019e0b95-7f53-7502-8886-e8c01d3aaad4").unwrap();
    let envelope = EngineRuntimeEvent::agent_chat(
        id,
        AgentChatStreamEvent::Delta {
            text: "hello".into(),
        },
    );
    let json = serde_json::to_string(&envelope).unwrap();
    assert!(json.contains("\"type\":\"event\""));
    assert!(json.contains("\"event\":{\"type\":\"delta\""));
    let decoded: EngineRuntimeEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, envelope);
}

#[test]
fn brain_api_requests_round_trip() {
    let scope = BrainReadScope {
        workspace_id: "default".into(),
        root_dir: Some("/tmp/Etyma".into()),
    };
    let requests = vec![
        EngineRequest::SearchBrain(SearchBrainRequest {
            scope: scope.clone(),
            query: "agent context".into(),
            limit: Some(5),
        }),
        EngineRequest::ReadSource(ReadSourceRequest {
            scope: scope.clone(),
            source_id: "source-123".into(),
            include_local_paths: false,
        }),
        EngineRequest::ReadPageEvidence(ReadPageEvidenceRequest {
            scope: scope.clone(),
            source_id: "source-123".into(),
            page: Some(1),
            include_local_paths: false,
        }),
        EngineRequest::ReadContextPack(ReadContextPackRequest {
            scope: scope.clone(),
            pack_id: Some("ctx_123".into()),
        }),
        EngineRequest::ReadWikiPage(ReadWikiPageRequest {
            scope: scope.clone(),
            path: "wiki/index.md".into(),
        }),
        EngineRequest::ReadNode(ReadNodeRequest {
            scope: scope.clone(),
            node_id: "concept-agent-context".into(),
        }),
        EngineRequest::ReadRecentEvents(ReadRecentEventsRequest {
            scope: scope.clone(),
            limit: Some(3),
            run_id: None,
            source_ref: None,
            node_id: None,
            edge_id: None,
            claim_id: None,
            memory_id: None,
            change_type: None,
        }),
        EngineRequest::ReadGraphHistory(ReadGraphHistoryRequest {
            scope: scope.clone(),
            limit: Some(3),
            record_kind: Some(GraphHistoryRecordKind::WikiPage),
            record_id: None,
            wiki_path: Some("wiki/index.md".into()),
            include_diff: true,
        }),
        EngineRequest::ReadGraphSnapshot(ReadGraphSnapshotRequest {
            scope: scope.clone(),
            include_local_paths: false,
        }),
        EngineRequest::GetContextPack(GetContextPackRequest {
            scope: scope.clone(),
            query: "agent context".into(),
            selected_node_id: None,
            budget: Some(8000),
            persist: false,
        }),
        EngineRequest::GetBrainHealth(GetBrainHealthRequest {
            scope: scope.clone(),
        }),
        EngineRequest::ApplyGraphPatch(ApplyGraphPatchRequest {
            scope: scope.clone(),
            agent_id: Some("codex".into()),
            graph_patch: GraphPatch {
                schema_version: GRAPH_PATCH_SCHEMA_VERSION.into(),
                source_ids: vec!["source-123".into()],
                evidence_refs: vec!["ev-1".into()],
                nodes: vec![GraphPatchNode {
                    node_id: "concept-agent-context".into(),
                    kind: BrainNodeKind::Concept,
                    label: "Agent context".into(),
                    scope: None,
                    aliases: Vec::new(),
                    source_ids: vec!["source-123".into()],
                    evidence_ids: vec!["ev-1".into()],
                }],
                relations: vec![GraphPatchRelation {
                    relation_id: "rel-source-agent-context".into(),
                    kind: BrainRelationKind::Mentions,
                    source_node_id: "source:source-123".into(),
                    target_node_id: "concept-agent-context".into(),
                    label: "mentions".into(),
                    evidence_ids: vec!["ev-1".into()],
                }],
                claims: vec![GraphPatchClaim {
                    claim_id: "claim-agent-context".into(),
                    statement: "Etyma exposes agent context.".into(),
                    topic_refs: vec!["concept-agent-context".into()],
                    source_refs: vec!["source-123".into()],
                    evidence_refs: vec!["ev-1".into()],
                    status: "agent_generated".into(),
                }],
                wiki_pages: vec![GraphPatchWikiPage {
                    page_id: "wiki-agent-context".into(),
                    path: "wiki/agent-context.md".into(),
                    title: "Agent context".into(),
                    body: "Evidence-backed page.".into(),
                    node_refs: vec!["concept-agent-context".into()],
                    source_refs: vec!["source-123".into()],
                    evidence_refs: vec!["ev-1".into()],
                }],
                agent_metadata: BTreeMap::new(),
            },
        }),
        EngineRequest::WritePropose(WriteProposeRequest {
            scope: scope.clone(),
            content_type: "memory".into(),
            title: "Agent-session write MCP".into(),
            body: "Evidence-backed memory body".into(),
            evidence_refs: vec!["ev-1".into()],
        }),
        EngineRequest::WriteCommit(WriteCommitRequest {
            scope: scope.clone(),
            proposal_id: "prop-1".into(),
            user_approved: false,
        }),
        EngineRequest::WriteCommitAll(WriteCommitAllRequest {
            scope: scope.clone(),
            proposal_ids: vec!["prop-1".into(), "prop-2".into()],
        }),
        EngineRequest::WriteList(WriteListRequest {
            scope: scope.clone(),
        }),
        EngineRequest::WriteReject(WriteRejectRequest {
            scope,
            proposal_id: "prop-1".into(),
        }),
    ];
    for request in requests {
        let json = serde_json::to_string(&request).unwrap();
        let decoded: EngineRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    let response = EngineSuccess::new(
        EngineCommand::SearchBrain,
        SearchBrainResponseData {
            results: vec![BrainSearchResult {
                kind: BrainSearchResultKind::WikiPage,
                id: "wiki-index".into(),
                title: "Brain Index".into(),
                path: Some("wiki/index.md".into()),
                score: 2,
                snippet: "Agent context".into(),
            }],
        },
    );
    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<SearchBrainResponseData> = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::SearchBrain);
    assert_eq!(
        decoded.data.results[0].kind,
        BrainSearchResultKind::WikiPage
    );
}

#[test]
fn graph_history_response_does_not_expose_rollback_target() {
    let response = EngineSuccess::new(
        EngineCommand::ReadGraphHistory,
        ReadGraphHistoryResponseData {
            states: vec![GraphHistoryEntry {
                snapshot_id: "snapshot-a".into(),
                materialized_at: 10,
                event_id: "event-a".into(),
                operation_type: Some("graph_snapshot_commit".into()),
                source_run_ids: Vec::new(),
                source_markdown_refs: Vec::new(),
                storage_locations: vec!["etyma.sqlite:graphqlite".into()],
                node_count: 1,
                edge_count: 0,
                claim_count: 0,
                memory_count: 0,
                wiki_page_count: 0,
            }],
            record_history: Some(GraphRecordHistoryResponse {
                query: GraphRecordHistoryQuery {
                    record_kind: GraphHistoryRecordKind::Node,
                    record_id: Some("concept-agent-context".into()),
                    wiki_path: None,
                },
                versions: vec![GraphRecordHistoryVersion {
                    record_kind: GraphHistoryRecordKind::Node,
                    logical_id: "concept-agent-context".into(),
                    version_id: "graph-node-version:default:concept-agent-context:event-a"
                        .into(),
                    created_by_event_id: "event-a".into(),
                    valid_from: 10,
                    valid_to: None,
                    superseded_by: None,
                    revision: None,
                    predecessor_revision: None,
                    title: Some("Agent context".into()),
                    source_node_id: None,
                    target_node_id: None,
                    evidence_refs: vec!["ev-1".into()],
                    source_refs: vec!["source-123".into()],
                    node_refs: Vec::new(),
                    relation_refs: Vec::new(),
                    storage_locations: vec!["etyma.sqlite:graphqlite".into()],
                    diff_json: None,
                }],
            }),
        },
    );

    let json = serde_json::to_string(&response).unwrap();

    assert!(!json.contains("rollbackTarget"));
    assert!(!json.contains("replaySelector"));
    assert!(!json.contains("replay://"));
}

#[test]
fn context_pack_v0_schema_requires_agent_facing_fields() {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/context-pack.schema.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display())),
    )
    .unwrap();

    assert_eq!(
        schema["properties"]["schemaVersion"]["const"],
        CONTEXT_PACK_V0_SCHEMA_VERSION
    );
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .expect("schema must define top-level required fields");
    for field in [
        "schemaVersion",
        "packId",
        "workspaceId",
        "query",
        "generatedAt",
        "sourceSet",
        "selectedEvidence",
        "findings",
        "warnings",
        "retrievalTrace",
        "suggestedNextReads",
    ] {
        assert!(
            required.iter().any(|value| value.as_str() == Some(field)),
            "schema must require {field}"
        );
    }

    let finding_required = schema
        .pointer("/$defs/finding/required")
        .and_then(Value::as_array)
        .expect("finding must define required fields");
    assert!(finding_required
        .iter()
        .any(|value| value.as_str() == Some("derivedFrom")));
    assert_eq!(
        schema.pointer("/$defs/finding/properties/status/const"),
        Some(&Value::String("derived_summary".into()))
    );
}

#[test]
fn source_pack_and_evidence_index_schemas_require_import_artifact_fields() {
    let source_pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/source-pack.schema.json");
    let source_pack_schema: Value =
        serde_json::from_str(&std::fs::read_to_string(&source_pack_path).unwrap_or_else(
            |err| panic!("failed to read {}: {err}", source_pack_path.display()),
        ))
        .unwrap();
    assert_eq!(
        source_pack_schema["properties"]["schemaVersion"]["const"],
        SOURCE_PACK_V0_SCHEMA_VERSION
    );
    for field in ["sourceId", "contentHash", "pages", "warnings"] {
        assert!(source_pack_schema["required"]
            .as_array()
            .expect("source pack required")
            .iter()
            .any(|value| value.as_str() == Some(field)));
    }

    let evidence_index_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/evidence-index.schema.json");
    let evidence_index_schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&evidence_index_path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", evidence_index_path.display())
        }),
    )
    .unwrap();
    assert_eq!(
        evidence_index_schema["properties"]["schemaVersion"]["const"],
        EVIDENCE_INDEX_V0_SCHEMA_VERSION
    );
    let evidence_required = evidence_index_schema
        .pointer("/$defs/evidence/required")
        .and_then(Value::as_array)
        .expect("evidence required");
    for field in [
        "evidenceRef",
        "sourceId",
        "page",
        "region",
        "quotedText",
        "contentHash",
    ] {
        assert!(evidence_required
            .iter()
            .any(|value| value.as_str() == Some(field)));
    }
}

#[test]
fn evidence_index_v1_schema_requires_evidence_type() {
    let evidence_index_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/evidence-index-v1.schema.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&evidence_index_path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", evidence_index_path.display())
        }),
    )
    .unwrap();

    assert_eq!(
        schema["properties"]["schemaVersion"]["const"],
        EVIDENCE_INDEX_V1_SCHEMA_VERSION
    );
    let evidence_required = schema
        .pointer("/$defs/evidence/required")
        .and_then(Value::as_array)
        .expect("evidence required");
    assert!(evidence_required
        .iter()
        .any(|value| value.as_str() == Some("evidenceType")));
    assert!(schema
        .pointer("/$defs/evidence/properties/evidenceType/enum")
        .and_then(Value::as_array)
        .expect("evidence type enum")
        .iter()
        .any(|value| value.as_str() == Some("table")));
}

#[test]
fn context_pack_v1_schema_requires_typed_evidence_trace_and_graph_trail() {
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/context-pack-v1.schema.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display())),
    )
    .unwrap();

    assert_eq!(
        schema["properties"]["schemaVersion"]["const"],
        CONTEXT_PACK_V1_SCHEMA_VERSION
    );
    let evidence_required = schema
        .pointer("/$defs/evidence/required")
        .and_then(Value::as_array)
        .expect("selected evidence required");
    assert!(evidence_required
        .iter()
        .any(|value| value.as_str() == Some("evidenceType")));
    let trace_required = schema
        .pointer("/$defs/retrievalTrace/required")
        .and_then(Value::as_array)
        .expect("retrieval trace required");
    assert!(trace_required
        .iter()
        .any(|value| value.as_str() == Some("evidenceTypeTrace")));
    assert!(schema
        .pointer("/$defs/evidence/properties/graphTrail")
        .is_some());
    for (variant, tool, handle_type, argument_field) in [
        (0, "read_node", "node", "nodeId"),
        (1, "read_source", "source", "sourceId"),
        (2, "read_page_evidence", "page_evidence", "page"),
        (3, "read_wiki_page", "wiki_page", "path"),
    ] {
        let follow_up = schema
            .pointer(&format!("/$defs/graphFollowUp/oneOf/{variant}/properties"))
            .and_then(Value::as_object)
            .expect("graph follow-up variant properties");
        assert_eq!(follow_up["tool"]["const"], tool);
        assert_eq!(follow_up["handleType"]["const"], handle_type);
        assert!(follow_up["arguments"]
            .get("$ref")
            .and_then(Value::as_str)
            .is_some());
        let argument_schema_ref = follow_up["arguments"]["$ref"]
            .as_str()
            .expect("argument schema ref");
        let argument_schema = schema
            .pointer(argument_schema_ref.trim_start_matches('#'))
            .expect("argument schema");
        assert!(argument_schema["required"]
            .as_array()
            .expect("argument required")
            .iter()
            .any(|value| value.as_str() == Some(argument_field)));
    }
}

#[test]
fn evidence_index_v1_round_trip_preserves_evidence_type() {
    let evidence_index = EvidenceIndexV1 {
        schema_version: EVIDENCE_INDEX_V1_SCHEMA_VERSION.into(),
        workspace_id: "default".into(),
        source_id: "source-alpha".into(),
        content_hash: "fnv64:abc123".into(),
        provider_route: "local_demo".into(),
        local_only: true,
        evidence: vec![EvidenceIndexItemV1 {
            evidence_ref: "ev-source-alpha-table-1".into(),
            source_id: "source-alpha".into(),
            page: 1,
            region: "page:Page 1".into(),
            span: Some("page".into()),
            quoted_text: "| A | B |\n| - | - |\n| 1 | 2 |".into(),
            parse_confidence: ContextPackParseConfidence::High,
            content_hash: "fnv64:abc123".into(),
            markdown_path: Some("/tmp/source-alpha/page_1.md".into()),
            image_path: Some("/tmp/source-alpha/page_1.png".into()),
            evidence_type: EvidenceType::Table,
        }],
        warnings: Vec::new(),
        generated_at: 42,
    };

    let json = serde_json::to_string(&evidence_index).unwrap();
    assert!(json.contains("\"evidenceType\":\"table\""));
    let decoded: EvidenceIndexV1 = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, evidence_index);
}

#[test]
fn source_pack_and_evidence_index_round_trip() {
    let warning = SourcePackWarningV0 {
        warning_type: "page_parse_failed".into(),
        severity: ContextPackWarningSeverity::High,
        message: "Page failed".into(),
        page: Some(2),
    };
    let source_pack = SourcePackV0 {
        schema_version: SOURCE_PACK_V0_SCHEMA_VERSION.into(),
        workspace_id: "default".into(),
        source_id: "source-alpha".into(),
        original_filename: "sample.pdf".into(),
        original_path: "/tmp/sample.pdf".into(),
        source_path: "/tmp/Etyma/default/sources/source-alpha/sample.pdf".into(),
        markdown_path: "/tmp/Etyma/default/artifacts/source-alpha/sample.md".into(),
        artifact_root: "/tmp/Etyma/default/artifacts/source-alpha".into(),
        content_hash: "fnv64:abc123".into(),
        format: DocumentFormat::Pdf,
        page_count: 2,
        ingestion_status: IngestStatus::Partial,
        provider_route: "unknown".into(),
        local_only: false,
        pages: vec![SourcePackPageV0 {
            page: 1,
            label: "Page 1".into(),
            image_path: Some("/tmp/page_1.png".into()),
            markdown_path: Some("/tmp/page_1.md".into()),
            plain_text_path: None,
            error_message: None,
        }],
        warnings: vec![warning.clone()],
        created_at: 1,
        updated_at: 2,
    };
    let decoded: SourcePackV0 =
        serde_json::from_str(&serde_json::to_string(&source_pack).unwrap()).unwrap();
    assert_eq!(decoded, source_pack);

    let evidence_index = EvidenceIndexV0 {
        schema_version: EVIDENCE_INDEX_V0_SCHEMA_VERSION.into(),
        workspace_id: "default".into(),
        source_id: "source-alpha".into(),
        content_hash: "fnv64:abc123".into(),
        provider_route: "unknown".into(),
        local_only: false,
        evidence: vec![EvidenceIndexItemV0 {
            evidence_ref: "ev-source-alpha-source-1".into(),
            source_id: "source-alpha".into(),
            page: 1,
            region: "page:Page 1".into(),
            span: Some("page".into()),
            quoted_text: "Evidence text.".into(),
            parse_confidence: ContextPackParseConfidence::Unknown,
            content_hash: "fnv64:abc123".into(),
            markdown_path: Some("/tmp/page_1.md".into()),
            image_path: Some("/tmp/page_1.png".into()),
        }],
        warnings: vec![warning],
        generated_at: 3,
    };
    let decoded: EvidenceIndexV0 =
        serde_json::from_str(&serde_json::to_string(&evidence_index).unwrap()).unwrap();
    assert_eq!(decoded, evidence_index);
}

#[test]
fn context_pack_v1_round_trip_preserves_graph_trail_handles() {
    let pack = ContextPackV1 {
        schema_version: CONTEXT_PACK_V1_SCHEMA_VERSION.into(),
        pack_id: "ctx_graph_trail".into(),
        workspace_id: "default".into(),
        query: "How does the ontology help agents?".into(),
        generated_at: "2026-06-04T09:00:00Z".into(),
        source_set: vec![ContextPackSourceV0 {
            source_id: "src_agent_graph".into(),
            original_filename: "agent-graph.md".into(),
            content_hash: "fnv64:graph".into(),
            page_count: 2,
            ingestion_status: "ingested".into(),
            staleness: ContextPackStaleness::Current,
            provider_route: "ollama".into(),
            local_only: true,
        }],
        selected_evidence: vec![ContextPackEvidenceV1 {
            evidence_ref: "ev_agent_graph_p1".into(),
            source_id: "src_agent_graph".into(),
            page: 1,
            region: Some("page:Page 1".into()),
            span: Some("page".into()),
            quoted_text: "Graph trails expose related concepts for follow-up reads.".into(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: "Selected from graph-aware retrieval.".into(),
            content_hash: "fnv64:graph".into(),
            evidence_type: EvidenceType::Relationship,
            graph_trail: Some(ContextPackGraphTrailV1 {
                direct: vec![ContextPackGraphRecordV1 {
                    record_type: ContextPackGraphRecordKindV1::Node,
                    id: "node-agent-context".into(),
                    reason: "Evidence directly mentions the agent context concept.".into(),
                }],
                adjacent: vec![ContextPackGraphRecordV1 {
                    record_type: ContextPackGraphRecordKindV1::Relation,
                    id: "rel-agent-context-source".into(),
                    reason: "Relation was reached from the selected node neighborhood.".into(),
                }],
                follow_up: vec![
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadNode,
                        handle_type: ContextPackGraphHandleTypeV1::Node,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadNode(
                            ContextPackGraphReadNodeArgumentsV1 {
                                node_id: "node-agent-context".into(),
                            },
                        ),
                        reason: "Inspect the concept node connected to this evidence.".into(),
                    },
                    ContextPackGraphFollowUpV1 {
                        tool: ContextPackGraphFollowUpToolV1::ReadPageEvidence,
                        handle_type: ContextPackGraphHandleTypeV1::PageEvidence,
                        arguments: ContextPackGraphFollowUpArgumentsV1::ReadPageEvidence(
                            ContextPackGraphReadPageEvidenceArgumentsV1 {
                                source_id: "src_agent_graph".into(),
                                page: 1,
                            },
                        ),
                        reason: "Verify the page-level cited evidence.".into(),
                    },
                ],
                unavailable_reason: None,
            }),
        }],
        findings: Vec::new(),
        warnings: Vec::new(),
        retrieval_trace: ContextPackRetrievalTraceV1 {
            strategy: "sqlite-graphqlite-fts5-hybrid".into(),
            chunks_considered: 1,
            chunks_selected: 1,
            budget_requested: 4000,
            budget_used: 1200,
            evidence_type_trace: ContextPackEvidenceTypeTraceV1 {
                considered: BTreeMap::from([("relationship".into(), 1)]),
                selected: BTreeMap::from([("relationship".into(), 1)]),
            },
        },
        suggested_next_reads: vec![ContextPackSuggestedNextReadV0 {
            source_id: "src_agent_graph".into(),
            page: 2,
            reason: "Review the adjacent source page.".into(),
        }],
    };

    let json = serde_json::to_string(&pack).unwrap();
    assert!(json.contains("\"graphTrail\""));
    assert!(json.contains("\"tool\":\"read_node\""));
    assert!(json.contains("\"handleType\":\"page_evidence\""));
    assert!(!json.contains("/tmp/"));
    assert!(!json.contains("docs/private"));
    assert!(!json.contains("\"suggestedNextReads\":[{\"nodeId\""));

    let decoded: ContextPackV1 = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, pack);
    let graph_trail = decoded.selected_evidence[0]
        .graph_trail
        .as_ref()
        .expect("graph trail");
    assert_eq!(
        graph_trail.direct[0].record_type,
        ContextPackGraphRecordKindV1::Node
    );
    assert_eq!(
        graph_trail.follow_up[0].tool,
        ContextPackGraphFollowUpToolV1::ReadNode
    );
    assert_eq!(
        graph_trail.follow_up[1].handle_type,
        ContextPackGraphHandleTypeV1::PageEvidence
    );
}

#[test]
fn context_pack_v0_round_trip_preserves_evidence_backed_findings() {
    let pack = ContextPackV0 {
        schema_version: CONTEXT_PACK_V0_SCHEMA_VERSION.into(),
        pack_id: "ctx_20260518_0001".into(),
        workspace_id: "default".into(),
        query: "What does the document say about agent reuse?".into(),
        generated_at: "2026-05-18T09:00:00Z".into(),
        source_set: vec![ContextPackSourceV0 {
            source_id: "src_agent_context".into(),
            original_filename: "agent-context.pdf".into(),
            content_hash: "sha256:abc123".into(),
            page_count: 2,
            ingestion_status: "ingested".into(),
            staleness: ContextPackStaleness::Current,
            provider_route: "ollama".into(),
            local_only: true,
        }],
        selected_evidence: vec![ContextPackEvidenceV0 {
            evidence_ref: "ev_src_agent_context_p1_b1".into(),
            source_id: "src_agent_context".into(),
            page: 1,
            region: Some("p1-block1".into()),
            span: Some("char:0-42".into()),
            quoted_text: "Context packs are reusable by coding agents.".into(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: "Directly answers the reuse question.".into(),
            content_hash: "sha256:abc123".into(),
        }],
        findings: vec![ContextPackFindingV0 {
            finding_id: "f_agent_reuse".into(),
            statement: "The document says context packs can be reused by coding agents.".into(),
            status: ContextPackFindingStatus::DerivedSummary,
            statement_confidence: ContextPackParseConfidence::High,
            derived_from: vec!["ev_src_agent_context_p1_b1".into()],
            relevance_reason: "Directly answers the query.".into(),
        }],
        warnings: vec![ContextPackWarningV0 {
            warning_type: "visual_content_not_fully_parsed".into(),
            severity: ContextPackWarningSeverity::Medium,
            message: "A diagram may need visual inspection.".into(),
            page_refs: vec![ContextPackPageRefV0 {
                source_id: "src_agent_context".into(),
                page: 2,
            }],
        }],
        retrieval_trace: ContextPackRetrievalTraceV0 {
            strategy: "local-text-search+evidence-expansion".into(),
            chunks_considered: 4,
            chunks_selected: 1,
            budget_requested: 4000,
            budget_used: 1200,
        },
        suggested_next_reads: vec![ContextPackSuggestedNextReadV0 {
            source_id: "src_agent_context".into(),
            page: 2,
            reason: "Related diagram.".into(),
        }],
    };

    let json = serde_json::to_string(&pack).unwrap();
    assert!(json.contains("\"schemaVersion\":\"etyma.context_pack.v0\""));
    assert!(json.contains("\"selectedEvidence\""));
    assert!(json.contains("\"derivedFrom\""));
    let decoded: ContextPackV0 = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, pack);

    let selected = decoded
        .selected_evidence
        .iter()
        .map(|evidence| evidence.evidence_ref.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(decoded.findings.iter().all(|finding| finding
        .derived_from
        .iter()
        .all(|evidence_ref| selected.contains(evidence_ref.as_str()))));
}

#[test]
fn context_pack_v0_can_project_internal_brain_context_pack() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "agent reuse".into(),
        token_budget: 4000,
        summary: "Agent context reuse summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![SourceRecord {
            source_id: "src_agent_context".into(),
            workspace_id: "default".into(),
            original_path: "/tmp/agent-context.pdf".into(),
            source_path: "/tmp/Etyma/default/sources/src_agent_context.pdf".into(),
            markdown_path: "/tmp/Etyma/default/sources/src_agent_context.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 2,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_agent_reuse".into(),
            workspace_id: "default".into(),
            statement: "Context packs can be reused by agents.".into(),
            topic_refs: vec![],
            source_refs: vec!["src_agent_context".into()],
            evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev_src_agent_context_p1_b1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Context packs are reusable by coding agents.".into(),
            source_path: None,
            source_id: Some("src_agent_context".into()),
            markdown_path: Some("/tmp/Etyma/default/sources/src_agent_context.md".into()),
            image_path: Some("/tmp/Etyma/default/artifacts/page_1.png".into()),
            provenance: Some("markdown_extract".into()),
        }],
        recent_events: vec![],
        warnings: vec!["budget truncated context pack".into()],
    };

    let source_metadata = BTreeMap::from([(
        "src_agent_context".into(),
        ContextPackSourceMetadataV0 {
            content_hash: "sha256:abc123".into(),
            provider_route: "ollama".into(),
            local_only: true,
        },
    )]);
    let mut artifact_metadata = ContextPackArtifactMetadataV0::from_sources(source_metadata);
    artifact_metadata
        .evidence
        .entry("src_agent_context".into())
        .or_default()
        .insert(
            "ev_src_agent_context_p1_b1".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "src_agent_context".into(),
                page: 1,
                region: Some("page:Page 1".into()),
                span: Some("page".into()),
                quoted_text: "Indexed source evidence quote.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                content_hash: "sha256:abc123".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Text,
            },
        );

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &artifact_metadata,
    );
    assert_eq!(external.schema_version, CONTEXT_PACK_V0_SCHEMA_VERSION);
    assert_eq!(
        external.source_set[0].original_filename,
        "agent-context.pdf"
    );
    assert_eq!(external.source_set[0].content_hash, "sha256:abc123");
    assert_eq!(external.source_set[0].provider_route, "ollama");
    assert_eq!(external.selected_evidence[0].page, 1);
    assert_eq!(external.selected_evidence[0].content_hash, "sha256:abc123");
    assert_eq!(
        external.selected_evidence[0].quoted_text,
        "Indexed source evidence quote."
    );
    assert_eq!(external.selected_evidence[0].span.as_deref(), Some("page"));
    assert_eq!(
        external.selected_evidence[0].parse_confidence,
        ContextPackParseConfidence::High
    );
    assert_eq!(
        external.findings[0].derived_from,
        vec!["ev_src_agent_context_p1_b1"]
    );
    assert_eq!(external.warnings[0].warning_type, "budget_truncated");
}

#[test]
fn context_pack_v1_projects_selected_evidence_types_and_trace() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "agent reuse".into(),
        token_budget: 4000,
        summary: "Agent context reuse summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![SourceRecord {
            source_id: "src_agent_context".into(),
            workspace_id: "default".into(),
            original_path: "/tmp/agent-context.pdf".into(),
            source_path: "/tmp/Etyma/default/sources/src_agent_context.pdf".into(),
            markdown_path: "/tmp/Etyma/default/sources/src_agent_context.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 2,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_agent_reuse".into(),
            workspace_id: "default".into(),
            statement: "Context packs can be reused by agents.".into(),
            topic_refs: vec![],
            source_refs: vec!["src_agent_context".into()],
            evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev_src_agent_context_p1_b1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Context packs are reusable by coding agents.".into(),
            source_path: None,
            source_id: Some("src_agent_context".into()),
            markdown_path: Some("/tmp/Etyma/default/sources/src_agent_context.md".into()),
            image_path: Some("/tmp/Etyma/default/artifacts/page_1.png".into()),
            provenance: Some("markdown_extract".into()),
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let source_metadata = BTreeMap::from([(
        "src_agent_context".into(),
        ContextPackSourceMetadataV0 {
            content_hash: "sha256:abc123".into(),
            provider_route: "ollama".into(),
            local_only: true,
        },
    )]);
    let mut artifact_metadata = ContextPackArtifactMetadataV0::from_sources(source_metadata);
    artifact_metadata
        .evidence
        .entry("src_agent_context".into())
        .or_default()
        .insert(
            "ev_src_agent_context_p1_b1".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "src_agent_context".into(),
                page: 1,
                region: Some("page:Page 1".into()),
                span: Some("page".into()),
                quoted_text: "Indexed source evidence quote.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                content_hash: "sha256:abc123".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Table,
            },
        );

    let external = ContextPackV1::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-29T00:00:00Z",
        &artifact_metadata,
    );

    assert_eq!(external.schema_version, CONTEXT_PACK_V1_SCHEMA_VERSION);
    assert_eq!(
        external.selected_evidence[0].evidence_type,
        EvidenceType::Table
    );
    assert_eq!(
        external
            .retrieval_trace
            .evidence_type_trace
            .selected
            .get("table"),
        Some(&1)
    );
    assert!(
        external
            .retrieval_trace
            .evidence_type_trace
            .considered
            .values()
            .sum::<usize>()
            >= 1
    );
}

#[test]
fn context_pack_v0_projection_warns_instead_of_emitting_unhashed_evidence() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "agent reuse".into(),
        token_budget: 4000,
        summary: "Agent context reuse summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_agent_reuse".into(),
            workspace_id: "default".into(),
            statement: "Context packs can be reused by agents.".into(),
            topic_refs: vec![],
            source_refs: vec!["src_agent_context".into()],
            evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev_src_agent_context_p1_b1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Context packs are reusable by coding agents.".into(),
            source_path: None,
            source_id: Some("src_agent_context".into()),
            markdown_path: None,
            image_path: None,
            provenance: Some("markdown_extract".into()),
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &ContextPackArtifactMetadataV0::default(),
    );
    assert!(external.selected_evidence.is_empty());
    assert!(external.findings.is_empty());
    assert_eq!(
        external.warnings[0].warning_type,
        "evidence_missing_content_hash"
    );
    assert_eq!(external.suggested_next_reads.len(), 1);
    assert_eq!(
        external.suggested_next_reads[0].source_id,
        "src_agent_context"
    );
    assert_eq!(external.suggested_next_reads[0].page, 1);
    assert!(external.suggested_next_reads[0]
        .reason
        .contains("could not be selected"));
}

#[test]
fn context_pack_v0_requires_indexed_evidence_before_emitting_findings() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "agent reuse".into(),
        token_budget: 4000,
        summary: "Agent context reuse summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![SourceRecord {
            source_id: "src_agent_context".into(),
            workspace_id: "default".into(),
            original_path: "/tmp/agent-context.pdf".into(),
            source_path: "/tmp/Etyma/default/sources/src_agent_context.pdf".into(),
            markdown_path: "/tmp/Etyma/default/sources/src_agent_context.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_agent_reuse".into(),
            workspace_id: "default".into(),
            statement: "Context packs can be reused by agents.".into(),
            topic_refs: vec![],
            source_refs: vec!["src_agent_context".into()],
            evidence_refs: vec!["ev_src_agent_context_p1_b1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev_src_agent_context_p1_b1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Internal snippet without Evidence Index backing.".into(),
            source_path: None,
            source_id: Some("src_agent_context".into()),
            markdown_path: None,
            image_path: None,
            provenance: Some("markdown_extract".into()),
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let artifact_metadata = ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([(
        "src_agent_context".into(),
        ContextPackSourceMetadataV0 {
            content_hash: "sha256:abc123".into(),
            provider_route: "ollama".into(),
            local_only: true,
        },
    )]));

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &artifact_metadata,
    );

    assert!(external.selected_evidence.is_empty());
    assert!(external.findings.is_empty());
    assert!(external.warnings.iter().any(|warning| warning.warning_type
        == "evidence_missing_content_hash"
        && warning.page_refs[0].source_id == "src_agent_context"
        && warning.page_refs[0].page == 1));
    assert_eq!(external.suggested_next_reads.len(), 1);
    assert_eq!(
        external.suggested_next_reads[0].source_id,
        "src_agent_context"
    );
}

#[test]
fn context_pack_v0_maps_retrieved_chunk_to_indexed_page_evidence() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "fixture evidence".into(),
        token_budget: 4000,
        summary: "Retrieved source chunk summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![SourceRecord {
            source_id: "source-fixture".into(),
            workspace_id: "default".into(),
            original_path: "/tmp/fixture-source.pdf".into(),
            source_path: "/tmp/Etyma/default/sources/source-fixture/source.pdf".into(),
            markdown_path: "/tmp/Etyma/default/artifacts/source-fixture/source.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 3,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_fixture_evidence_mapping".into(),
            workspace_id: "default".into(),
            statement: "The fixture source discusses evidence mapping.".into(),
            topic_refs: vec![],
            source_refs: vec!["source-fixture".into()],
            evidence_refs: vec!["retrieved:source-fixture:chunk-1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "retrieved:source-fixture:chunk-1".into(),
            page_label: "Fixture Source / Page 1".into(),
            page_index: None,
            snippet: "Evidence mapping is discussed on this page.".into(),
            source_path: None,
            source_id: Some("source-fixture".into()),
            markdown_path: None,
            image_path: None,
            provenance: Some("retrieval".into()),
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let mut artifact_metadata =
        ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([(
            "source-fixture".into(),
            ContextPackSourceMetadataV0 {
                content_hash: "fnv64:fixture".into(),
                provider_route: "ollama".into(),
                local_only: true,
            },
        )]));
    artifact_metadata
        .evidence
        .entry("source-fixture".into())
        .or_default()
        .insert(
            "ev-source-fixture-source-1".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "source-fixture".into(),
                page: 1,
                region: Some("page:Page 1".into()),
                span: Some("page".into()),
                quoted_text: "Evidence mapping is discussed on this page.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                content_hash: "fnv64:fixture".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Text,
            },
        );

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &artifact_metadata,
    );

    assert_eq!(external.selected_evidence.len(), 1);
    assert_eq!(
        external.selected_evidence[0].evidence_ref,
        "ev-source-fixture-source-1"
    );
    assert_eq!(external.selected_evidence[0].source_id, "source-fixture");
    assert_eq!(external.selected_evidence[0].page, 1);
    assert_eq!(
        external.selected_evidence[0].quoted_text,
        "Evidence mapping is discussed on this page."
    );
    assert_eq!(external.selected_evidence[0].content_hash, "fnv64:fixture");
    assert_eq!(
        external.findings[0].derived_from,
        vec!["ev-source-fixture-source-1"]
    );
    assert!(!external
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "evidence_missing_content_hash"));
}

#[test]
fn context_pack_v0_warns_and_suggests_next_read_for_low_confidence_evidence() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "visual table".into(),
        token_budget: 4000,
        summary: "Visual table summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![SourceRecord {
            source_id: "src_visual_table".into(),
            workspace_id: "default".into(),
            original_path: "/tmp/visual-table.pdf".into(),
            source_path: "/tmp/Etyma/default/sources/src_visual_table.pdf".into(),
            markdown_path: "/tmp/Etyma/default/sources/src_visual_table.md".into(),
            format: SourceFormat::pdf(),
            status: SourceStatus::ingested(),
            page_count: 1,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            updated_at: 1,
        }],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim_visual_table".into(),
            workspace_id: "default".into(),
            statement: "The visual table needs verification.".into(),
            topic_refs: vec![],
            source_refs: vec!["src_visual_table".into()],
            evidence_refs: vec!["ev_visual_table_p1".into()],
            status: "active".into(),
            updated_at: 2,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev_visual_table_p1".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Visual table extracted text.".into(),
            source_path: None,
            source_id: Some("src_visual_table".into()),
            markdown_path: None,
            image_path: None,
            provenance: Some("visual_extract".into()),
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let mut artifact_metadata =
        ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([(
            "src_visual_table".into(),
            ContextPackSourceMetadataV0 {
                content_hash: "sha256:visual-table".into(),
                provider_route: "ollama".into(),
                local_only: true,
            },
        )]));
    artifact_metadata
        .evidence
        .entry("src_visual_table".into())
        .or_default()
        .insert(
            "ev_visual_table_p1".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "src_visual_table".into(),
                page: 1,
                region: Some("page:Page 1".into()),
                span: Some("table".into()),
                quoted_text: "Visual table extracted text.".into(),
                parse_confidence: ContextPackParseConfidence::Low,
                content_hash: "sha256:visual-table".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Text,
            },
        );

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &artifact_metadata,
    );

    assert_eq!(
        external.selected_evidence[0].parse_confidence,
        ContextPackParseConfidence::Low
    );
    assert!(external
        .warnings
        .iter()
        .any(|warning| warning.warning_type == "low_parse_confidence"
            && warning.page_refs[0].source_id == "src_visual_table"
            && warning.page_refs[0].page == 1));
    assert_eq!(external.suggested_next_reads.len(), 1);
    assert_eq!(
        external.suggested_next_reads[0].source_id,
        "src_visual_table"
    );
    assert_eq!(external.suggested_next_reads[0].page, 1);
    assert!(external.suggested_next_reads[0]
        .reason
        .contains("low parse confidence"));
}

#[test]
fn context_pack_v0_scopes_indexed_evidence_metadata_by_source() {
    let internal = BrainContextPack {
        workspace_id: "default".into(),
        query: "shared evidence ref".into(),
        token_budget: 4000,
        summary: "Shared ref summary.".into(),
        wiki_pages: vec![],
        nodes: vec![],
        sources: vec![
            SourceRecord {
                source_id: "source-alpha".into(),
                workspace_id: "default".into(),
                original_path: "/tmp/alpha.md".into(),
                source_path: "/tmp/Etyma/default/sources/source-alpha.md".into(),
                markdown_path: "/tmp/Etyma/default/sources/source-alpha.md".into(),
                format: SourceFormat::markdown(),
                status: SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
            SourceRecord {
                source_id: "source-beta".into(),
                workspace_id: "default".into(),
                original_path: "/tmp/beta.md".into(),
                source_path: "/tmp/Etyma/default/sources/source-beta.md".into(),
                markdown_path: "/tmp/Etyma/default/sources/source-beta.md".into(),
                format: SourceFormat::markdown(),
                status: SourceStatus::ingested(),
                page_count: 1,
                description: String::new(),
                user_context: String::new(),
                ingest_instruction: String::new(),
                updated_at: 1,
            },
        ],
        memories: vec![],
        entities: vec![],
        claims: vec![ClaimRecord {
            claim_id: "claim-beta".into(),
            workspace_id: "default".into(),
            statement: "Beta claim.".into(),
            topic_refs: vec![],
            source_refs: vec!["source-beta".into()],
            evidence_refs: vec!["ev-shared".into()],
            status: "active".into(),
            updated_at: 1,
        }],
        relations: vec![],
        evidence: vec![EvidenceRef {
            id: "ev-shared".into(),
            page_label: "Page 1".into(),
            page_index: Some(0),
            snippet: "Fallback beta snippet.".into(),
            source_path: None,
            source_id: Some("source-beta".into()),
            markdown_path: None,
            image_path: None,
            provenance: None,
        }],
        recent_events: vec![],
        warnings: vec![],
    };

    let mut artifact_metadata = ContextPackArtifactMetadataV0::from_sources(BTreeMap::from([
        (
            "source-alpha".into(),
            ContextPackSourceMetadataV0 {
                content_hash: "fnv64:alpha".into(),
                provider_route: "ollama".into(),
                local_only: true,
            },
        ),
        (
            "source-beta".into(),
            ContextPackSourceMetadataV0 {
                content_hash: "fnv64:beta".into(),
                provider_route: "ollama".into(),
                local_only: true,
            },
        ),
    ]));
    artifact_metadata
        .evidence
        .entry("source-alpha".into())
        .or_default()
        .insert(
            "ev-shared".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "source-alpha".into(),
                page: 1,
                region: Some("page:Alpha".into()),
                span: Some("alpha".into()),
                quoted_text: "Wrong alpha quote.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                content_hash: "fnv64:alpha".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Text,
            },
        );
    artifact_metadata
        .evidence
        .entry("source-beta".into())
        .or_default()
        .insert(
            "ev-shared".into(),
            ContextPackEvidenceMetadataV0 {
                source_id: "source-beta".into(),
                page: 1,
                region: Some("page:Beta".into()),
                span: Some("beta".into()),
                quoted_text: "Correct beta quote.".into(),
                parse_confidence: ContextPackParseConfidence::High,
                content_hash: "fnv64:beta".into(),
                markdown_path: None,
                image_path: None,
                evidence_type: EvidenceType::Text,
            },
        );

    let external = ContextPackV0::from_brain_context_pack(
        &internal,
        "ctx_test",
        "2026-05-18T09:00:00Z",
        &artifact_metadata,
    );
    assert_eq!(external.selected_evidence.len(), 1);
    assert_eq!(external.selected_evidence[0].source_id, "source-beta");
    assert_eq!(
        external.selected_evidence[0].quoted_text,
        "Correct beta quote."
    );
    assert_eq!(external.selected_evidence[0].content_hash, "fnv64:beta");
}

#[test]
fn provider_model_catalog_round_trip() {
    let mut provider_models = BTreeMap::new();
    provider_models.insert("open_router".into(), vec!["openai/gpt-4.1-mini".into()]);
    provider_models.insert("ollama".into(), vec!["qwen3-vl:8b".into()]);

    let response = EngineSuccess::new(
        EngineCommand::ListProviderModels,
        ProviderModelCatalogResponseData {
            provider_models,
            ollama_vision_prefixes: vec!["qwen3-vl".into()],
        },
    );

    let json = serde_json::to_string(&response).unwrap();
    let decoded: EngineSuccess<ProviderModelCatalogResponseData> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::ListProviderModels);
    assert!(decoded.data.provider_models.contains_key("open_router"));
}

#[test]
fn readiness_response_round_trip() {
    let response = EngineSuccess::new(
        EngineCommand::CheckReadiness,
        RuntimeReadinessResponseData {
            ready: true,
            provider: "ollama".into(),
            model_id: "qwen3-vl:8b".into(),
            checks: vec![ReadinessCheck {
                id: "runtime_process".into(),
                label: "Runtime process".into(),
                ready: true,
                required: true,
                message: "Runtime process is accepting commands.".into(),
            }],
        },
    );

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"command\":\"check_readiness\""));
    let decoded: EngineSuccess<RuntimeReadinessResponseData> =
        serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.command, EngineCommand::CheckReadiness);
    assert!(decoded.data.ready);
}

#[test]
fn import_lifecycle_status_strings_are_stable() {
    assert_eq!(ImportLifecycleStatus::Imported.as_str(), "imported");
    assert_eq!(ImportLifecycleStatus::Parsing.as_str(), "parsing");
    assert_eq!(ImportLifecycleStatus::Packaging.as_str(), "packaging");
    assert_eq!(
        ImportLifecycleStatus::CitationReady.as_str(),
        "citation_ready"
    );
    assert_eq!(
        ImportLifecycleStatus::CitationReadyGraphPending.as_str(),
        "citation_ready_graph_pending"
    );
    assert_eq!(
        ImportLifecycleStatus::CitationReadyGraphSkipped.as_str(),
        "citation_ready_graph_skipped"
    );
    assert_eq!(
        ImportLifecycleStatus::GraphRetryWaiting.as_str(),
        "graph_retry_waiting"
    );
    assert_eq!(
        ImportLifecycleStatus::ContextReady.as_str(),
        "context_ready"
    );
    assert_eq!(ImportLifecycleStatus::Failed.as_str(), "failed");
    assert_eq!(ImportLifecycleStatus::Cancelled.as_str(), "cancelled");
}

#[test]
fn import_lifecycle_truth_table_separates_citation_and_graph_readiness() {
    let pending = ImportLifecycleState::from_persisted(
        "citation_ready_graph_pending",
        "pending",
        true,
        false,
        true,
        true,
    );
    assert_eq!(
        pending.status,
        ImportLifecycleStatus::CitationReadyGraphPending
    );
    assert_eq!(pending.phase, ImportLifecyclePhase::GraphPending);
    assert!(pending.citation_ready);
    assert!(!pending.graph_ready);
    assert!(pending.retryable);
    assert!(pending.manual_retry_available);
    assert!(pending.terminal);

    let skipped = ImportLifecycleState::from_persisted(
        "citation_ready_graph_skipped",
        "skipped",
        true,
        false,
        false,
        true,
    );
    assert_eq!(skipped.phase, ImportLifecyclePhase::GraphSkipped);
    assert!(skipped.citation_ready);
    assert!(!skipped.graph_ready);
    assert!(skipped.terminal);

    let retry_waiting = ImportLifecycleState::from_persisted(
        "graph_retry_waiting",
        "failed_no_materialization",
        true,
        false,
        true,
        true,
    );
    assert_eq!(retry_waiting.phase, ImportLifecyclePhase::GraphRetryWaiting);
    assert!(retry_waiting.citation_ready);
    assert!(!retry_waiting.graph_ready);
    assert!(!retry_waiting.terminal);

    let ready = ImportLifecycleState::from_persisted(
        "context_ready",
        "rebuilt",
        true,
        true,
        false,
        false,
    );
    assert_eq!(ready.phase, ImportLifecyclePhase::ContextReady);
    assert!(ready.citation_ready);
    assert!(ready.graph_ready);
    assert!(ready.terminal);
}

#[test]
fn import_lifecycle_maps_legacy_completed_status_to_graph_pending() {
    let lifecycle =
        ImportLifecycleState::from_persisted("completed", "", true, false, false, true);
    assert_eq!(
        lifecycle.status,
        ImportLifecycleStatus::CitationReadyGraphPending
    );
    assert_eq!(lifecycle.phase, ImportLifecyclePhase::GraphPending);
    assert!(lifecycle.citation_ready);
    assert!(!lifecycle.graph_ready);
}

#[test]
fn import_lifecycle_maps_source_manifest_statuses_to_graph_pending_when_citation_ready() {
    let ingested =
        ImportLifecycleState::from_persisted("ingested", "", true, false, false, true);
    assert_eq!(
        ingested.status,
        ImportLifecycleStatus::CitationReadyGraphPending
    );
    assert_eq!(ingested.phase, ImportLifecyclePhase::GraphPending);
    assert!(ingested.terminal);

    let partial = ImportLifecycleState::from_persisted("partial", "", true, false, false, true);
    assert_eq!(
        partial.status,
        ImportLifecycleStatus::CitationReadyGraphPending
    );
    assert_eq!(partial.phase, ImportLifecyclePhase::GraphPending);
    assert!(partial.terminal);
}

#[test]
fn import_lifecycle_rejects_unknown_persisted_status_as_failed() {
    let lifecycle = ImportLifecycleState::from_persisted(
        "legacy_unrecognized",
        "",
        true,
        false,
        false,
        true,
    );
    assert_eq!(lifecycle.status, ImportLifecycleStatus::Failed);
    assert_eq!(lifecycle.phase, ImportLifecyclePhase::Failed);
    assert!(lifecycle.terminal);
}

#[test]
fn import_lifecycle_normalizes_ready_graph_state_to_context_ready() {
    let lifecycle = ImportLifecycleState::from_persisted(
        "citation_ready_graph_pending",
        "ready",
        true,
        true,
        false,
        false,
    );
    assert_eq!(lifecycle.status, ImportLifecycleStatus::ContextReady);
    assert_eq!(lifecycle.phase, ImportLifecyclePhase::ContextReady);
    assert!(lifecycle.graph_ready);
}

#[test]
fn shared_graph_ready_allowlist_rejects_non_ready_graph_states() {
    assert!(graph_status_is_ready(Some("rebuilt")));
    assert!(graph_status_is_ready(Some("partially_applied")));
    assert!(graph_status_is_ready(Some("ready")));
    assert!(!graph_status_is_ready(Some("skipped")));
    assert!(!graph_status_is_ready(Some("pending")));
    assert!(!graph_status_is_ready(Some("failed_no_materialization")));
    assert!(!graph_status_is_ready(None));
}

#[test]
fn failure_round_trip() {
    let failure = EngineFailure::new(
        EngineCommand::ValidateProvider,
        "invalid_api_key",
        "missing key",
    );
    let json = serde_json::to_string(&failure).unwrap();
    let decoded: EngineFailure = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, failure);
}

#[test]
fn event_round_trip() {
    let event = ParseEvent::Parsing {
        current: 1,
        total: 3,
    };
    let json = serde_json::to_string(&event).unwrap();
    let decoded: ParseEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn options_decode_with_missing_fields() {
    let decoded: ParseOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(decoded, ParseOptions::default());
}
