use anyhow::Result;
use std::path::{Path, PathBuf};

pub(super) fn collect_fixture_paths(fixtures_dir: &Path) -> Result<Vec<PathBuf>> {
    if !fixtures_dir.exists() {
        anyhow::bail!("Fixtures directory not found: {}", fixtures_dir.display());
    }
    if !fixtures_dir.is_dir() {
        anyhow::bail!(
            "Fixtures path is not a directory: {}",
            fixtures_dir.display()
        );
    }

    let mut paths = Vec::new();
    let mut stack = vec![fixtures_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if matches!(extension.as_deref(), Some("json" | "yml" | "yaml")) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}
