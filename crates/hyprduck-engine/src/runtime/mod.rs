use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};

use anyhow::{Context, Result};
use hyprduck_engine_types::{
    AgentChatAskRequest, AgentChatStreamEvent, EngineCommand, EngineFailure, EngineRequest,
    EngineRuntimeEvent, EngineRuntimeFailure, EngineRuntimeRequest, EngineRuntimeResponse,
    EngineSuccess, ParseEvent, ParseRequest,
};
use serde_json::{json, Value};
use uuid::{Uuid, Version};

use crate::provider::EngineConfigStore;

thread_local! {
    static RUNTIME_EVENT_REQUEST_ID: RefCell<Option<Uuid>> = const { RefCell::new(None) };
}

pub fn run() -> Result<()> {
    if std::env::args().skip(1).any(|arg| arg == "serve") {
        return run_runtime_server();
    }

    let mut payload = String::new();
    io::stdin()
        .read_to_string(&mut payload)
        .context("failed to read engine request")?;
    let request = decode_request(&payload)?;
    let config_store = EngineConfigStore::default()?;
    let response = match request {
        EngineRequest::Parse(request) => encode_parse_response(request, &payload, &config_store)?,
        request => crate::application::commands::encode_success_response(request, &config_store)?,
    };
    io::stdout()
        .write_all(response.as_bytes())
        .context("failed to write engine response")?;
    Ok(())
}

fn run_runtime_server() -> Result<()> {
    let stdin = io::stdin();
    let config_store = EngineConfigStore::default()?;

    for line in stdin.lock().lines() {
        let payload = line.context("failed to read runtime request")?;
        if payload.trim().is_empty() {
            continue;
        }

        let response = match decode_runtime_request(&payload) {
            Ok(envelope) if !is_uuid_v7(envelope.id) => {
                let command = crate::application::commands::request_command(&envelope.request);
                serde_json::to_string(&EngineRuntimeFailure::new(
                    envelope.id,
                    EngineFailure::new(
                        command,
                        "invalid_request_id",
                        "runtime request id must be a UUIDv7 string",
                    ),
                ))
                .context("failed to encode invalid runtime request id response")?
            }
            Ok(envelope) => {
                let id = envelope.id;
                match envelope.request {
                    EngineRequest::Parse(request) => {
                        encode_runtime_parse_response(id, request, &payload, &config_store)
                            .unwrap_or_else(|error| {
                                encode_runtime_failure_response(id, EngineCommand::Parse, &error)
                            })
                    }
                    EngineRequest::AgentChatAsk(request) => {
                        encode_runtime_agent_chat_response(id, request, &config_store)
                            .unwrap_or_else(|error| {
                                encode_runtime_failure_response(
                                    id,
                                    EngineCommand::AgentChatAsk,
                                    &error,
                                )
                            })
                    }
                    request => {
                        let command = crate::application::commands::request_command(&request);
                        crate::application::commands::encode_success_response(
                            request,
                            &config_store,
                        )
                        .and_then(|response| wrap_runtime_response(id, &response))
                        .unwrap_or_else(|error| {
                            encode_runtime_failure_response(id, command, &error)
                        })
                    }
                }
            }
            Err(error) => serde_json::to_string(&json!({
                "id": null,
                "type": "response",
                "ok": false,
                "error": {
                    "code": "invalid_request",
                    "message": error.to_string()
                }
            }))
            .context("failed to encode invalid runtime request response")?,
        };
        io::stdout()
            .write_all(response.as_bytes())
            .context("failed to write runtime response")?;
        io::stdout()
            .write_all(b"\n")
            .context("failed to write runtime response newline")?;
        io::stdout()
            .flush()
            .context("failed to flush runtime response")?;
    }
    Ok(())
}

fn decode_request(payload: &str) -> Result<EngineRequest> {
    serde_json::from_str(payload)
        .or_else(|_| serde_json::from_str::<ParseRequest>(payload).map(EngineRequest::Parse))
        .context("failed to decode engine request JSON")
}

fn decode_runtime_request(payload: &str) -> Result<EngineRuntimeRequest> {
    serde_json::from_str(payload).context("failed to decode runtime request JSON")
}

fn is_uuid_v7(value: Uuid) -> bool {
    value.get_version() == Some(Version::SortRand)
}

fn encode_runtime_parse_response(
    request_id: Uuid,
    request: ParseRequest,
    raw_payload: &str,
    config_store: &EngineConfigStore,
) -> Result<String> {
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = Some(request_id);
    });
    let response = encode_parse_response(request, raw_payload, config_store)
        .and_then(|response| wrap_runtime_response(request_id, &response));
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = None;
    });
    response
}

