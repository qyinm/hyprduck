set shell := ["bash", "-cu"]

desktop_dir := "apps/desktop"
site_dir := "apps/site"

default:
  @just --list

paths:
  @echo "desktop_dir={{desktop_dir}}"
  @echo "site_dir={{site_dir}}"

desktop-build:
  pnpm --dir {{desktop_dir}} build

desktop-dev:
  pnpm --dir {{desktop_dir}} dev

desktop-check:
  cargo check -p duckdocs-desktop

desktop-test:
  cargo test -p duckdocs-desktop

frontend-typecheck:
  pnpm --dir {{desktop_dir}} frontend:typecheck

frontend-build:
  pnpm --dir {{desktop_dir}} frontend:build

core-test:
  cargo test -p duckdocs-engine-types -p duckdocs-engine-client -p duckdocs-engine -p duckdocs-cli

site-stage:
  pnpm --dir {{site_dir}} build

site-dev:
  pnpm --dir {{site_dir}} dev

site-check:
  pnpm --dir {{site_dir}} exec astro check

clean:
  rm -rf {{site_dir}}/dist build {{desktop_dir}}/dist
