use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::plugins;

use super::super::context_helpers::{resolve_pattern_repositories, PatternRepositoryMap};
use super::super::feedback::{generate_feedback_context, load_feedback_store, FeedbackStore};
use super::super::rule_helpers::load_review_rules;
use super::context::build_symbol_index;
use super::types::ProgressCallback;

pub(super) struct PipelineServices {
    pub config: config::Config,
    pub repo_path: PathBuf,
    pub context_fetcher: core::ContextFetcher,
    pub pattern_repositories: PatternRepositoryMap,
    pub review_rules: Vec<core::ReviewRule>,
    pub feedback: FeedbackStore,
    pub feedback_context: String,
    pub plugin_manager: plugins::plugin::PluginManager,
    pub adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub verification_adapter: Arc<dyn adapters::llm::LLMAdapter>,
    pub embedding_adapter: Option<Arc<dyn adapters::llm::LLMAdapter>>,
    pub base_prompt_config: core::prompt::PromptConfig,
    pub convention_store_path: Option<PathBuf>,
    pub is_local: bool,
}

impl PipelineServices {
    pub(super) async fn new(config: config::Config, repo_path: &Path) -> Result<Self> {
        let repo_path = repo_path.to_path_buf();
        let is_local = should_optimize_for_local(&config);
        let convention_store_path = resolve_convention_store_path(&config);
        let pattern_repositories = resolve_pattern_repositories(&config, &repo_path);
        let review_rules = load_review_rules(&config, &pattern_repositories, &repo_path);

        let mut plugin_manager = plugins::plugin::PluginManager::new();
        plugin_manager.load_builtin_plugins(&config.plugins).await?;

        let feedback = load_feedback_store(&config);
        let feedback_context = if config.enhanced_feedback {
            generate_feedback_context(&feedback)
        } else {
            String::new()
        };

        let model_config = config.to_model_config();
        let adapter: Arc<dyn adapters::llm::LLMAdapter> =
            Arc::from(adapters::llm::create_adapter(&model_config)?);
        info!("Review adapter: {}", adapter.model_name());

        let verification_adapter: Arc<dyn adapters::llm::LLMAdapter> = {
            let verification_config =
                config.to_model_config_for_role(config.verification_model_role);
            if verification_config.model_name != model_config.model_name {
                info!(
                    "Using '{}' model '{}' for verification pass",
                    format!("{:?}", config.verification_model_role).to_lowercase(),
                    verification_config.model_name
                );
                Arc::from(adapters::llm::create_adapter(&verification_config)?)
            } else {
                adapter.clone()
            }
        };

        let embedding_adapter = if config.semantic_rag || config.semantic_feedback {
            let embedding_config = config.to_model_config_for_role(config::ModelRole::Embedding);
            if embedding_config.model_name == model_config.model_name {
                Some(adapter.clone())
            } else {
                match adapters::llm::create_adapter(&embedding_config) {
                    Ok(adapter) => Some(Arc::from(adapter)),
                    Err(error) => {
                        warn!(
                            "Embedding adapter initialization failed for '{}': {}",
                            embedding_config.model_name, error
                        );
                        None
                    }
                }
            }
        } else {
            None
        };

        let base_prompt_config = core::prompt::PromptConfig {
            max_context_chars: config.max_context_chars,
            max_diff_chars: config.max_diff_chars,
            ..Default::default()
        };

        Ok(Self {
            config,
            repo_path: repo_path.clone(),
            context_fetcher: core::ContextFetcher::new(repo_path),
            pattern_repositories,
            review_rules,
            feedback,
            feedback_context,
            plugin_manager,
            adapter,
            verification_adapter,
            embedding_adapter,
            base_prompt_config,
            convention_store_path,
            is_local,
        })
    }

    pub(super) fn repo_path_str(&self) -> String {
        self.repo_path.to_string_lossy().to_string()
    }
}

