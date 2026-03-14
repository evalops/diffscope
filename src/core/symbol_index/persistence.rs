use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use super::SymbolIndex;

pub fn default_symbol_index_path(repo_root: &Path, cache_key: &str) -> PathBuf {
    let repo_key = hash_text(&repo_root.to_string_lossy());
    let cache_key = if cache_key.trim().is_empty() {
        "default".to_string()
    } else {
        cache_key.trim().to_string()
    };

    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("diffscope")
        .join("symbol-index")
        .join(format!(
            "{}-{}.json",
            &repo_key[..16],
            &cache_key[..cache_key.len().min(16)]
        ))
}

pub fn load_symbol_index(path: &Path) -> Option<SymbolIndex> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| SymbolIndex::from_json(&content).ok())
}

pub fn save_symbol_index(path: &Path, index: &SymbolIndex) -> Result<()> {
    atomic_write_string(path, &index.to_json()?)
}

fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("symbol-index.json");
    let tmp_path = path.with_file_name(format!("{}.{}.tmp", file_name, std::process::id()));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn hash_text(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SymbolIndex;

    #[test]
    fn save_and_load_symbol_index_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let cache_path = dir.path().join("symbol-index.json");

        let source_file = repo_root.join("src/lib.rs");
        std::fs::create_dir_all(source_file.parent().unwrap()).unwrap();
        std::fs::write(&source_file, "pub fn helper() {}\n").unwrap();

        let index = SymbolIndex::build(&repo_root, 16, 128 * 1024, 8, |_path| false).unwrap();
        save_symbol_index(&cache_path, &index).unwrap();

        let loaded = load_symbol_index(&cache_path).unwrap();
        assert_eq!(loaded.files_indexed(), index.files_indexed());
        assert_eq!(loaded.symbols_indexed(), index.symbols_indexed());
        assert!(loaded.lookup("helper").is_some());
    }
}
