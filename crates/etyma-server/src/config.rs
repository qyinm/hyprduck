use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: String,
    /// Process-local path for the spike metadata SQLite file only (not a product workspace root).
    pub database_path: PathBuf,
    /// Local filesystem blob root (`ETYMA_BLOB_ROOT`). Dev/CI adapter only.
    pub blob_root: PathBuf,
    pub spike_admin_token: Option<String>,
    /// Postgres DSN from `ETYMA_DATABASE_URL` (not connected in Task 1).
    pub database_url: Option<String>,
    /// When true, `ETYMA_DATABASE_URL` is required at parse time.
    pub cloud_mode: bool,
    /// When true, the SQLite spike metadata path is allowed without a Postgres DSN.
    pub allow_sqlite: bool,
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
        let cloud_mode = parse_truthy(get("ETYMA_CLOUD_MODE"));
        let allow_sqlite = match get("ETYMA_ALLOW_SQLITE") {
            None => true,
            Some(raw) => match parse_bool_flag(raw) {
                Some(v) => v,
                None => bail!(
                    "invalid ETYMA_ALLOW_SQLITE value {raw:?}; expected 1/true/yes or 0/false/no"
                ),
            },
        };

        if cloud_mode && database_url.is_none() {
            bail!("ETYMA_CLOUD_MODE requires ETYMA_DATABASE_URL");
        }
        if database_url.is_none() && !allow_sqlite {
            bail!(
                "ETYMA_ALLOW_SQLITE disables the SQLite metadata store but ETYMA_DATABASE_URL is not set"
            );
        }

        Ok(Self {
            bind,
            database_path,
            blob_root,
            spike_admin_token,
            database_url,
            cloud_mode,
            allow_sqlite,
        })
    }
}

fn parse_truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes")
    )
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
        assert!(cfg.cloud_mode);
        assert_eq!(
            cfg.database_url.as_deref(),
            Some("postgres://etyma:etyma@127.0.0.1:5432/etyma")
        );
    }

    #[test]
    fn no_cloud_mode_no_dsn_default_allows_sqlite() {
        let cfg = ServerConfig::from_env_map(&HashMap::new()).expect("default spike path");
        assert!(!cfg.cloud_mode);
        assert!(cfg.database_url.is_none());
        assert!(cfg.allow_sqlite);
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
    fn dsn_without_cloud_mode_ok() {
        let cfg = ServerConfig::from_env_map(&map(&[(
            "ETYMA_DATABASE_URL",
            "postgres://etyma:etyma@127.0.0.1:5432/etyma",
        )]))
        .expect("DSN without cloud mode");
        assert!(!cfg.cloud_mode);
        assert!(cfg.allow_sqlite);
        assert_eq!(
            cfg.database_url.as_deref(),
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
        assert!(cfg.allow_sqlite);
        assert!(cfg.database_url.is_none());
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
        .expect("DSN present allows disallowing sqlite");
        assert!(!cfg.allow_sqlite);
        assert!(cfg.database_url.is_some());
    }

    #[test]
    fn cloud_mode_zero_is_not_cloud() {
        let cfg = ServerConfig::from_env_map(&map(&[("ETYMA_CLOUD_MODE", "0")]))
            .expect("cloud mode 0 is spike path");
        assert!(!cfg.cloud_mode);
        assert!(cfg.allow_sqlite);
    }
}
