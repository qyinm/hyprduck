-- Control plane product tables (S-PG2).
-- knowledge/graph product tables arrive in S-PG3/4.

CREATE TABLE IF NOT EXISTS control.orgs (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS control.workspaces (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES control.orgs(id),
  created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS control.api_tokens (
  token_hash TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES control.workspaces(id) ON DELETE CASCADE,
  label TEXT,
  created_at BIGINT NOT NULL
);

-- Stubs for S3/S4 (schema must not block later identity work).
CREATE TABLE IF NOT EXISTS control.users (
  id TEXT PRIMARY KEY,
  created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS control.memberships (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES control.orgs(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES control.users(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'member',
  created_at BIGINT NOT NULL,
  UNIQUE (org_id, user_id)
);

-- Stub audit hook for S9.
CREATE TABLE IF NOT EXISTS control.audit_events (
  id BIGSERIAL PRIMARY KEY,
  org_id TEXT,
  workspace_id TEXT,
  actor TEXT,
  action TEXT NOT NULL,
  meta JSONB,
  created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_control_workspaces_org_id
  ON control.workspaces(org_id);

CREATE INDEX IF NOT EXISTS idx_control_api_tokens_workspace_id
  ON control.api_tokens(workspace_id);

CREATE INDEX IF NOT EXISTS idx_control_memberships_org_id
  ON control.memberships(org_id);

CREATE INDEX IF NOT EXISTS idx_control_memberships_user_id
  ON control.memberships(user_id);
