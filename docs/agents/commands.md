# Agent Command Reference

Operational commands live here so root instruction files stay concise. Use Bun for JavaScript work in this repo. Do not add new pnpm commands.

## Desktop

```bash
bun run --cwd apps/desktop dev
bun run --cwd apps/desktop frontend:typecheck
bun run --cwd apps/desktop frontend:build
bun run --cwd apps/desktop build
bun run --cwd apps/desktop test:ia
```

## Rust

```bash
cargo test -p etyma-engine-types -p etyma-engine-client -p etyma-engine -p etyma-cli
cargo check -p etyma-cli
```

## etyma-server

```bash
# Local Postgres (mandatory SaaS storage)
docker compose up -d
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
# Optional generic OIDC login; client id/secret come from the deployment secret manager.
export ETYMA_OIDC_ISSUER_URL=https://accounts.google.com
export ETYMA_OIDC_CLIENT_ID="$OIDC_CLIENT_ID"
export ETYMA_OIDC_CLIENT_SECRET="$OIDC_CLIENT_SECRET"
export ETYMA_OIDC_REDIRECT_URL=http://127.0.0.1:8787/v1/auth/callback
export ETYMA_AUTH_COOKIE_SECURE=false
cargo test -p etyma-server -- --include-ignored
cargo run -p etyma-server
```

`etyma-server` refuses to start without `ETYMA_DATABASE_URL`. Control,
source/evidence metadata, import jobs, and the cloud graph projection live in
Postgres; original bytes live in the configured blob backend. GraphQLite is
local/engine only. Ignored integration tests require the DSN.

OIDC configuration is optional for the spike. When enabled, all four
`ETYMA_OIDC_*` variables are required together and the callback must be
registered as `/v1/auth/callback` at the configured public origin. The login
cookie flow is documented in `crates/etyma-server/README.md`; human sessions
identify `/v1/me` and authorize organization discovery plus owner-only token
management. Agent REST and MCP data access still uses workspace API bearer
tokens. Newly created users receive a personal organization and owner
membership during the identity transaction.

## Site

```bash
bun run --cwd apps/site astro check
bun run --cwd apps/site build
```

## Just Aliases

```bash
just desktop-build
just desktop-check
just core-test
just site-check
just site-stage
```
