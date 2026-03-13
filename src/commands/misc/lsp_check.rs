use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config;
use crate::core;

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

    let mut normalized_languages = HashMap::new();
    let mut invalid_mappings = Vec::new();
    for (ext, language) in &config.symbol_index_lsp_languages {
        let ext = ext.trim().to_ascii_lowercase();
        let language = language.trim().to_string();
        if ext.is_empty() || language.is_empty() {
            invalid_mappings.push(format!("{}:{}", ext, language));
            continue;
        }
        normalized_languages.insert(ext, language);
    }

    if normalized_languages.is_empty() {
        println!("language map: empty (set symbol_index_lsp_languages)");
    } else {
        println!("language map entries: {}", normalized_languages.len());
    }
    if !invalid_mappings.is_empty() {
        println!(
            "invalid language map entries: {}",
            invalid_mappings.join(", ")
        );
    }

    let extension_counts = core::SymbolIndex::scan_extension_counts(
        &repo_root,
        config.symbol_index_max_files,
        |path| config.should_exclude(path),
    );
    if extension_counts.is_empty() {
        println!("repo extensions: none detected (check path or excludes)");
        return Ok(());
    }

    let mut extension_list: Vec<_> = extension_counts.iter().collect();
    extension_list.sort_by(|(a_ext, a_count), (b_ext, b_count)| {
        b_count.cmp(a_count).then_with(|| a_ext.cmp(b_ext))
    });
    let top_extensions: Vec<String> = extension_list
        .iter()
        .take(10)
        .map(|(ext, count)| format!("{}({})", ext, count))
        .collect();
    println!("top extensions: {}", top_extensions.join(", "));

    let mut unmapped = Vec::new();
    for ext in extension_counts.keys() {
        if !normalized_languages.contains_key(ext) {
            unmapped.push(ext.clone());
        }
    }
    unmapped.sort();
    if !unmapped.is_empty() {
        println!("unmapped repo extensions: {}", unmapped.join(", "));
    }

    let mut unused = Vec::new();
    for ext in normalized_languages.keys() {
        if !extension_counts.contains_key(ext) {
            unused.push(ext.clone());
        }
    }
    unused.sort();
    if !unused.is_empty() {
        println!("unused language map entries: {}", unused.join(", "));
    }

    Ok(())
}