pub(super) struct ReviewSession {
    pub diffs: Vec<core::UnifiedDiff>,
    pub source_files: HashMap<PathBuf, String>,
    pub files_total: usize,
    pub on_progress: Option<ProgressCallback>,
    pub enhanced_ctx: crate::core::enhanced_review::EnhancedReviewContext,
    pub enhanced_guidance: String,
    pub auto_instructions: Option<String>,
    pub symbol_index: Option<core::SymbolIndex>,
    pub semantic_index: Option<core::semantic::SemanticIndex>,
    pub semantic_feedback_store: Option<core::SemanticFeedbackStore>,
    pub verification_context: HashMap<PathBuf, Vec<core::LLMContextChunk>>,
}

impl ReviewSession {
    pub(super) async fn new(
        diff_content: &str,
        services: &PipelineServices,
        on_progress: Option<ProgressCallback>,
    ) -> Result<Self> {
        let diffs = core::DiffParser::parse_unified_diff(diff_content)?;
        info!("Parsed {} file diffs", diffs.len());

        if let Some(limit) = services.config.file_change_limit {
            if limit > 0 && diffs.len() > limit {
                anyhow::bail!(
                    "Diff contains {} files, exceeding file_change_limit of {}. \
                     Increase the limit or split the review.",
                    diffs.len(),
                    limit
                );
            }
        }

        let source_files: HashMap<PathBuf, String> = diffs
            .iter()
            .filter_map(|diff| {
                std::fs::read_to_string(services.repo_path.join(&diff.file_path))
                    .ok()
                    .map(|content| (diff.file_path.clone(), content))
            })
            .collect();

        let git_log_output = gather_git_log(&services.repo_path);
        let convention_json = services
            .convention_store_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());

        let enhanced_ctx = core::build_enhanced_context(
            &diffs,
            &source_files,
            git_log_output.as_deref(),
            None,
            convention_json.as_deref(),
            None,
        );
        let enhanced_guidance = core::generate_enhanced_guidance(&enhanced_ctx, "rs");
        if !enhanced_guidance.is_empty() {
            info!(
                "Enhanced guidance generated ({} chars)",
                enhanced_guidance.len()
            );
        }

        let auto_instructions = if services.config.auto_detect_instructions
            && services.config.review_instructions.is_none()
        {
            let detected = detect_instruction_files(&services.repo_path);
            if detected.is_empty() {
                None
            } else {
                Some(
                    detected
                        .iter()
                        .map(|(name, content)| format!("# From {}\n{}", name, content))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                )
            }
        } else {
            None
        };

        let symbol_index = build_symbol_index(&services.config, &services.repo_path);

        let semantic_index = if services.config.semantic_rag {
            let index_path = core::default_index_path(&services.repo_path);
            let changed_files = diffs
                .iter()
                .map(|diff| diff.file_path.clone())
                .collect::<Vec<_>>();
            match core::refresh_semantic_index(
                &services.repo_path,
                &index_path,
                services.embedding_adapter.as_deref(),
                &changed_files,
                |path| services.config.should_exclude(path),
                services.config.semantic_rag_max_files,
            )
            .await
            {
                Ok(index) => Some(index),
                Err(error) => {
                    warn!("Semantic index refresh failed: {}", error);
                    None
                }
            }
        } else {
            None
        };

        let semantic_feedback_store = if services.config.semantic_feedback {
            let path = core::default_semantic_feedback_path(&services.config.feedback_path);
            let mut store = core::load_semantic_feedback_store(&path);
            core::align_semantic_feedback_store(&mut store, services.embedding_adapter.as_deref());
            Some(store)
        } else {
            None
        };

        Ok(Self {
            files_total: diffs.len(),
            diffs,
            source_files,
            on_progress,
            enhanced_ctx,
            enhanced_guidance,
            auto_instructions,
            symbol_index,
            semantic_index,
            semantic_feedback_store,
            verification_context: HashMap::new(),
        })
    }
}

