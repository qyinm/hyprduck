use crate::store::Store;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub data_dir: std::path::PathBuf,
    pub spike_admin_token: Option<String>,
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
        let header = parts
            .headers
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
        let workspace_id = state
            .store
            .resolve_token(token)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "invalid token".into()))?;
        Ok(Self { workspace_id })
    }
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
