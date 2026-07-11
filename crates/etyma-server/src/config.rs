use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// How the process boots storage for S-PG1.
///
/// Product metadata still lives in the SQLite [`crate::store::Store`] until S-PG2.
/// This enum only models whether Postgres foundation (pool + migrate) is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    /// Spike/dev: no Postgres pool; SQLite product meta only.
    SpikeSqlite,
    /// Cloud foundation: connect + migrate plane schemas; product meta still SQLite.
    PostgresFoundation { database_url: String },
}

impl StorageMode {
    pub fn postgres_url(&self) -> Option<&str> {
        match self {
            Self::PostgresFoundation { database_url } => Some(database_url.as_str()),
            Self::SpikeSqlite => None,
        }
    }

    pub fn host_mode(&self) -> HostMode {
        match self {
            Self::SpikeSqlite => HostMode::Spike,
            Self::PostgresFoundation { .. } => HostMode::CloudFoundation,
        }
    }
}

/// Process role label for health / ops (not a product feature flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    Spike,
    CloudFoundation,
}

impl HostMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spike => "spike",
            Self::CloudFoundation => "cloud-foundation",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: String,
    /// Process-local path for the spike metadata SQLite file only (not a product workspace root).
    pub database_path: PathBuf,
    /// Local filesystem blob root (`ETYMA_BLOB_ROOT`). Dev/CI adapter only.
    pub blob_root: PathBuf,
    pub spike_admin_token: Option<String>,
    /// Spike SQLite-only vs Postgres foundation (pool + migrate).
    pub storage: StorageMode,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let mut vars = HashMap::new();
        for key in [
            "ETYMA_SERVER_DATA",
            "ETYMA_SERVER_DB",
            "ETYMA_BLOB_ROOT",
            "ETYMA_SERVER_BIND",
            "ETYMA_SPIKE_ADMIN_TOKEN",
            "ETYMA_DATABASE_URL",
            "ETYMA_CLOUD_MODE",
            "ETYMA_ALLOW_SQLITE",
        ] {
            if let Ok(value) = env::var(key) {
                vars.insert(key.to_string(), value);
            }
        }
        let config = Self::from_env_map(&vars)?;
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed creating db parent {}", parent.display()))?;
        }
        std::fs::create_dir_all(&config.blob_root)
            .with_context(|| format!("failed creating blob root {}", config.blob_root.display()))?;
        Ok(config)
    }

    /// Pure config parse from a string map (testable without process env or filesystem side effects).
    pub fn from_env_map(vars: &HashMap<String, String>) -> Result<Self> {
        let get = |key: &str| vars.get(key).map(|s| s.as_str());

        let data_dir = get("ETYMA_SERVER_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./.etyma-server-data"));
        let database_path = get("ETYMA_SERVER_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("server.sqlite3"));
        let blob_root = get("ETYMA_BLOB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("blobs"));
        let bind = get("ETYMA_SERVER_BIND")
            .unwrap_or("127.0.0.1:8787")
            .to_string();
        let spike_admin_token = get("ETYMA_SPIKE_ADMIN_TOKEN")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let database_url = get("ETYMA_DATABASE_URL")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Env flags only decide whether a DSN is required; they are not runtime fields.
        let cloud_mode = parse_bool_flag_default(get("ETYMA_CLOUD_MODE"), false, "ETYMA_CLOUD_MODE")?;
        let allow_sqlite =
            parse_bool_flag_default(get("ETYMA_ALLOW_SQLITE"), true, "ETYMA_ALLOW_SQLITE")?;
        let require_postgres = cloud_mode || !allow_sqlite;

        let storage = match database_url {
            Some(database_url) => StorageMode::PostgresFoundation { database_url },
            None if require_postgres => {
                if cloud_mode {
                    bail!("ETYMA_CLOUD_MODE requires ETYMA_DATABASE_URL");
                }
                bail!(
                    "ETYMA_DATABASE_URL is required when ETYMA_ALLOW_SQLITE is false \
                     (refuses SQLite-only boot; product Store still uses SQLite until S-PG2)"
                );
            }
            None => StorageMode::SpikeSqlite,
        };

        Ok(Self {
            bind,
            database_path,
            blob_root,
            spike_admin_token,
            storage,
        })
    }
}

fn parse_bool_flag_default(
    value: Option<&str>,
    default: bool,
    env_name: &str,
) -> Result<bool> {
    match value {
        None => Ok(default),
        Some(raw) => match parse_bool_flag(raw) {
            Some(v) => Ok(v),
            None => bail!(
                "invalid {env_name} value {raw:?}; expected 1/true/yes or 0/false/no"
            ),
        },
    }
}

