use crate::provider::{EngineConfig, ProviderKind};
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
    config: &EngineConfig,
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
    write_output_package_with_fallback(
        &output_roots,
        &safe_name,
        &timestamp,
        request,
        result,
        config,
    )
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
    config: &EngineConfig,
) -> Result<SourceArtifactManifest> {
    let mut last_error = None;

    for output_root in output_roots {
        match write_output_package_to_root(
            output_root,
            safe_name,
            timestamp,
            request,
            result,
            config,
        ) {
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
    config: &EngineConfig,
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
    let content_hash = file_content_hash(&source_path)?;
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
    write_source_pack_and_evidence_index(&manifest, &content_hash, now, config)?;
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

pub(crate) fn retry_failed_page_artifacts(
    request: &RetryFailedPagesRequest,
    config: &EngineConfig,
) -> Result<RetryFailedPagesResponseData> {
    if request.pages.is_empty() {
        bail!("retry failed pages requires at least one page update");
    }

    let mut manifest = load_source_manifest_from_path(&request.source_manifest_path)?;
    let warnings_before = source_pack_warnings(&manifest).len();
    let mut retried_page_count = 0usize;

    for update in &request.pages {
        let page = manifest
            .pages
            .iter_mut()
            .find(|page| page.index == update.page_index)
            .ok_or_else(|| {
                anyhow!(
                    "retry page {} was not found in source manifest",
                    update.page_index + 1
                )
            })?;

        if page.error_message.is_none() {
            bail!(
                "retry page {} is not failed; retry is limited to failed pages",
                update.page_index + 1
            );
        }

        if update.error_message.is_none()
            && update
                .markdown
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
            && update
                .plain_text
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!(
                "retry page {} needs markdown, plain text, or an error message",
                update.page_index + 1
            );
        }

        if let Some(error_message) = &update.error_message {
            page.error_message = Some(error_message.clone());
            continue;
        }

        let page_number = update.page_index + 1;
        let pages_dir = Path::new(&manifest.artifact_root).join("pages");
        fs::create_dir_all(&pages_dir).with_context(|| {
            format!(
                "failed creating page artifact directory {}",
                pages_dir.display()
            )
        })?;

        if let Some(markdown) = update
            .markdown
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            let path = pages_dir.join(format!("page_{page_number}.md"));
            fs::write(&path, markdown).with_context(|| {
                format!("failed writing retry page markdown {}", path.display())
            })?;
            page.markdown_path = Some(path.display().to_string());
        }

        if let Some(plain_text) = update
            .plain_text
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            let path = pages_dir.join(format!("page_{page_number}.txt"));
            fs::write(&path, plain_text)
                .with_context(|| format!("failed writing retry page text {}", path.display()))?;
            page.plain_text_path = Some(path.display().to_string());
        }

        if let Some(image_asset_path) = &update.image_asset_path {
            let image_path = Path::new(image_asset_path);
            page.image_path = Some(
                if image_path.is_absolute() {
                    image_path.to_path_buf()
                } else {
                    Path::new(&manifest.artifact_root).join(image_path)
                }
                .display()
                .to_string(),
            );
        }

        page.error_message = None;
        retried_page_count += 1;
    }

    manifest.status = ingest_status_for_pages(&manifest.pages);
    manifest.updated_at = unix_timestamp_seconds();
    rewrite_manifest_markdown(&manifest)?;
    write_source_manifest(&manifest)?;

    let content_hash = file_content_hash(Path::new(&manifest.source_path))?;
    write_source_pack_and_evidence_index(&manifest, &content_hash, manifest.updated_at, config)?;

    let remaining_failed_count = failed_page_count(&manifest.pages);
    Ok(RetryFailedPagesResponseData {
        source_pack_path: Path::new(&manifest.artifact_root)
            .join("source_pack.json")
            .display()
            .to_string(),
        evidence_index_path: Path::new(&manifest.artifact_root)
            .join("evidence_index.json")
            .display()
            .to_string(),
        warnings_after: source_pack_warnings(&manifest).len(),
        warnings_before,
        remaining_failed_count,
        retried_page_count,
        source_manifest: manifest,
    })
}

