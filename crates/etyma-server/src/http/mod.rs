use crate::auth::{require_admin, validate_workspace_id, AppState, AuthenticatedWorkspace};
use crate::compose::compose_pack;
use crate::seed::seed_multi_source_workspace;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use etyma_engine_types::ContextPackV1;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/sources", get(list_sources))
        .route("/v1/packs", post(create_pack))
        .route("/v1/spike/workspaces", post(create_workspace))
        .route(
            "/v1/spike/workspaces/{workspace_id}/tokens",
            post(mint_token),
        )
        .route(
            "/v1/spike/workspaces/{workspace_id}/seed",
            post(seed_workspace),
        )
}

async fn healthz() -> Json<Value> {
    Json(json!({ "ok": true, "service": "etyma-server", "mode": "spike" }))
}

async fn list_sources(
    State(state): State<AppState>,
    auth: AuthenticatedWorkspace,
) -> Result<Json<Value>, (StatusCode, String)> {
    let sources = state
        .store
        .list_sources(&auth.workspace_id)
        .map_err(internal)?;
    let body: Vec<Value> = sources
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "kind": s.kind,
                "title": s.title,
                "externalId": s.external_id,
            })
        })
        .collect();
    Ok(Json(json!({ "workspaceId": auth.workspace_id, "sources": body })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackRequest {
    query: String,
}

async fn create_pack(
    State(state): State<AppState>,
    auth: AuthenticatedWorkspace,
    Json(body): Json<PackRequest>,
) -> Result<Json<ContextPackV1>, (StatusCode, String)> {
    let pack = compose_pack(&state.store, &auth.workspace_id, &body.query).map_err(internal)?;
    Ok(Json(pack))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkspaceRequest {
    #[serde(default)]
    workspace_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkspaceResponse {
    workspace_id: String,
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

async fn create_workspace(
    State(state): State<AppState>,
    _admin: AdminAuth,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<Json<CreateWorkspaceResponse>, (StatusCode, String)> {
    let workspace_id = body
        .workspace_id
        .unwrap_or_else(|| format!("ws_{}", Uuid::now_v7().simple()));
    validate_workspace_id(&workspace_id)?;
    state
        .store
        .create_workspace(&workspace_id)
        .map_err(internal)?;
    Ok(Json(CreateWorkspaceResponse { workspace_id }))
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
    if state
        .store
        .get_workspace(&workspace_id)
        .map_err(internal)?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, "workspace not found".into()));
    }
    let token = state
        .store
        .mint_token(&workspace_id, body.label.as_deref())
        .map_err(internal)?;
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
    if state
        .store
        .get_workspace(&workspace_id)
        .map_err(internal)?
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, "workspace not found".into()));
    }
    let source_count = seed_multi_source_workspace(&state.store, &workspace_id).map_err(internal)?;
    let sources = state.store.list_sources(&workspace_id).map_err(internal)?;
    Ok(Json(json!({
        "workspaceId": workspace_id,
        "sourceCount": source_count,
        "kinds": sources.iter().map(|s| s.kind.clone()).collect::<Vec<_>>(),
    })))
}

fn internal(err: anyhow::Error) -> (StatusCode, String) {
    let msg = format!("{err:#}");
    if msg.contains("workspace not found") {
        return (StatusCode::NOT_FOUND, msg);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, msg)
}
