use crate::blob::BlobStore;
use crate::store::Store;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub blobs: Arc<dyn BlobStore>,
    pub spike_admin_token: Option<String>,
    /// Postgres pool when `ETYMA_DATABASE_URL` is set; `None` keeps the spike SQLite path.
    pub pg_pool: Option<sqlx::PgPool>,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedWorkspace {
    pub workspace_id: String,
}

impl FromRequestParts<AppState> for AuthenticatedWorkspace {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let workspace_id = resolve_bearer_workspace(state, &parts.headers)?;
        Ok(Self { workspace_id })
    }
}

/// Single place for workspace bearer auth (REST + MCP).
pub fn resolve_bearer_workspace(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, (StatusCode, String)> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "missing Authorization bearer token".into(),
        ))?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or(header)
        .trim();
    if token.is_empty() {
        return Err((StatusCode::UNAUTHORIZED, "empty bearer token".into()));
    }
    state
        .store
        .resolve_token(token)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "invalid token".into()))
}

pub fn require_admin(state: &AppState, parts: &Parts) -> Result<(), (StatusCode, String)> {
    let expected = state.spike_admin_token.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "ETYMA_SPIKE_ADMIN_TOKEN is not configured".into(),
    ))?;
    let header = parts
        .headers
        .get("x-etyma-admin-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if header != expected {
        return Err((StatusCode::UNAUTHORIZED, "invalid admin token".into()));
    }
    Ok(())
}

/// Org / workspace ids are tenant keys (not filesystem paths).
pub fn validate_tenant_id(kind: &str, id: &str) -> Result<(), (StatusCode, String)> {
    if id.is_empty() || id.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("invalid {kind} id length"),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{kind} id must be alphanumeric, '_' or '-'"),
        ));
    }
    Ok(())
}

pub fn validate_workspace_id(id: &str) -> Result<(), (StatusCode, String)> {
    validate_tenant_id("workspace", id)
}

pub fn validate_org_id(id: &str) -> Result<(), (StatusCode, String)> {
    validate_tenant_id("org", id)
}
