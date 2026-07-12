use crate::auth::{
    build_clear_cookie, build_session_cookie, parse_session_cookie, require_admin, validate_org_id,
    validate_workspace_id, AppState, AuthError, AuthenticatedUser, AuthenticatedWorkspace,
};
use crate::compose::compose_pack;
use crate::seed::seed_multi_source_workspace;
use crate::store::StoreError;
use axum::extract::{FromRequestParts, State};
use axum::http::header::{HeaderValue, COOKIE, LOCATION, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use etyma_engine_types::ContextPackV1;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        // `/health` is the plan-aligned path; `/healthz` stays for backward compatibility.
        .route("/health", get(health))
        .route("/healthz", get(health))
        .route("/v1/auth/login", get(auth_login))
        .route("/v1/auth/callback", get(auth_callback))
        .route("/v1/auth/logout", post(auth_logout))
        .route("/v1/me", get(me))
        .route("/v1/sources", get(list_sources))
        .route("/v1/graph/snapshot", get(graph_snapshot))
        .route("/v1/packs", post(create_pack))
        .route("/v1/spike/orgs", post(create_org).get(list_orgs))
        .route(
            "/v1/spike/orgs/{org_id}/workspaces",
            post(create_workspace).get(list_workspaces),
        )
        .route(
            "/v1/spike/workspaces/{workspace_id}/tokens",
            post(mint_token),
        )
        .route(
            "/v1/spike/workspaces/{workspace_id}/seed",
            post(seed_workspace),
        )
}

async fn auth_login(State(state): State<AppState>) -> Result<Response, (StatusCode, String)> {
    let location = state.auth.begin_login().await.map_err(auth_error)?;
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&location).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid login redirect".into(),
            )
        })?,
    );
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct AuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    #[serde(rename = "error_description")]
    error_description: Option<String>,
}

async fn auth_callback(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<AuthCallbackQuery>,
) -> Result<Response, (StatusCode, String)> {
    if query.error.is_some() || query.error_description.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "OIDC provider rejected authentication".into(),
        ));
    }
    let code = query.code.as_deref().unwrap_or_default();
    let state_value = query.state.as_deref().unwrap_or_default();
    let session = state
        .auth
        .finish_login(code, state_value)
        .await
        .map_err(auth_error)?;
    let mut response = axum::response::Redirect::to(state.auth.success_redirect()).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&build_session_cookie(
            &session.raw_token,
            session.max_age_seconds,
            state.auth.session_cookie_secure(),
        ))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid session cookie".into(),
            )
        })?,
    );
    Ok(response)
}

async fn me(auth: AuthenticatedUser) -> Result<Json<MeResponse>, (StatusCode, String)> {
    Ok(Json(MeResponse {
        user_id: auth.user.id,
        email: auth.user.email,
        display_name: auth.user.display_name,
        avatar_url: auth.user.avatar_url,
        email_verified: auth.user.email_verified,
    }))
}

async fn auth_logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, (StatusCode, String)> {
    if let Some(raw_token) = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_session_cookie)
    {
        state.auth.logout(&raw_token).await.map_err(auth_error)?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&build_clear_cookie(state.auth.session_cookie_secure())).map_err(
            |_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "invalid session cookie".into(),
                )
            },
        )?,
    );
    Ok(response)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeResponse {
    user_id: String,
    email: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    email_verified: bool,
}

fn auth_error(error: AuthError) -> (StatusCode, String) {
    match error {
        AuthError::Disabled => (
            StatusCode::SERVICE_UNAVAILABLE,
            "OIDC authentication is not configured".into(),
        ),
        AuthError::InvalidCallback(_) | AuthError::InvalidState => (
            StatusCode::BAD_REQUEST,
            "invalid authentication callback".into(),
        ),
        AuthError::Provider(_) => (
            StatusCode::BAD_GATEWAY,
            "OIDC provider authentication failed".into(),
        ),
        AuthError::Storage(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "authentication storage failed".into(),
        ),
        AuthError::Configuration(_) => (
            StatusCode::BAD_GATEWAY,
            "OIDC provider configuration failed".into(),
        ),
    }
}

