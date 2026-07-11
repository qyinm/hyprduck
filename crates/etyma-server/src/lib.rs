pub mod auth;
pub mod blob;
pub mod compose;
pub mod config;
pub mod db;
pub mod http;
pub mod ingest;
pub mod mcp;
pub mod seed;
pub mod store;

use crate::auth::AppState;
use crate::blob::LocalFsBlobStore;
use crate::config::ServerConfig;
use crate::store::Store;
use anyhow::{Context, Result};
use axum::Router;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// Build the HTTP/MCP router.
///
/// When `config.database_url` is set, connects a Postgres pool and applies
/// migrations before serving. Product metadata remains on the SQLite `Store`.
pub async fn build_app(config: &ServerConfig) -> Result<Router> {
    let pg_pool = if let Some(url) = config.database_url.as_ref() {
        Some(
            db::connect_and_migrate(url)
                .await
                .context("postgres connect/migrate")?,
        )
    } else {
        None
    };

    // Spike product metadata stays on SQLite regardless of Postgres pool presence.
    let store = Store::open(&config.database_path).context("open server store")?;
    let blobs = LocalFsBlobStore::open(&config.blob_root).context("open blob store")?;
    let state = AppState {
        store: Arc::new(store),
        blobs: Arc::new(blobs),
        spike_admin_token: config.spike_admin_token.clone(),
        pg_pool,
    };
    Ok(Router::new()
        .merge(http::router())
        .merge(mcp::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state))
}

pub async fn serve(config: ServerConfig) -> Result<()> {
    let app = build_app(&config).await?;
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("bind {}", config.bind))?;
    if config.database_url.is_some() {
        tracing::info!("postgres pool ready (migrations applied)");
    } else {
        tracing::info!("postgres pool skipped (spike SQLite metadata path)");
    }
    tracing::info!(%config.bind, "etyma-server listening");
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
