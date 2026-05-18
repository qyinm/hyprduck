use crate::*;

pub(crate) fn build_markdown(title: String, pages: &[ParsedPage]) -> String {
    let mut markdown = format!("# {title}\n\n");
    for (idx, page) in pages.iter().enumerate() {
        markdown.push_str(&format!("## Page {}\n\n", idx + 1));
        if let Some(image_path) = &page.image_asset_path {
            markdown.push_str(&format!("![Page {}]({image_path})\n\n", idx + 1));
        }
        if let Some(body) = page
            .markdown
            .as_ref()
            .or(page.plain_text.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            markdown.push_str(body);
            markdown.push_str("\n\n");
        } else if let Some(error_message) = &page.error_message {
            markdown.push_str(&format!("_AI analysis unavailable: {error_message}_\n\n"));
        } else {
            markdown.push_str("_AI analysis unavailable._\n\n");
        }
    }
    markdown
}

pub(crate) fn export_output_package(
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<Option<SourceArtifactManifest>> {
    let Some(output) = &request.output else {
        return Ok(None);
    };

    let base_name = output
        .name
        .clone()
        .or_else(|| {
            Path::new(&request.input.path)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "document".to_string());
    let safe_name = sanitize_name(&base_name);
    let timestamp = chrono_like_timestamp();
    let output_roots = output_root_candidates(output)?;
    write_output_package_with_fallback(&output_roots, &safe_name, &timestamp, request, result)
        .map(Some)
}

fn output_root_candidates(
    output: &hyprduck_engine_types::ParseOutputTarget,
) -> Result<Vec<PathBuf>> {
    if let Some(root) = &output.root_dir {
        return Ok(vec![PathBuf::from(root)]);
    }

    let mut candidates = Vec::new();

    if let Some(override_root) = std::env::var_os("HYPRDUCK_OUTPUT_DIR") {
        candidates.push(PathBuf::from(override_root));
    } else {
        if let Some(application_support_root) = dirs::data_local_dir() {
            candidates.push(application_support_root.join("HyprDuck"));
        }
        candidates.push(std::env::temp_dir().join("HyprDuck"));
    }

    candidates.dedup();
    Ok(candidates)
}

pub(crate) fn write_output_package_with_fallback(
    output_roots: &[PathBuf],
    safe_name: &str,
    timestamp: &str,
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<SourceArtifactManifest> {
    let mut last_error = None;

    for output_root in output_roots {
        match write_output_package_to_root(output_root, safe_name, timestamp, request, result) {
            Ok(manifest) => return Ok(manifest),
            Err(error) => {
                eprintln!(
                    "output packaging failed under {}: {error:#}",
                    output_root.display()
                );
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("failed writing markdown package")))
}

fn write_output_package_to_root(
    output_root: &Path,
    safe_name: &str,
    timestamp: &str,
    request: &ParseRequest,
    result: &ParseResult,
) -> Result<SourceArtifactManifest> {
    let workspace_id = request
        .output
        .as_ref()
        .and_then(|output| output.workspace_id.clone())
        .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string());
    let source_id = request
        .output
        .as_ref()
        .and_then(|output| output.source_id.clone())
        .unwrap_or_else(new_source_id);
    let workspace_root = output_root.join(&workspace_id);
    let sources_root = workspace_root.join("sources");
    let artifacts_root = workspace_root.join("artifacts");
    for required_dir in [
        &sources_root,
        &artifacts_root,
        &workspace_root.join("wiki"),
        &workspace_root.join("graph"),
    ] {
        fs::create_dir_all(required_dir)
            .with_context(|| format!("failed creating {}", required_dir.display()))?;
    }

    let source_dir = sources_root.join(&source_id);
    let output_dir = artifacts_root.join(&source_id);
    let images_dir = output_dir.join("images");
    let pages_dir = output_dir.join("pages");
    fs::create_dir_all(&source_dir)
        .with_context(|| format!("failed creating source directory {}", source_dir.display()))?;
    fs::create_dir_all(&images_dir).with_context(|| {
        format!(
            "failed creating image output directory {}",
            images_dir.display()
        )
    })?;
    fs::create_dir_all(&pages_dir).with_context(|| {
        format!(
            "failed creating page artifact directory {}",
            pages_dir.display()
        )
    })?;

    let source_filename = Path::new(&request.input.path)
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{safe_name}.{timestamp}"));
    let source_path = source_dir.join(sanitize_name(&source_filename));
    fs::copy(&request.input.path, &source_path).with_context(|| {
        format!(
            "failed copying source document {} to {}",
            request.input.path,
            source_path.display()
        )
    })?;

    for asset in &result.assets {
        let target = output_dir.join(&asset.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating asset directory {}", parent.display()))?;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&asset.base64)
            .with_context(|| format!("failed decoding asset {}", asset.relative_path))?;
        fs::write(&target, bytes)
            .with_context(|| format!("failed writing asset {}", target.display()))?;
    }

    let markdown_path = output_dir.join(format!("{safe_name}.md"));
    fs::write(&markdown_path, &result.markdown)
        .with_context(|| format!("failed writing markdown {}", markdown_path.display()))?;

    let mut page_artifacts = Vec::new();
    for page in &result.pages {
        let page_number = page.index + 1;
        let markdown_path = if let Some(markdown) = &page.markdown {
            let path = pages_dir.join(format!("page_{page_number}.md"));
            fs::write(&path, markdown)
                .with_context(|| format!("failed writing page markdown {}", path.display()))?;
            Some(path.display().to_string())
        } else {
            None
        };
        let plain_text_path = if let Some(plain_text) = &page.plain_text {
            let path = pages_dir.join(format!("page_{page_number}.txt"));
            fs::write(&path, plain_text)
                .with_context(|| format!("failed writing page text {}", path.display()))?;
            Some(path.display().to_string())
        } else {
            None
        };
        page_artifacts.push(PageArtifact {
            index: page.index,
            label: format!("Page {page_number}"),
            image_path: page
                .image_asset_path
                .as_ref()
                .map(|relative_path| output_dir.join(relative_path).display().to_string()),
            markdown_path,
            plain_text_path,
            error_message: page.error_message.clone(),
        });
    }

    let status = ingest_status_for_result(result);
    let now = unix_timestamp_seconds();
    let manifest_path = output_dir.join("source-manifest.json");
    let manifest = SourceArtifactManifest {
        workspace_id,
        source_id,
        original_path: request.input.path.clone(),
        source_path: source_path.display().to_string(),
        markdown_path: markdown_path.display().to_string(),
        artifact_root: output_dir.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
        format: request.input.format.clone(),
        output_name: safe_name.to_string(),
        status,
        description: String::new(),
        user_context: String::new(),
        ingest_instruction: String::new(),
        pages: page_artifacts,
        created_at: now,
        updated_at: now,
    };
    write_source_manifest(&manifest)?;
    Ok(manifest)
}

fn ingest_status_for_result(result: &ParseResult) -> IngestStatus {
    if result.success_count == 0 && result.failed_count > 0 {
        IngestStatus::Failed
    } else if result.failed_count > 0 {
        IngestStatus::Partial
    } else {
        IngestStatus::Ingested
    }
}

pub(crate) fn write_source_manifest(manifest: &SourceArtifactManifest) -> Result<()> {
    let json =
        serde_json::to_string_pretty(manifest).context("failed to encode source manifest")?;
    if let Some(parent) = Path::new(&manifest.manifest_path).parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating source manifest directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&manifest.manifest_path, json)
        .with_context(|| format!("failed writing source manifest {}", manifest.manifest_path))
}

pub(crate) fn load_source_manifest(
    request: &CompileProjectRequest,
) -> Result<Option<SourceArtifactManifest>> {
    let Some(path) = &request.source_manifest_path else {
        return Ok(None);
    };
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed reading source manifest {path}"))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed decoding source manifest {path}"))
        .map(Some)
}

pub(crate) fn resolved_source_ids(
    request: &CompileProjectRequest,
    manifest: Option<&SourceArtifactManifest>,
) -> Result<(WorkspaceId, SourceId)> {
    if let Some(manifest) = manifest {
        if let Some(request_workspace_id) = &request.workspace_id {
            if request_workspace_id != &manifest.workspace_id {
                bail!(
                    "compile_project workspace_id {} does not match source manifest workspace_id {}",
                    request_workspace_id,
                    manifest.workspace_id
                );
            }
        }
        if let Some(request_source_id) = &request.source_id {
            if request_source_id != &manifest.source_id {
                bail!(
                    "compile_project source_id {} does not match source manifest source_id {}",
                    request_source_id,
                    manifest.source_id
                );
            }
        }
        return Ok((manifest.workspace_id.clone(), manifest.source_id.clone()));
    }

    Ok((
        request
            .workspace_id
            .clone()
            .unwrap_or_else(|| DEFAULT_WORKSPACE_ID.to_string()),
        request
            .source_id
            .clone()
            .unwrap_or_else(|| build_source_id(&request.source_markdown_path, 0)),
    ))
}

pub(crate) fn build_source_id(seed: &str, timestamp: u64) -> SourceId {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{seed}|{timestamp}").as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("source-{hash:016x}")
}

fn new_source_id() -> SourceId {
    format!("source-{}", Uuid::now_v7())
}

pub(crate) fn source_summary_from_manifest(manifest: &SourceArtifactManifest) -> SourceSummary {
    let failed_count = manifest
        .pages
        .iter()
        .filter(|page| page.error_message.is_some())
        .count();
    SourceSummary {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: manifest.format.clone(),
        status: manifest.status.clone(),
        page_count: manifest.pages.len(),
        success_count: manifest.pages.len().saturating_sub(failed_count),
        failed_count,
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
    }
}
