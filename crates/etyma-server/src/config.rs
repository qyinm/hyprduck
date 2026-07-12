use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub oidc: Option<OidcConfig>,
    pub session_ttl_seconds: i64,
    pub session_cookie_secure: bool,
    pub success_redirect: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            oidc: None,
            session_ttl_seconds: 604_800,
            session_cookie_secure: true,
            success_redirect: "/".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    Postgres { database_url: String },
}

impl StorageMode {
    pub fn postgres_url(&self) -> &str {
        match self {
            Self::Postgres { database_url } => database_url,
        }
    }

    pub fn host_mode(&self) -> HostMode {
        HostMode::Saas
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    Saas,
}

impl HostMode {
    pub fn as_str(self) -> &'static str {
        "saas"
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: String,
    /// Local filesystem blob adapter root. Source metadata always lives in Postgres.
    pub blob_root: PathBuf,
    pub spike_admin_token: Option<String>,
    pub storage: StorageMode,
    pub auth: AuthConfig,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let mut vars = HashMap::new();
        for key in [
            "ETYMA_SERVER_DATA",
            "ETYMA_BLOB_ROOT",
            "ETYMA_SERVER_BIND",
            "ETYMA_SPIKE_ADMIN_TOKEN",
            "ETYMA_DATABASE_URL",
            "ETYMA_OIDC_ISSUER_URL",
            "ETYMA_OIDC_CLIENT_ID",
            "ETYMA_OIDC_CLIENT_SECRET",
            "ETYMA_OIDC_REDIRECT_URL",
            "ETYMA_AUTH_COOKIE_SECURE",
            "ETYMA_AUTH_SESSION_TTL_SECONDS",
            "ETYMA_AUTH_SUCCESS_REDIRECT",
        ] {
            if let Ok(value) = env::var(key) {
                vars.insert(key.to_string(), value);
            }
        }
        let config = Self::from_env_map(&vars)?;
        std::fs::create_dir_all(&config.blob_root)
            .with_context(|| format!("failed creating blob root {}", config.blob_root.display()))?;
        Ok(config)
    }

    pub fn from_env_map(vars: &HashMap<String, String>) -> Result<Self> {
        let get = |key: &str| vars.get(key).map(String::as_str);
        let data_dir = get("ETYMA_SERVER_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./.etyma-server-data"));
        let blob_root = get("ETYMA_BLOB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("blobs"));
        let bind = get("ETYMA_SERVER_BIND")
            .unwrap_or("127.0.0.1:8787")
            .to_owned();
        let spike_admin_token = get("ETYMA_SPIKE_ADMIN_TOKEN")
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let database_url = get("ETYMA_DATABASE_URL")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("ETYMA_DATABASE_URL is required for SaaS storage"))?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            bail!("ETYMA_DATABASE_URL must be a Postgres URL");
        }
        let auth = parse_auth_config(vars)?;

        Ok(Self {
            bind,
            blob_root,
            spike_admin_token,
            storage: StorageMode::Postgres { database_url },
            auth,
        })
    }
}

fn parse_auth_config(vars: &HashMap<String, String>) -> Result<AuthConfig> {
    let trimmed = |key: &str| {
        vars.get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    let issuer_url = trimmed("ETYMA_OIDC_ISSUER_URL");
    let client_id = trimmed("ETYMA_OIDC_CLIENT_ID");
    let client_secret = trimmed("ETYMA_OIDC_CLIENT_SECRET");
    let redirect_url = trimmed("ETYMA_OIDC_REDIRECT_URL");
    let oidc = match (issuer_url, client_id, client_secret, redirect_url) {
        (None, None, None, None) => None,
        (issuer_url, client_id, client_secret, redirect_url) => {
            let mut missing = Vec::new();
            if issuer_url.is_none() {
                missing.push("ETYMA_OIDC_ISSUER_URL");
            }
            if client_id.is_none() {
                missing.push("ETYMA_OIDC_CLIENT_ID");
            }
            if client_secret.is_none() {
                missing.push("ETYMA_OIDC_CLIENT_SECRET");
            }
            if redirect_url.is_none() {
                missing.push("ETYMA_OIDC_REDIRECT_URL");
            }
            if !missing.is_empty() {
                bail!(
                    "OIDC configuration is incomplete; missing {}",
                    missing.join(", ")
                );
            }

            let issuer_url = issuer_url.expect("validated issuer URL");
            let redirect_url = redirect_url.expect("validated redirect URL");
            validate_callback_url("ETYMA_OIDC_ISSUER_URL", &issuer_url)?;
            validate_callback_url("ETYMA_OIDC_REDIRECT_URL", &redirect_url)?;
            Some(OidcConfig {
                issuer_url,
                client_id: client_id.expect("validated client id"),
                client_secret: client_secret.expect("validated client secret"),
                redirect_url,
            })
        }
    };

    let session_cookie_secure = match trimmed("ETYMA_AUTH_COOKIE_SECURE").as_deref() {
        None => true,
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        Some(value) => bail!("ETYMA_AUTH_COOKIE_SECURE must be true, false, 1, or 0; got {value}"),
    };
    let session_ttl_seconds = match trimmed("ETYMA_AUTH_SESSION_TTL_SECONDS").as_deref() {
        None => 604_800,
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|seconds| (300..=2_592_000).contains(seconds))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ETYMA_AUTH_SESSION_TTL_SECONDS must be an integer from 300 through 2592000"
                )
            })?,
    };
    let success_redirect = trimmed("ETYMA_AUTH_SUCCESS_REDIRECT").unwrap_or_else(|| "/".into());
    if !success_redirect.starts_with('/') || success_redirect.starts_with("//") {
        bail!("ETYMA_AUTH_SUCCESS_REDIRECT must be a local path beginning with a single '/'");
    }

    Ok(AuthConfig {
        oidc,
        session_ttl_seconds,
        session_cookie_secure,
        success_redirect,
    })
}