fn write_source_pack_and_evidence_index(
    manifest: &SourceArtifactManifest,
    content_hash: &str,
    generated_at: u64,
    config: &EngineConfig,
) -> Result<()> {
    let artifacts =
        SourceImportArtifactBuilder::new(manifest, content_hash, generated_at, config)?.build();
    let source_pack_path = Path::new(&manifest.artifact_root).join("source_pack.json");
    let evidence_index_path = Path::new(&manifest.artifact_root).join("evidence_index.json");
    fs::write(
        &source_pack_path,
        serde_json::to_string_pretty(&artifacts.source_pack)
            .context("failed to encode source pack")?,
    )
    .with_context(|| format!("failed writing source pack {}", source_pack_path.display()))?;
    fs::write(
        &evidence_index_path,
        serde_json::to_string_pretty(&artifacts.evidence_index)
            .context("failed to encode evidence index")?,
    )
    .with_context(|| {
        format!(
            "failed writing evidence index {}",
            evidence_index_path.display()
        )
    })?;
    Ok(())
}

struct SourceImportArtifacts {
    source_pack: hyprduck_engine_types::SourcePackV0,
    evidence_index: hyprduck_engine_types::EvidenceIndexV1,
}

#[derive(Debug)]
struct SourceImportArtifactBuilder<'a> {
    manifest: &'a SourceArtifactManifest,
    content_hash: &'a str,
    generated_at: u64,
    provider_route: String,
    local_only: bool,
    warnings: Vec<hyprduck_engine_types::SourcePackWarningV0>,
}

impl<'a> SourceImportArtifactBuilder<'a> {
    fn new(
        manifest: &'a SourceArtifactManifest,
        content_hash: &'a str,
        generated_at: u64,
        config: &EngineConfig,
    ) -> Result<Self> {
        if manifest.workspace_id.trim().is_empty() {
            bail!("source import artifacts require workspace_id");
        }
        if manifest.source_id.trim().is_empty() {
            bail!("source import artifacts require source_id");
        }
        if content_hash.trim().is_empty() {
            bail!("source import artifacts require content_hash");
        }

        let (provider_route, local_only) = provider_disclosure(config);
        Ok(Self {
            manifest,
            content_hash,
            generated_at,
            provider_route,
            local_only,
            warnings: source_pack_warnings(manifest),
        })
    }

    fn build(self) -> SourceImportArtifacts {
        let source_pack = hyprduck_engine_types::SourcePackV0 {
            schema_version: hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION.into(),
            workspace_id: self.manifest.workspace_id.clone(),
            source_id: self.manifest.source_id.clone(),
            original_filename: Path::new(&self.manifest.original_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(self.manifest.original_path.as_str())
                .into(),
            original_path: self.manifest.original_path.clone(),
            source_path: self.manifest.source_path.clone(),
            markdown_path: self.manifest.markdown_path.clone(),
            artifact_root: self.manifest.artifact_root.clone(),
            content_hash: self.content_hash.to_string(),
            format: self.manifest.format.clone(),
            page_count: self.manifest.pages.len(),
            ingestion_status: self.manifest.status.clone(),
            provider_route: self.provider_route.clone(),
            local_only: self.local_only,
            pages: self
                .manifest
                .pages
                .iter()
                .map(|page| hyprduck_engine_types::SourcePackPageV0 {
                    page: page.index + 1,
                    label: page.label.clone(),
                    image_path: page.image_path.clone(),
                    markdown_path: page.markdown_path.clone(),
                    plain_text_path: page.plain_text_path.clone(),
                    error_message: page.error_message.clone(),
                })
                .collect(),
            warnings: self.warnings.clone(),
            created_at: self.manifest.created_at,
            updated_at: self.manifest.updated_at,
        };
        let evidence_index = hyprduck_engine_types::EvidenceIndexV1 {
            schema_version: hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION.into(),
            workspace_id: self.manifest.workspace_id.clone(),
            source_id: self.manifest.source_id.clone(),
            content_hash: self.content_hash.to_string(),
            provider_route: self.provider_route,
            local_only: self.local_only,
            evidence: self
                .manifest
                .pages
                .iter()
                .filter_map(|page| evidence_index_item(self.manifest, page, self.content_hash))
                .collect(),
            warnings: self.warnings,
            generated_at: self.generated_at,
        };
        SourceImportArtifacts {
            source_pack,
            evidence_index,
        }
    }
}

fn load_source_manifest_from_path(path: &str) -> Result<SourceArtifactManifest> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed reading source manifest {path}"))?;
    serde_json::from_str(&json).with_context(|| format!("failed decoding source manifest {path}"))
}