fn parse_bool_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn cloud_mode_without_dsn_fails() {
        let err = ServerConfig::from_env_map(&map(&[("ETYMA_CLOUD_MODE", "1")]))
            .expect_err("cloud mode without DSN must fail");
        assert!(
            err.to_string().contains("ETYMA_DATABASE_URL"),
            "error should mention DATABASE_URL: {err}"
        );
    }

    #[test]
    fn cloud_mode_true_case_insensitive_without_dsn_fails() {
        for flag in ["TRUE", "Yes", "true"] {
            let err = ServerConfig::from_env_map(&map(&[("ETYMA_CLOUD_MODE", flag)]))
                .expect_err("cloud mode without DSN must fail");
            assert!(
                err.to_string().contains("ETYMA_DATABASE_URL"),
                "flag={flag}: {err}"
            );
        }
    }

    #[test]
    fn cloud_mode_with_dsn_ok() {
        let cfg = ServerConfig::from_env_map(&map(&[
            ("ETYMA_CLOUD_MODE", "true"),
            (
                "ETYMA_DATABASE_URL",
                "postgres://etyma:etyma@127.0.0.1:5432/etyma",
            ),
        ]))
        .expect("cloud mode + DSN");
        assert_eq!(
            cfg.storage,
            StorageMode::PostgresFoundation {
                database_url: "postgres://etyma:etyma@127.0.0.1:5432/etyma".into(),
            }
        );
        assert_eq!(cfg.storage.host_mode(), HostMode::CloudFoundation);
    }

    #[test]
    fn no_cloud_mode_no_dsn_default_spike_sqlite() {
        let cfg = ServerConfig::from_env_map(&HashMap::new()).expect("default spike path");
        assert_eq!(cfg.storage, StorageMode::SpikeSqlite);
        assert_eq!(cfg.storage.host_mode(), HostMode::Spike);
        assert_eq!(cfg.bind, "127.0.0.1:8787");
    }

    #[test]
    fn no_cloud_mode_no_dsn_allow_sqlite_false_fails() {
        for flag in ["0", "false", "no", "FALSE"] {
            let err = ServerConfig::from_env_map(&map(&[("ETYMA_ALLOW_SQLITE", flag)]))
                .expect_err("ALLOW_SQLITE false without DSN must fail");
            assert!(
                err.to_string().contains("ETYMA_ALLOW_SQLITE")
                    || err.to_string().contains("ETYMA_DATABASE_URL"),
                "flag={flag}: {err}"
            );
        }
    }

    #[test]
    fn dsn_without_cloud_mode_is_postgres_foundation() {
        let cfg = ServerConfig::from_env_map(&map(&[(
            "ETYMA_DATABASE_URL",
            "postgres://etyma:etyma@127.0.0.1:5432/etyma",
        )]))
        .expect("DSN without cloud mode");
        assert!(matches!(
            cfg.storage,
            StorageMode::PostgresFoundation { .. }
        ));
        assert_eq!(
            cfg.storage.postgres_url(),
            Some("postgres://etyma:etyma@127.0.0.1:5432/etyma")
        );
    }

    #[test]
    fn empty_database_url_treated_as_unset() {
        let err = ServerConfig::from_env_map(&map(&[
            ("ETYMA_CLOUD_MODE", "1"),
            ("ETYMA_DATABASE_URL", "   "),
        ]))
        .expect_err("whitespace-only DSN must fail in cloud mode");
        assert!(err.to_string().contains("ETYMA_DATABASE_URL"));
    }

    #[test]
    fn allow_sqlite_true_without_dsn_ok() {
        let cfg = ServerConfig::from_env_map(&map(&[("ETYMA_ALLOW_SQLITE", "1")]))
            .expect("explicit allow sqlite");
        assert_eq!(cfg.storage, StorageMode::SpikeSqlite);
    }

    #[test]
    fn allow_sqlite_false_with_dsn_ok() {
        let cfg = ServerConfig::from_env_map(&map(&[
            ("ETYMA_ALLOW_SQLITE", "0"),
            (
                "ETYMA_DATABASE_URL",
                "postgres://etyma:etyma@127.0.0.1:5432/etyma",
            ),
        ]))
        .expect("DSN present allows disallowing sqlite-only boot");
        assert!(matches!(
            cfg.storage,
            StorageMode::PostgresFoundation { .. }
        ));
    }

    #[test]
    fn cloud_mode_zero_is_spike() {
        let cfg = ServerConfig::from_env_map(&map(&[("ETYMA_CLOUD_MODE", "0")]))
            .expect("cloud mode 0 is spike path");
        assert_eq!(cfg.storage, StorageMode::SpikeSqlite);
    }

    #[test]
    fn invalid_cloud_mode_flag_errors() {
        let err = ServerConfig::from_env_map(&map(&[("ETYMA_CLOUD_MODE", "ture")]))
            .expect_err("garbage cloud mode must fail");
        assert!(
            err.to_string().contains("ETYMA_CLOUD_MODE"),
            "{err}"
        );
    }
}