fn validate_callback_url(key: &str, value: &str) -> Result<()> {
    let https = value.strip_prefix("https://");
    let local_http = value.strip_prefix("http://").and_then(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = host.split(':').next().unwrap_or_default();
        (host == "localhost" || host == "127.0.0.1").then_some(rest)
    });
    if https.is_some_and(|rest| !rest.is_empty()) || local_http.is_some() {
        Ok(())
    } else {
        bail!(
            "{key} must use https://, except http://localhost and http://127.0.0.1 are allowed for local development"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn saas_server_without_dsn_fails() {
        let error = ServerConfig::from_env_map(&HashMap::new())
            .expect_err("SaaS server without DSN must fail");
        assert!(error.to_string().contains("ETYMA_DATABASE_URL"));
    }

    #[test]
    fn dsn_configures_postgres_saas() {
        let config = ServerConfig::from_env_map(&map(&[(
            "ETYMA_DATABASE_URL",
            "postgres://etyma:etyma@127.0.0.1:5432/etyma",
        )]))
        .expect("SaaS DSN");
        assert_eq!(
            config.storage,
            StorageMode::Postgres {
                database_url: "postgres://etyma:etyma@127.0.0.1:5432/etyma".into(),
            }
        );
        assert_eq!(config.storage.host_mode(), HostMode::Saas);
    }

    #[test]
    fn empty_database_url_treated_as_unset() {
        let error = ServerConfig::from_env_map(&map(&[("ETYMA_DATABASE_URL", "   ")]))
            .expect_err("whitespace-only DSN must fail");
        assert!(error.to_string().contains("ETYMA_DATABASE_URL"));
    }

    #[test]
    fn no_oidc_variables_leave_auth_disabled() {
        let config = ServerConfig::from_env_map(&map(&[(
            "ETYMA_DATABASE_URL",
            "postgres://etyma:etyma@localhost/etyma",
        )]))
        .expect("database config");

        assert!(config.auth.oidc.is_none());
        assert_eq!(config.auth.session_ttl_seconds, 604_800);
        assert!(config.auth.session_cookie_secure);
        assert_eq!(config.auth.success_redirect, "/");
    }

    #[test]
    fn partial_oidc_variables_fail_with_missing_keys() {
        let error = ServerConfig::from_env_map(&map(&[
            (
                "ETYMA_DATABASE_URL",
                "postgres://etyma:etyma@localhost/etyma",
            ),
            ("ETYMA_OIDC_ISSUER_URL", "https://accounts.google.com"),
            ("ETYMA_OIDC_CLIENT_ID", "client-id"),
        ]))
        .expect_err("partial OIDC config must fail");

        let message = error.to_string();
        assert!(message.contains("ETYMA_OIDC_CLIENT_SECRET"), "{message}");
        assert!(message.contains("ETYMA_OIDC_REDIRECT_URL"), "{message}");
    }

    #[test]
    fn oidc_config_accepts_google_compatible_callback_settings() {
        let config = ServerConfig::from_env_map(&map(&[
            (
                "ETYMA_DATABASE_URL",
                "postgres://etyma:etyma@localhost/etyma",
            ),
            ("ETYMA_OIDC_ISSUER_URL", "https://accounts.google.com"),
            ("ETYMA_OIDC_CLIENT_ID", "client-id"),
            ("ETYMA_OIDC_CLIENT_SECRET", "client-secret"),
            (
                "ETYMA_OIDC_REDIRECT_URL",
                "https://app.example.com/v1/auth/callback",
            ),
            ("ETYMA_AUTH_COOKIE_SECURE", "false"),
            ("ETYMA_AUTH_SESSION_TTL_SECONDS", "3600"),
            ("ETYMA_AUTH_SUCCESS_REDIRECT", "/signed-in"),
        ]))
        .expect("OIDC config");

        let oidc = config.auth.oidc.expect("OIDC enabled");
        assert_eq!(oidc.issuer_url, "https://accounts.google.com");
        assert_eq!(oidc.client_id, "client-id");
        assert_eq!(oidc.client_secret, "client-secret");
        assert_eq!(
            oidc.redirect_url,
            "https://app.example.com/v1/auth/callback"
        );
        assert_eq!(config.auth.session_ttl_seconds, 3600);
        assert!(!config.auth.session_cookie_secure);
        assert_eq!(config.auth.success_redirect, "/signed-in");
    }

    #[test]
    fn auth_success_redirect_rejects_external_urls() {
        let error = ServerConfig::from_env_map(&map(&[
            (
                "ETYMA_DATABASE_URL",
                "postgres://etyma:etyma@localhost/etyma",
            ),
            ("ETYMA_AUTH_SUCCESS_REDIRECT", "https://evil.example/steal"),
        ]))
        .expect_err("external redirect must fail");

        assert!(error.to_string().contains("ETYMA_AUTH_SUCCESS_REDIRECT"));
    }
}
