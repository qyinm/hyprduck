use super::*;

pub(crate) fn provider_model_catalog() -> ProviderModelCatalogResponseData {
    let provider_models = ProviderKind::all()
        .into_iter()
        .map(|provider| {
            (
                provider.id_slug().to_string(),
                model_options_for(&provider)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        })
        .collect();

    ProviderModelCatalogResponseData {
        provider_models,
        ollama_vision_prefixes: etyma_engine_types::ollama_vision_prefixes()
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}
