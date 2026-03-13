use std::path::Path;
use tracing::{info, warn};

use crate::config;
use crate::core;

pub fn build_symbol_index(config: &config::Config, repo_root: &Path) -> Option<core::SymbolIndex> {
    if !config.symbol_index {
        return None;
    }

    let provider = config.symbol_index_provider.as_str();
    let result = if provider == "lsp" {
        let detected_command = if config.symbol_index_lsp_command.is_none() {
            core::SymbolIndex::detect_lsp_command(
                repo_root,
                config.symbol_index_max_files,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            )
        } else {
            None
        };

        let command = config
            .symbol_index_lsp_command
            .as_deref()
            .map(str::to_string)
            .or(detected_command);

        if let Some(command) = command {
            if config.symbol_index_lsp_command.is_none() {
                info!("Auto-detected LSP command: {}", command);
            }

            match core::SymbolIndex::build_with_lsp(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                &command,
                &config.symbol_index_lsp_languages,
                |path| config.should_exclude(path),
            ) {
                Ok(index) => Ok(index),
                Err(err) => {
                    warn!("LSP indexer failed (falling back to regex): {}", err);
                    core::SymbolIndex::build(
                        repo_root,
                        config.symbol_index_max_files,
                        config.symbol_index_max_bytes,
                        config.symbol_index_max_locations,
                        |path| config.should_exclude(path),
                    )
                }
            }
        } else {
            warn!("No LSP command configured or detected; falling back to regex indexer.");
            core::SymbolIndex::build(
                repo_root,
                config.symbol_index_max_files,
                config.symbol_index_max_bytes,
                config.symbol_index_max_locations,
                |path| config.should_exclude(path),
            )
        }
    } else {
        core::SymbolIndex::build(
            repo_root,
            config.symbol_index_max_files,
            config.symbol_index_max_bytes,
            config.symbol_index_max_locations,
            |path| config.should_exclude(path),
        )
    };

    match result {
        Ok(index) => {
            info!(
                "Indexed {} symbols across {} files",
                index.symbols_indexed(),
                index.files_indexed()
            );
            Some(index)
        }
        Err(err) => {
            warn!("Symbol index build failed: {}", err);
            None
        }
    }
}
