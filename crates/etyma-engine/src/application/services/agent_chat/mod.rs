mod citations;
mod intent;
mod prompts;
mod providers;
mod query_plan;
mod state;
mod workflow;

pub(crate) use workflow::{handle_agent_chat_ask, handle_agent_chat_stream};

#[cfg(test)]
mod tests {
    use super::citations::validate_model_citations;
    use super::intent::{
        looks_like_evidence_question, should_answer_as_general_chat, should_retrieve_context,
    };
    use super::prompts::{build_citation_repair_user_prompt, build_evidence_user_prompt};
    use super::providers::run_rig_agent;
    use super::query_plan::{
        build_context_query, build_context_query_candidates, parse_context_query_plan,
    };
    use super::state::{empty_context_pack, AgentChatRunState};
    use super::workflow::{
        build_response, classify_answer_mode, filter_context_pack_for_scope,
        has_citation_ready_context, provider_summary,
    };
    use crate::provider::{EngineConfig, ProviderKind};
    use etyma_engine_types::{
        AgentChatAnswerMode, AgentChatAskRequest, AgentChatMessage, AgentChatMessageRole,
        AgentChatProviderSummary, AgentChatScopeMode, AnswerStatus, BrainReadScope,
        ContextPackEvidenceTypeTraceV1, ContextPackEvidenceV1, ContextPackFindingV0,
        ContextPackGraphRecordKindV1, ContextPackGraphRecordV1, ContextPackGraphTrailV1,
        ContextPackParseConfidence, ContextPackRetrievalTraceV1, ContextPackSourceV0,
        ContextPackStaleness, ContextPackV1, ContextPackWarningSeverity, ContextPackWarningV0,
        EvidenceType, AGENT_CHAT_SCHEMA_VERSION,
    };

    fn request(mode: AgentChatScopeMode) -> AgentChatAskRequest {
        AgentChatAskRequest {
            schema_version: AGENT_CHAT_SCHEMA_VERSION.into(),
            conversation_id: "conversation_1".into(),
            assistant_message_id: None,
            scope: BrainReadScope {
                workspace_id: "default".into(),
                root_dir: None,
            },
            mode,
            selected_node_id: Some("node_a".into()),
            source_ids: vec!["source_a".into()],
            question: "What changed?".into(),
            history: Vec::new(),
            budget: Some(1024),
            persist_context_pack: false,
        }
    }

    fn context_pack() -> ContextPackV1 {
        ContextPackV1 {
            schema_version: "etyma.context_pack.v1".into(),
            pack_id: "ctx_test".into(),
            workspace_id: "default".into(),
            query: "What changed?".into(),
            generated_at: "2026-06-18T00:00:00Z".into(),
            source_set: vec![ContextPackSourceV0 {
                source_id: "source_a".into(),
                original_filename: "a.pdf".into(),
                content_hash: "hash".into(),
                page_count: 1,
                ingestion_status: "ingested".into(),
                staleness: ContextPackStaleness::Current,
                provider_route: "local".into(),
                local_only: true,
            }],
            selected_evidence: vec![ContextPackEvidenceV1 {
                evidence_ref: "ev_a".into(),
                source_id: "source_a".into(),
                page: 1,
                region: None,
                span: None,
                quoted_text: "quoted evidence".into(),
                parse_confidence: ContextPackParseConfidence::High,
                selection_reason: "top match".into(),
                content_hash: "hash".into(),
                evidence_type: EvidenceType::Text,
                graph_trail: Some(ContextPackGraphTrailV1 {
                    direct: vec![ContextPackGraphRecordV1 {
                        record_type: ContextPackGraphRecordKindV1::Evidence,
                        id: "ev_a".into(),
                        reason: "selected".into(),
                    }],
                    adjacent: Vec::new(),
                    follow_up: Vec::new(),
                    unavailable_reason: None,
                }),
            }],
            findings: Vec::<ContextPackFindingV0>::new(),
            warnings: vec![ContextPackWarningV0 {
                warning_type: "low_parse_confidence".into(),
                severity: ContextPackWarningSeverity::Low,
                message: "warning".into(),
                page_refs: Vec::new(),
            }],
            retrieval_trace: ContextPackRetrievalTraceV1 {
                strategy: "test".into(),
                chunks_considered: 1,
                chunks_selected: 1,
                budget_requested: 1024,
                budget_used: 10,
                evidence_type_trace: ContextPackEvidenceTypeTraceV1::default(),
            },
            suggested_next_reads: Vec::new(),
        }
    }

