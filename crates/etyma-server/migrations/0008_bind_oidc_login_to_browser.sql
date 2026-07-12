-- Bind each OIDC login transaction to the browser that initiated it.

ALTER TABLE control.oidc_login_states
  ADD COLUMN IF NOT EXISTS browser_binding_hash TEXT;

-- Existing rows predate browser binding and must not remain redeemable.
DELETE FROM control.oidc_login_states
  WHERE browser_binding_hash IS NULL;

ALTER TABLE control.oidc_login_states
  ALTER COLUMN browser_binding_hash SET NOT NULL;
