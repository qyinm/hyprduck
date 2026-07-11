use super::*;
use crate::chat_openai_compatible_client::{
    parse_openai_compatible, provider_failure, provider_unavailable, ProviderFailureKind,
};

pub(crate) fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    reject_unknown_provider(config)?;
    if provider_unavailable(config) {
        return Err(provider_failure(
            ProviderFailureKind::ProviderConfig,
            format!(
                "provider `{}` is not configured for image parsing; template `{template}`, image bytes {}",
                config.provider.id_slug(),
                image_bytes.len()
            ),
        ));
    }

    let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let prompt = format!(
        "Convert this document page into clean markdown. Template: {template}. Preserve headings, lists, tables, and code blocks where possible."
    );
    if config.provider.uses_openai_compatible_chat_api() {
        parse_openai_compatible(config, &prompt, Some(image_base64))
    } else {
        unreachable!("unknown provider rejected before parse dispatch")
    }
}

pub(crate) fn parse_text_with_provider(
    config: &EngineConfig,
    text: &str,
    template: &str,
) -> Result<String> {
    reject_unknown_provider(config)?;
    if provider_unavailable(config) {
        return Err(provider_failure(
            ProviderFailureKind::ProviderConfig,
            format!(
                "provider `{}` is not configured for text parsing; template `{template}`",
                config.provider.id_slug()
            ),
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    if config.provider.uses_openai_compatible_chat_api() {
        parse_openai_compatible(config, &prompt, None)
    } else {
        unreachable!("unknown provider rejected before parse dispatch")
    }
}

fn reject_unknown_provider(config: &EngineConfig) -> Result<()> {
    if let ProviderKind::Unknown(slug) = &config.provider {
        return Err(provider_failure(
            ProviderFailureKind::UnsupportedProvider,
            format!("unsupported provider `{slug}`"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: ProviderKind) -> EngineConfig {
        EngineConfig {
            provider,
            model_id: "test-model".into(),
            api_key: String::new(),
            base_url: None,
            prompt_template: "General".into(),
        }
    }

    #[test]
    fn text_parse_classifies_unknown_provider() {
        let error = parse_text_with_provider(
            &config(ProviderKind::Unknown("legacy_ai".into())),
            "source text",
            "General",
        )
        .expect_err("unknown provider should be rejected");

        assert!(error.to_string().starts_with("unsupported_provider:"));
    }

    #[test]
    fn image_parse_classifies_unknown_provider() {
        let error = parse_image_with_provider(
            &config(ProviderKind::Unknown("legacy_ai".into())),
            b"image",
            "General",
        )
        .expect_err("unknown provider should be rejected");

        assert!(error.to_string().starts_with("unsupported_provider:"));
    }

    #[test]
    fn text_parse_classifies_missing_openrouter_key() {
        let error =
            parse_text_with_provider(&config(ProviderKind::OpenRouter), "source text", "General")
                .expect_err("missing OpenRouter key should be classified");

        assert!(error.to_string().starts_with("provider_config:"));
    }

    #[test]
    fn image_parse_classifies_missing_openrouter_key() {
        let error =
            parse_image_with_provider(&config(ProviderKind::OpenRouter), b"image", "General")
                .expect_err("missing OpenRouter key should be classified");

        assert!(error.to_string().starts_with("provider_config:"));
    }
}