fn rewrite_manifest_markdown(manifest: &SourceArtifactManifest) -> Result<()> {
    let mut markdown = format!("# {}\n\n", manifest.output_name);
    for page in &manifest.pages {
        markdown.push_str(&format!("## {}\n\n", page.label));
        if let Some(image_path) = &page.image_path {
            markdown.push_str(&format!("![{}]({image_path})\n\n", page.label));
        }
        if let Some(page_markdown_path) = &page.markdown_path {
            let body = fs::read_to_string(page_markdown_path)
                .with_context(|| format!("failed reading page markdown {page_markdown_path}"))?;
            markdown.push_str(&body);
            markdown.push_str("\n\n");
        } else if let Some(plain_text_path) = &page.plain_text_path {
            let body = fs::read_to_string(plain_text_path)
                .with_context(|| format!("failed reading page text {plain_text_path}"))?;
            markdown.push_str(&body);
            markdown.push_str("\n\n");
        } else if let Some(error_message) = &page.error_message {
            markdown.push_str(&format!("_AI analysis unavailable: {error_message}_\n\n"));
        } else {
            markdown.push_str("_AI analysis unavailable._\n\n");
        }
    }
    fs::write(&manifest.markdown_path, markdown)
        .with_context(|| format!("failed writing markdown {}", manifest.markdown_path))
}

fn failed_page_count(pages: &[PageArtifact]) -> usize {
    pages
        .iter()
        .filter(|page| page.error_message.is_some())
        .count()
}

fn ingest_status_for_pages(pages: &[PageArtifact]) -> IngestStatus {
    let failed_count = failed_page_count(pages);
    if failed_count == 0 {
        IngestStatus::Ingested
    } else if failed_count == pages.len() {
        IngestStatus::Failed
    } else {
        IngestStatus::Partial
    }
}

fn source_pack_warnings(
    manifest: &SourceArtifactManifest,
) -> Vec<hyprduck_engine_types::SourcePackWarningV0> {
    manifest
        .pages
        .iter()
        .filter_map(|page| {
            page.error_message
                .as_ref()
                .map(|message| hyprduck_engine_types::SourcePackWarningV0 {
                    warning_type: "page_parse_failed".into(),
                    severity: hyprduck_engine_types::ContextPackWarningSeverity::High,
                    message: message.clone(),
                    page: Some(page.index + 1),
                })
        })
        .collect()
}

fn evidence_index_item(
    manifest: &SourceArtifactManifest,
    page: &PageArtifact,
    content_hash: &str,
) -> Option<hyprduck_engine_types::EvidenceIndexItemV1> {
    let raw_text = page
        .markdown_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .or_else(|| {
            page.plain_text_path
                .as_ref()
                .and_then(|path| fs::read_to_string(path).ok())
        })
        .unwrap_or_default();
    let evidence_type = infer_evidence_type(page, &raw_text);
    let quoted_text = excerpt(&raw_text, 280);
    let quoted_text = if quoted_text.trim().is_empty()
        && evidence_type == hyprduck_engine_types::EvidenceType::ImageRegion
    {
        format!("Image region evidence for {}.", page.label)
    } else {
        quoted_text
    };
    if quoted_text.trim().is_empty() {
        return None;
    }
    Some(hyprduck_engine_types::EvidenceIndexItemV1 {
        evidence_ref: format!("ev-{}-source-{}", manifest.source_id, page.index + 1),
        source_id: manifest.source_id.clone(),
        page: page.index + 1,
        region: format!("page:{}", page.label),
        span: Some("page".into()),
        quoted_text,
        parse_confidence: hyprduck_engine_types::ContextPackParseConfidence::Unknown,
        content_hash: content_hash.to_string(),
        markdown_path: page.markdown_path.clone(),
        image_path: page.image_path.clone(),
        evidence_type,
    })
}

fn infer_evidence_type(page: &PageArtifact, raw_text: &str) -> hyprduck_engine_types::EvidenceType {
    if page
        .markdown_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .is_some_and(|markdown| contains_markdown_table(&markdown))
    {
        return hyprduck_engine_types::EvidenceType::Table;
    }

    if raw_text.trim().is_empty() && page.image_path.is_some() && page.error_message.is_none() {
        return hyprduck_engine_types::EvidenceType::ImageRegion;
    }

    hyprduck_engine_types::EvidenceType::Text
}

fn contains_markdown_table(markdown: &str) -> bool {
    let mut previous_has_pipe = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        let has_pipe = trimmed.matches('|').count() >= 2;
        let is_separator = has_pipe
            && trimmed.chars().all(|character| {
                matches!(character, '|' | '-' | ':' | ' ') || character.is_whitespace()
            });
        if previous_has_pipe && is_separator {
            return true;
        }
        previous_has_pipe = has_pipe;
    }
    false
}

fn file_content_hash(path: &Path) -> Result<String> {
    let content = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    Ok(format!("fnv64:{:016x}", fnv1a64(&content)))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for value in text.split_whitespace() {
        let extra = usize::from(!output.is_empty());
        if output.len() + extra + value.len() > max_chars {
            break;
        }
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(value);
    }
    output
}

