use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::core::convention_learner::ConventionStore;
use crate::plugins;

use super::super::feedback::derive_file_patterns;
use super::FileMetric;

pub fn extract_symbols_from_diff(diff: &core::UnifiedDiff) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut seen = std::collections::HashSet::new();
    static SYMBOL_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\b([A-Z][a-zA-Z0-9_]*|[a-z][a-zA-Z0-9_]*)\s*\(").unwrap());
    static CLASS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\b(class|struct|interface|enum)\s+([A-Z][a-zA-Z0-9_]*)").unwrap()
    });

    for hunk in &diff.hunks {
        for line in &hunk.changes {
            if matches!(
                line.change_type,
                core::diff_parser::ChangeType::Added | core::diff_parser::ChangeType::Removed
            ) {
                // Extract function calls and references
                for capture in SYMBOL_REGEX.captures_iter(&line.content) {
                    if let Some(symbol) = capture.get(1) {
                        let symbol_str = symbol.as_str().to_string();
                        if symbol_str.len() > 2 && seen.insert(symbol_str.clone()) {
                            symbols.push(symbol_str);
                        }
                    }
                }

                // Also look for class/struct references
                for capture in CLASS_REGEX.captures_iter(&line.content) {
                    if let Some(class_name) = capture.get(2) {
                        let class_str = class_name.as_str().to_string();
                        if seen.insert(class_str.clone()) {
                            symbols.push(class_str);
                        }
                    }
                }
            }
        }
    }

    symbols
}

/// Deduplicate comments that appear in multiple specialized passes.
/// When multi-pass review is enabled, the same issue may be flagged by both
/// the security and correctness passes. We merge near-identical comments,
/// keeping the one with the highest confidence and combining their tags.
pub(super) fn deduplicate_specialized_comments(
    mut comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    if comments.len() <= 1 {
        return comments;
    }
    // Sort by file_path then line_number for stable dedup
    comments.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });

    let mut deduped: Vec<core::Comment> = Vec::with_capacity(comments.len());
    for comment in comments {
        let dominated = deduped.iter_mut().find(|existing| {
            existing.file_path == comment.file_path
                && existing.line_number == comment.line_number
                && core::multi_pass::content_similarity(&existing.content, &comment.content) > 0.6
        });
        if let Some(existing) = dominated {
            // Merge: keep higher confidence, combine tags
            if comment.confidence > existing.confidence {
                existing.content = comment.content;
                existing.confidence = comment.confidence;
                existing.severity = comment.severity;
            }
            for tag in &comment.tags {
                if !existing.tags.contains(tag) {
                    existing.tags.push(tag.clone());
                }
            }
        } else {
            deduped.push(comment);
        }
    }
    deduped
}

pub fn filter_comments_for_diff(
    diff: &core::UnifiedDiff,
    comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    let mut filtered = Vec::new();
    let total = comments.len();
    for comment in comments {
        if is_line_in_diff(diff, comment.line_number) {
            filtered.push(comment);
        }
    }

    if filtered.len() != total {
        let dropped = total.saturating_sub(filtered.len());
        info!(
            "Dropped {} comment(s) for {} due to unmatched line numbers",
            dropped,
            diff.file_path.display()
        );
    }

    filtered
}

pub(super) fn synthesize_analyzer_comments(
    findings: Vec<plugins::AnalyzerFinding>,
) -> Result<Vec<core::Comment>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }

    let raw_comments = findings
        .into_iter()
        .map(|finding| finding.into_raw_comment())
        .collect::<Vec<_>>();
    core::CommentSynthesizer::synthesize(raw_comments)
}

pub(super) fn is_analyzer_comment(comment: &core::Comment) -> bool {
    comment.tags.iter().any(|tag| tag.starts_with("source:"))
}

