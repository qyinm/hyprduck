use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::domains::ingest::pipeline::ProcessLocator;

pub(crate) fn extract_text_via_textutil(
    path: &Path,
    process_locator: &impl ProcessLocator,
) -> Result<String> {
    let output = Command::new(process_locator.resolve_binary("textutil", &["/usr/bin/textutil"]))
        .arg("-convert")
        .arg("txt")
        .arg("-stdout")
        .arg(path)
        .output()
        .context("failed to launch textutil")?;

    if !output.status.success() {
        bail!("textutil failed for {}", path.display());
    }

    let text = String::from_utf8(output.stdout).context("textutil output was not valid UTF-8")?;
    if text.trim().is_empty() {
        bail!(
            "text extraction produced empty output for {}",
            path.display()
        );
    }
    Ok(text)
}
