set shell := ["bash", "-cu"]

app_project := "apps/macos/DuckDocs.xcodeproj"
app_scheme := "DuckDocs"
derived_data := "/tmp/DuckDocsDerivedData"
site_dir := "apps/site"
site_out := "_site"

default:
  @just --list

paths:
  @echo "app_project={{app_project}}"
  @echo "app_scheme={{app_scheme}}"
  @echo "site_dir={{site_dir}}"
  @echo "site_out={{site_out}}"

macos-build:
  xcodebuild -project {{app_project}} -scheme {{app_scheme}} build

macos-build-unsigned:
  xcodebuild -project {{app_project}} -scheme {{app_scheme}} -derivedDataPath {{derived_data}} CODE_SIGNING_ALLOWED=NO build

macos-test:
  xcodebuild test -project {{app_project}} -scheme {{app_scheme}} -derivedDataPath {{derived_data}}

site-stage:
  rm -rf {{site_out}}
  mkdir -p {{site_out}}
  cp -R {{site_dir}}/. {{site_out}}/

clean:
  rm -rf {{site_out}} build
