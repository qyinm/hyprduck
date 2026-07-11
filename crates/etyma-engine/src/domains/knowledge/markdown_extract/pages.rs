use super::*;

pub(crate) fn extract_page_sections(markdown: &str) -> Vec<PageSection> {
    let normalized = markdown.replace("\r\n", "\n");
    let headers = regex_like_page_headers(&normalized);
    if headers.is_empty() {
        return vec![PageSection {
            page_index: 0,
            page_label: "Imported text".into(),
            content: normalized,
            markdown_path: None,
            image_path: None,
        }];
    }

    let mut sections = Vec::with_capacity(headers.len());
    for index in 0..headers.len() {
        let (page_label, _, content_start) = &headers[index];
        let next_start = headers
            .get(index + 1)
            .map(|(_, next_start, _)| *next_start)
            .unwrap_or(normalized.len());
        sections.push(PageSection {
            page_index: index,
            page_label: page_label.clone(),
            content: normalized[*content_start..next_start].trim().to_string(),
            markdown_path: None,
            image_path: None,
        });
    }
    sections
}

pub(crate) fn attach_page_artifacts_to_sections(
    sections: &mut [PageSection],
    source_manifest: Option<&SourceArtifactManifest>,
) {
    let Some(manifest) = source_manifest else {
        return;
    };
    for section in sections {
        let artifact = manifest
            .pages
            .iter()
            .find(|page| page.label == section.page_label)
            .or_else(|| manifest.pages.get(section.page_index));
        if let Some(artifact) = artifact {
            section.page_index = artifact.index;
            section.markdown_path = artifact.markdown_path.clone();
            section.image_path = artifact.image_path.clone();
        }
    }
}

pub(crate) fn regex_like_page_headers(markdown: &str) -> Vec<(String, usize, usize)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for line in markdown.lines() {
        let line_len = line.len();
        if let Some(page_label) = line
            .strip_prefix("## Page ")
            .map(|page| format!("Page {}", page.trim()))
        {
            headers.push((page_label, offset, offset + line_len + 1));
        }
        offset += line_len + 1;
    }
    headers
}

pub(crate) fn infer_markdown_title(markdown_path: &str, markdown: &str) -> String {
    if let Some(heading) = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
    {
        return heading.to_string();
    }

    Path::new(markdown_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "Etyma import".into())
}
