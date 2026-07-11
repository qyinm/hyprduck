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
# Local Postgres (control plane hybrid: S-PG2)
docker compose up -d
export ETYMA_DATABASE_URL=postgres://etyma:etyma@127.0.0.1:5432/etyma
cargo test -p etyma-server -- --include-ignored
cargo run -p etyma-server
```

Without `ETYMA_DATABASE_URL`, `cargo test -p etyma-server` still runs (SQLite spike path; PG tests are `#[ignore]`d). With a DSN, control (orgs/workspaces/tokens) is Postgres; sources/evidence stay on local SQLite until S-PG3. See `crates/etyma-server/README.md` and `docs/storage-planes.md`.

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
