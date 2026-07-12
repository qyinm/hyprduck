-- S4: organization roles and revocable workspace token metadata.

DO $$
DECLARE
  invalid_roles TEXT;
BEGIN
  SELECT string_agg(DISTINCT role, ', ' ORDER BY role)
  INTO invalid_roles
  FROM control.memberships
  WHERE role NOT IN ('owner', 'member');
  IF invalid_roles IS NOT NULL THEN
    RAISE EXCEPTION
      'S4 migration stopped: unsupported control.memberships.role values: %; repair them before retrying',
      invalid_roles;
  END IF;
END
$$;

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

-- S3 users may exist before memberships were introduced. Give every user
-- without a deterministic personal membership a personal organization.
INSERT INTO control.orgs (id, name, created_at)
SELECT
  'org_personal_' || md5('personal-org:' || u.id),
  COALESCE(NULLIF(BTRIM(u.display_name), ''), NULLIF(BTRIM(u.email), ''), 'Personal')
    || ' Personal Organization',
  u.created_at
FROM control.users u
WHERE NOT EXISTS (
  SELECT 1
  FROM control.memberships m
  WHERE m.id = 'mem_personal_' || md5('personal-membership:' || u.id)
)
ON CONFLICT (id) DO NOTHING;

INSERT INTO control.memberships (id, org_id, user_id, role, created_at)
SELECT
  'mem_personal_' || md5('personal-membership:' || u.id),
  'org_personal_' || md5('personal-org:' || u.id),
  u.id,
  'owner',
  u.created_at
FROM control.users u
WHERE NOT EXISTS (
  SELECT 1
  FROM control.memberships m
  WHERE m.id = 'mem_personal_' || md5('personal-membership:' || u.id)
)
ON CONFLICT (id) DO NOTHING;

ALTER TABLE control.api_tokens
  ALTER COLUMN id SET NOT NULL;

ALTER TABLE control.api_tokens
  ADD CONSTRAINT control_api_tokens_id_unique UNIQUE (id);

CREATE INDEX IF NOT EXISTS idx_control_api_tokens_workspace_active
  ON control.api_tokens(workspace_id)
  WHERE revoked_at IS NULL;
