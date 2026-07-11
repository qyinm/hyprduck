use etyma_server::auth::AppState;
use etyma_server::blob::{BlobStore, LocalFsBlobStore};
use etyma_server::config::HostMode;
use etyma_server::knowledge::KnowledgeStore;
use etyma_server::seed::seed_multi_source_workspace;
use etyma_server::store::Store;
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

fn assert_multi_source_pack(pack: &serde_json::Value, workspace_id: &str) {
    assert_eq!(pack["workspaceId"], workspace_id);
    assert_eq!(pack["schemaVersion"], "etyma.context_pack.v1");
    let evidence = pack["selectedEvidence"]
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
        "missing document evidence: {reasons:?}"
    );
    assert!(
        reasons.iter().any(|r| r.contains("issue")),
        "missing issue evidence: {reasons:?}"
    );
}

/// Full workspace isolation + spike operator flow against an already-open store.
async fn run_isolation_suite(
    store: Arc<Store>,
    knowledge: KnowledgeStore,
    blobs: Arc<LocalFsBlobStore>,
    host_mode: HostMode,
    pg_pool: sqlx::PgPool,
    id_prefix: &str,
) {
    let org = format!("{id_prefix}_org_demo");
    let w1 = format!("{id_prefix}_ws_alpha");
    let w2 = format!("{id_prefix}_ws_beta");
    let org_http = format!("{id_prefix}_org_http");
    let ws_http = format!("{id_prefix}_ws_http");

    store.create_org(&org, "Demo Org").await.unwrap();
    store.create_workspace(&org, &w1).await.unwrap();
    store.create_workspace(&org, &w2).await.unwrap();
    seed_multi_source_workspace(&store, &knowledge, blobs.as_ref(), &w1)
        .await
        .unwrap();
    seed_multi_source_workspace(&store, &knowledge, blobs.as_ref(), &w2)
        .await
        .unwrap();

    // Seed created real blob objects on disk.
    let w1_sources = knowledge.list_sources(&w1).await.unwrap();
    assert_eq!(w1_sources.len(), 2);
    for src in &w1_sources {
        assert!(blobs.exists(&src.blob_key).unwrap());
        assert!(src.content_hash.starts_with("sha256:"));
    }

    let t1 = store.mint_token(&w1, Some("a")).await.unwrap();
    let t2 = store.mint_token(&w2, Some("b")).await.unwrap();

    let app = {
        let state = AppState {
            store: store.clone(),
            knowledge: knowledge.clone(),
            blobs: blobs.clone(),
            spike_admin_token: Some("admin-secret".into()),
            host_mode,
            pg_pool,
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
    let admin = "admin-secret";

    // AE1: operator create org + workspace via HTTP, then multi-source pack
    let org_res = client
        .post(format!("{base}/v1/spike/orgs"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({ "name": "HTTP Org", "orgId": org_http }))
        .send()
        .await
        .unwrap();
    assert_eq!(org_res.status(), 200, "{}", org_res.text().await.unwrap());
    let ws_res = client
        .post(format!("{base}/v1/spike/orgs/{org_http}/workspaces"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({ "workspaceId": ws_http }))
        .send()
        .await
        .unwrap();
    assert_eq!(ws_res.status(), 200);
    let ws_body: serde_json::Value = ws_res.json().await.unwrap();
    assert_eq!(ws_body["orgId"], org_http);
    assert_eq!(ws_body["workspaceId"], ws_http);

    let tok_res = client
        .post(format!("{base}/v1/spike/workspaces/{ws_http}/tokens"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({ "label": "dev" }))
        .send()
        .await
        .unwrap();
    assert_eq!(tok_res.status(), 200);
    let tok_body: serde_json::Value = tok_res.json().await.unwrap();
    let t_http = tok_body["token"].as_str().unwrap();

    let seed_res = client
        .post(format!("{base}/v1/spike/workspaces/{ws_http}/seed"))
        .header("x-etyma-admin-token", admin)
        .send()
        .await
        .unwrap();
    assert_eq!(seed_res.status(), 200);
    let seed_body: serde_json::Value = seed_res.json().await.unwrap();
    assert_eq!(seed_body["sourceCount"], 2);
    let seed_blobs = seed_body["blobs"]
        .as_array()
        .expect("seed returns blob meta");
    assert_eq!(seed_blobs.len(), 2);
    for b in seed_blobs {
        let key = b["blobKey"].as_str().unwrap();
        assert!(blobs.exists(key).unwrap(), "missing blob {key}");
        assert!(b["contentHash"].as_str().unwrap().starts_with("sha256:"));
    }

    let pack_http = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t_http}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_http.status(), 200);
    let pack_http: serde_json::Value = pack_http.json().await.unwrap();
    assert_multi_source_pack(&pack_http, &ws_http);

    // Multi-source pack for w1
    let pack_res = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t1}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_res.status(), 200);
    let pack: serde_json::Value = pack_res.json().await.unwrap();
    assert_multi_source_pack(&pack, &w1);
    let w1_source_ids: Vec<String> = pack["sourceSet"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| s["sourceId"].as_str().map(str::to_string))
        .collect();
    assert!(!w1_source_ids.is_empty());
    // Pack source hashes come from blob-backed metadata.
    for s in pack["sourceSet"].as_array().unwrap() {
        assert!(s["contentHash"].as_str().unwrap().starts_with("sha256:"));
    }

    // AE2: same org sibling isolation
    let pack_w2 = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t2}"))
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_w2.status(), 200);
    let pack_w2: serde_json::Value = pack_w2.json().await.unwrap();
    assert_multi_source_pack(&pack_w2, &w2);
    let w2_sources: Vec<String> = pack_w2["sourceSet"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| s["sourceId"].as_str().map(str::to_string))
        .collect();
    for sid in &w1_source_ids {
        assert!(
            !w2_sources.contains(sid),
            "same-org sibling leaked w1 source {sid}"
        );
    }

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
    for sid in &w1_source_ids {
        assert!(!listed.contains(sid), "t2 listed w1 source {sid}");
    }
    // Listed sources expose blob meta, not body.
    for s in body["sources"].as_array().unwrap() {
        assert!(s.get("blobKey").and_then(|v| v.as_str()).is_some());
        assert!(s.get("contentHash").and_then(|v| v.as_str()).is_some());
        assert!(s.get("body").is_none());
    }

    // AE3: orphan workspace create denied (missing org)
    let missing_org = format!("{id_prefix}_org_does_not_exist");
    let orphan = client
        .post(format!("{base}/v1/spike/orgs/{missing_org}/workspaces"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(orphan.status(), 404);

    // Duplicate org id → 409
    let dup = client
        .post(format!("{base}/v1/spike/orgs"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({ "name": "Again", "orgId": org_http }))
        .send()
        .await
        .unwrap();
    assert_eq!(dup.status(), 409);

    // Flat create route removed
    let flat = client
        .post(format!("{base}/v1/spike/workspaces"))
        .header("x-etyma-admin-token", admin)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(flat.status(), 404);

    // Invalid token
    let denied = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", "Bearer etyma_invalid")
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);

    // MCP scoped to w1
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
    assert!(text.contains(&w1), "{mcp_body}");
    assert!(
        !text.contains(&format!("\"workspaceId\": \"{w2}\"")),
        "{mcp_body}"
    );
}

#[tokio::test]
#[ignore = "requires ETYMA_DATABASE_URL"]
async fn org_hierarchy_sibling_isolation_on_postgres_control_and_knowledge() {
    let url = std::env::var("ETYMA_DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .expect(
            "ETYMA_DATABASE_URL required for ignored Postgres tests \
             (run: docker compose up -d && cargo test -p etyma-server -- --include-ignored)",
        );

    let pool = etyma_server::db::connect_and_migrate(&url)
        .await
        .expect("connect_and_migrate");
    let dir = tempdir().unwrap();
    let store = Arc::new(Store::new(pool.clone()));
    let knowledge = KnowledgeStore::new(pool.clone());
    let blobs = Arc::new(LocalFsBlobStore::open(dir.path().join("blobs")).unwrap());
    // Unique prefix so shared CI/dev Postgres DBs do not collide on fixed org ids.
    let prefix = format!("pg_{}", Uuid::now_v7().simple());
    run_isolation_suite(store, knowledge, blobs, HostMode::Saas, pool, &prefix).await;
}
