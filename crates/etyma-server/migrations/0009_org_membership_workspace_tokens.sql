-- S4: organization roles and revocable workspace token metadata.

ALTER TABLE control.memberships
  ADD CONSTRAINT control_memberships_role_check
  CHECK (role IN ('owner', 'member'));

ALTER TABLE control.api_tokens
  ADD COLUMN IF NOT EXISTS id TEXT,
  ADD COLUMN IF NOT EXISTS revoked_at BIGINT;

-- Existing S-PG2 tokens predate public token identifiers. Derive a stable,
-- opaque identifier from the already-unique hash and creation timestamp.
UPDATE control.api_tokens
SET id = 'tok_' || md5(token_hash || ':' || created_at::TEXT)
WHERE id IS NULL;

ALTER TABLE control.api_tokens
  ALTER COLUMN id SET NOT NULL;

ALTER TABLE control.api_tokens
  ADD CONSTRAINT control_api_tokens_id_unique UNIQUE (id);

CREATE INDEX IF NOT EXISTS idx_control_api_tokens_workspace_active
  ON control.api_tokens(workspace_id)
  WHERE revoked_at IS NULL;
