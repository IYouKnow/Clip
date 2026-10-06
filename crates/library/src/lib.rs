//! Clip library: discovers and manages saved clip files.

use std::path::Path;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};

/// A saved clip on disk.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClipSummary {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
}

/// Lists clip files in `dir`, newest first.
pub fn scan(dir: &Path) -> Result<Vec<ClipSummary>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut clips = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !is_clip(&path) {
            continue;
        }
        clips.push(summary(&path)?);
    }

    clips.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    Ok(clips)
}

/// Metadata for a single clip file.
pub fn summary(path: &Path) -> Result<ClipSummary> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);

    Ok(ClipSummary {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: path.to_string_lossy().to_string(),
        size_bytes: metadata.len(),
        modified_ms,
    })
}

/// Creates `dir` if it does not exist.
pub fn ensure_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(())
}

/// Deletes a clip file.
pub fn delete(path: &Path) -> Result<()> {
    std::fs::remove_file(path).with_context(|| format!("deleting {}", path.display()))?;
    Ok(())
}

fn is_clip(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
}
