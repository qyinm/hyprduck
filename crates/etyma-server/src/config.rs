use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: String,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub spike_admin_token: Option<String>,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let data_dir = env::var("ETYMA_SERVER_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./.etyma-server-data"));
        let database_path = env::var("ETYMA_SERVER_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("server.sqlite3"));
        let bind = env::var("ETYMA_SERVER_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
        let spike_admin_token = env::var("ETYMA_SPIKE_ADMIN_TOKEN").ok().filter(|s| !s.is_empty());
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed creating data dir {}", data_dir.display()))?;
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed creating db parent {}", parent.display()))?;
        }
        Ok(Self {
            bind,
            data_dir,
            database_path,
            spike_admin_token,
        })
    }
}
