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
cargo test -p hyprduck-engine-types -p hyprduck-engine-client -p hyprduck-engine -p hyprduck-cli
cargo check -p hyprduck-cli
```

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
