-- S3: durable human identity, OIDC login transactions, and server sessions.

ALTER TABLE control.users
  ADD COLUMN IF NOT EXISTS email TEXT,
  ADD COLUMN IF NOT EXISTS display_name TEXT,
  ADD COLUMN IF NOT EXISTS avatar_url TEXT,
  ADD COLUMN IF NOT EXISTS email_verified BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN IF NOT EXISTS updated_at BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS control.user_identities (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES control.users(id) ON DELETE CASCADE,
  issuer TEXT NOT NULL,
  subject TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  last_login_at BIGINT NOT NULL,
  UNIQUE (issuer, subject)
);

CREATE TABLE IF NOT EXISTS control.oidc_login_states (
  state_hash TEXT PRIMARY KEY,
  nonce TEXT NOT NULL,
  pkce_verifier TEXT NOT NULL,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  consumed_at BIGINT
);

CREATE TABLE IF NOT EXISTS control.sessions (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES control.users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL,
  last_seen_at BIGINT NOT NULL,
  revoked_at BIGINT
);

CREATE INDEX IF NOT EXISTS idx_control_user_identities_user_id
  ON control.user_identities(user_id);

CREATE INDEX IF NOT EXISTS idx_control_oidc_login_states_expiry
  ON control.oidc_login_states(expires_at);

CREATE INDEX IF NOT EXISTS idx_control_sessions_user_id
  ON control.sessions(user_id);

CREATE INDEX IF NOT EXISTS idx_control_sessions_active_token
  ON control.sessions(token_hash, expires_at)
  WHERE revoked_at IS NULL;
