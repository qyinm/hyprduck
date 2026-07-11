use crate::auth::{
    require_admin, validate_org_id, validate_workspace_id, AppState, AuthenticatedWorkspace,
};
use crate::compose::compose_pack;
use crate::seed::seed_multi_source_workspace;
use crate::store::StoreError;
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
        .map_err(store_err)?;
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
    let pack =
        compose_pack(&state.store, &auth.workspace_id, &body.query).map_err(store_err)?;
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
    let org = state.store.create_org(&org_id, name).map_err(store_err)?;
    Ok(Json(OrgBody {
        org_id: org.id,
        name: org.name,
    }))
}

async fn list_orgs(
    State(state): State<AppState>,
    _admin: AdminAuth,
) -> Result<Json<Value>, (StatusCode, String)> {
    let orgs = state.store.list_orgs().map_err(store_err)?;
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
    let workspaces = state.store.list_workspaces(&org_id).map_err(store_err)?;
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
    let source_count =
        seed_multi_source_workspace(&state.store, &workspace_id).map_err(store_err)?;
    let sources = state.store.list_sources(&workspace_id).map_err(store_err)?;
    Ok(Json(json!({
        "workspaceId": workspace_id,
        "sourceCount": source_count,
        "kinds": sources.iter().map(|s| s.kind.clone()).collect::<Vec<_>>(),
    })))
}

fn store_err(err: StoreError) -> (StatusCode, String) {
    let status = match &err {
        StoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::Conflict(_) => StatusCode::CONFLICT,
        StoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, err.to_string())
}