pub(super) fn chunk_diff_for_context(diff_content: &str, max_chars: usize) -> Vec<String> {
    if diff_content.len() <= max_chars {
        return vec![diff_content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for section in diff_content.split("\ndiff --git ") {
        let section = if chunks.is_empty() && current_chunk.is_empty() {
            section.to_string()
        } else {
            format!("diff --git {}", section)
        };

        if current_chunk.len() + section.len() > max_chars && !current_chunk.is_empty() {
            chunks.push(current_chunk);
            current_chunk = section;
        } else {
            current_chunk.push_str(&section);
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

pub(super) fn detect_instruction_files(repo_path: &Path) -> Vec<(String, String)> {
    const INSTRUCTION_FILES: &[&str] = &[
        ".cursorrules",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "agents.md",
        ".github/copilot-instructions.md",
        "GEMINI.md",
        ".diffscope-instructions.md",
    ];
    const MAX_INSTRUCTION_SIZE: u64 = 10_000;

    let mut results = Vec::new();
    for filename in INSTRUCTION_FILES {
        let path = repo_path.join(filename);
        if path.is_file() {
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > MAX_INSTRUCTION_SIZE {
                    warn!(
                        "Skipping instruction file {} ({} bytes exceeds {})",
                        filename,
                        meta.len(),
                        MAX_INSTRUCTION_SIZE
                    );
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim().to_string();
                if !trimmed.is_empty() {
                    info!("Auto-detected instruction file: {}", filename);
                    results.push((filename.to_string(), trimmed));
                }
            }
        }
    }
    results
}

pub(super) fn should_optimize_for_local(config: &config::Config) -> bool {
    if config.context_window.is_some() {
        return true;
    }
    if config.model.starts_with("ollama:") {
        return true;
    }
    if config.adapter.as_deref() == Some("ollama") {
        return true;
    }
    config.is_local_endpoint()
}

pub(super) fn gather_git_log(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--numstat",
            "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ai%n%n    %s%n",
            "-100",
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let log_text = String::from_utf8_lossy(&out.stdout).to_string();
            if log_text.trim().is_empty() {
                None
            } else {
                info!("Gathered git log ({} bytes)", log_text.len());
                Some(log_text)
            }
        }
        _ => {
            info!("Git log unavailable (not a git repo or git not found)");
            None
        }
    }
}

pub(super) fn resolve_convention_store_path(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    dirs::data_local_dir().map(|dir| dir.join("diffscope").join("conventions.json"))
}

pub(super) fn save_convention_store(
    store: &core::convention_learner::ConventionStore,
    path: &PathBuf,
) {
    if let Ok(json) = store.to_json() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(path, json) {
            warn!(
                "Failed to save convention store to {}: {}",
                path.display(),
                error
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_instruction_files_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = detect_instruction_files(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn detect_instruction_files_finds_cursorrules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".cursorrules"), "Use tabs not spaces").unwrap();
        let results = detect_instruction_files(dir.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, ".cursorrules");
        assert!(results[0].1.contains("Use tabs"));
    }

    #[test]
    fn chunk_diff_small_diff_returns_single_chunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+hello\n";
        let chunks = chunk_diff_for_context(diff, 1000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], diff);
    }

    #[test]
    fn chunk_diff_splits_at_file_boundaries() {
        let diff = "diff --git a/a.rs b/a.rs\n+line1\n\ndiff --git a/b.rs b/b.rs\n+line2\n\ndiff --git a/c.rs b/c.rs\n+line3\n";
        let chunks = chunk_diff_for_context(diff, 40);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.contains("diff --git"));
        }
    }

    #[test]
    fn chunk_diff_empty_input() {
        let chunks = chunk_diff_for_context("", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn chunk_diff_single_large_file_not_split_midfile() {
        let diff = format!("diff --git a/big.rs b/big.rs\n{}", "+line\n".repeat(100));
        let chunks = chunk_diff_for_context(&diff, 50);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_diff_preserves_all_content() {
        let file_a = "diff --git a/a.rs b/a.rs\n+alpha\n";
        let file_b = "\ndiff --git a/b.rs b/b.rs\n+beta\n";
        let file_c = "\ndiff --git a/c.rs b/c.rs\n+gamma\n";
        let diff = format!("{}{}{}", file_a, file_b, file_c);
        let chunks = chunk_diff_for_context(&diff, 50);
        let rejoined = chunks.join("");
        assert!(rejoined.contains("+alpha"));
        assert!(rejoined.contains("+beta"));
        assert!(rejoined.contains("+gamma"));
    }
}