fn encode_runtime_agent_chat_response(
    request_id: Uuid,
    request: AgentChatAskRequest,
    config_store: &EngineConfigStore,
) -> Result<String> {
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = Some(request_id);
    });
    let response = crate::application::services::agent_chat_service::handle_agent_chat_stream(
        request,
        &config_store.load()?,
        &mut |event| emit_agent_chat_event(&event),
    )
    .map(|data| serde_json::to_string(&EngineSuccess::new(EngineCommand::AgentChatAsk, data)))
    .and_then(|response| response.context("failed to encode agent chat response"))
    .and_then(|response| wrap_runtime_response(request_id, &response));
    if let Err(error) = &response {
        let _ = emit_agent_chat_event(&AgentChatStreamEvent::Error {
            code: "runtime_error".into(),
            message: error.to_string(),
        });
    }
    RUNTIME_EVENT_REQUEST_ID.with(|current| {
        *current.borrow_mut() = None;
    });
    response
}

fn encode_parse_response(
    request: ParseRequest,
    raw_payload: &str,
    config_store: &EngineConfigStore,
) -> Result<String> {
    crate::application::services::ingest_service::maybe_write_debug(
        &request.options.debug_request_path,
        raw_payload,
    )?;
    let debug_result_path = request.options.debug_result_path.clone();
    let response =
        crate::application::services::ingest_service::handle_parse(request, config_store)
            .map(|data| serde_json::to_string(&EngineSuccess::new(EngineCommand::Parse, data)))
            .unwrap_or_else(|error| {
                let _ = emit_event(&ParseEvent::Failed {
                    message: error.to_string(),
                });
                serde_json::to_string(&crate::engine_failure(EngineCommand::Parse, &error))
            })
            .context("failed to encode parse response")?;
    crate::application::services::ingest_service::maybe_write_debug(&debug_result_path, &response)?;
    Ok(response)
}

fn wrap_runtime_response(request_id: Uuid, response: &str) -> Result<String> {
    if let Ok(success) = serde_json::from_str::<EngineSuccess<Value>>(response) {
        return serde_json::to_string(&EngineRuntimeResponse::new(request_id, success))
            .context("failed to encode runtime response");
    }

    let failure = serde_json::from_str::<EngineFailure>(response)
        .context("failed to decode engine response for runtime envelope")?;
    serde_json::to_string(&EngineRuntimeFailure::new(request_id, failure))
        .context("failed to encode runtime failure response")
}

fn encode_runtime_failure_response(
    request_id: Uuid,
    command: EngineCommand,
    error: &anyhow::Error,
) -> String {
    serde_json::to_string(&EngineRuntimeFailure::new(
        request_id,
        crate::engine_failure(command, error),
    ))
    .unwrap_or_else(|_| crate::encode_failure_response(command, error))
}

pub(crate) fn emit_event(event: &ParseEvent) -> Result<()> {
    if let Some(request_id) = RUNTIME_EVENT_REQUEST_ID.with(|current| *current.borrow()) {
        let line = serde_json::to_string(&EngineRuntimeEvent::parse(request_id, event.clone()))
            .context("failed to encode runtime parse event")?;
        let mut stderr = io::stderr().lock();
        stderr
            .write_all(line.as_bytes())
            .context("failed to write runtime parse event")?;
        stderr
            .write_all(b"\n")
            .context("failed to write runtime parse event newline")?;
        stderr
            .flush()
            .context("failed to flush runtime parse event")?;
        return Ok(());
    }

    let line = serde_json::to_string(event).context("failed to encode parse event")?;
    eprintln!("{line}");
    Ok(())
}

pub(crate) fn emit_agent_chat_event(event: &AgentChatStreamEvent) -> Result<()> {
    if let Some(request_id) = RUNTIME_EVENT_REQUEST_ID.with(|current| *current.borrow()) {
        let line =
            serde_json::to_string(&EngineRuntimeEvent::agent_chat(request_id, event.clone()))
                .context("failed to encode runtime agent chat event")?;
        let mut stderr = io::stderr().lock();
        stderr
            .write_all(line.as_bytes())
            .context("failed to write runtime agent chat event")?;
        stderr
            .write_all(b"\n")
            .context("failed to write runtime agent chat event newline")?;
        stderr
            .flush()
            .context("failed to flush runtime agent chat event")?;
        return Ok(());
    }

    let line = serde_json::to_string(event).context("failed to encode agent chat event")?;
    eprintln!("{line}");
    Ok(())
}
