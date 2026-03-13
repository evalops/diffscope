use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::core;

use super::extensions::audit_extension_counts;
use super::languages::audit_language_map;

pub async fn lsp_check_command(path: PathBuf, config: config::Config) -> Result<()> {
    let repo_root = core::GitIntegration::new(&path)
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or(path);

    println!("LSP health check");
    println!("repo: {}", repo_root.display());
    println!(
        "symbol_index: {}",
        if config.symbol_index {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("symbol_index_provider: {}", config.symbol_index_provider);
    if !config.symbol_index {
        println!("note: symbol_index is disabled (set symbol_index: true)");
    }
    if config.symbol_index_provider != "lsp" {
        println!("note: symbol_index_provider is not lsp (set symbol_index_provider: lsp)");
    }

    let configured_command = config.symbol_index_lsp_command.clone();
    let detected_command = if configured_command.is_none() {
        core::SymbolIndex::detect_lsp_command(
            &repo_root,
            config.symbol_index_max_files,
            &config.symbol_index_lsp_languages,
            |path| config.should_exclude(path),
        )
    } else {
        None
    };

    if let Some(command) = &configured_command {
        println!("configured LSP command: {}", command);
    }
    if let Some(command) = &detected_command {
        println!("detected LSP command: {}", command);
    }

    let effective_command = configured_command.or(detected_command);
    if let Some(command) = &effective_command {
        let available = core::SymbolIndex::lsp_command_available(command);
        println!("effective LSP command: {}", command);
        println!(
            "command available: {}",
            if available { "yes" } else { "no" }
        );
    } else {
        println!("effective LSP command: <none>");
        println!("command available: no");
    }

    let language_audit = audit_language_map(&config.symbol_index_lsp_languages);
    if language_audit.normalized.is_empty() {
        println!("language map: empty (set symbol_index_lsp_languages)");
    } else {
        println!("language map entries: {}", language_audit.normalized.len());
    }
    if !language_audit.invalid_entries.is_empty() {
        println!(
            "invalid language map entries: {}",
            language_audit.invalid_entries.join(", ")
        );
    }

    let extension_counts = core::SymbolIndex::scan_extension_counts(
        &repo_root,
        config.symbol_index_max_files,
        |path| config.should_exclude(path),
    );
    let extension_audit = audit_extension_counts(extension_counts, &language_audit.normalized);
    if extension_audit.counts.is_empty() {
        println!("repo extensions: none detected (check path or excludes)");
        return Ok(());
    }

    println!(
        "top extensions: {}",
        extension_audit.top_extensions.join(", ")
    );
    if !extension_audit.unmapped.is_empty() {
        println!(
            "unmapped repo extensions: {}",
            extension_audit.unmapped.join(", ")
        );
    }
    if !extension_audit.unused.is_empty() {
        println!(
            "unused language map entries: {}",
            extension_audit.unused.join(", ")
        );
    }

    Ok(())
}
