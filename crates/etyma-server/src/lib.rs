pub mod auth;
pub mod blob;
pub mod compose;
pub mod config;
pub mod http;
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

pub fn build_app(config: &ServerConfig) -> Result<Router> {
    let store = Store::open(&config.database_path).context("open server store")?;
    let blobs = LocalFsBlobStore::open(&config.blob_root).context("open blob store")?;
    let state = AppState {
        store: Arc::new(store),
        blobs: Arc::new(blobs),
        spike_admin_token: config.spike_admin_token.clone(),
    };
    Ok(Router::new()
        .merge(http::router())
        .merge(mcp::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state))
}

pub async fn serve(config: ServerConfig) -> Result<()> {
    let app = build_app(&config)?;
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("bind {}", config.bind))?;
    tracing::info!(%config.bind, "etyma-server listening");
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
