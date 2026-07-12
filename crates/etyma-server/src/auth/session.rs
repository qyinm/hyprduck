use crate::auth::AppState;
use crate::store::UserRow;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;

pub const SESSION_COOKIE_NAME: &str = "etyma_session";

pub fn build_session_cookie(raw_token: &str, max_age_seconds: i64, secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={raw_token}; Max-Age={max_age_seconds}; Path=/; HttpOnly; SameSite=Lax{secure_attribute}"
    )
}

pub fn build_clear_cookie(secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}=; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/; HttpOnly; SameSite=Lax{secure_attribute}"
    )
}

pub fn parse_session_cookie(header: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name.trim() != SESSION_COOKIE_NAME {
            return None;
        }
        let value = value.trim();
        if value.is_empty() || value.contains([';', '\r', '\n']) {
            return None;
        }
        Some(value.to_owned())
    })
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user: UserRow,
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw_token = parts
            .headers
            .get(axum::http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_session_cookie)
            .ok_or((StatusCode::UNAUTHORIZED, "authentication required".into()))?;
        let user = state
            .auth
            .current_user(&raw_token)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "authentication lookup failed".into(),
                )
            })?
            .ok_or((StatusCode::UNAUTHORIZED, "authentication required".into()))?;
        Ok(Self { user })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_is_httponly_lax_and_path_rooted() {
        let header = build_session_cookie("opaque", 3600, true);
        assert!(header.contains("etyma_session=opaque"));
        assert!(header.contains("Max-Age=3600"));
        assert!(header.contains("Path=/"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Lax"));
        assert!(header.contains("Secure"));
    }

    #[test]
    fn clear_cookie_expires_immediately() {
        let header = build_clear_cookie(true);
        assert!(header.contains("etyma_session="));
        assert!(header.contains("Max-Age=0"));
        assert!(header.contains("Expires=Thu, 01 Jan 1970 00:00:00 GMT"));
        assert!(header.contains("HttpOnly"));
    }

    #[test]
    fn cookie_parser_ignores_other_cookie_pairs() {
        assert_eq!(
            parse_session_cookie("theme=dark; etyma_session=opaque; flag=true"),
            Some("opaque".into())
        );
        assert_eq!(parse_session_cookie("theme=dark"), None);
    }
}
