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
cargo test -p etyma-server -- --include-ignored
cargo run -p etyma-server
```

`etyma-server` refuses to start without `ETYMA_DATABASE_URL`. Control,
source/evidence metadata, and import jobs live in Postgres; original bytes live
in the configured blob backend. Ignored integration tests require the DSN.

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