pub(super) async fn apply_semantic_feedback_adjustment(
    comments: Vec<core::Comment>,
    store: Option<&core::SemanticFeedbackStore>,
    embedding_adapter: Option<&dyn adapters::llm::LLMAdapter>,
    config: &config::Config,
) -> Vec<core::Comment> {
    let Some(store) = store else {
        return comments;
    };
    if store.examples.len() < config.semantic_feedback_min_examples {
        return comments;
    }

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(embedding_adapter, &embedding_texts).await;

    comments
        .into_iter()
        .zip(embeddings)
        .map(|(mut comment, embedding)| {
            if is_analyzer_comment(&comment) {
                return comment;
            }

            let file_patterns = derive_file_patterns(&comment.file_path);
            let matches = core::find_similar_feedback_examples(
                store,
                &embedding,
                comment.category.as_str(),
                &file_patterns,
                config.semantic_feedback_similarity,
                config.semantic_feedback_max_neighbors,
            );
            let accepted = matches
                .iter()
                .filter(|(example, _)| example.accepted)
                .count();
            let rejected = matches
                .iter()
                .filter(|(example, _)| !example.accepted)
                .count();
            let observations = accepted + rejected;

            if observations < config.semantic_feedback_min_examples {
                return comment;
            }

            if rejected > accepted {
                let delta = ((rejected - accepted) as f32 * 0.15).min(0.45);
                comment.confidence = (comment.confidence - delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:rejected")
                {
                    comment.tags.push("semantic-feedback:rejected".to_string());
                }
            } else if accepted > rejected {
                let delta = ((accepted - rejected) as f32 * 0.10).min(0.25);
                comment.confidence = (comment.confidence + delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:accepted")
                {
                    comment.tags.push("semantic-feedback:accepted".to_string());
                }
            }

            comment
        })
        .collect()
}

pub fn is_line_in_diff(diff: &core::UnifiedDiff, line_number: usize) -> bool {
    if line_number == 0 {
        return false;
    }
    diff.hunks.iter().any(|hunk| {
        hunk.changes
            .iter()
            .any(|line| line.new_line_no == Some(line_number))
    })
}

pub fn build_review_guidance(
    config: &config::Config,
    path_config: Option<&config::PathConfig>,
) -> Option<String> {
    let mut sections = Vec::new();

    let strictness_guidance = match config.strictness {
        1 => "Prefer high-signal findings only. Avoid low-impact nitpicks and optional suggestions.",
        3 => {
            "Be exhaustive. Surface meaningful edge cases and maintainability concerns, including lower-severity findings."
        }
        _ => "Balance precision and coverage; prioritize clear, actionable findings.",
    };
    sections.push(format!(
        "Strictness ({}): {}",
        config.strictness, strictness_guidance
    ));
    if !config.comment_types.is_empty() {
        sections.push(format!(
            "Enabled comment types: {}. Do not emit findings outside these types.",
            config.comment_types.join(", ")
        ));
    }

    if let Some(profile) = config.review_profile.as_deref() {
        let guidance = match profile {
            "chill" => Some(
                "Be conservative and only surface high-confidence, high-impact issues. Avoid nitpicks and redundant comments.",
            ),
            "assertive" => Some(
                "Be thorough and proactive. Surface edge cases, latent risks, and maintainability concerns even if they are subtle.",
            ),
            _ => None,
        };
        if let Some(text) = guidance {
            sections.push(format!("Review profile ({}): {}", profile, text));
        }
    }

    if let Some(instructions) = config.review_instructions.as_deref() {
        let trimmed = instructions.trim();
        if !trimmed.is_empty() {
            sections.push(format!("Global review instructions:\n{}", trimmed));
        }
    }

    if let Some(pc) = path_config {
        if let Some(instructions) = pc.review_instructions.as_deref() {
            let trimmed = instructions.trim();
            if !trimmed.is_empty() {
                sections.push(format!("Path-specific instructions:\n{}", trimmed));
            }
        }
    }

    // Output language directive
    if let Some(ref lang) = config.output_language {
        if lang != "en" && !lang.starts_with("en-") {
            sections.push(format!(
                "Write all review comments and suggestions in {}.",
                lang
            ));
        }
    }

    // Fix suggestions toggle
    if !config.include_fix_suggestions {
        sections.push("Do not include code fix suggestions. Only describe the issue. Do not include <<<ORIGINAL/>>>SUGGESTED blocks.".to_string());
    } else {
        sections.push(
            "For every finding where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:\n\n<<<ORIGINAL\n<the problematic code>\n===\n<the fixed code>\n>>>SUGGESTED\n\nAlways copy the original code verbatim from the diff. Only omit the block when no concrete fix can be expressed in code.".to_string(),
        );
    }

    if sections.is_empty() {
        None
    } else {
        Some(format!(
            "Additional review guidance:\n{}",
            sections.join("\n\n")
        ))
    }
}

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

/// Split a large diff into chunks that fit within context budget.
/// Each chunk gets its own LLM call, results are merged.
pub(super) fn chunk_diff_for_context(diff_content: &str, max_chars: usize) -> Vec<String> {
    if diff_content.len() <= max_chars {
        return vec![diff_content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    // Split by file boundaries (diff --git)
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

/// Validate LLM response quality for common local model issues.
pub(super) fn validate_llm_response(response: &str) -> Result<(), String> {
    let trimmed = response.trim();

    // Empty response
    if trimmed.is_empty() {
        return Err("Empty response from model".to_string());
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if is_structured_review_payload(&value) {
            return Ok(());
        }

        return Err("JSON response did not match the review output contract".to_string());
    }

    // Response too short to contain valid review
    if response.len() < 10 {
        return Err("Response too short to contain valid review".to_string());
    }

    // Repeated token detection (common with small models)
    if has_excessive_repetition(response) {
        return Err("Response contains excessive repetition (model may be stuck)".to_string());
    }

    Ok(())
}

fn is_structured_review_payload(value: &serde_json::Value) -> bool {
    let items = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value
        .get("comments")
        .or_else(|| value.get("findings"))
        .or_else(|| value.get("results"))
        .and_then(|items| items.as_array())
    {
        array
    } else {
        return false;
    };

    items.iter().all(|item| {
        item.is_object()
            && (item.get("line").is_some()
                || item.get("line_number").is_some()
                || item.get("content").is_some()
                || item.get("issue").is_some())
    })
}

pub(super) fn has_excessive_repetition(text: &str) -> bool {
    // Check if any 20-char substring repeats more than 5 times
    if text.len() < 100 {
        return false;
    }
    let window = 20.min(text.len() / 5);
    let search_end = text.len().saturating_sub(window);
    for start in 0..search_end.max(1) {
        if !text.is_char_boundary(start) || !text.is_char_boundary(start + window) {
            continue;
        }
        let pattern = &text[start..start + window];
        if pattern.trim().is_empty() {
            continue;
        }
        let count = text.matches(pattern).count();
        if count > 5 {
            return true;
        }
    }
    false
}

pub(super) fn review_comments_response_schema() -> adapters::llm::StructuredOutputSchema {
    adapters::llm::StructuredOutputSchema::json_schema(
        "review_findings",
        serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["line", "content", "severity", "category", "confidence", "fix_effort", "tags"],
                "properties": {
                    "line": {"type": "integer", "minimum": 1},
                    "content": {"type": "string"},
                    "severity": {"type": "string", "enum": ["error", "warning", "info", "suggestion"]},
                    "category": {"type": "string", "enum": ["bug", "security", "performance", "style", "best_practice"]},
                    "confidence": {"type": ["number", "string"]},
                    "fix_effort": {"type": "string", "enum": ["low", "medium", "high"]},
                    "rule_id": {"type": ["string", "null"]},
                    "suggestion": {"type": ["string", "null"]},
                    "code_suggestion": {"type": ["string", "null"]},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                }
            }
        }),
    )
}

pub(super) fn merge_file_metric(
    file_metrics: &mut Vec<FileMetric>,
    file_path: &Path,
    latency_ms: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
    comment_count: usize,
) {
    if let Some(existing) = file_metrics
        .iter_mut()
        .find(|metric| metric.file_path == file_path)
    {
        existing.prompt_tokens += prompt_tokens;
        existing.completion_tokens += completion_tokens;
        existing.total_tokens += total_tokens;
        existing.comment_count += comment_count;
        if latency_ms > existing.latency_ms {
            existing.latency_ms = latency_ms;
        }
        return;
    }

    file_metrics.push(FileMetric {
        file_path: file_path.to_path_buf(),
        latency_ms,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        comment_count,
    });
}

/// Auto-detect instruction files commonly used by AI coding tools.
/// Returns the concatenated contents of any found files (.cursorrules, CLAUDE.md, etc.)
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
            // Skip files larger than 10KB
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
    // Optimize if context_window is explicitly set
    if config.context_window.is_some() {
        return true;
    }
    // Optimize for ollama: prefix models
    if config.model.starts_with("ollama:") {
        return true;
    }
    // Optimize if adapter is explicitly set to ollama
    if config.adapter.as_deref() == Some("ollama") {
        return true;
    }
    // Optimize if base_url points to localhost
    config.is_local_endpoint()
}

/// Run `git log --numstat` against repo_path to gather commit history.
/// Returns None if the command fails (e.g., not a git repo).
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

/// Resolve the convention store path from config or default location.
pub(super) fn resolve_convention_store_path(config: &config::Config) -> Option<PathBuf> {
    if let Some(ref path) = config.convention_store_path {
        return Some(PathBuf::from(path));
    }
    // Default: ~/.local/share/diffscope/conventions.json
    dirs::data_local_dir().map(|d| d.join("diffscope").join("conventions.json"))
}

/// Save the convention store to the given path, creating directories if needed.
pub(super) fn save_convention_store(store: &ConventionStore, path: &PathBuf) {
    if let Ok(json) = store.to_json() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(path, json) {
            warn!(
                "Failed to save convention store to {}: {}",
                path.display(),
                e
            );
        }
    }
}

