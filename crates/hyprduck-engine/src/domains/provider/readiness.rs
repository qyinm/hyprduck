use super::*;
use crate::infra::process::resolve_binary;

pub(crate) fn validate_provider(config: &EngineConfig) -> ValidateProviderResponseData {
    let mut issues = Vec::new();
    if let ProviderKind::Unknown(slug) = &config.provider {
        issues.push(ValidationIssue {
            code: "unsupported_provider".into(),
            message: format!(
                "Provider `{slug}` is no longer supported. Choose OpenRouter or Ollama."
            ),
        });
    }

    if config.provider.requires_api_key() && config.api_key.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "provider_config".into(),
            message: format!("{} requires an API key.", config.provider.label()),
        });
    }

    if config.model_id.trim().is_empty() {
        issues.push(ValidationIssue {
            code: "provider_config".into(),
            message: "A model ID is required.".into(),
        });
    }

    if let Some(base_url) = &config.base_url {
        if !(base_url.trim().is_empty()
            || base_url.starts_with("http://")
            || base_url.starts_with("https://"))
        {
            issues.push(ValidationIssue {
                code: "provider_config".into(),
                message: "Base URL must start with http:// or https://".into(),
            });
        }
    }

    ValidateProviderResponseData {
        ready: issues.is_empty(),
        issues,
    }
}

pub(crate) fn check_readiness(config_store: &EngineConfigStore) -> RuntimeReadinessResponseData {
    let mut checks = vec![ReadinessCheck {
        id: "runtime_process".into(),
        label: "Runtime process".into(),
        ready: true,
        required: true,
        message: "Runtime process is accepting commands.".into(),
    }];

    let config = match config_store.load() {
        Ok(config) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: true,
                required: true,
                message: format!("Loaded {}", config_store.path.display()),
            });
            config
        }
        Err(error) => {
            checks.push(ReadinessCheck {
                id: "config_file".into(),
                label: "Engine config".into(),
                ready: false,
                required: true,
                message: error.to_string(),
            });
            return RuntimeReadinessResponseData {
                ready: false,
                provider: "unknown".into(),
                model_id: String::new(),
                checks,
            };
        }
    };

    let validation = validate_provider(&config);
    checks.push(ReadinessCheck {
        id: "provider_config".into(),
        label: "Provider config".into(),
        ready: validation.ready,
        required: true,
        message: if validation.ready {
            format!(
                "{} is configured with model {}.",
                config.provider.label(),
                config.model_id
            )
        } else {
            validation
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        },
    });

    if matches!(&config.provider, ProviderKind::Ollama) {
        checks.push(check_ollama_endpoint(&config));
    }

    checks.push(check_path_exists(
        "pdf_converter",
        "PDF converter",
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
        false,
    ));
    checks.push(check_path_exists(
        "text_converter",
        "DOC/DOCX text converter",
        "textutil",
        &["/usr/bin/textutil"],
        false,
    ));
    checks.push(check_path_exists(
        "knowledge_store",
        "Knowledge store",
        "sqlite3",
        &["/usr/bin/sqlite3"],
        true,
    ));

    RuntimeReadinessResponseData {
        ready: checks
            .iter()
            .filter(|check| check.required)
            .all(|check| check.ready),
        provider: config.provider.id_slug().into(),
        model_id: config.model_id,
        checks,
    }
}

fn check_path_exists(
    id: &str,
    label: &str,
    binary_name: &str,
    common_paths: &[&str],
    required: bool,
) -> ReadinessCheck {
    let path = resolve_binary(binary_name, common_paths);
    let ready = path.exists();
    ReadinessCheck {
        id: id.into(),
        label: label.into(),
        ready,
        required,
        message: if ready {
            format!("Found {}", path.display())
        } else {
            format!("Missing {binary_name} in PATH or common install locations")
        },
    }
}

fn check_ollama_endpoint(config: &EngineConfig) -> ReadinessCheck {
    let endpoint = ollama_models_endpoint(config);
    let result = Client::builder()
        .timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(1))
        .build()
        .and_then(|client| client.get(&endpoint).send())
        .and_then(|response| response.error_for_status().map(|_| ()));

    match result {
        Ok(()) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: true,
            required: true,
            message: format!("Ollama responded at {endpoint}."),
        },
        Err(error) => ReadinessCheck {
            id: "ollama_endpoint".into(),
            label: "Ollama endpoint".into(),
            ready: false,
            required: true,
            message: format!("Ollama is not reachable at {endpoint}: {error}"),
        },
    }
}

pub(crate) fn ollama_models_endpoint(config: &EngineConfig) -> String {
    let raw = config
        .base_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| config.provider.default_base_url().to_string())
        .replace("/v1/chat/completions", "/v1/models")
        .replace("/api/generate", "/api/tags");

    if let Ok(mut url) = Url::parse(&raw) {
        let path = url.path().trim_end_matches('/');
        if path.is_empty() {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/v1" {
            url.set_path("/v1/models");
            return url.to_string();
        }
        if path == "/api" {
            url.set_path("/api/tags");
            return url.to_string();
        }
    }

    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_validation_reports_unknown_provider() {
        let config = EngineConfig {
            provider: ProviderKind::Unknown("legacy_ai".into()),
            model_id: "legacy-model".into(),
            api_key: "legacy-key".into(),
            base_url: None,
            prompt_template: "General".into(),
        };

        let validation = validate_provider(&config);

        assert!(!validation.ready);
        assert!(validation.issues.iter().any(|issue| {
            issue.code == "unsupported_provider" && issue.message.contains("legacy_ai")
        }));
    }

    #[test]
    fn provider_validation_reports_missing_hosted_api_key() {
        let config = EngineConfig {
            provider: ProviderKind::OpenRouter,
            model_id: "google/gemini-2.5-flash".into(),
            api_key: String::new(),
            base_url: None,
            prompt_template: "General".into(),
        };

        let validation = validate_provider(&config);

        assert!(!validation.ready);
        assert!(validation.issues.iter().any(|issue| {
            issue.code == "provider_config" && issue.message == "OpenRouter requires an API key."
        }));
    }
}
