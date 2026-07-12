use crate::config::{AuthConfig, OidcConfig};
use crate::store::{hash_token, OidcIdentityProfile, Store, StoreError, UserRow};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("OIDC authentication is not configured")]
    Disabled,
    #[error("invalid authentication callback: {0}")]
    InvalidCallback(&'static str),
    #[error("authentication state is invalid or expired")]
    InvalidState,
    #[error("OIDC provider rejected authentication")]
    Provider(String),
    #[error("identity storage failed")]
    Storage(#[from] StoreError),
    #[error("OIDC client configuration failed: {0}")]
    Configuration(String),
}

pub type OidcFuture = Pin<Box<dyn Future<Output = Result<OidcIdentityProfile, AuthError>> + Send>>;

pub trait OidcBackend: Send + Sync {
    fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, AuthError>;

    fn exchange_code(&self, code: String, pkce_verifier: String, nonce: String) -> OidcFuture;
}

#[derive(Clone)]
struct OpenIdConnectBackend {
    client: CoreClient<
        EndpointSet,
        EndpointNotSet,
        EndpointNotSet,
        EndpointNotSet,
        EndpointMaybeSet,
        EndpointMaybeSet,
    >,
    http_client: reqwest::Client,
    issuer: String,
}

impl OpenIdConnectBackend {
    async fn discover(config: &OidcConfig) -> Result<Self, AuthError> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let issuer = IssuerUrl::new(config.issuer_url.clone())
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let provider_metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(|error| AuthError::Configuration(error.to_string()))?;
        let client: CoreClient<
            EndpointSet,
            EndpointNotSet,
            EndpointNotSet,
            EndpointNotSet,
            EndpointMaybeSet,
            EndpointMaybeSet,
        > = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.clone())),
        )
        .set_redirect_uri(
            RedirectUrl::new(config.redirect_url.clone())
                .map_err(|error| AuthError::Configuration(error.to_string()))?,
        );
        Ok(Self {
            client,
            http_client,
            issuer: config.issuer_url.clone(),
        })
    }
}

impl OidcBackend for OpenIdConnectBackend {
    fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, AuthError> {
        let verifier = PkceCodeVerifier::new(pkce_verifier.to_owned());
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&verifier);
        let state = state.to_owned();
        let nonce = nonce.to_owned();
        let (url, _, _) = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state.clone()),
                move || Nonce::new(nonce.clone()),
            )
            .add_scope(Scope::new("openid".into()))
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(url.to_string())
    }

    fn exchange_code(&self, code: String, pkce_verifier: String, nonce: String) -> OidcFuture {
        let backend = self.clone();
        Box::pin(async move {
            let token_response = backend
                .client
                .exchange_code(AuthorizationCode::new(code))
                .map_err(|error| AuthError::Provider(error.to_string()))?
                .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier))
                .request_async(&backend.http_client)
                .await
                .map_err(|error| AuthError::Provider(error.to_string()))?;
            let id_token = token_response
                .id_token()
                .ok_or_else(|| AuthError::Provider("provider did not return an ID token".into()))?;
            let claims = id_token
                .claims(&backend.client.id_token_verifier(), &Nonce::new(nonce))
                .map_err(|error| AuthError::Provider(error.to_string()))?;
            let display_name = claims
                .name()
                .and_then(|localized| localized.get(None))
                .map(|value| value.as_str().to_owned());
            let avatar_url = claims
                .picture()
                .and_then(|localized| localized.get(None))
                .map(|value| value.as_str().to_owned());
            Ok(OidcIdentityProfile {
                issuer: backend.issuer,
                subject: claims.subject().as_str().to_owned(),
                email: claims.email().map(|value| value.as_str().to_owned()),
                display_name,
                avatar_url,
                email_verified: claims.email_verified().unwrap_or(false),
            })
        })
    }
}

pub struct AuthenticatedSession {
    pub user: UserRow,
    pub raw_token: String,
    pub max_age_seconds: i64,
}

#[derive(Clone)]
pub struct AuthService {
    store: Arc<Store>,
    config: AuthConfig,
    backend: Option<Arc<dyn OidcBackend>>,
}