/// Gather related file context: reverse dependencies (callers) and test files.
/// These are included as Reference context chunks for the LLM.
pub(super) fn gather_related_file_context(
    index: &core::SymbolIndex,
    file_path: &Path,
    repo_path: &Path,
) -> Vec<core::LLMContextChunk> {
    let mut chunks: Vec<core::LLMContextChunk> = Vec::new();

    // 1. Reverse dependencies (files that import/depend on this file)
    let callers = index.reverse_deps(file_path);
    for caller_path in callers.iter().take(3) {
        if let Some(summary) = index.file_summary(caller_path) {
            let truncated: String = if summary.len() > 2000 {
                let mut end = 2000;
                while end > 0 && !summary.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &summary[..end])
            } else {
                summary.to_string()
            };
            chunks.push(
                core::LLMContextChunk::reference(
                    caller_path.clone(),
                    format!("[Caller/dependent file]\n{}", truncated),
                )
                .with_provenance(core::ContextProvenance::ReverseDependencySummary),
            );
        }
    }

    // 2. Look for matching test files
    let test_files = find_test_files(file_path, repo_path);
    for test_path in test_files.iter().take(2) {
        let relative: &Path = test_path.strip_prefix(repo_path).unwrap_or(test_path);
        // Read first 60 lines of the test file for context
        if let Ok(content) = std::fs::read_to_string(test_path) {
            let snippet: String = content.lines().take(60).collect::<Vec<_>>().join("\n");
            if !snippet.is_empty() {
                chunks.push(
                    core::LLMContextChunk::reference(
                        relative.to_path_buf(),
                        format!("[Test file]\n{}", snippet),
                    )
                    .with_provenance(core::ContextProvenance::RelatedTestFile),
                );
            }
        }
    }

    chunks
}

