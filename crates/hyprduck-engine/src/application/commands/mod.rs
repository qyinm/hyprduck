use anyhow::Context;
use anyhow::Result;
use hyprduck_engine_types::{
    EngineCommand, EngineRequest, EngineSuccess, LoadConfigRequest, SaveConfigRequest,
    SaveConfigResponseData, ValidateProviderRequest,
};

use crate::application::services::{
    agent_chat_service, brain_health_service, brain_read_service, context_pack_service,
    project_service,
};
use crate::provider::{
    check_readiness, provider_model_catalog, validate_provider, EngineConfig, EngineConfigStore,
};

pub(crate) mod brain_write;
pub(crate) mod graph;

pub(crate) fn encode_success_response(
    request: EngineRequest,
    config_store: &EngineConfigStore,
) -> Result<String> {
    let response = match request {
        EngineRequest::Parse(_) => {
            unreachable!("parse requests are handled by runtime::encode_parse_response")
        }
        EngineRequest::RetryFailedPages(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::RetryFailedPages,
            crate::application::services::ingest_service::handle_retry_failed_pages(
                request,
                &config_store.load()?,
            )?,
        )),
        EngineRequest::CompileProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::CompileProject,
            project_service::handle_compile_project(request)?,
        )),
        EngineRequest::ReadImportJob(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadImportJob,
            project_service::handle_read_import_job(request)?,
        )),
        EngineRequest::UpdateImportJobGraphStatus(request) => {
            serde_json::to_string(&EngineSuccess::new(
                EngineCommand::UpdateImportJobGraphStatus,
                project_service::handle_update_import_job_graph_status(request)?,
            ))
        }
        EngineRequest::LoadProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::LoadProject,
            project_service::handle_load_project(request)?,
        )),
        EngineRequest::ApplyCorrection(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ApplyCorrection,
            project_service::handle_apply_correction(request)?,
        )),
        EngineRequest::AnswerProject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::AnswerProject,
            project_service::handle_answer_project(request)?,
        )),
        EngineRequest::AgentChatAsk(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::AgentChatAsk,
            agent_chat_service::handle_agent_chat_ask(request, &config_store.load()?)?,
        )),
        EngineRequest::SearchBrain(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::SearchBrain,
            brain_read_service::handle_search_brain(request)?,
        )),
        EngineRequest::ReadSource(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadSource,
            brain_read_service::handle_read_source(request)?,
        )),
        EngineRequest::ReadPageEvidence(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadPageEvidence,
            brain_read_service::handle_read_page_evidence(request)?,
        )),
        EngineRequest::ReadContextPack(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadContextPack,
            context_pack_service::handle_read_context_pack(request)?,
        )),
        EngineRequest::ReadWikiPage(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadWikiPage,
            brain_read_service::handle_read_wiki_page(request)?,
        )),
        EngineRequest::ReadNode(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadNode,
            brain_read_service::handle_read_node(request)?,
        )),
        EngineRequest::ReadRecentEvents(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ReadRecentEvents,
            brain_read_service::handle_read_recent_events(request)?,
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
            context_pack_service::handle_get_context_pack(request)?,
        )),
        EngineRequest::GetBrainHealth(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::GetBrainHealth,
            brain_health_service::handle_get_brain_health(request)?,
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
        EngineRequest::ApplyGraphPatch(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::ApplyGraphPatch,
            graph::handle_apply_graph_patch(request)?,
        )),
        EngineRequest::WritePropose(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::WritePropose,
            brain_write::handle_write_propose(request)?,
        )),
        EngineRequest::WriteCommit(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::WriteCommit,
            brain_write::handle_write_commit(request)?,
        )),
        EngineRequest::WriteCommitAll(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::WriteCommitAll,
            brain_write::handle_write_commit_all(request)?,
        )),
        EngineRequest::WriteList(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::WriteList,
            brain_write::handle_write_list(request)?,
        )),
        EngineRequest::WriteReject(request) => serde_json::to_string(&EngineSuccess::new(
            EngineCommand::WriteReject,
            brain_write::handle_write_reject(request)?,
        )),
    }
    .context("failed to encode engine response")?;
    Ok(response)
}

pub(crate) fn request_command(request: &EngineRequest) -> EngineCommand {
    match request {
        EngineRequest::Parse(_) => EngineCommand::Parse,
        EngineRequest::RetryFailedPages(_) => EngineCommand::RetryFailedPages,
        EngineRequest::CompileProject(_) => EngineCommand::CompileProject,
        EngineRequest::ReadImportJob(_) => EngineCommand::ReadImportJob,
        EngineRequest::UpdateImportJobGraphStatus(_) => EngineCommand::UpdateImportJobGraphStatus,
        EngineRequest::LoadProject(_) => EngineCommand::LoadProject,
        EngineRequest::ApplyCorrection(_) => EngineCommand::ApplyCorrection,
        EngineRequest::AnswerProject(_) => EngineCommand::AnswerProject,
        EngineRequest::AgentChatAsk(_) => EngineCommand::AgentChatAsk,
        EngineRequest::SearchBrain(_) => EngineCommand::SearchBrain,
        EngineRequest::ReadSource(_) => EngineCommand::ReadSource,
        EngineRequest::ReadPageEvidence(_) => EngineCommand::ReadPageEvidence,
        EngineRequest::ReadContextPack(_) => EngineCommand::ReadContextPack,
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
        EngineRequest::ApplyGraphPatch(_) => EngineCommand::ApplyGraphPatch,
        EngineRequest::WritePropose(_) => EngineCommand::WritePropose,
        EngineRequest::WriteCommit(_) => EngineCommand::WriteCommit,
        EngineRequest::WriteCommitAll(_) => EngineCommand::WriteCommitAll,
        EngineRequest::WriteList(_) => EngineCommand::WriteList,
        EngineRequest::WriteReject(_) => EngineCommand::WriteReject,
    }
}