    #[test]
    fn citation_validation_drops_hallucinated_refs() {
        let pack = context_pack();
        let citations = validate_model_citations("Use [ev_a] but not [ev_fake].", &pack);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].evidence_ref, "ev_a");
    }

    #[test]
    fn source_set_without_selected_evidence_is_not_citation_ready() {
        let mut pack = context_pack();
        pack.selected_evidence.clear();

        assert!(!has_citation_ready_context(&pack));
        assert_eq!(
            classify_answer_mode(&request(AgentChatScopeMode::Auto), false, false),
            AgentChatAnswerMode::General
        );
        assert_eq!(
            classify_answer_mode(&request(AgentChatScopeMode::AllDocs), false, false),
            AgentChatAnswerMode::Blocked
        );
    }

    #[test]
    fn citation_repair_prompt_names_valid_evidence_refs() {
        let request = request(AgentChatScopeMode::Auto);
        let pack = context_pack();
        let prompt = build_citation_repair_user_prompt(&request, &pack, "draft without refs");

        assert!(prompt.contains("Valid evidenceRefs: ev_a"));
        assert!(prompt.contains("Previous draft:"));
        assert!(prompt.contains("draft without refs"));
    }

    #[test]
    fn evidence_prompt_excludes_stale_assistant_history() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![
            AgentChatMessage {
                id: "msg_user_1".into(),
                role: AgentChatMessageRole::User,
                text: "summarize the parser fixture".into(),
                created_at: 1,
            },
            AgentChatMessage {
                id: "msg_assistant_1".into(),
                role: AgentChatMessageRole::Assistant,
                text: "The context pack only contains unrelated queue content.".into(),
                created_at: 2,
            },
        ];
        let prompt = build_evidence_user_prompt(&request, &context_pack());

        assert!(prompt.contains("summarize the parser fixture"));
        assert!(prompt.contains("Do not treat previous assistant messages as evidence."));
        assert!(!prompt.contains("unrelated queue content"));
    }

    #[test]
    fn selected_source_filters_context_pack() {
        let mut pack = context_pack();
        pack.selected_evidence.push(ContextPackEvidenceV1 {
            evidence_ref: "ev_b".into(),
            source_id: "source_b".into(),
            page: 1,
            region: None,
            span: None,
            quoted_text: "other evidence".into(),
            parse_confidence: ContextPackParseConfidence::High,
            selection_reason: "other".into(),
            content_hash: "hash_b".into(),
            evidence_type: EvidenceType::Text,
            graph_trail: None,
        });
        let filtered =
            filter_context_pack_for_scope(&pack, &request(AgentChatScopeMode::SelectedSource));
        assert_eq!(filtered.selected_evidence.len(), 1);
        assert_eq!(filtered.selected_evidence[0].source_id, "source_a");
    }

    #[test]
    fn general_greeting_uses_general_answer_mode_without_context() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "HI".into();
        assert!(should_answer_as_general_chat(&request));
        assert!(!should_retrieve_context(&request, true));
        assert_eq!(
            classify_answer_mode(&request, true, false),
            AgentChatAnswerMode::General
        );
    }

    #[test]
    fn auto_topic_question_retrieves_context_before_general_fallback() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "dynamic hashing 내용 설명해".into();
        assert!(!should_answer_as_general_chat(&request));
        assert!(should_retrieve_context(&request, false));
        assert_eq!(
            classify_answer_mode(&request, false, false),
            AgentChatAnswerMode::General
        );
        assert_eq!(build_context_query(&request), "dynamic hashing 내용 설명해");
    }

    #[test]
    fn follow_up_context_query_reuses_previous_user_topic() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![
            AgentChatMessage {
                id: "msg_user_1".into(),
                role: AgentChatMessageRole::User,
                text: "dynamic hashing 알려줘".into(),
                created_at: 1,
            },
            AgentChatMessage {
                id: "msg_assistant_1".into(),
                role: AgentChatMessageRole::Assistant,
                text: "It is in section 8.3.".into(),
                created_at: 2,
            },
        ];
        request.question = "내용 없어?".into();

        assert_eq!(
            build_context_query(&request),
            "dynamic hashing 알려줘 내용 없어?"
        );
    }

    #[test]
    fn generic_evidence_follow_up_context_query_reuses_previous_user_topic() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history = vec![AgentChatMessage {
            id: "msg_user_1".into(),
            role: AgentChatMessageRole::User,
            text: "graph contains parser fixture evidence".into(),
            created_at: 1,
        }];
        request.question = "source summary".into();

        let candidates = build_context_query_candidates(&request);

        assert!(
            candidates
                .iter()
                .any(|candidate| candidate
                    == "graph contains parser fixture evidence source summary"),
            "{candidates:#?}"
        );
    }

    #[test]
    fn context_query_candidates_keep_raw_question_first() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "source summary".into();

        let candidates = build_context_query_candidates(&request);

        assert_eq!(
            candidates.first().map(String::as_str),
            Some("source summary")
        );
    }

    #[test]
    fn blocked_retrieval_can_advance_to_cleaned_query_candidate() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "dynamic hashing 내용".into();
        let mut run = AgentChatRunState::new(
            &request,
            AgentChatProviderSummary {
                id: "test".into(),
                label: "Test".into(),
                model_id: "test-model".into(),
                hosted: false,
            },
        );
        run.set_context_query_candidates(build_context_query_candidates(&request));
        run.retrieval_attempts = 1;

        assert_eq!(run.context_query, "dynamic hashing 내용");
        assert!(run.advance_context_query());
        assert_eq!(run.context_query, "dynamic hashing");
    }

    #[test]
    fn planned_context_queries_are_appended_for_retrieval_retry() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.history.clear();
        request.question = "source summary".into();
        let mut run = AgentChatRunState::new(
            &request,
            AgentChatProviderSummary {
                id: "test".into(),
                label: "Test".into(),
                model_id: "test-model".into(),
                hosted: false,
            },
        );
        run.set_context_query_candidates(build_context_query_candidates(&request));
        run.retrieval_attempts = 1;

        assert_eq!(run.context_query, "source summary");
        assert!(!run.advance_context_query());
        assert!(run.extend_context_query_candidates(vec![
            "parser fixture".into(),
            "source summary".into()
        ]));
        assert!(run.advance_context_query());
        assert_eq!(run.context_query, "parser fixture");
    }

    #[test]
    fn context_query_plan_parser_accepts_json_object() {
        let queries =
            parse_context_query_plan(r#"{"queries":["parser fixture","indexed source"]}"#);

        assert_eq!(queries, vec!["parser fixture", "indexed source"]);
    }

    #[test]
    fn context_query_plan_parser_accepts_embedded_json_array() {
        let queries = parse_context_query_plan(
            "Plan:\n```json\n[\"fixture parser\", \"metadata source\"]\n```",
        );

        assert_eq!(queries, vec!["fixture parser", "metadata source"]);
    }

    #[test]
    fn evidence_question_without_context_is_blocked() {
        let mut request = request(AgentChatScopeMode::AllDocs);
        request.question = "Summarize the source evidence".into();
        assert!(!should_answer_as_general_chat(&request));
        assert_eq!(
            classify_answer_mode(&request, false, false),
            AgentChatAnswerMode::Blocked
        );
    }

    #[test]
    fn evidence_question_without_context_is_blocked_instead_of_general_chat() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.question = "source summary".into();

        assert!(!should_answer_as_general_chat(&request));
        assert!(looks_like_evidence_question(&request.question));
        assert_eq!(
            classify_answer_mode(&request, false, false),
            AgentChatAnswerMode::Blocked
        );
    }

    #[test]
    fn build_response_preserves_requested_assistant_message_id() {
        let mut request = request(AgentChatScopeMode::Auto);
        request.assistant_message_id = Some("assistant-1".into());
        let response = build_response(
            &request,
            &empty_context_pack(&request),
            AgentChatAnswerMode::General,
            provider_summary(&EngineConfig {
                provider: ProviderKind::Ollama,
                model_id: "llama3.1".into(),
                api_key: "".into(),
                base_url: None,
                prompt_template: "General".into(),
            }),
            None,
            "Hello".into(),
            AnswerStatus::LowConfidence,
            Vec::new(),
            Vec::new(),
            "General response.",
        );
        assert_eq!(response.assistant_message.id, "assistant-1");
        assert_eq!(response.answer_mode, AgentChatAnswerMode::General);
    }

    #[test]
    fn provider_config_error_is_specific_for_missing_openrouter_key() {
        let config = EngineConfig {
            provider: ProviderKind::OpenRouter,
            model_id: "openai/gpt-4.1-mini".into(),
            api_key: "".into(),
            base_url: None,
            prompt_template: "General".into(),
        };
        let error = run_rig_agent(&config, "preamble", "context", "prompt")
            .unwrap_err()
            .to_string();
        assert!(error.contains("OpenRouter requires an API key"));
    }
}

