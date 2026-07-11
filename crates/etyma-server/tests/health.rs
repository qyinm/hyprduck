//! Health endpoint coverage for Postgres pool wiring (S-PG1 / PON-16).

use etyma_server::auth::AppState;
use etyma_server::blob::LocalFsBlobStore;
use etyma_server::config::ServerConfig;
use etyma_server::store::Store;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

async fn spawn_app(state: AppState) -> String {
    let app = axum::Router::new()
        .merge(etyma_server::http::router())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn spike_state(dir: &tempfile::TempDir) -> AppState {
    let store = Store::open(&dir.path().join("server.sqlite3")).unwrap();
    let blobs = LocalFsBlobStore::open(dir.path().join("blobs")).unwrap();
    AppState {
        store: Arc::new(store),
        blobs: Arc::new(blobs),
        spike_admin_token: None,
        pg_pool: None,
    }
}

#[tokio::test]
async fn health_without_pool_reports_postgres_skipped() {
    let dir = tempdir().unwrap();
    let base = spawn_app(spike_state(&dir)).await;
    let client = reqwest::Client::new();

    for path in ["/health", "/healthz"] {
        let res = client
            .get(format!("{base}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{path}");
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["ok"], true, "{path}: {body}");
        assert_eq!(body["status"], "ok", "{path}: {body}");
        assert_eq!(body["service"], "etyma-server", "{path}: {body}");
        assert_eq!(body["mode"], "spike", "{path}: {body}");
        assert_eq!(body["postgres"], "skipped", "{path}: {body}");
    }
}

#[tokio::test]
async fn health_with_pool_reports_postgres_up_after_migrate() {
    let Some(url) = std::env::var("ETYMA_DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        eprintln!("skipping: ETYMA_DATABASE_URL unset");
        return;
    };

    let dir = tempdir().unwrap();
    let mut vars = HashMap::new();
    vars.insert(
        "ETYMA_SERVER_DATA".into(),
        dir.path().to_string_lossy().into_owned(),
    );
    vars.insert("ETYMA_DATABASE_URL".into(), url);
    let config = ServerConfig::from_env_map(&vars).expect("config with DSN");

    // build_app connects, migrates, and wires pg_pool.
    let app = etyma_server::build_app(&config)
        .await
        .expect("build_app with postgres");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    for path in ["/health", "/healthz"] {
        let res = client
            .get(format!("{base}{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{path}");
        let body: serde_json::Value = res.json().await.unwrap();
        assert_eq!(body["ok"], true, "{path}: {body}");
        assert_eq!(body["status"], "ok", "{path}: {body}");
        assert_eq!(body["postgres"], "up", "{path}: {body}");
        assert_eq!(body["service"], "etyma-server", "{path}: {body}");
        assert_eq!(body["mode"], "spike", "{path}: {body}");
    }
}

#[tokio::test]
async fn build_app_without_dsn_keeps_sqlite_spike_path() {
    let dir = tempdir().unwrap();
    let mut vars = HashMap::new();
    vars.insert(
        "ETYMA_SERVER_DATA".into(),
        dir.path().to_string_lossy().into_owned(),
    );
    let config = ServerConfig::from_env_map(&vars).expect("default spike config");
    assert!(config.database_url.is_none());

    let app = etyma_server::build_app(&config)
        .await
        .expect("build_app without DSN");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["postgres"], "skipped");
    assert_eq!(body["ok"], true);
}
