//! Health + boot coverage for Postgres foundation (S-PG1 / PON-16).

use etyma_server::config::{HostMode, ServerConfig, StorageMode};
use std::collections::HashMap;
use tempfile::tempdir;

async fn spawn_router(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn data_dir_vars(dir: &tempfile::TempDir) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert(
        "ETYMA_SERVER_DATA".into(),
        dir.path().to_string_lossy().into_owned(),
    );
    vars
}

async fn assert_health(base: &str, path: &str, postgres: &str, mode: &str) {
    let client = reqwest::Client::new();
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
    assert_eq!(body["mode"], mode, "{path}: {body}");
    assert_eq!(body["postgres"], postgres, "{path}: {body}");
}

#[tokio::test]
async fn build_app_without_dsn_reports_postgres_skipped() {
    let dir = tempdir().unwrap();
    let config = ServerConfig::from_env_map(&data_dir_vars(&dir)).expect("default spike config");
    assert_eq!(config.storage, StorageMode::SpikeSqlite);
    assert_eq!(config.storage.host_mode(), HostMode::Spike);

    let app = etyma_server::build_app(&config)
        .await
        .expect("build_app without DSN");
    let base = spawn_router(app).await;

    for path in ["/health", "/healthz"] {
        assert_health(&base, path, "skipped", "spike").await;
    }
}

#[tokio::test]
#[ignore = "requires ETYMA_DATABASE_URL"]
async fn build_app_with_dsn_reports_postgres_up_after_migrate() {
    let url = std::env::var("ETYMA_DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .expect(
            "ETYMA_DATABASE_URL required for ignored Postgres tests \
             (run: docker compose up -d && cargo test -p etyma-server -- --include-ignored)",
        );

    let dir = tempdir().unwrap();
    let mut vars = data_dir_vars(&dir);
    vars.insert("ETYMA_DATABASE_URL".into(), url);
    let config = ServerConfig::from_env_map(&vars).expect("config with DSN");
    assert!(matches!(
        config.storage,
        StorageMode::PostgresFoundation { .. }
    ));

    let app = etyma_server::build_app(&config)
        .await
        .expect("build_app with postgres");
    let base = spawn_router(app).await;

    for path in ["/health", "/healthz"] {
        assert_health(&base, path, "up", "cloud-foundation").await;
    }
}
