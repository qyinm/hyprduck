use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

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

        Ok(Self {
            bind,
            blob_root,
            spike_admin_token,
            storage: StorageMode::Postgres { database_url },
        })
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
}
