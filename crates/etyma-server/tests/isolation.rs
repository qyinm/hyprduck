use etyma_server::auth::AppState;
use etyma_server::build_app;
use etyma_server::config::ServerConfig;
use etyma_server::seed::seed_multi_source_workspace;
use etyma_server::store::Store;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn mixed_pack_and_cross_workspace_denial() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("server.sqlite3");
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let store = Arc::new(Store::open(&db).unwrap());

    let w1 = "ws_alpha";
    let w2 = "ws_beta";
    let root1 = data.join(w1);
    let root2 = data.join(w2);
    store.create_workspace(w1, &root1).unwrap();
    store.create_workspace(w2, &root2).unwrap();
    seed_multi_source_workspace(&store, w1, &root1).unwrap();
    // w2 intentionally empty of seed content

    let t1 = store.mint_token(w1, Some("a")).unwrap();
    let t2 = store.mint_token(w2, Some("b")).unwrap();

    let _config = ServerConfig {
        bind: "127.0.0.1:0".into(),
        data_dir: data.clone(),
        database_path: db,
        spike_admin_token: Some("admin-secret".into()),
    };
    let app = {
        let state = AppState {
            store: store.clone(),
            data_dir: data,
            spike_admin_token: Some("admin-secret".into()),
        };
        axum::Router::new()
            .merge(etyma_server::http::router())
            .merge(etyma_server::mcp::router())
            .with_state(state)
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // AE1: mixed pack for w1
    let pack_res = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t1}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_res.status(), 200);
    let pack: serde_json::Value = pack_res.json().await.unwrap();
    let evidence = pack["contextPackV1"]["selectedEvidence"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        evidence.len() >= 2,
        "expected multi-source evidence, got {evidence:?}"
    );
    let reasons: Vec<String> = evidence
        .iter()
        .filter_map(|e| e["selectionReason"].as_str().map(str::to_string))
        .collect();
    assert!(
        reasons.iter().any(|r| r.contains("document")),
        "{reasons:?}"
    );
    assert!(reasons.iter().any(|r| r.contains("issue")), "{reasons:?}");

    // AE2: t2 cannot list w1 sources via its own token (empty w2)
    let sources_w2 = client
        .get(format!("{base}/v1/sources"))
        .header("Authorization", format!("Bearer {t2}"))
        .send()
        .await
        .unwrap();
    assert_eq!(sources_w2.status(), 200);
    let body: serde_json::Value = sources_w2.json().await.unwrap();
    assert_eq!(body["sources"].as_array().map(|a| a.len()).unwrap_or(0), 0);

    // Invalid token denied
    let denied = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", "Bearer etyma_invalid")
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);

    // AE3: MCP get_context_pack with t1
    let mcp = client
        .post(format!("{base}/v1/mcp"))
        .header("Authorization", format!("Bearer {t1}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "get_context_pack",
                "arguments": { "query": "alpha-token" }
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(mcp.status(), 200);
    let mcp_body: serde_json::Value = mcp.json().await.unwrap();
    assert!(
        mcp_body["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("alpha-token"),
        "{mcp_body}"
    );
}
