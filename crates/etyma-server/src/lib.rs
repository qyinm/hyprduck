pub mod auth;
pub mod blob;
pub mod compose;
pub mod config;
pub mod db;
pub mod graph;
pub mod http;
pub mod import_job;
pub mod ingest;
pub mod knowledge;
pub mod mcp;
pub mod seed;
pub mod store;

use crate::auth::{AppState, AuthService};
use crate::blob::LocalFsBlobStore;
use crate::config::ServerConfig;
use crate::graph::GraphStore;
use crate::knowledge::KnowledgeStore;
use crate::store::Store;
use anyhow::{Context, Result};
use axum::Router;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// Build the HTTP/MCP router.
///
/// Connects Postgres, applies control/knowledge/graph migrations, and builds the SaaS router.
pub async fn build_app(config: &ServerConfig) -> Result<Router> {
    let pool = db::connect_and_migrate(config.storage.postgres_url())
        .await
        .context("postgres connect/migrate")?;
    let store = Arc::new(Store::new(pool.clone()));
    let auth = Arc::new(
        AuthService::initialize(store.clone(), config.auth.clone())
            .await
            .context("initialize authentication")?,
    );

    let blobs = LocalFsBlobStore::open(&config.blob_root).context("open blob store")?;
    let knowledge = KnowledgeStore::new(pool.clone());
    let graph = GraphStore::new(pool.clone());
    let blobs = Arc::new(blobs);
    import_job::spawn_upload_recovery_loop(knowledge.clone(), blobs.clone());
    let state = AppState {
        auth,
        store,
        knowledge,
        graph,
        blobs,
        spike_admin_token: config.spike_admin_token.clone(),
        host_mode: config.storage.host_mode(),
        pg_pool: pool,
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
    tracing::info!("postgres pool ready (control/knowledge/graph migrations applied)");
    tracing::info!(%config.bind, "etyma-server listening");
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
