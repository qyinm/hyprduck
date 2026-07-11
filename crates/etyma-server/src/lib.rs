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
use crate::config::{ServerConfig, StorageMode};
use crate::store::Store;
use anyhow::{Context, Result};
use axum::Router;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// Build the HTTP/MCP router.
///
/// In [`StorageMode::PostgresFoundation`], connects a pool, applies migrations, and
/// opens a hybrid store (control on Postgres, sources/evidence on local SQLite).
/// Spike mode keeps the full product schema on SQLite only.
pub async fn build_app(config: &ServerConfig) -> Result<Router> {
    let (store, pg_pool) = match &config.storage {
        StorageMode::PostgresFoundation { database_url } => {
            let pool = db::connect_and_migrate(database_url)
                .await
                .context("postgres connect/migrate")?;
            let store = Store::open_hybrid(pool.clone(), &config.database_path)
                .context("open hybrid store (control=pg, knowledge=sqlite)")?;
            (store, Some(pool))
        }
        StorageMode::SpikeSqlite => {
            let store = Store::open(&config.database_path).context("open server store")?;
            (store, None)
        }
    };

    let blobs = LocalFsBlobStore::open(&config.blob_root).context("open blob store")?;
    let state = AppState {
        store: Arc::new(store),
        blobs: Arc::new(blobs),
        spike_admin_token: config.spike_admin_token.clone(),
        host_mode: config.storage.host_mode(),
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
    match &config.storage {
        StorageMode::PostgresFoundation { .. } => {
            tracing::info!("postgres pool ready (migrations applied); control on pg, knowledge on sqlite");
        }
        StorageMode::SpikeSqlite => {
            tracing::info!("postgres pool skipped (spike SQLite metadata path)");
        }
    }
    tracing::info!(%config.bind, "etyma-server listening");
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
