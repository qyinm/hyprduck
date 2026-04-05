set shell := ["bash", "-cu"]

desktop_dir := "apps/desktop"
legacy_app_project := "apps/macos/DuckDocs.xcodeproj"
legacy_app_scheme := "DuckDocs"
derived_data := "/tmp/DuckDocsDerivedData"
site_dir := "apps/site"
site_out := "_site"

default:
  @just --list

paths:
  @echo "desktop_dir={{desktop_dir}}"
  @echo "legacy_app_project={{legacy_app_project}}"
  @echo "legacy_app_scheme={{legacy_app_scheme}}"
  @echo "site_dir={{site_dir}}"
  @echo "site_out={{site_out}}"

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

legacy-macos-build:
  xcodebuild -project {{legacy_app_project}} -scheme {{legacy_app_scheme}} build

legacy-macos-build-unsigned:
  xcodebuild -project {{legacy_app_project}} -scheme {{legacy_app_scheme}} -derivedDataPath {{derived_data}} CODE_SIGNING_ALLOWED=NO build

legacy-macos-test:
  xcodebuild test -project {{legacy_app_project}} -scheme {{legacy_app_scheme}} -derivedDataPath {{derived_data}}

site-stage:
  rm -rf {{site_out}}
  mkdir -p {{site_out}}
  cp -R {{site_dir}}/. {{site_out}}/

clean:
  rm -rf {{site_out}} build {{desktop_dir}}/dist
