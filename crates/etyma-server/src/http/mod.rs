use crate::auth::{
    AppState, AuthError, AuthenticatedUser, AuthenticatedWorkspace, authorize_human_org,
    authorize_human_workspace, build_clear_cookie, build_clear_login_transaction_cookie,
    build_login_transaction_cookie, build_session_cookie, parse_login_transaction_cookie,
    parse_session_cookie, require_admin, validate_org_id, validate_tenant_id,
    validate_workspace_id,
};
use crate::compose::compose_pack;
use crate::ingest::ingest_source;
use crate::knowledge::{ImportJobRow, SourceRow};
use crate::seed::seed_multi_source_workspace;
use crate::store::StoreError;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::header::{COOKIE, HeaderValue, LOCATION, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use etyma_engine_types::ContextPackV1;
use mime::Mime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
        .route("/v1/orgs", get(list_my_orgs))
        .route("/v1/orgs/{org_id}/members", get(list_my_org_members))
        .route("/v1/orgs/{org_id}/workspaces", get(list_my_workspaces))
        .route(
            "/v1/workspaces/{workspace_id}/tokens",
            get(list_human_tokens).post(mint_human_token),
        )
        .route(
            "/v1/workspaces/{workspace_id}/tokens/{token_id}",
            delete(revoke_human_token),
        )
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
        .route(
            "/v1/spike/workspaces/{workspace_id}/sources",
            post(upload_source).layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route(
            "/v1/spike/workspaces/{workspace_id}/import-jobs/{job_id}",
            get(import_job_status),
        )
}

async fn auth_login(State(state): State<AppState>) -> Result<Response, (StatusCode, String)> {
    let login = state.auth.begin_login().await.map_err(auth_error)?;
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&login.authorization_url).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid login redirect".into(),
            )
        })?,
    );
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&build_login_transaction_cookie(
            &login.browser_binding,
            state.auth.session_cookie_secure(),
        ))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid login transaction cookie".into(),
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
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<AuthCallbackQuery>,
) -> Response {
    let secure = state.auth.session_cookie_secure();
    let result: Result<Response, (StatusCode, String)> = async {
        if query.error.is_some() || query.error_description.is_some() {
            return Err((
                StatusCode::BAD_REQUEST,
                "OIDC provider rejected authentication".into(),
            ));
        }
        let browser_binding = headers
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_login_transaction_cookie)
            .ok_or((
                StatusCode::BAD_REQUEST,
                "invalid authentication callback".into(),
            ))?;
        let session = state
            .auth
            .finish_login(
                query.code.as_deref().unwrap_or_default(),
                query.state.as_deref().unwrap_or_default(),
                &browser_binding,
            )
            .await
            .map_err(auth_error)?;
        let mut response =
            axum::response::Redirect::to(state.auth.success_redirect()).into_response();
        response.headers_mut().append(
            SET_COOKIE,
            HeaderValue::from_str(&build_session_cookie(
                &session.raw_token,
                session.max_age_seconds,
                secure,
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
    .await;
    let mut response = result.unwrap_or_else(IntoResponse::into_response);
    if let Ok(clear_cookie) = HeaderValue::from_str(&build_clear_login_transaction_cookie(secure)) {
        response.headers_mut().append(SET_COOKIE, clear_cookie);
    }
    response
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

async fn list_my_orgs(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
) -> Result<Json<Value>, (StatusCode, String)> {
    let orgs = state
        .store
        .list_user_orgs(&auth.user.id)
        .await
        .map_err(store_err)?;
    Ok(Json(json!({
        "orgs": orgs.into_iter().map(|org| json!({
            "orgId": org.org_id,
            "name": org.name,
            "role": org.role.as_str(),
        })).collect::<Vec<_>>(),
    })))
}

async fn list_my_org_members(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    validate_org_id(&org_id)?;
    authorize_human_org(&state, &auth.user.id, &org_id).await?;
    let members = state
        .store
        .list_org_members(&org_id)
        .await
        .map_err(store_err)?;
    Ok(Json(json!({
        "orgId": org_id,
        "members": members.into_iter().map(|member| json!({
            "userId": member.user_id,
            "email": member.email,
            "displayName": member.display_name,
            "role": member.role.as_str(),
        })).collect::<Vec<_>>(),
    })))
}

async fn list_my_workspaces(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(org_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    validate_org_id(&org_id)?;
    authorize_human_org(&state, &auth.user.id, &org_id).await?;
    let workspaces = state
        .store
        .list_user_workspaces(&auth.user.id, &org_id)
        .await
        .map_err(store_err)?;
    Ok(Json(json!({
        "orgId": org_id,
        "workspaces": workspaces.into_iter().map(|workspace| json!({
            "workspaceId": workspace.workspace_id,
            "orgId": workspace.org_id,
            "role": workspace.role.as_str(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanTokenBody {
    token_id: String,
    workspace_id: String,
    label: Option<String>,
    created_at: i64,
    revoked_at: Option<i64>,
}

impl From<crate::store::ApiTokenRow> for HumanTokenBody {
    fn from(token: crate::store::ApiTokenRow) -> Self {
        Self {
            token_id: token.id,
            workspace_id: token.workspace_id,
            label: token.label,
            created_at: token.created_at,
            revoked_at: token.revoked_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanMintTokenResponse {
    token_id: String,
    workspace_id: String,
    token: String,
    label: Option<String>,
    created_at: i64,
}

fn normalize_token_label(label: Option<&str>) -> Result<Option<String>, (StatusCode, String)> {
    let label = label.map(str::trim).filter(|label| !label.is_empty());
    if label.is_some_and(|label| label.chars().count() > 128) {
        return Err((
            StatusCode::BAD_REQUEST,
            "token label must be at most 128 characters".into(),
        ));
    }
    Ok(label.map(str::to_owned))
}

async fn mint_human_token(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(workspace_id): Path<String>,
    Json(body): Json<MintTokenRequest>,
) -> Result<Json<HumanMintTokenResponse>, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    authorize_human_workspace(&state, &auth.user.id, &workspace_id, true).await?;
    let label = normalize_token_label(body.label.as_deref())?;
    let minted = state
        .store
        .mint_token_with_metadata(&workspace_id, label.as_deref())
        .await
        .map_err(store_err)?;
    Ok(Json(HumanMintTokenResponse {
        token_id: minted.token.id,
        workspace_id: minted.token.workspace_id,
        token: minted.raw_token,
        label: minted.token.label,
        created_at: minted.token.created_at,
    }))
}

async fn list_human_tokens(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path(workspace_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    authorize_human_workspace(&state, &auth.user.id, &workspace_id, true).await?;
    let tokens = state
        .store
        .list_tokens(&workspace_id)
        .await
        .map_err(store_err)?;
    Ok(Json(json!({
        "workspaceId": workspace_id,
        "tokens": tokens.into_iter().map(HumanTokenBody::from).collect::<Vec<_>>(),
    })))
}

async fn revoke_human_token(
    State(state): State<AppState>,
    auth: AuthenticatedUser,
    Path((workspace_id, token_id)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    validate_tenant_id("token", &token_id)?;
    authorize_human_workspace(&state, &auth.user.id, &workspace_id, true).await?;
    state
        .store
        .revoke_token(&workspace_id, &token_id, current_unix_seconds())
        .await
        .map_err(store_err)?;
    Ok(StatusCode::NO_CONTENT)
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadSourceResponse {
    workspace_id: String,
    source: SourceBody,
    job: ImportJobBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceBody {
    id: String,
    kind: String,
    title: String,
    blob_key: String,
    content_hash: String,
    byte_size: i64,
    content_type: String,
    external_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportJobBody {
    id: String,
    workspace_id: String,
    source_id: Option<String>,
    kind: String,
    status: crate::knowledge::ImportJobStatus,
    attempts: i32,
    max_attempts: i32,
    last_error: Option<String>,
    available_at: i64,
    created_at: i64,
    updated_at: i64,
}

impl From<SourceRow> for SourceBody {
    fn from(source: SourceRow) -> Self {
        Self {
            id: source.id,
            kind: source.kind,
            title: source.title,
            blob_key: source.blob_key,
            content_hash: source.content_hash,
            byte_size: source.byte_size,
            content_type: source.content_type,
            external_id: source.external_id,
        }
    }
}

impl From<ImportJobRow> for ImportJobBody {
    fn from(job: ImportJobRow) -> Self {
        Self {
            id: job.id,
            workspace_id: job.workspace_id,
            source_id: job.source_id,
            kind: job.kind,
            status: job.status,
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            last_error: job.last_error,
            available_at: job.available_at,
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }
}

async fn upload_source(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<UploadSourceResponse>), (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    let title = required_bounded_header(&headers, "x-etyma-source-title", "source title", 512)?;
    let kind = optional_kind_header(&headers)?;
    let external_id =
        optional_bounded_header(&headers, "x-etyma-source-external-id", "external id", 512)?;
    let content_type = optional_content_type(&headers)?;
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "source body is required".into()));
    }

    let source = ingest_source(
        &state.store,
        &state.knowledge,
        state.blobs.as_ref(),
        &workspace_id,
        &kind,
        &title,
        &body,
        &content_type,
        external_id.as_deref(),
    )
    .await
    .map_err(store_err)?;
    let job = state
        .knowledge
        .enqueue_upload_job(&workspace_id, &source.id)
        .await
        .map_err(knowledge_err)?;
    let source = state
        .knowledge
        .get_source(&workspace_id, &source.id)
        .await
        .map_err(knowledge_err)?
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("uploaded source not found: {}", source.id),
        ))?;
    crate::import_job::spawn_upload_job(
        state.knowledge.clone(),
        state.blobs.clone(),
        workspace_id.clone(),
        source.clone(),
        job.id.clone(),
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(UploadSourceResponse {
            workspace_id,
            source: source.into(),
            job: job.into(),
        }),
    ))
}

async fn import_job_status(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Path((workspace_id, job_id)): Path<(String, String)>,
) -> Result<Json<ImportJobBody>, (StatusCode, String)> {
    validate_workspace_id(&workspace_id)?;
    let job = state
        .knowledge
        .get_import_job(&workspace_id, &job_id)
        .await
        .map_err(knowledge_err)?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("import job not found: {job_id}"),
        ))?;
    Ok(Json(job.into()))
}

fn required_bounded_header(
    headers: &HeaderMap,
    name: &'static str,
    label: &'static str,
    max_len: usize,
) -> Result<String, (StatusCode, String)> {
    let value = header_value(headers, name)?
        .ok_or((StatusCode::BAD_REQUEST, format!("{label} is required")))?;
    bounded_nonempty_value(value, label, max_len)
}

fn optional_bounded_header(
    headers: &HeaderMap,
    name: &'static str,
    label: &'static str,
    max_len: usize,
) -> Result<Option<String>, (StatusCode, String)> {
    match header_value(headers, name)? {
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => bounded_nonempty_value(value, label, max_len).map(Some),
        None => Ok(None),
    }
}

fn optional_kind_header(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let Some(value) = header_value(headers, "x-etyma-source-kind")? else {
        return Ok("document".into());
    };
    let value = bounded_nonempty_value(value, "source kind", 64)?;
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "source kind must match [A-Za-z0-9_-]+".into(),
        ));
    }
    Ok(value)
}

fn optional_content_type(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    match headers.get(header::CONTENT_TYPE) {
        Some(value) => {
            let value = value
                .to_str()
                .map_err(|_| (StatusCode::BAD_REQUEST, "content-type is invalid".into()))?
                .trim();
            if value.is_empty() || value.len() > 255 {
                return Err((StatusCode::BAD_REQUEST, "content-type is invalid".into()));
            }
            let parsed: Mime = value
                .parse()
                .map_err(|_| (StatusCode::BAD_REQUEST, "content-type is invalid".into()))?;
            Ok(parsed.to_string())
        }
        None => Ok("application/octet-stream".into()),
    }
}

fn header_value<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<Option<&'a str>, (StatusCode, String)> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| (StatusCode::BAD_REQUEST, format!("{name} header is invalid")))
        })
        .transpose()
}

fn bounded_nonempty_value(
    value: &str,
    label: &'static str,
    max_len: usize,
) -> Result<String, (StatusCode, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err((StatusCode::BAD_REQUEST, format!("{label} is required")));
    }
    if trimmed.chars().count() > max_len {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be at most {max_len} characters"),
        ));
    }
    Ok(trimmed.to_owned())
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

fn current_unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
