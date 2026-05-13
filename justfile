set shell := ["bash", "-cu"]

desktop_dir := "apps/desktop"
site_dir := "apps/site"

default:
  @just --list

paths:
  @echo "desktop_dir={{desktop_dir}}"
  @echo "site_dir={{site_dir}}"

desktop-build:
  bun run --cwd {{desktop_dir}} build

desktop-dev:
  bun run --cwd {{desktop_dir}} dev

desktop-check:
  bun run --cwd {{desktop_dir}} frontend:typecheck

desktop-test:
  just desktop-check

frontend-typecheck:
  bun run --cwd {{desktop_dir}} frontend:typecheck

frontend-build:
  bun run --cwd {{desktop_dir}} frontend:build

desktop-web-preview:
  VITE_PLATFORM=web bun run --cwd {{desktop_dir}} frontend:build

core-test:
  cargo test -p hyprduck-engine-types -p hyprduck-engine-client -p hyprduck-engine -p hyprduck-cli

cli-build:
  cargo build -p hyprduck-cli --release

cli-dev:
  cargo run -p hyprduck-cli

cli-check:
  cargo check -p hyprduck-cli

cli-test:
  cargo test -p hyprduck-cli

site-stage:
  bun run --cwd {{site_dir}} build

site-dev:
  bun run --cwd {{site_dir}} dev

site-check:
  bun run --cwd {{site_dir}} astro check

clean:
  rm -rf {{site_dir}}/dist build {{desktop_dir}}/dist
