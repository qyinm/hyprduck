use etyma_server::auth::AppState;
use etyma_server::seed::seed_multi_source_workspace;
use etyma_server::store::Store;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn mixed_pack_and_cross_workspace_denial() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("server.sqlite3");
    let store = Arc::new(Store::open(&db).unwrap());

    let w1 = "ws_alpha";
    let w2 = "ws_beta";
    store.create_workspace(w1).unwrap();
    store.create_workspace(w2).unwrap();
    // Both tenants seeded so cross-token leakage is observable if present.
    seed_multi_source_workspace(&store, w1).unwrap();
    seed_multi_source_workspace(&store, w2).unwrap();

    let t1 = store.mint_token(w1, Some("a")).unwrap();
    let t2 = store.mint_token(w2, Some("b")).unwrap();

    let app = {
        let state = AppState {
            store: store.clone(),
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

    // AE1: mixed multi-source pack for w1 (V1 body)
    let pack_res = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t1}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_res.status(), 200);
    let pack: serde_json::Value = pack_res.json().await.unwrap();
    assert_eq!(pack["workspaceId"], w1);
    assert_eq!(pack["schemaVersion"], "etyma.context_pack.v1");
    let evidence = pack["selectedEvidence"].as_array().cloned().unwrap_or_default();
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

    // AE2: t2 pack is scoped to w2 only (both seeded; must not emit w1 workspaceId)
    let pack_w2 = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t2}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_w2.status(), 200);
    let pack_w2: serde_json::Value = pack_w2.json().await.unwrap();
    assert_eq!(pack_w2["workspaceId"], w2);
    let w2_sources: Vec<String> = pack_w2["sourceSet"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| s["sourceId"].as_str().map(str::to_string))
        .collect();
    let w1_sources: Vec<String> = pack["sourceSet"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| s["sourceId"].as_str().map(str::to_string))
        .collect();
    for sid in &w1_sources {
        assert!(
            !w2_sources.contains(sid),
            "w2 pack leaked w1 source id {sid}"
        );
    }

    // t2 sources list never includes w1 source ids
    let sources_w2 = client
        .get(format!("{base}/v1/sources"))
        .header("Authorization", format!("Bearer {t2}"))
        .send()
        .await
        .unwrap();
    assert_eq!(sources_w2.status(), 200);
    let body: serde_json::Value = sources_w2.json().await.unwrap();
    assert_eq!(body["workspaceId"], w2);
    let listed: Vec<String> = body["sources"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect();
    for sid in &w1_sources {
        assert!(!listed.contains(sid), "t2 listed w1 source {sid}");
    }

    // Invalid token denied
    let denied = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", "Bearer etyma_invalid")
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);

    // AE3: MCP get_context_pack with t1 returns V1 workspace scope
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
    let text = mcp_body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(text.contains("alpha-token"), "{mcp_body}");
    assert!(text.contains(w1), "{mcp_body}");
    assert!(!text.contains(&format!("\"workspaceId\": \"{w2}\"")), "{mcp_body}");
}
