use anyhow::Result;
use hyprduck_engine_types::{AgentChatAskRequest, AgentChatMessageRole};
use serde_json::Value;

use super::intent::should_reuse_previous_topic_for_context;
use super::prompts::{build_context_query_planner_prompt, CONTEXT_QUERY_PLANNER_PREAMBLE, MAX_PLANNED_CONTEXT_QUERIES, MAX_PLANNED_CONTEXT_QUERY_CHARS};
use super::providers::run_rig_agent;
use crate::domains::retrieval::brain_search::db_search_terms;
use crate::provider::EngineConfig;

pub(super) fn build_context_query(request: &AgentChatAskRequest) -> String {
    let question = request.question.trim();
    if should_reuse_previous_topic_for_context(question) {
        if let Some(query) = build_history_augmented_context_query(request) {
            return query;
        }
    }
    question.into()
}

pub(super) fn build_context_query_candidates(request: &AgentChatAskRequest) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_context_query(&mut candidates, build_context_query(request));
    if let Some(query) = build_history_augmented_context_query(request) {
        push_unique_context_query(&mut candidates, query);
    }
    if let Some(query) = build_cleaned_context_query(request.question.trim()) {
        push_unique_context_query(&mut candidates, query);
    }
    if let Some(query) = build_history_augmented_clean_context_query(request) {
        push_unique_context_query(&mut candidates, query);
    }
    if candidates.is_empty() {
        push_unique_context_query(&mut candidates, request.question.trim().into());
    }
    candidates
}

pub(super) fn push_unique_context_query(candidates: &mut Vec<String>, query: String) {
    let query = query.trim();
    if query.is_empty() || candidates.iter().any(|candidate| candidate == query) {
        return;
    }
    candidates.push(query.into());
}

pub(super) fn build_history_augmented_context_query(request: &AgentChatAskRequest) -> Option<String> {
    let question = request.question.trim();
    request
        .history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .map(|message| message.text.trim())
        .find(|text| !text.is_empty() && !db_search_terms(text).is_empty())
        .map(|previous_question| format!("{previous_question} {question}"))
}

pub(super) fn build_cleaned_context_query(text: &str) -> Option<String> {
    let query = db_search_terms(text).join(" ");
    (!query.is_empty()).then_some(query)
}

pub(super) fn build_history_augmented_clean_context_query(request: &AgentChatAskRequest) -> Option<String> {
    let question = build_cleaned_context_query(request.question.trim());
    request
        .history
        .iter()
        .rev()
        .filter(|message| matches!(message.role, AgentChatMessageRole::User))
        .filter_map(|message| build_cleaned_context_query(message.text.trim()))
        .next()
        .map(|previous_question| match &question {
            Some(question) if !question.is_empty() => format!("{previous_question} {question}"),
            _ => previous_question,
        })
}

pub(super) fn plan_context_query_candidates(
    config: &EngineConfig,
    request: &AgentChatAskRequest,
    attempted_queries: &[String],
) -> Result<Vec<String>> {
    let prompt = build_context_query_planner_prompt(request, attempted_queries);
    let output = run_rig_agent(config, CONTEXT_QUERY_PLANNER_PREAMBLE, "", &prompt)?;
    Ok(parse_context_query_plan(&output))
}

pub(super) fn parse_context_query_plan(output: &str) -> Vec<String> {
    let Some(value) = parse_json_value_from_text(output) else {
        return Vec::new();
    };
    let queries = match value {
        Value::Object(map) => map
            .get("queries")
            .and_then(Value::as_array)
            .map(|values| query_strings_from_json_array(values))
            .unwrap_or_default(),
        Value::Array(values) => query_strings_from_json_array(&values),
        _ => Vec::new(),
    };
    sanitize_planned_context_queries(queries)
}

pub(super) fn query_strings_from_json_array(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

pub(super) fn sanitize_planned_context_queries(queries: Vec<String>) -> Vec<String> {
    let mut sanitized = Vec::new();
    for query in queries {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        let query = query
            .trim()
            .chars()
            .take(MAX_PLANNED_CONTEXT_QUERY_CHARS)
            .collect::<String>();
        if query.is_empty() || sanitized.iter().any(|candidate| candidate == &query) {
            continue;
        }
        sanitized.push(query);
        if sanitized.len() >= MAX_PLANNED_CONTEXT_QUERIES {
            break;
        }
    }
    sanitized
}

pub(super) fn parse_json_value_from_text(output: &str) -> Option<Value> {
    let trimmed = output.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    for (start, _) in output
        .char_indices()
        .filter(|(_, ch)| matches!(ch, '{' | '['))
    {
        let Some(end) = json_value_end(&output[start..]) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&output[start..start + end]) {
            return Some(value);
        }
    }
    None
}

pub(super) fn json_value_end(text: &str) -> Option<usize> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.pop() != Some(ch) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}
