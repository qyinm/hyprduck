//! Postgres control-plane store.

use crate::blob::BlobError;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrgRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: String,
    pub org_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipRole {
    Owner,
    Member,
}

impl MembershipRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }

    fn parse(value: String) -> StoreResult<Self> {
        match value.as_str() {
            "owner" => Ok(Self::Owner),
            "member" => Ok(Self::Member),
            _ => Err(StoreError::Integrity(format!(
                "unsupported membership role: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipRow {
    pub org_id: String,
    pub user_id: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserOrgRow {
    pub org_id: String,
    pub name: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserWorkspaceRow {
    pub workspace_id: String,
    pub org_id: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgMemberRow {
    pub user_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenRow {
    pub id: String,
    pub workspace_id: String,
    pub label: Option<String>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedApiToken {
    pub token: ApiTokenRow,
    pub raw_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified: bool,
}

#[derive(Debug, Clone)]
pub struct OidcIdentityProfile {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified: bool,
}

#[derive(Debug, Clone)]
pub struct OidcLoginStateRow {
    pub nonce: String,
    pub pkce_verifier: String,
}

#[derive(Debug)]
pub enum StoreError {
    NotFound { entity: &'static str, id: String },
    Conflict(String),
    Integrity(String),
    Internal(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} not found: {id}"),
            Self::Conflict(message) | Self::Integrity(message) | Self::Internal(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<BlobError> for StoreError {
    fn from(error: BlobError) -> Self {
        match error {
            BlobError::NotFound { key } => Self::NotFound {
                entity: "blob",
                id: key,
            },
            BlobError::Integrity { expected, actual } => Self::Integrity(format!(
                "blob integrity mismatch: expected {expected}, got {actual}"
            )),
            BlobError::InvalidKey(message) => {
                Self::Integrity(format!("invalid blob key: {message}"))
            }
            BlobError::Io(message) => Self::Internal(format!("blob io: {message}")),
        }
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_org(&self, id: &str, name: &str) -> StoreResult<OrgRow> {
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(name)
            .bind(unix_now())
            .execute(&self.pool)
            .await
            .map_err(|error| map_pg_write(error, &format!("org {id}")))?;
        Ok(OrgRow {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }

    pub async fn get_org(&self, id: &str) -> StoreResult<Option<OrgRow>> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM control.orgs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(id, name)| OrgRow { id, name }))
    }

    pub async fn require_org(&self, id: &str) -> StoreResult<OrgRow> {
        self.get_org(id).await?.ok_or_else(|| StoreError::NotFound {
            entity: "org",
            id: id.to_owned(),
        })
    }

    pub async fn list_orgs(&self) -> StoreResult<Vec<OrgRow>> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM control.orgs ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(id, name)| OrgRow { id, name })
                .collect()
        })
        .map_err(pg_err)
    }

    pub async fn workspace_count_for_org(&self, org_id: &str) -> StoreResult<usize> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM control.workspaces WHERE org_id = $1",
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(count as usize)
    }

    pub async fn delete_org(&self, org_id: &str) -> StoreResult<()> {
        self.require_org(org_id).await?;
        let count = self.workspace_count_for_org(org_id).await?;
        if count > 0 {
            return Err(StoreError::Conflict(format!(
                "org has workspaces: {org_id} ({count})"
            )));
        }
        sqlx::query("DELETE FROM control.orgs WHERE id = $1")
            .bind(org_id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    pub async fn create_workspace(&self, org_id: &str, id: &str) -> StoreResult<WorkspaceRow> {
        self.require_org(org_id).await?;
        sqlx::query("INSERT INTO control.workspaces (id, org_id, created_at) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org_id)
            .bind(unix_now())
            .execute(&self.pool)
            .await
            .map_err(|error| map_pg_write(error, &format!("workspace {id}")))?;
        Ok(WorkspaceRow {
            id: id.to_owned(),
            org_id: org_id.to_owned(),
        })
    }

    pub async fn get_workspace(&self, id: &str) -> StoreResult<Option<WorkspaceRow>> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT id, org_id FROM control.workspaces WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(row.map(|(id, org_id)| WorkspaceRow { id, org_id }))
    }

    pub async fn require_workspace(&self, id: &str) -> StoreResult<WorkspaceRow> {
        self.get_workspace(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "workspace",
                id: id.to_owned(),
            })
    }

    pub async fn list_workspaces(&self, org_id: &str) -> StoreResult<Vec<WorkspaceRow>> {
        self.require_org(org_id).await?;
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, org_id FROM control.workspaces WHERE org_id = $1 ORDER BY created_at, id",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(id, org_id)| WorkspaceRow { id, org_id })
                .collect()
        })
        .map_err(pg_err)
    }

    pub async fn get_membership_for_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> StoreResult<Option<MembershipRow>> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT m.org_id, m.user_id, m.role
             FROM control.memberships m
             JOIN control.workspaces w ON w.org_id = m.org_id
             WHERE m.user_id = $1 AND w.id = $2",
        )
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        row.map(|(org_id, user_id, role)| {
            Ok(MembershipRow {
                org_id,
                user_id,
                role: MembershipRole::parse(role)?,
            })
        })
        .transpose()
    }

    pub async fn get_membership_for_org(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> StoreResult<Option<MembershipRow>> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT org_id, user_id, role
             FROM control.memberships
             WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        row.map(|(org_id, user_id, role)| {
            Ok(MembershipRow {
                org_id,
                user_id,
                role: MembershipRole::parse(role)?,
            })
        })
        .transpose()
    }

    pub async fn create_membership(
        &self,
        org_id: &str,
        user_id: &str,
        role: MembershipRole,
    ) -> StoreResult<MembershipRow> {
        self.require_org(org_id).await?;
        let membership_id = format!("mem_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO control.memberships (id, org_id, user_id, role, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(membership_id)
        .bind(org_id)
        .bind(user_id)
        .bind(role.as_str())
        .bind(unix_now())
        .execute(&self.pool)
        .await
        .map_err(|error| map_pg_write(error, &format!("membership for user {user_id}")))?;
        Ok(MembershipRow {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            role,
        })
    }

    pub async fn list_user_orgs(&self, user_id: &str) -> StoreResult<Vec<UserOrgRow>> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT o.id, o.name, m.role
             FROM control.memberships m
             JOIN control.orgs o ON o.id = m.org_id
             WHERE m.user_id = $1
             ORDER BY o.created_at, o.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        rows.into_iter()
            .map(|(org_id, name, role)| {
                Ok(UserOrgRow {
                    org_id,
                    name,
                    role: MembershipRole::parse(role)?,
                })
            })
            .collect()
    }

    pub async fn list_user_workspaces(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> StoreResult<Vec<UserWorkspaceRow>> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT w.id, w.org_id, m.role
             FROM control.workspaces w
             JOIN control.memberships m ON m.org_id = w.org_id
             WHERE m.user_id = $1 AND w.org_id = $2
             ORDER BY w.created_at, w.id",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        rows.into_iter()
            .map(|(workspace_id, org_id, role)| {
                Ok(UserWorkspaceRow {
                    workspace_id,
                    org_id,
                    role: MembershipRole::parse(role)?,
                })
            })
            .collect()
    }

    pub async fn list_org_members(&self, org_id: &str) -> StoreResult<Vec<OrgMemberRow>> {
        let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, String)>(
            "SELECT u.id, u.email, u.display_name, m.role
             FROM control.memberships m
             JOIN control.users u ON u.id = m.user_id
             WHERE m.org_id = $1
             ORDER BY m.created_at, u.id",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        rows.into_iter()
            .map(|(user_id, email, display_name, role)| {
                Ok(OrgMemberRow {
                    user_id,
                    email,
                    display_name,
                    role: MembershipRole::parse(role)?,
                })
            })
            .collect()
    }

    pub async fn create_oidc_login_state(
        &self,
        state_hash: &str,
        nonce: &str,
        pkce_verifier: &str,
        browser_binding_hash: &str,
        expires_at: i64,
    ) -> StoreResult<()> {
        let now = unix_now();
        sqlx::query(
            "INSERT INTO control.oidc_login_states (state_hash, nonce, pkce_verifier, browser_binding_hash, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(state_hash)
        .bind(nonce)
        .bind(pkce_verifier)
        .bind(browser_binding_hash)
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    pub async fn consume_oidc_login_state(
        &self,
        state_hash: &str,
        browser_binding_hash: &str,
        now: i64,
    ) -> StoreResult<Option<OidcLoginStateRow>> {
        sqlx::query_as::<_, (String, String)>(
            "UPDATE control.oidc_login_states SET consumed_at = $3 WHERE state_hash = $1 AND browser_binding_hash = $2 AND consumed_at IS NULL AND expires_at > $3 RETURNING nonce, pkce_verifier",
        )
        .bind(state_hash)
        .bind(browser_binding_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|(nonce, pkce_verifier)| OidcLoginStateRow {
                nonce,
                pkce_verifier,
            })
        })
        .map_err(pg_err)
    }

    pub async fn upsert_oidc_identity(
        &self,
        profile: &OidcIdentityProfile,
    ) -> StoreResult<UserRow> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let existing = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, bool)>(
            "SELECT u.id, u.email, u.display_name, u.avatar_url, u.email_verified FROM control.user_identities i JOIN control.users u ON u.id = i.user_id WHERE i.issuer = $1 AND i.subject = $2",
        )
        .bind(&profile.issuer)
        .bind(&profile.subject)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_err)?;

        if let Some((id, _, _, _, _)) = existing {
            sqlx::query(
                "UPDATE control.users u SET email = $2, display_name = $3, avatar_url = $4, email_verified = $5, updated_at = $6 FROM control.user_identities i WHERE u.id = i.user_id AND i.issuer = $1 AND i.subject = $7",
            )
            .bind(&profile.issuer)
            .bind(&profile.email)
            .bind(&profile.display_name)
            .bind(&profile.avatar_url)
            .bind(profile.email_verified)
            .bind(unix_now())
            .bind(&profile.subject)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            sqlx::query(
                "UPDATE control.user_identities SET last_login_at = $3 WHERE issuer = $1 AND subject = $2",
            )
            .bind(&profile.issuer)
            .bind(&profile.subject)
            .bind(unix_now())
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            tx.commit().await.map_err(pg_err)?;
            return Ok(UserRow {
                id,
                email: profile.email.clone(),
                display_name: profile.display_name.clone(),
                avatar_url: profile.avatar_url.clone(),
                email_verified: profile.email_verified,
            });
        }

        let now = unix_now();
        let user_id = format!("usr_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO control.users (id, email, display_name, avatar_url, email_verified, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $6)",
        )
        .bind(&user_id)
        .bind(&profile.email)
        .bind(&profile.display_name)
        .bind(&profile.avatar_url)
        .bind(profile.email_verified)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;

        let identity_id = format!("ident_{}", Uuid::now_v7().simple());
        let inserted_user_id = sqlx::query_scalar::<_, String>(
            "INSERT INTO control.user_identities (id, user_id, issuer, subject, created_at, last_login_at) VALUES ($1, $2, $3, $4, $5, $5) ON CONFLICT (issuer, subject) DO NOTHING RETURNING user_id",
        )
        .bind(identity_id)
        .bind(&user_id)
        .bind(&profile.issuer)
        .bind(&profile.subject)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_err)?;

        if inserted_user_id.is_none() {
            sqlx::query("DELETE FROM control.users WHERE id = $1")
                .bind(&user_id)
                .execute(&mut *tx)
                .await
                .map_err(pg_err)?;
            let existing_user_id = sqlx::query_scalar::<_, String>(
                "SELECT user_id FROM control.user_identities WHERE issuer = $1 AND subject = $2",
            )
            .bind(&profile.issuer)
            .bind(&profile.subject)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| {
                StoreError::Conflict("OIDC identity race could not be resolved".into())
            })?;
            sqlx::query(
                "UPDATE control.users SET email = $2, display_name = $3, avatar_url = $4, email_verified = $5, updated_at = $6 WHERE id = $1",
            )
            .bind(&existing_user_id)
            .bind(&profile.email)
            .bind(&profile.display_name)
            .bind(&profile.avatar_url)
            .bind(profile.email_verified)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            sqlx::query("UPDATE control.user_identities SET last_login_at = $2 WHERE id = (SELECT id FROM control.user_identities WHERE issuer = $1 AND subject = $3)")
                .bind(&profile.issuer)
                .bind(now)
                .bind(&profile.subject)
                .execute(&mut *tx)
                .await
                .map_err(pg_err)?;
            tx.commit().await.map_err(pg_err)?;
            return Ok(UserRow {
                id: existing_user_id,
                email: profile.email.clone(),
                display_name: profile.display_name.clone(),
                avatar_url: profile.avatar_url.clone(),
                email_verified: profile.email_verified,
            });
        }

        let personal_org_id = format!("org_personal_{}", Uuid::now_v7().simple());
        let personal_org_name = profile
            .display_name
            .as_deref()
            .or(profile.email.as_deref())
            .map(|name| format!("{name}'s Personal Organization"))
            .unwrap_or_else(|| "Personal Organization".to_owned());
        sqlx::query("INSERT INTO control.orgs (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(&personal_org_id)
            .bind(personal_org_name)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
        let membership_id = format!("mem_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO control.memberships (id, org_id, user_id, role, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(membership_id)
        .bind(&personal_org_id)
        .bind(&user_id)
        .bind(MembershipRole::Owner.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;

        tx.commit().await.map_err(pg_err)?;
        Ok(UserRow {
            id: user_id,
            email: profile.email.clone(),
            display_name: profile.display_name.clone(),
            avatar_url: profile.avatar_url.clone(),
            email_verified: profile.email_verified,
        })
    }

    pub async fn create_session(
        &self,
        user_id: &str,
        raw_token: &str,
        expires_at: i64,
    ) -> StoreResult<()> {
        let now = unix_now();
        let session_id = format!("ses_{}", Uuid::now_v7().simple());
        sqlx::query(
            "INSERT INTO control.sessions (id, user_id, token_hash, expires_at, created_at, last_seen_at) VALUES ($1, $2, $3, $4, $5, $5)",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(hash_token(raw_token))
        .bind(expires_at)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    pub async fn resolve_session(&self, raw_token: &str, now: i64) -> StoreResult<Option<UserRow>> {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, bool)>(
            "SELECT u.id, u.email, u.display_name, u.avatar_url, u.email_verified FROM control.sessions s JOIN control.users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > $2",
        )
        .bind(hash_token(raw_token))
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        if let Some((id, email, display_name, avatar_url, email_verified)) = row {
            if let Err(error) = sqlx::query(
                "UPDATE control.sessions SET last_seen_at = $2 WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > $2",
            )
            .bind(hash_token(raw_token))
            .bind(now)
            .execute(&self.pool)
            .await
            {
                tracing::warn!(%error, "failed updating session last_seen_at");
            }
            Ok(Some(UserRow {
                id,
                email,
                display_name,
                avatar_url,
                email_verified,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn revoke_session(&self, raw_token: &str, now: i64) -> StoreResult<bool> {
        let result = sqlx::query(
            "UPDATE control.sessions SET revoked_at = $2, last_seen_at = $2 WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > $2",
        )
        .bind(hash_token(raw_token))
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mint_token_with_metadata(
        &self,
        workspace_id: &str,
        label: Option<&str>,
    ) -> StoreResult<MintedApiToken> {
        self.require_workspace(workspace_id).await?;
        let raw = format!("etyma_{}", Uuid::now_v7().simple());
        let token_id = format!("tok_{}", Uuid::now_v7().simple());
        let created_at = unix_now();
        sqlx::query(
            "INSERT INTO control.api_tokens (id, token_hash, workspace_id, label, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&token_id)
        .bind(hash_token(&raw))
        .bind(workspace_id)
        .bind(label)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(MintedApiToken {
            token: ApiTokenRow {
                id: token_id,
                workspace_id: workspace_id.to_owned(),
                label: label.map(str::to_owned),
                created_at,
                revoked_at: None,
            },
            raw_token: raw,
        })
    }

    pub async fn mint_token_for_user(
        &self,
        user_id: &str,
        workspace_id: &str,
        label: Option<&str>,
    ) -> StoreResult<Option<MintedApiToken>> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let role = sqlx::query_scalar::<_, String>(
            "SELECT m.role
             FROM control.memberships m
             JOIN control.workspaces w ON w.org_id = m.org_id
             WHERE m.user_id = $1 AND w.id = $2
             FOR UPDATE OF m",
        )
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_err)?;
        if role.as_deref() != Some(MembershipRole::Owner.as_str()) {
            return Ok(None);
        }

        let raw = format!("etyma_{}", Uuid::now_v7().simple());
        let token_id = format!("tok_{}", Uuid::now_v7().simple());
        let created_at = unix_now();
        sqlx::query(
            "INSERT INTO control.api_tokens (id, token_hash, workspace_id, label, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&token_id)
        .bind(hash_token(&raw))
        .bind(workspace_id)
        .bind(label)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;
        tx.commit().await.map_err(pg_err)?;
        Ok(Some(MintedApiToken {
            token: ApiTokenRow {
                id: token_id,
                workspace_id: workspace_id.to_owned(),
                label: label.map(str::to_owned),
                created_at,
                revoked_at: None,
            },
            raw_token: raw,
        }))
    }

    pub async fn mint_token(&self, workspace_id: &str, label: Option<&str>) -> StoreResult<String> {
        Ok(self
            .mint_token_with_metadata(workspace_id, label)
            .await?
            .raw_token)
    }

    pub async fn list_tokens(&self, workspace_id: &str) -> StoreResult<Vec<ApiTokenRow>> {
        self.require_workspace(workspace_id).await?;
        sqlx::query_as::<_, (String, String, Option<String>, i64, Option<i64>)>(
            "SELECT id, workspace_id, label, created_at, revoked_at
             FROM control.api_tokens
             WHERE workspace_id = $1
             ORDER BY created_at, id",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(
                    |(id, workspace_id, label, created_at, revoked_at)| ApiTokenRow {
                        id,
                        workspace_id,
                        label,
                        created_at,
                        revoked_at,
                    },
                )
                .collect()
        })
        .map_err(pg_err)
    }

    pub async fn list_tokens_for_user(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> StoreResult<Option<Vec<ApiTokenRow>>> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let role = sqlx::query_scalar::<_, String>(
            "SELECT m.role
             FROM control.memberships m
             JOIN control.workspaces w ON w.org_id = m.org_id
             WHERE m.user_id = $1 AND w.id = $2
             FOR UPDATE OF m",
        )
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_err)?;
        if role.as_deref() != Some(MembershipRole::Owner.as_str()) {
            return Ok(None);
        }
        let rows = sqlx::query_as::<_, (String, String, Option<String>, i64, Option<i64>)>(
            "SELECT id, workspace_id, label, created_at, revoked_at
             FROM control.api_tokens
             WHERE workspace_id = $1
             ORDER BY created_at, id",
        )
        .bind(workspace_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(pg_err)?;
        tx.commit().await.map_err(pg_err)?;
        Ok(Some(
            rows.into_iter()
                .map(
                    |(id, workspace_id, label, created_at, revoked_at)| ApiTokenRow {
                        id,
                        workspace_id,
                        label,
                        created_at,
                        revoked_at,
                    },
                )
                .collect(),
        ))
    }

    pub async fn revoke_token(
        &self,
        workspace_id: &str,
        token_id: &str,
        now: i64,
    ) -> StoreResult<bool> {
        self.require_workspace(workspace_id).await?;
        let result = sqlx::query(
            "UPDATE control.api_tokens
             SET revoked_at = $3
             WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL",
        )
        .bind(token_id)
        .bind(workspace_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn revoke_token_for_user(
        &self,
        user_id: &str,
        workspace_id: &str,
        token_id: &str,
        now: i64,
    ) -> StoreResult<Option<bool>> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let role = sqlx::query_scalar::<_, String>(
            "SELECT m.role
             FROM control.memberships m
             JOIN control.workspaces w ON w.org_id = m.org_id
             WHERE m.user_id = $1 AND w.id = $2
             FOR UPDATE OF m",
        )
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_err)?;
        if role.as_deref() != Some(MembershipRole::Owner.as_str()) {
            return Ok(None);
        }
        let result = sqlx::query(
            "UPDATE control.api_tokens
             SET revoked_at = $3
             WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL",
        )
        .bind(token_id)
        .bind(workspace_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;
        tx.commit().await.map_err(pg_err)?;
        Ok(Some(result.rows_affected() == 1))
    }

    pub async fn resolve_token(&self, raw_token: &str) -> StoreResult<Option<String>> {
        sqlx::query_scalar::<_, String>(
            "SELECT workspace_id FROM control.api_tokens WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(hash_token(raw_token))
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)
    }
}

pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn oidc_identity_and_session_round_trip_is_stable_and_revocable() {
        let url = std::env::var("ETYMA_DATABASE_URL")
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests");
        let pool = crate::db::connect_and_migrate(&url).await.expect("migrate");
        let store = Store::new(pool);
        let suffix = Uuid::now_v7().simple().to_string();

        let profile = OidcIdentityProfile {
            issuer: "https://accounts.google.com".into(),
            subject: format!("subject-{suffix}"),
            email: Some(format!("user-{suffix}@example.com")),
            display_name: Some("Example User".into()),
            avatar_url: Some("https://example.com/avatar.png".into()),
            email_verified: true,
        };
        let first = store.upsert_oidc_identity(&profile).await.expect("user");
        let second = store
            .upsert_oidc_identity(&profile)
            .await
            .expect("same user");
        assert_eq!(first, second);

        let raw_token = format!("session-{suffix}");
        store
            .create_session(&first.id, &raw_token, 2_000_000_000)
            .await
            .expect("session");
        assert_eq!(
            store
                .resolve_session(&raw_token, 1_900_000_000)
                .await
                .expect("resolve")
                .expect("live session"),
            first
        );
        assert!(
            store
                .resolve_session(&raw_token, 2_000_000_001)
                .await
                .expect("expired lookup")
                .is_none()
        );
        assert!(
            store
                .revoke_session(&raw_token, 1_900_000_001)
                .await
                .expect("revoke")
        );
        assert!(
            store
                .resolve_session(&raw_token, 1_900_000_002)
                .await
                .expect("revoked lookup")
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn first_oidc_identity_gets_personal_org_owner_membership() {
        let url = std::env::var("ETYMA_DATABASE_URL")
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests");
        let pool = crate::db::connect_and_migrate(&url).await.expect("migrate");
        let store = Store::new(pool);
        let suffix = Uuid::now_v7().simple().to_string();
        let profile = OidcIdentityProfile {
            issuer: "https://accounts.example.com".into(),
            subject: format!("personal-org-{suffix}"),
            email: Some(format!("personal-{suffix}@example.com")),
            display_name: Some("Personal User".into()),
            avatar_url: None,
            email_verified: true,
        };

        let user = store.upsert_oidc_identity(&profile).await.expect("user");
        let orgs = store.list_user_orgs(&user.id).await.expect("orgs");
        assert_eq!(orgs.len(), 1);
        assert_eq!(orgs[0].role, MembershipRole::Owner);
        assert!(orgs[0].org_id.starts_with("org_personal_"));

        let repeated = store.upsert_oidc_identity(&profile).await.expect("repeat");
        assert_eq!(repeated.id, user.id);
        assert_eq!(store.list_user_orgs(&user.id).await.expect("orgs").len(), 1);
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn concurrent_first_oidc_logins_converge_on_one_personal_org() {
        let url = std::env::var("ETYMA_DATABASE_URL")
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests");
        let pool = crate::db::connect_and_migrate(&url).await.expect("migrate");
        let store = Store::new(pool);
        let suffix = Uuid::now_v7().simple().to_string();
        let profile = OidcIdentityProfile {
            issuer: "https://accounts.example.com".into(),
            subject: format!("concurrent-{suffix}"),
            email: Some(format!("concurrent-{suffix}@example.com")),
            display_name: Some("Concurrent User".into()),
            avatar_url: None,
            email_verified: true,
        };

        let (first, second) = tokio::join!(
            store.upsert_oidc_identity(&profile),
            store.upsert_oidc_identity(&profile)
        );
        let first = first.expect("first concurrent user");
        let second = second.expect("second concurrent user");
        assert_eq!(first.id, second.id);
        let orgs = store.list_user_orgs(&first.id).await.expect("orgs");
        assert_eq!(orgs.len(), 1);
        assert_eq!(orgs[0].role, MembershipRole::Owner);
    }

    #[tokio::test]
    #[ignore = "requires ETYMA_DATABASE_URL"]
    async fn token_revocation_is_bound_to_the_workspace() {
        let url = std::env::var("ETYMA_DATABASE_URL")
            .expect("ETYMA_DATABASE_URL required for ignored Postgres tests");
        let pool = crate::db::connect_and_migrate(&url).await.expect("migrate");
        let store = Store::new(pool);
        let suffix = Uuid::now_v7().simple().to_string();
        let org_id = format!("revoke_org_{suffix}");
        let first_workspace = format!("revoke_ws_a_{suffix}");
        let second_workspace = format!("revoke_ws_b_{suffix}");
        store.create_org(&org_id, "Revoke Test Org").await.expect("org");
        store
            .create_workspace(&org_id, &first_workspace)
            .await
            .expect("first workspace");
        store
            .create_workspace(&org_id, &second_workspace)
            .await
            .expect("second workspace");
        let minted = store
            .mint_token_with_metadata(&first_workspace, Some("revoke"))
            .await
            .expect("token");
        assert!(!store
            .revoke_token(&second_workspace, &minted.token.id, 1_900_000_000)
            .await
            .expect("wrong workspace revoke"));
        assert_eq!(
            store
                .resolve_token(&minted.raw_token)
                .await
                .expect("active token")
                .as_deref(),
            Some(first_workspace.as_str())
        );
        assert!(store
            .revoke_token(&first_workspace, &minted.token.id, 1_900_000_001)
            .await
            .expect("correct workspace revoke"));
        assert!(store
            .resolve_token(&minted.raw_token)
            .await
            .expect("revoked token")
            .is_none());
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn pg_err(error: sqlx::Error) -> StoreError {
    tracing::warn!(%error, "control database operation failed");
    StoreError::Internal("control database operation failed".into())
}

fn map_pg_write(error: sqlx::Error, entity: &str) -> StoreError {
    if let Some(database_error) = error.as_database_error() {
        return match database_error.code().as_deref() {
            Some("23503") => StoreError::NotFound {
                entity: "parent",
                id: entity.to_owned(),
            },
            Some("23505") => StoreError::Conflict(format!("{entity} already exists")),
            _ => pg_err(error),
        };
    }
    pg_err(error)
}