fn provider_disclosure(config: &EngineConfig) -> (String, bool) {
    let provider_route = config.provider.id_slug().to_string();
    let local_only = matches!(config.provider, ProviderKind::Ollama)
        && config
            .base_url
            .as_deref()
            .map(is_loopback_url)
            .unwrap_or(true);
    (provider_route, local_only)
}

fn is_loopback_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    let Some(after_scheme) = lower.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host.starts_with('[') {
        host.split_once(']')
            .map(|(host, _)| format!("{host}]"))
            .unwrap_or_else(|| host.to_string())
    } else {
        host.split_once(':')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| host.to_string())
    };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
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
    let success_count = manifest.pages.len().saturating_sub(failed_count);
    SourceSummary {
        workspace_id: manifest.workspace_id.clone(),
        source_id: manifest.source_id.clone(),
        original_path: manifest.original_path.clone(),
        source_path: manifest.source_path.clone(),
        markdown_path: manifest.markdown_path.clone(),
        format: manifest.format.clone(),
        status: manifest.status.clone(),
        page_count: manifest.pages.len(),
        success_count,
        failed_count,
        citation_ready: success_count > 0,
        graph_ready: false,
        description: manifest.description.clone(),
        user_context: manifest.user_context.clone(),
        ingest_instruction: manifest.ingest_instruction.clone(),
        updated_at: manifest.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_import_artifact_builder_shares_required_metadata() {
        let manifest = test_manifest();
        let mut config = EngineConfig::default();
        config.provider = ProviderKind::Ollama;
        config.base_url = Some("http://127.0.0.1:11434".into());

        let artifacts = SourceImportArtifactBuilder::new(&manifest, "sha256:test", 42, &config)
            .expect("builder should accept complete metadata")
            .build();

        assert_eq!(
            artifacts.source_pack.schema_version,
            hyprduck_engine_types::SOURCE_PACK_V0_SCHEMA_VERSION
        );
        assert_eq!(
            artifacts.evidence_index.schema_version,
            hyprduck_engine_types::EVIDENCE_INDEX_V1_SCHEMA_VERSION
        );
        assert_eq!(artifacts.source_pack.workspace_id, "default");
        assert_eq!(artifacts.evidence_index.workspace_id, "default");
        assert_eq!(artifacts.source_pack.source_id, "source-a");
        assert_eq!(artifacts.evidence_index.source_id, "source-a");
        assert_eq!(artifacts.source_pack.content_hash, "sha256:test");
        assert_eq!(artifacts.evidence_index.content_hash, "sha256:test");
        assert_eq!(artifacts.source_pack.provider_route, "ollama");
        assert_eq!(artifacts.evidence_index.provider_route, "ollama");
        assert!(artifacts.source_pack.local_only);
        assert!(artifacts.evidence_index.local_only);
        assert_eq!(
            artifacts.source_pack.warnings,
            artifacts.evidence_index.warnings
        );
        assert_eq!(artifacts.evidence_index.generated_at, 42);
    }

    #[test]
    fn source_import_artifact_builder_rejects_missing_required_metadata() {
        let mut manifest = test_manifest();
        manifest.source_id.clear();

        let error = SourceImportArtifactBuilder::new(
            &manifest,
            "sha256:test",
            42,
            &EngineConfig::default(),
        )
        .expect_err("source_id is required");

        assert!(error
            .to_string()
            .contains("source import artifacts require source_id"));
    }

    fn test_manifest() -> SourceArtifactManifest {
        SourceArtifactManifest {
            workspace_id: "default".into(),
            source_id: "source-a".into(),
            original_path: "/tmp/source-a.pdf".into(),
            source_path: "/tmp/workspace/sources/source-a/source-a.pdf".into(),
            markdown_path: "/tmp/workspace/artifacts/source-a/source-a.md".into(),
            artifact_root: "/tmp/workspace/artifacts/source-a".into(),
            manifest_path: "/tmp/workspace/artifacts/source-a/source-manifest.json".into(),
            format: DocumentFormat::Pdf,
            output_name: "source-a".into(),
            status: IngestStatus::Ingested,
            description: String::new(),
            user_context: String::new(),
            ingest_instruction: String::new(),
            pages: vec![PageArtifact {
                index: 0,
                label: "Page 1".into(),
                image_path: None,
                markdown_path: Some("/tmp/workspace/artifacts/source-a/pages/page_1.md".into()),
                plain_text_path: None,
                error_message: None,
            }],
            created_at: 1,
            updated_at: 2,
        }
    }
}
