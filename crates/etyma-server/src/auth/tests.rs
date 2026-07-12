use super::*;
use crate::blob::LocalFsBlobStore;
use crate::config::{AuthConfig, HostMode, OidcConfig};
use crate::graph::GraphStore;
use crate::knowledge::KnowledgeStore;
use crate::seed::seed_multi_source_workspace;
use crate::store::{MembershipRole, OidcIdentityProfile, Store};
use std::sync::Arc;

struct FakeOidcBackend;

impl OidcBackend for FakeOidcBackend {
    fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, AuthError> {
        Ok(format!(
            "https://idp.example/authorize?state={state}&nonce={nonce}&code_challenge={pkce_verifier}"
        ))
    }

    fn exchange_code(&self, code: String, _pkce_verifier: String, _nonce: String) -> OidcFuture {
        Box::pin(async move {
            if code != "code-ok" {
                return Err(AuthError::Provider("fake exchange rejected".into()));
            }
            Ok(OidcIdentityProfile {
                issuer: "https://idp.example".into(),
                subject: "subject-1".into(),
                email: Some("user@example.com".into()),
                display_name: Some("Example User".into()),
                avatar_url: None,
                email_verified: true,
            })
        })
    }
}

fn test_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .merge(crate::http::router())
        .merge(crate::mcp::router())
        .with_state(state)
}

async fn spawn_router(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    format!("http://{address}")
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        oidc: Some(OidcConfig {
            issuer_url: "https://idp.example".into(),
            client_id: "client".into(),
            client_secret: "secret".into(),
            redirect_url: "http://127.0.0.1:8787/v1/auth/callback".into(),
        }),
        session_ttl_seconds: 3600,
        session_cookie_secure: false,
        success_redirect: "/".into(),
    }
}

async fn make_test_state(
    pool: sqlx::PgPool,
    store: Arc<Store>,
    blobs: Arc<LocalFsBlobStore>,
    knowledge: KnowledgeStore,
    graph: GraphStore,
) -> AppState {
    let auth = Arc::new(AuthService::with_backend_for_test(
        store.clone(),
        auth_config(),
        Arc::new(FakeOidcBackend),
    ));
    AppState {
        auth,
        store,
        knowledge,
        graph,
        blobs,
        spike_admin_token: Some("admin-secret".into()),
        host_mode: HostMode::Saas,
        pg_pool: pool,
    }
}