/// Find test files that correspond to a given source file.
/// Looks for patterns like test_<stem>, <stem>_test, <stem>.test, tests/<stem>.
fn find_test_files(file_path: &Path, repo_path: &Path) -> Vec<PathBuf> {
    let stem = match file_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = file_path.parent().unwrap_or(Path::new(""));

    let candidates: Vec<PathBuf> = vec![
        repo_path
            .join(parent)
            .join(format!("test_{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}_test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.test.{}", stem, ext)),
        repo_path
            .join(parent)
            .join(format!("{}.spec.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("tests")
            .join(format!("{}.{}", stem, ext)),
        repo_path
            .join(parent)
            .join("__tests__")
            .join(format!("{}.{}", stem, ext)),
    ];

    candidates
        .into_iter()
        .filter(|p: &PathBuf| p.is_file())
        .collect()
}

/// Apply convention-based suppression: filter out comments whose content
/// matches learned suppression patterns with high confidence.
/// Returns the filtered comments and the number of comments that were suppressed.
pub(super) fn apply_convention_suppression(
    comments: Vec<core::Comment>,
    convention_store: &ConventionStore,
) -> (Vec<core::Comment>, usize) {
    let suppression_patterns = convention_store.suppression_patterns();
    if suppression_patterns.is_empty() {
        return (comments, 0);
    }

    let before_count = comments.len();
    let filtered: Vec<core::Comment> = comments
        .into_iter()
        .filter(|comment| {
            let category_str = comment.category.to_string();
            let score = convention_store.score_comment(&comment.content, &category_str);
            // Only suppress if the convention store strongly indicates suppression
            // (score <= -0.25 means the team has consistently rejected similar comments)
            score > -0.25
        })
        .collect();

    let suppressed = before_count.saturating_sub(filtered.len());
    if suppressed > 0 {
        info!(
            "Convention learning suppressed {} comment(s) based on team feedback patterns",
            suppressed
        );
    }

    (filtered, suppressed)
}