/// Readiness for `/health` and `/healthz` includes the mandatory Postgres probe.
///
/// Prefer this for readiness, not process liveness: a brief DB blip should not
/// restart the process.
async fn health(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (http_status, postgres, ok) = match crate::db::health_check(&state.pg_pool).await {
        Ok(()) => (StatusCode::OK, "up", true),
        Err(err) => {
            tracing::warn!(error = %err, "postgres health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "down", false)
        }
    };

    let body = json!({
        "ok": ok,
        "status": if ok { "ok" } else { "degraded" },
        "service": "etyma-server",
        "mode": state.host_mode.as_str(),
        "postgres": postgres,
    });

    if ok {
        Ok(Json(body))
    } else {
        Err((http_status, Json(body)))
    }
}

async fn list_sources(
    State(state): State<AppState>,
    auth: AuthenticatedWorkspace,
) -> Result<Json<Value>, (StatusCode, String)> {
    let sources = state
        .knowledge
        .list_sources(&auth.workspace_id)
        .await
        .map_err(knowledge_err)?;
    let body: Vec<Value> = sources
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "kind": s.kind,
                "title": s.title,
                "blobKey": s.blob_key,
                "contentHash": s.content_hash,
                "byteSize": s.byte_size,
                "contentType": s.content_type,
                "externalId": s.external_id,
            })
        })
        .collect();
    Ok(Json(
        json!({ "workspaceId": auth.workspace_id, "sources": body }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackRequest {
    query: String,
}

async fn graph_snapshot(
    State(state): State<AppState>,
    auth: AuthenticatedWorkspace,
) -> Result<Json<crate::graph::GraphSnapshotResponse>, (StatusCode, String)> {
    let snap = state
        .graph
        .live_snapshot(&auth.workspace_id)
        .await
        .map_err(graph_err)?;
    Ok(Json(crate::graph::GraphSnapshotResponse::from(snap)))
}

async fn create_pack(
    State(state): State<AppState>,
    auth: AuthenticatedWorkspace,
    Json(body): Json<PackRequest>,
) -> Result<Json<ContextPackV1>, (StatusCode, String)> {
    let pack = compose_pack(
        &state.knowledge,
        &state.graph,
        state.blobs.as_ref(),
        &auth.workspace_id,
        &body.query,
    )
    .await
    .map_err(store_err)?;
    Ok(Json(pack))
}

struct AdminAuth;

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        require_admin(state, parts)?;
        Ok(Self)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateOrgRequest {
    name: String,
    #[serde(default)]
    org_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OrgBody {
    org_id: String,
    name: String,
}

async fn create_org(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<CreateOrgRequest>,
) -> Result<Json<OrgBody>, (StatusCode, String)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let org_id = body
        .org_id
        .unwrap_or_else(|| format!("org_{}", Uuid::now_v7().simple()));
    validate_org_id(&org_id)?;
    let org = state
        .store
        .create_org(&org_id, name)
        .await
        .map_err(store_err)?;
    Ok(Json(OrgBody {
        org_id: org.id,
        name: org.name,
    }))
}

async fn list_orgs(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> Result<Json<Value>, (StatusCode, String)> {
    let orgs = state.store.list_orgs().await.map_err(store_err)?;
    let body: Vec<OrgBody> = orgs
        .into_iter()
        .map(|o| OrgBody {
            org_id: o.id,
            name: o.name,
        })
        .collect();
    Ok(Json(json!({ "orgs": body })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkspaceRequest {
    #[serde(default)]
    workspace_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceBody {
    workspace_id: String,
    org_id: String,
}

async fn create_workspace(
    State(state): State<AppState>,
    _admin: AdminAuth,
    axum::extract::Path(org_id): axum::extract::Path<String>,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceBody>, (StatusCode, String)> {
    validate_org_id(&org_id)?;
    let workspace_id = body
        .workspace_id
        .unwrap_or_else(|| format!("ws_{}", Uuid::now_v7().simple()));
    validate_workspace_id(&workspace_id)?;
    // Store enforces parent org existence + unique id (no double HTTP pre-check).
    let ws = state
        .store
        .create_workspace(&org_id, &workspace_id)
        .await
        .map_err(store_err)?;
    Ok(Json(WorkspaceBody {
        workspace_id: ws.id,
        org_id: ws.org_id,
    }))
}

async fn list_workspaces(
    State(state): State<AppState>,
    _admin: AdminAuth,
    axum::extract::Path(org_id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    validate_org_id(&org_id)?;
    let workspaces = state
        .store
        .list_workspaces(&org_id)
        .await
        .map_err(store_err)?;
    let body: Vec<WorkspaceBody> = workspaces
        .into_iter()
        .map(|w| WorkspaceBody {
            workspace_id: w.id,
            org_id: w.org_id,
        })
        .collect();
    Ok(Json(json!({ "orgId": org_id, "workspaces": body })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MintTokenRequest {
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MintTokenResponse {
    workspace_id: String,
    token: String,
}

async fn mint_token(
    State(state): State<AppState>,
    _admin: AdminAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Json(body): Json<MintTokenRequest>,
) -> Result<Json<MintTokenResponse>, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    let token = state
        .store
        .mint_token(&workspace_id, body.label.as_deref())
        .await
        .map_err(store_err)?;
    Ok(Json(MintTokenResponse {
        workspace_id,
        token,
    }))
}

async fn seed_workspace(
    State(state): State<AppState>,
    _admin: AdminAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    let source_count = seed_multi_source_workspace(
        &state.store,
        &state.knowledge,
        state.blobs.as_ref(),
        &workspace_id,
    )
    .await
    .map_err(store_err)?;
    let sources = state
        .knowledge
        .list_sources(&workspace_id)
        .await
        .map_err(knowledge_err)?;
    Ok(Json(json!({
        "workspaceId": workspace_id,
        "sourceCount": source_count,
        "kinds": sources.iter().map(|s| s.kind.clone()).collect::<Vec<_>>(),
        "blobs": sources.iter().map(|s| json!({
            "sourceId": s.id,
            "blobKey": s.blob_key,
            "contentHash": s.content_hash,
            "byteSize": s.byte_size,
        })).collect::<Vec<_>>(),
    })))
}

fn store_err(err: StoreError) -> (StatusCode, String) {
    let status = match &err {
        StoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::Conflict(_) => StatusCode::CONFLICT,
        StoreError::Integrity(_) => StatusCode::BAD_REQUEST,
        StoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, err.to_string())
}

fn knowledge_err(err: crate::knowledge::KnowledgeError) -> (StatusCode, String) {
    store_err(err.into())
}

fn graph_err(err: crate::graph::GraphError) -> (StatusCode, String) {
    store_err(err.into())
}
