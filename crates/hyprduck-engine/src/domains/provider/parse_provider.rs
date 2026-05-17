use super::*;
use crate::chat_openai_compatible_client::{parse_openai_compatible, provider_unavailable};

pub(crate) fn parse_image_with_provider(
    config: &EngineConfig,
    image_bytes: &[u8],
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this page was packaged as an image-only placeholder.\n\n- Template: {}\n- Image bytes: {}\n",
            config.provider.id_slug(),
            template,
            image_bytes.len()
        ));
    }

    let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
    let prompt = format!(
        "Convert this document page into clean markdown. Template: {template}. Preserve headings, lists, tables, and code blocks where possible."
    );
    match &config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, Some(image_base64))
        }
        ProviderKind::Unknown(slug) => Err(anyhow!("unsupported provider `{slug}`")),
    }
}

pub(crate) fn parse_text_with_provider(
    config: &EngineConfig,
    text: &str,
    template: &str,
) -> Result<String> {
    if provider_unavailable(config) {
        return Ok(format!(
            "_HyprDuck fallback parse._\n\nProvider `{}` is not configured or reachable, so this document was returned from extracted text.\n\n- Template: {}\n\n{}",
            config.provider.id_slug(),
            template,
            text
        ));
    }

    let prompt = format!(
        "Convert the following extracted document text into clean markdown. Template: {template}.\n\n{text}"
    );
    match &config.provider {
        ProviderKind::OpenRouter | ProviderKind::Ollama => {
            parse_openai_compatible(config, &prompt, None)
        }
        ProviderKind::Unknown(slug) => Err(anyhow!("unsupported provider `{slug}`")),
    }
}
