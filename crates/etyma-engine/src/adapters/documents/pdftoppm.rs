use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use tempfile::tempdir;

use crate::domains::ingest::pipeline::ProcessLocator;

pub(crate) fn convert_pdf_to_pngs(
    path: &Path,
    process_locator: &impl ProcessLocator,
) -> Result<Vec<PathBuf>> {
    let temp = tempdir().context("failed to create temp directory for pdf conversion")?;
    let prefix = temp.path().join("page");
    let status = Command::new(process_locator.resolve_binary(
        "pdftoppm",
        &["/opt/homebrew/bin/pdftoppm", "/usr/local/bin/pdftoppm"],
    ))
    .arg("-png")
    .arg(path)
    .arg(&prefix)
    .status()
    .context("failed to launch pdftoppm")?;
    if !status.success() {
        bail!("pdftoppm failed for {}", path.display());
    }

    let mut outputs = fs::read_dir(temp.path())
        .with_context(|| {
            format!(
                "failed listing converted PDF pages in {}",
                temp.path().display()
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    outputs.sort_by_key(|path| rendered_page_sort_key(path));

    if outputs.is_empty() {
        bail!("pdf conversion produced no pages for {}", path.display());
    }

    let persisted_root = temp.keep();
    Ok(outputs
        .into_iter()
        .map(|path| persisted_root.join(path.file_name().unwrap()))
        .collect())
}

fn rendered_page_sort_key(path: &Path) -> usize {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| {
            stem.rsplit_once('-')
                .map(|(_, suffix)| suffix)
                .or(Some(stem))
        })
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::rendered_page_sort_key;
    use std::path::Path;

    #[test]
    fn rendered_pdf_pages_sort_by_numeric_suffix() {
        let mut pages = [
            "page-10.png",
            "page-2.png",
            "page-1.png",
            "page-11.png",
            "page-3.png",
        ];

        pages.sort_by_key(|page| rendered_page_sort_key(Path::new(page)));

        assert_eq!(
            pages,
            [
                "page-1.png",
                "page-2.png",
                "page-3.png",
                "page-10.png",
                "page-11.png"
            ]
        );
    }
}
