use anyhow::Context;
use anyhow::Result;
use hyprduck_engine_types::{
    EngineCommand, EngineRequest, EngineSuccess, LoadConfigRequest, SaveConfigRequest,
    SaveConfigResponseData, ValidateProviderRequest,
};

use crate::provider::{
    check_readiness, provider_model_catalog, validate_provider, EngineConfig, EngineConfigStore,
};

pub(crate) fn encode_success_response(
    request: EngineRequest,
    config_store: &EngineConfigStore,
) -> Result<String> {
    let response = match request {
        EngineRequest::Parse(_) => {
            unreachable!("parse requests are handled by runtime::encode_parse_response")
        }
        EngineRequest::CompileProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::CompileProject,
            crate::handle_compile_project(request)?,
        )),
        EngineRequest::LoadProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::LoadProject,
            crate::handle_load_project(request)?,
        )),
        EngineRequest::ApplyCorrection(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ApplyCorrection,
            crate::handle_apply_correction(request)?,
        )),
        EngineRequest::AnswerProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::AnswerProject,
            crate::handle_answer_project(request)?,
        )),
        EngineRequest::SearchBrain(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::SearchBrain,
            crate::handle_search_brain(request)?,
        )),
        EngineRequest::ReadSource(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadSource,
            crate::handle_read_source(request)?,
        )),
        EngineRequest::ReadWikiPage(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadWikiPage,
            crate::handle_read_wiki_page(request)?,
        )),
        EngineRequest::ReadNode(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadNode,
            crate::handle_read_node(request)?,
        )),
        EngineRequest::ReadRecentEvents(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadRecentEvents,
            crate::handle_read_recent_events(request)?,
        )),
        EngineRequest::ReadGraphHistory(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadGraphHistory,
            crate::handle_read_graph_history(request)?,
        )),
        EngineRequest::ReadGraphSnapshot(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadGraphSnapshot,
            crate::handle_read_graph_snapshot(request)?,
        )),
        EngineRequest::ReconstructBrain(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReconstructBrain,
            crate::handle_reconstruct_brain(request)?,
        )),
        EngineRequest::GetContextPack(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::GetContextPack,
            crate::handle_get_context_pack(request)?,
        )),
        EngineRequest::GetBrainHealth(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::GetBrainHealth,
            crate::handle_get_brain_health(request)?,
        )),
        EngineRequest::LoadConfig(LoadConfigRequest {}) => {
            let config = config_store.load()?;
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::LoadConfig,
                config.to_payload(),
            ))
        }
        EngineRequest::SaveConfig(SaveConfigRequest { config }) => {
            let config = EngineConfig::from_payload(config);
            config_store.save(&config)?;
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::SaveConfig,
                SaveConfigResponseData {
                    config: config.to_payload(),
                    persisted: true,
                },
            ))
        }
        EngineRequest::ValidateProvider(ValidateProviderRequest { config }) => {
            let config = config
                .map(EngineConfig::from_payload)
                .unwrap_or(config_store.load()?);
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::ValidateProvider,
                validate_provider(&config),
            ))
        }
        EngineRequest::ListProviderModels(_) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ListProviderModels,
            provider_model_catalog(),
        )),
        EngineRequest::CheckReadiness(_) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::CheckReadiness,
            check_readiness(config_store),
        )),
    }
    .context("failed to encode engine response")?;
    Ok(response)
}

pub(crate) fn request_command(request: &EngineRequest) -> EngineCommand {
    match request {
        EngineRequest::Parse(_) => EngineCommand::Parse,
        EngineRequest::CompileProject(_) => EngineCommand::CompileProject,
        EngineRequest::LoadProject(_) => EngineCommand::LoadProject,
        EngineRequest::ApplyCorrection(_) => EngineCommand::ApplyCorrection,
        EngineRequest::AnswerProject(_) => EngineCommand::AnswerProject,
        EngineRequest::SearchBrain(_) => EngineCommand::SearchBrain,
        EngineRequest::ReadSource(_) => EngineCommand::ReadSource,
        EngineRequest::ReadWikiPage(_) => EngineCommand::ReadWikiPage,
        EngineRequest::ReadNode(_) => EngineCommand::ReadNode,
        EngineRequest::ReadRecentEvents(_) => EngineCommand::ReadRecentEvents,
        EngineRequest::ReadGraphHistory(_) => EngineCommand::ReadGraphHistory,
        EngineRequest::ReadGraphSnapshot(_) => EngineCommand::ReadGraphSnapshot,
        EngineRequest::GetContextPack(_) => EngineCommand::GetContextPack,
        EngineRequest::GetBrainHealth(_) => EngineCommand::GetBrainHealth,
        EngineRequest::ReconstructBrain(_) => EngineCommand::ReconstructBrain,
        EngineRequest::LoadConfig(_) => EngineCommand::LoadConfig,
        EngineRequest::SaveConfig(_) => EngineCommand::SaveConfig,
        EngineRequest::ValidateProvider(_) => EngineCommand::ValidateProvider,
        EngineRequest::ListProviderModels(_) => EngineCommand::ListProviderModels,
        EngineRequest::CheckReadiness(_) => EngineCommand::CheckReadiness,
    }
}
