use etyma_server::auth::{AppState, AuthService};
use etyma_server::blob::{BlobStore, LocalFsBlobStore};
use etyma_server::config::{AuthConfig, HostMode};
use etyma_server::graph::GraphStore;
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

async fn upload_raw_source(
    client: &reqwest::Client,
    base: &str,
    workspace_id: &str,
    admin: &str,
) -> reqwest::Response {
    upload_source_with(
        client,
        base,
        workspace_id,
        admin,
        "upload.md",
        Some("document"),
        Some("text/markdown"),
        b"# alpha upload\n\nworkspace-scoped evidence".to_vec(),
    )
    .await
}

async fn upload_source_with(
    client: &reqwest::Client,
    base: &str,
    workspace_id: &str,
    admin: &str,
    title: &str,
    kind: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/v1/spike/workspaces/{workspace_id}/sources"))
        .header("x-etyma-admin-token", admin)
        .header("x-etyma-source-title", title);
    if let Some(kind) = kind {
        request = request.header("x-etyma-source-kind", kind);
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    request.body(body).send().await.unwrap()
}

async fn wait_for_job(
    client: &reqwest::Client,
    base: &str,
    workspace_id: &str,
    job_id: &str,
    admin: &str,
) -> serde_json::Value {
    let mut last = serde_json::Value::Null;
    for _ in 0..100 {
        let response = client
            .get(format!(
                "{base}/v1/spike/workspaces/{workspace_id}/import-jobs/{job_id}"
            ))
            .header("x-etyma-admin-token", admin)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        last = response.json().await.unwrap();
        if matches!(last["status"].as_str(), Some("succeeded" | "failed")) {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("upload job did not finish: {last}");
}

async fn wait_for_succeeded_job(
    client: &reqwest::Client,
    base: &str,
    workspace_id: &str,
    job_id: &str,
    admin: &str,
) -> serde_json::Value {
    let last = wait_for_job(client, base, workspace_id, job_id, admin).await;
    assert_ne!(last["status"], "failed", "upload job failed: {last}");
    assert_eq!(last["status"], "succeeded");
    last
}

/// Full workspace isolation + spike operator flow against an already-open store.
async fn run_isolation_suite(
    store: Arc<Store>,
    knowledge: KnowledgeStore,
    graph: GraphStore,
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
        etyma_server::import_job::spawn_upload_recovery_loop(
            knowledge.clone(),
            graph.clone(),
            blobs.clone(),
        );
        let state = AppState {
            auth: Arc::new(AuthService::disabled(store.clone(), AuthConfig::default())),
            store: store.clone(),
            knowledge: knowledge.clone(),
            graph,
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

    let upload = upload_raw_source(&client, &base, &ws_http, admin).await;
    assert_eq!(upload.status(), 202);
    let upload_body: serde_json::Value = upload.json().await.unwrap();
    assert_eq!(upload_body["workspaceId"], ws_http);
    assert_eq!(upload_body["source"]["title"], "upload.md");
    assert_eq!(upload_body["source"]["kind"], "document");
    assert!(upload_body["source"]["blobKey"]
        .as_str()
        .unwrap()
        .starts_with(&format!("w/{ws_http}/sha256/")));
    assert_eq!(upload_body["job"]["status"], "queued");
    assert!(upload_body["job"].get("leaseToken").is_none());

    let job_id = upload_body["job"]["id"].as_str().unwrap();
    let status_body = wait_for_succeeded_job(&client, &base, &ws_http, job_id, admin).await;
    assert_eq!(status_body["id"], job_id);
    assert_eq!(status_body["workspaceId"], ws_http);
    assert_eq!(status_body["sourceId"], upload_body["source"]["id"]);
    assert_eq!(status_body["attempts"], 1);
    assert!(status_body.get("leaseOwner").is_none());
    assert!(status_body.get("leaseToken").is_none());

    let pack = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t_http}"))
        .json(&serde_json::json!({ "query": "workspace-scoped evidence" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack.status(), 200);
    let pack: serde_json::Value = pack.json().await.unwrap();
    assert_eq!(pack["workspaceId"], ws_http);
    assert!(pack["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceId"] == upload_body["source"]["id"]));
    let uploaded_evidence_id = pack["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["sourceId"] == upload_body["source"]["id"])
        .and_then(|e| e["evidenceRef"].as_str())
        .expect("uploaded evidence selected")
        .to_string();

    let graph_snapshot = client
        .get(format!("{base}/v1/graph/snapshot"))
        .header("Authorization", format!("Bearer {t_http}"))
        .send()
        .await
        .unwrap();
    assert_eq!(graph_snapshot.status(), 200);
    let graph_snapshot: serde_json::Value = graph_snapshot.json().await.unwrap();
    assert_eq!(graph_snapshot["workspaceId"], ws_http);
    assert_eq!(graph_snapshot["store"], "postgres.graph");
    assert!(graph_snapshot["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["sourceIds"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|source_id| source_id == &upload_body["source"]["id"])
            && node["evidenceIds"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .any(|evidence_id| evidence_id == &uploaded_evidence_id)));
    assert!(pack["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["evidenceRef"] == uploaded_evidence_id
            && e.get("graphTrail")
                .and_then(|trail| trail["direct"].as_array())
                .is_some_and(|direct| !direct.is_empty())));

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
        .post(format!("{base}/v1/spike/workspaces/{ws_http}/sources"))
        .header("x-etyma-source-title", "denied.md")
        .header("content-type", "text/markdown")
        .body("denied")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);

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

    let upload_one = client
        .post(format!("{base}/v1/spike/workspaces/{w1}/sources"))
        .header("x-etyma-admin-token", admin)
        .header("x-etyma-source-title", "alpha.md")
        .header("content-type", "text/markdown")
        .body("alpha private evidence")
        .send();
    let upload_two = client
        .post(format!("{base}/v1/spike/workspaces/{w2}/sources"))
        .header("x-etyma-admin-token", admin)
        .header("x-etyma-source-title", "beta.md")
        .header("content-type", "text/markdown")
        .body("beta private evidence")
        .send();
    let (response_one, response_two) = tokio::join!(upload_one, upload_two);
    let response_one = response_one.unwrap();
    let response_two = response_two.unwrap();
    assert_eq!(response_one.status(), 202);
    assert_eq!(response_two.status(), 202);
    let body_one: serde_json::Value = response_one.json().await.unwrap();
    let body_two: serde_json::Value = response_two.json().await.unwrap();
    assert_ne!(body_one["source"]["id"], body_two["source"]["id"]);
    assert!(body_one["source"]["blobKey"]
        .as_str()
        .unwrap()
        .starts_with(&format!("w/{w1}/")));
    assert!(body_two["source"]["blobKey"]
        .as_str()
        .unwrap()
        .starts_with(&format!("w/{w2}/")));

    let job_one = body_one["job"]["id"].as_str().unwrap();
    let job_two = body_two["job"]["id"].as_str().unwrap();
    let sibling_job = client
        .get(format!(
            "{base}/v1/spike/workspaces/{w1}/import-jobs/{job_two}"
        ))
        .header("x-etyma-admin-token", admin)
        .send()
        .await
        .unwrap();
    assert_eq!(sibling_job.status(), 404);
    let (status_one, status_two) = tokio::join!(
        wait_for_succeeded_job(&client, &base, &w1, job_one, admin),
        wait_for_succeeded_job(&client, &base, &w2, job_two, admin)
    );
    assert_eq!(status_one["status"], "succeeded");
    assert_eq!(status_two["status"], "succeeded");

    let pack_w1_private = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t1}"))
        .json(&serde_json::json!({ "query": "alpha private evidence" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_w1_private.status(), 200);
    let pack_w1_private: serde_json::Value = pack_w1_private.json().await.unwrap();
    let pack_w2_private = client
        .post(format!("{base}/v1/packs"))
        .header("Authorization", format!("Bearer {t2}"))
        .json(&serde_json::json!({ "query": "beta private evidence" }))
        .send()
        .await
        .unwrap();
    assert_eq!(pack_w2_private.status(), 200);
    let pack_w2_private: serde_json::Value = pack_w2_private.json().await.unwrap();
    let w1_uploaded_source = body_one["source"]["id"].as_str().unwrap();
    let w2_uploaded_source = body_two["source"]["id"].as_str().unwrap();
    assert!(pack_w1_private["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceId"] == body_one["source"]["id"]
            && e["quotedText"]
                .as_str()
                .unwrap_or_default()
                .contains("alpha private evidence")));
    assert!(!pack_w1_private["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceId"] == body_two["source"]["id"]
            || e["quotedText"]
                .as_str()
                .unwrap_or_default()
                .contains("beta private evidence")));
    assert!(pack_w2_private["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceId"] == body_two["source"]["id"]
            && e["quotedText"]
                .as_str()
                .unwrap_or_default()
                .contains("beta private evidence")));
    assert!(!pack_w2_private["selectedEvidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["sourceId"] == body_one["source"]["id"]
            || e["quotedText"]
                .as_str()
                .unwrap_or_default()
                .contains("alpha private evidence")));
    for source in pack_w1_private["sourceSet"].as_array().unwrap_or(&vec![]) {
        assert_ne!(source["sourceId"], w2_uploaded_source);
    }
    for source in pack_w2_private["sourceSet"].as_array().unwrap_or(&vec![]) {
        assert_ne!(source["sourceId"], w1_uploaded_source);
    }

    let invalid_upload = client
        .post(format!("{base}/v1/spike/workspaces/{ws_http}/sources"))
        .header("x-etyma-admin-token", admin)
        .header("x-etyma-source-title", "invalid.bin")
        .header("content-type", "application/octet-stream")
        .body(vec![0xff, 0xfe, 0xfd])
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_upload.status(), 202);
    let invalid_upload: serde_json::Value = invalid_upload.json().await.unwrap();
    let invalid_job_id = invalid_upload["job"]["id"].as_str().unwrap();
    let invalid_source_id = invalid_upload["source"]["id"].as_str().unwrap();
    let invalid_status = wait_for_job(&client, &base, &ws_http, invalid_job_id, admin).await;
    assert_eq!(invalid_status["status"], "failed");
    assert_eq!(invalid_status["attempts"], 1);
    assert!(invalid_status["lastError"]
        .as_str()
        .unwrap()
        .contains("UTF-8"));
    assert!(
        invalid_status["lastError"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 4096
    );

    let failed_source = knowledge
        .get_source(&ws_http, invalid_source_id)
        .await
        .unwrap()
        .expect("failed upload source row exists");
    assert!(blobs.exists(&failed_source.blob_key).unwrap());
    let failed_evidence = knowledge.list_evidence(&ws_http).await.unwrap();
    assert!(!failed_evidence
        .iter()
        .any(|e| e.source_id == invalid_source_id));

    let accepted_boundary = upload_source_with(
        &client,
        &base,
        &ws_http,
        admin,
        &"t".repeat(512),
        None,
        Some("text/plain; charset=utf-8"),
        b"title boundary".to_vec(),
    )
    .await;
    assert_eq!(accepted_boundary.status(), 202);

    let oversized_title = upload_source_with(
        &client,
        &base,
        &ws_http,
        admin,
        &"t".repeat(513),
        None,
        Some("text/plain"),
        b"title too long".to_vec(),
    )
    .await;
    assert_eq!(oversized_title.status(), 400);

    let empty_body = upload_source_with(
        &client,
        &base,
        &ws_http,
        admin,
        "empty.md",
        None,
        Some("text/plain"),
        Vec::new(),
    )
    .await;
    assert_eq!(empty_body.status(), 400);

    let malformed_content_type = upload_source_with(
        &client,
        &base,
        &ws_http,
        admin,
        "bad-type.md",
        None,
        Some("text plain"),
        b"bad content type".to_vec(),
    )
    .await;
    assert_eq!(malformed_content_type.status(), 400);

    let malformed_content_type_params = upload_source_with(
        &client,
        &base,
        &ws_http,
        admin,
        "bad-type-params.md",
        None,
        Some("text/plain; charset==utf-8"),
        b"bad content type params".to_vec(),
    )
    .await;
    assert_eq!(malformed_content_type_params.status(), 400);
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
    let graph = GraphStore::new(pool.clone());
    let blobs = Arc::new(LocalFsBlobStore::open(dir.path().join("blobs")).unwrap());
    // Unique prefix so shared CI/dev Postgres DBs do not collide on fixed org ids.
    let prefix = format!("pg_{}", Uuid::now_v7().simple());
    run_isolation_suite(
        store,
        knowledge,
        graph,
        blobs,
        HostMode::Saas,
        pool,
        &prefix,
    )
    .await;
}