#[tokio::test]
#[ignore = "requires ETYMA_DATABASE_URL"]
async fn oidc_login_creates_session_and_me_returns_human_principal() {
    let pool = crate::db::connect_and_migrate(
        &std::env::var("ETYMA_DATABASE_URL").expect("ETYMA_DATABASE_URL"),
    )
    .await
    .expect("migrate");
    let store = Arc::new(Store::new(pool.clone()));
    let blob_dir = tempfile::tempdir().expect("blob dir");
    let blobs =
        Arc::new(LocalFsBlobStore::open(blob_dir.path().join("blobs")).expect("blob store"));
    let knowledge = KnowledgeStore::new(pool.clone());
    let graph = GraphStore::new(pool.clone());

    let prefix = uuid::Uuid::now_v7().simple().to_string();
    let org_id = format!("auth_org_{prefix}");
    let workspace_id = format!("auth_ws_{prefix}");
    store
        .create_org(&org_id, "Auth Test Org")
        .await
        .expect("org");
    store
        .create_workspace(&org_id, &workspace_id)
        .await
        .expect("workspace");
    seed_multi_source_workspace(&store, &knowledge, blobs.as_ref(), &workspace_id)
        .await
        .expect("seed workspace");
    let api_token = store
        .mint_token(&workspace_id, Some("auth-test"))
        .await
        .expect("api token");

    let state = make_test_state(pool.clone(), store.clone(), blobs, knowledge, graph).await;
    let base = spawn_router(test_router(state)).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");

    let invalid_cookie = client
        .get(format!("{base}/v1/me"))
        .header(reqwest::header::COOKIE, "etyma_session=invalid")
        .send()
        .await
        .expect("invalid cookie");
    assert_eq!(invalid_cookie.status(), reqwest::StatusCode::UNAUTHORIZED);

    let missing_callback_state = client
        .get(format!("{base}/v1/auth/callback?code=code-ok"))
        .send()
        .await
        .expect("missing state callback");
    assert_eq!(
        missing_callback_state.status(),
        reqwest::StatusCode::BAD_REQUEST
    );

    let provider_error_callback = client
        .get(format!(
            "{base}/v1/auth/callback?error=access_denied&error_description=cancelled"
        ))
        .send()
        .await
        .expect("provider error callback");
    assert_eq!(
        provider_error_callback.status(),
        reqwest::StatusCode::BAD_REQUEST
    );

    let login = client
        .get(format!("{base}/v1/auth/login"))
        .send()
        .await
        .expect("login");
    assert_eq!(login.status(), reqwest::StatusCode::FOUND);
    let login_cookie = login
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("login transaction cookie")
        .to_str()
        .expect("login cookie text")
        .split(';')
        .next()
        .expect("login cookie pair")
        .to_owned();
    let location = login
        .headers()
        .get(reqwest::header::LOCATION)
        .expect("authorization location")
        .to_str()
        .expect("location text")
        .to_owned();
    let state = location
        .split("state=")
        .nth(1)
        .and_then(|value| value.split('&').next())
        .expect("state")
        .to_owned();

    let swapped_browser = client
        .get(format!(
            "{base}/v1/auth/callback?code=code-ok&state={state}"
        ))
        .header(reqwest::header::COOKIE, "etyma_login=attacker-binding")
        .send()
        .await
        .expect("swapped browser callback");
    assert_eq!(swapped_browser.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(
        swapped_browser
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.starts_with("etyma_login=") && value.contains("Max-Age=0"))
    );

    let callback = client
        .get(format!(
            "{base}/v1/auth/callback?code=code-ok&state={state}"
        ))
        .header(reqwest::header::COOKIE, &login_cookie)
        .send()
        .await
        .expect("callback");
    assert_eq!(callback.status(), reqwest::StatusCode::SEE_OTHER);
    assert!(
        callback
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.starts_with("etyma_login=") && value.contains("Max-Age=0"))
    );
    let set_cookie = callback
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("session cookie")
        .to_str()
        .expect("cookie text")
        .to_owned();
    assert!(set_cookie.starts_with("etyma_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let cookie = set_cookie.split(';').next().expect("cookie pair");

    let me = client
        .get(format!("{base}/v1/me"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("me");
    assert_eq!(me.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = me.json().await.expect("me json");
    assert_eq!(body["email"], "user@example.com");
    assert_eq!(body["displayName"], "Example User");
    assert_eq!(body["emailVerified"], true);
    assert!(body["userId"].as_str().unwrap_or("").starts_with("usr_"));

    let orgs = client
        .get(format!("{base}/v1/orgs"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("org discovery");
    assert_eq!(orgs.status(), reqwest::StatusCode::OK);
    let orgs: serde_json::Value = orgs.json().await.expect("org discovery json");
    assert_eq!(orgs["orgs"].as_array().map(Vec::len), Some(1));
    let personal_org_id = orgs["orgs"][0]["orgId"].as_str().expect("org id");
    assert_eq!(orgs["orgs"][0]["role"], "owner");

    let members = client
        .get(format!("{base}/v1/orgs/{personal_org_id}/members"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("member discovery");
    assert_eq!(members.status(), reqwest::StatusCode::OK);
    let members: serde_json::Value = members.json().await.expect("member discovery json");
    assert_eq!(members["members"].as_array().map(Vec::len), Some(1));
    assert_eq!(members["members"][0]["role"], "owner");

    let workspaces = client
        .get(format!("{base}/v1/orgs/{personal_org_id}/workspaces"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("workspace discovery");
    assert_eq!(workspaces.status(), reqwest::StatusCode::OK);
    let workspaces: serde_json::Value = workspaces.json().await.expect("workspace json");
    assert_eq!(workspaces["workspaces"].as_array().map(Vec::len), Some(0));

    let foreign = client
        .get(format!("{base}/v1/orgs/unknown-org/members"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("foreign org discovery");
    assert_eq!(foreign.status(), reqwest::StatusCode::NOT_FOUND);

    let human_workspace_id = format!("auth_human_ws_{prefix}");
    let workspace = client
        .post(format!("{base}/v1/spike/orgs/{personal_org_id}/workspaces"))
        .header("x-etyma-admin-token", "admin-secret")
        .json(&serde_json::json!({ "workspaceId": human_workspace_id }))
        .send()
        .await
        .expect("human workspace bootstrap");
    assert_eq!(workspace.status(), reqwest::StatusCode::OK);

    let member_profile = OidcIdentityProfile {
        issuer: "https://idp.example".into(),
        subject: format!("member-{prefix}"),
        email: Some(format!("member-{prefix}@example.com")),
        display_name: Some("Member User".into()),
        avatar_url: None,
        email_verified: true,
    };
    let member = store
        .upsert_oidc_identity(&member_profile)
        .await
        .expect("member user");
    store
        .create_membership(personal_org_id, &member.id, MembershipRole::Member)
        .await
        .expect("member membership");
    let member_session = format!("member-session-{prefix}");
    store
        .create_session(&member.id, &member_session, 2_000_000_000)
        .await
        .expect("member session");
    let member_orgs = client
        .get(format!("{base}/v1/orgs"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .send()
        .await
        .expect("member org discovery");
    assert_eq!(member_orgs.status(), reqwest::StatusCode::OK);
    let member_orgs: serde_json::Value = member_orgs.json().await.expect("member org json");
    assert!(
        member_orgs["orgs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|org| { org["orgId"] == personal_org_id && org["role"] == "member" })
    );
    let member_members = client
        .get(format!("{base}/v1/orgs/{personal_org_id}/members"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .send()
        .await
        .expect("member member discovery");
    assert_eq!(member_members.status(), reqwest::StatusCode::OK);
    let member_members: serde_json::Value = member_members.json().await.expect("member list");
    assert_eq!(member_members["members"].as_array().map(Vec::len), Some(2));
    let member_workspaces = client
        .get(format!("{base}/v1/orgs/{personal_org_id}/workspaces"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .send()
        .await
        .expect("member workspace discovery");
    assert_eq!(member_workspaces.status(), reqwest::StatusCode::OK);
    let member_workspaces: serde_json::Value = member_workspaces
        .json()
        .await
        .expect("member workspace list");
    assert!(
        member_workspaces["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["workspaceId"] == human_workspace_id)
    );
    let member_token_attempt = client
        .post(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .json(&serde_json::json!({ "label": "member-cannot-mint" }))
        .send()
        .await
        .expect("member token attempt");
    assert_eq!(
        member_token_attempt.status(),
        reqwest::StatusCode::FORBIDDEN
    );

    let outsider_profile = OidcIdentityProfile {
        issuer: "https://idp.example".into(),
        subject: format!("outsider-{prefix}"),
        email: Some(format!("outsider-{prefix}@example.com")),
        display_name: Some("Outsider User".into()),
        avatar_url: None,
        email_verified: true,
    };
    let outsider = store
        .upsert_oidc_identity(&outsider_profile)
        .await
        .expect("outsider user");
    let outsider_session = format!("outsider-session-{prefix}");
    store
        .create_session(&outsider.id, &outsider_session, 2_000_000_000)
        .await
        .expect("outsider session");
    let outsider_token_attempt = client
        .post(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={outsider_session}"),
        )
        .json(&serde_json::json!({ "label": "outsider-cannot-mint" }))
        .send()
        .await
        .expect("outsider token attempt");
    assert_eq!(
        outsider_token_attempt.status(),
        reqwest::StatusCode::NOT_FOUND
    );

    let minted = client
        .post(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(reqwest::header::COOKIE, cookie)
        .json(&serde_json::json!({ "label": "  human-test  " }))
        .send()
        .await
        .expect("human token mint");
    assert_eq!(minted.status(), reqwest::StatusCode::OK);
    let minted: serde_json::Value = minted.json().await.expect("human token json");
    let human_token = minted["token"]
        .as_str()
        .expect("raw human token")
        .to_owned();
    let human_token_id = minted["tokenId"]
        .as_str()
        .expect("human token id")
        .to_owned();
    assert!(human_token.starts_with("etyma_"));
    assert!(human_token_id.starts_with("tok_"));
    assert_eq!(minted["label"], "human-test");
    assert!(minted.get("tokenHash").is_none());

    let too_long_label = client
        .post(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(reqwest::header::COOKIE, cookie)
        .json(&serde_json::json!({ "label": "x".repeat(129) }))
        .send()
        .await
        .expect("overlong token label");
    assert_eq!(too_long_label.status(), reqwest::StatusCode::BAD_REQUEST);

    let token_list = client
        .get(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("human token list");
    assert_eq!(token_list.status(), reqwest::StatusCode::OK);
    let token_list: serde_json::Value = token_list.json().await.expect("token list json");
    assert_eq!(token_list["tokens"].as_array().map(Vec::len), Some(1));
    assert!(token_list["tokens"][0].get("token").is_none());
    assert!(token_list["tokens"][0].get("tokenHash").is_none());

    let member_list_attempt = client
        .get(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .send()
        .await
        .expect("member token list attempt");
    assert_eq!(member_list_attempt.status(), reqwest::StatusCode::FORBIDDEN);
    let member_revoke_attempt = client
        .delete(format!(
            "{base}/v1/workspaces/{human_workspace_id}/tokens/{human_token_id}"
        ))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={member_session}"),
        )
        .send()
        .await
        .expect("member token revoke attempt");
    assert_eq!(
        member_revoke_attempt.status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let outsider_list_attempt = client
        .get(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={outsider_session}"),
        )
        .send()
        .await
        .expect("outsider token list attempt");
    assert_eq!(
        outsider_list_attempt.status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let outsider_revoke_attempt = client
        .delete(format!(
            "{base}/v1/workspaces/{human_workspace_id}/tokens/{human_token_id}"
        ))
        .header(
            reqwest::header::COOKIE,
            format!("etyma_session={outsider_session}"),
        )
        .send()
        .await
        .expect("outsider token revoke attempt");
    assert_eq!(
        outsider_revoke_attempt.status(),
        reqwest::StatusCode::NOT_FOUND
    );

    let agent_sources = client
        .get(format!("{base}/v1/sources"))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {human_token}"),
        )
        .send()
        .await
        .expect("human token agent request");
    assert_eq!(agent_sources.status(), reqwest::StatusCode::OK);
    let agent_sources: serde_json::Value = agent_sources.json().await.expect("sources json");
    assert_eq!(agent_sources["workspaceId"], human_workspace_id);

    let revoked = client
        .delete(format!(
            "{base}/v1/workspaces/{human_workspace_id}/tokens/{human_token_id}"
        ))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("human token revoke");
    assert_eq!(revoked.status(), reqwest::StatusCode::NO_CONTENT);

    let after_revoke = client
        .get(format!("{base}/v1/workspaces/{human_workspace_id}/tokens"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("revoked token metadata");
    assert_eq!(after_revoke.status(), reqwest::StatusCode::OK);
    let after_revoke: serde_json::Value = after_revoke.json().await.expect("revoked token list");
    let revoked_metadata = after_revoke["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .find(|token| token["tokenId"] == human_token_id)
        .expect("revoked token metadata row");
    assert!(revoked_metadata["revokedAt"].as_i64().is_some());

    let repeat_revoke = client
        .delete(format!(
            "{base}/v1/workspaces/{human_workspace_id}/tokens/{human_token_id}"
        ))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("repeat human token revoke");
    assert_eq!(repeat_revoke.status(), reqwest::StatusCode::NO_CONTENT);

    let revoked_agent = client
        .get(format!("{base}/v1/sources"))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {human_token}"),
        )
        .send()
        .await
        .expect("revoked agent request");
    assert_eq!(revoked_agent.status(), reqwest::StatusCode::UNAUTHORIZED);
    let revoked_mcp = client
        .post(format!("{base}/v1/mcp"))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {human_token}"),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "get_context_pack",
                "arguments": { "query": "alpha-token" }
            }
        }))
        .send()
        .await
        .expect("revoked MCP request");
    assert_eq!(revoked_mcp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let pack = client
        .post(format!("{base}/v1/packs"))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {api_token}"),
        )
        .json(&serde_json::json!({ "query": "alpha-token" }))
        .send()
        .await
        .expect("api token pack");
    assert_eq!(pack.status(), reqwest::StatusCode::OK);

    let reused = client
        .get(format!(
            "{base}/v1/auth/callback?code=code-ok&state={state}"
        ))
        .header(reqwest::header::COOKIE, &login_cookie)
        .send()
        .await
        .expect("reused callback");
    assert_eq!(reused.status(), reqwest::StatusCode::BAD_REQUEST);

    let logout = client
        .post(format!("{base}/v1/auth/logout"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("logout");
    assert_eq!(logout.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(
        logout
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .expect("clear cookie")
            .to_str()
            .expect("clear cookie text")
            .contains("Max-Age=0")
    );

    let me_after_logout = client
        .get(format!("{base}/v1/me"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("me after logout");
    assert_eq!(me_after_logout.status(), reqwest::StatusCode::UNAUTHORIZED);

    let admin = client
        .post(format!("{base}/v1/spike/orgs"))
        .header("x-etyma-admin-token", "admin-secret")
        .json(&serde_json::json!({ "name": "Admin Org" }))
        .send()
        .await
        .expect("admin route");
    assert_eq!(admin.status(), reqwest::StatusCode::OK);
}