impl AuthService {
    pub async fn initialize(store: Arc<Store>, config: AuthConfig) -> Result<Self, AuthError> {
        let backend = match config.oidc.as_ref() {
            Some(oidc) => {
                Some(Arc::new(OpenIdConnectBackend::discover(oidc).await?) as Arc<dyn OidcBackend>)
            }
            None => None,
        };
        Ok(Self {
            store,
            config,
            backend,
        })
    }

    pub fn disabled(store: Arc<Store>, config: AuthConfig) -> Self {
        Self {
            store,
            config,
            backend: None,
        }
    }

    pub(crate) fn with_backend_for_test(
        store: Arc<Store>,
        config: AuthConfig,
        backend: Arc<dyn OidcBackend>,
    ) -> Self {
        Self {
            store,
            config,
            backend: Some(backend),
        }
    }

    pub async fn begin_login(&self) -> Result<String, AuthError> {
        let backend = self.backend.as_ref().ok_or(AuthError::Disabled)?;
        let state = new_opaque_value();
        let nonce = new_opaque_value();
        let pkce_verifier = new_pkce_verifier();
        let authorization_url = backend.authorization_url(&state, &nonce, &pkce_verifier)?;
        self.store
            .create_oidc_login_state(
                &hash_token(&state),
                &nonce,
                &pkce_verifier,
                now_unix_seconds() + 300,
            )
            .await?;
        Ok(authorization_url)
    }

    pub async fn finish_login(
        &self,
        code: &str,
        state: &str,
    ) -> Result<AuthenticatedSession, AuthError> {
        let backend = self.backend.as_ref().ok_or(AuthError::Disabled)?;
        validate_callback_inputs(code, state)?;
        let login_state = self
            .store
            .consume_oidc_login_state(&hash_token(state), now_unix_seconds())
            .await?
            .ok_or(AuthError::InvalidState)?;
        let profile = backend
            .exchange_code(
                code.to_owned(),
                login_state.pkce_verifier,
                login_state.nonce,
            )
            .await?;
        let user = self.store.upsert_oidc_identity(&profile).await?;
        let raw_token = new_session_token();
        self.store
            .create_session(
                &user.id,
                &raw_token,
                now_unix_seconds() + self.config.session_ttl_seconds,
            )
            .await?;
        Ok(AuthenticatedSession {
            user,
            raw_token,
            max_age_seconds: self.config.session_ttl_seconds,
        })
    }

    pub async fn current_user(&self, raw_token: &str) -> Result<Option<UserRow>, AuthError> {
        if raw_token.is_empty() || self.backend.is_none() {
            return Ok(None);
        }
        Ok(self
            .store
            .resolve_session(raw_token, now_unix_seconds())
            .await?)
    }

    pub async fn logout(&self, raw_token: &str) -> Result<bool, AuthError> {
        if raw_token.is_empty() || self.backend.is_none() {
            return Ok(false);
        }
        Ok(self
            .store
            .revoke_session(raw_token, now_unix_seconds())
            .await?)
    }
}

fn new_opaque_value() -> String {
    Uuid::now_v7().simple().to_string()
}

fn new_pkce_verifier() -> String {
    format!("{}{}", new_opaque_value(), new_opaque_value())
}

fn new_session_token() -> String {
    format!("ses_{}{}", new_opaque_value(), new_opaque_value())
}

fn validate_callback_inputs(code: &str, state: &str) -> Result<(), AuthError> {
    if code.trim().is_empty() {
        return Err(AuthError::InvalidCallback("missing code"));
    }
    if state.trim().is_empty() {
        return Err(AuthError::InvalidCallback("missing state"));
    }
    Ok(())
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_is_long_enough_for_openidconnect() {
        let verifier = new_pkce_verifier();
        assert!((43..=128).contains(&verifier.len()));
        assert!(verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)));
    }

    #[test]
    fn callback_requires_non_empty_code_and_state() {
        assert!(matches!(
            validate_callback_inputs("", "state"),
            Err(AuthError::InvalidCallback(_))
        ));
        assert!(matches!(
            validate_callback_inputs("code", ""),
            Err(AuthError::InvalidCallback(_))
        ));
    }

    #[test]
    fn provider_errors_are_not_returned_as_successful_profiles() {
        let error = AuthError::Provider("token exchange failed".into());
        assert!(matches!(error, AuthError::Provider(_)));
    }
}
