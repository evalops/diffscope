use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

use crate::adapters;
use crate::config;
use crate::core;
use crate::review;

pub async fn changelog_command(
    from: Option<String>,
    to: Option<String>,
    release: Option<String>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!("Generating changelog/release notes");

    let generator = core::ChangelogGenerator::new(".")?;

    let output = if let Some(version) = release {
        info!("Generating release notes for version {}", version);
        generator.generate_release_notes(&version, from.as_deref())?
    } else {
        let to_ref = to.as_deref().unwrap_or("HEAD");
        info!("Generating changelog from {:?} to {}", from, to_ref);
        generator.generate_changelog(from.as_deref(), to_ref)?
    };

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
        info!("Changelog written to file");
    } else {
        println!("{}", output);
    }

    Ok(())
}

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

pub async fn feedback_command(
    config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<()> {
    let (action, input_path) = match (accept, reject) {
        (Some(path), None) => ("accept", path),
        (None, Some(path)) => ("reject", path),
        _ => {
            anyhow::bail!("Specify exactly one of --accept or --reject");
        }
    };

    let feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    let content = tokio::fs::read_to_string(&input_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let mut store = review::load_feedback_store_from_path(&feedback_path);

    let updated = if action == "accept" {
        apply_feedback_accept(&mut store, &comments)
    } else {
        apply_feedback_reject(&mut store, &comments)
    };

    review::save_feedback_store(&feedback_path, &store)?;
    println!(
        "Updated feedback store at {} ({} {} comment(s))",
        feedback_path.display(),
        updated,
        action
    );

    Ok(())
}

fn apply_feedback_accept(store: &mut review::FeedbackStore, comments: &[core::Comment]) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.accept.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.accepted = stats.accepted.saturating_add(1);
        }
        store.suppress.remove(&comment.id);
    }
    updated
}

fn apply_feedback_reject(store: &mut review::FeedbackStore, comments: &[core::Comment]) -> usize {
    let mut updated = 0;
    for comment in comments {
        let is_new = store.suppress.insert(comment.id.clone());
        if is_new {
            updated += 1;
            let key = review::classify_comment_type(comment).as_str().to_string();
            let stats = store.by_comment_type.entry(key).or_default();
            stats.rejected = stats.rejected.saturating_add(1);
        }
        store.accept.remove(&comment.id);
    }
    updated
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DiscussionTurn {
    role: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DiscussionThread {
    comment_id: String,
    turns: Vec<DiscussionTurn>,
}

pub async fn discuss_command(
    config: config::Config,
    review_path: PathBuf,
    comment_id: Option<String>,
    comment_index: Option<usize>,
    question: Option<String>,
    thread_path: Option<PathBuf>,
    interactive: bool,
) -> Result<()> {
    let content = tokio::fs::read_to_string(&review_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;
    if comments.is_empty() {
        anyhow::bail!("No comments found in {}", review_path.display());
    }

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let selected = select_discussion_comment(&comments, comment_id, comment_index)?;
    let mut thread = load_discussion_thread(thread_path.as_deref(), &selected.id);

    let model_config = config.to_model_config();
    let adapter = adapters::llm::create_adapter(&model_config)?;

    let mut next_question = question;
    if next_question.is_none() && !interactive {
        anyhow::bail!("Provide --question or use --interactive");
    }

    loop {
        let current_question = if let Some(question) = next_question.take() {
            question
        } else if interactive {
            match read_follow_up_question()? {
                Some(question) => question,
                None => break,
            }
        } else {
            break;
        };

        let answer =
            answer_discussion_question(adapter.as_ref(), &selected, &thread, &current_question)
                .await?;

        println!("{}", answer.trim());

        thread.turns.push(DiscussionTurn {
            role: "user".to_string(),
            message: current_question,
        });
        thread.turns.push(DiscussionTurn {
            role: "assistant".to_string(),
            message: answer,
        });

        if let Some(path) = &thread_path {
            save_discussion_thread(path, &thread)?;
        }

        if !interactive {
            break;
        }
    }

    Ok(())
}

fn select_discussion_comment(
    comments: &[core::Comment],
    comment_id: Option<String>,
    comment_index: Option<usize>,
) -> Result<core::Comment> {
    if comment_id.is_some() && comment_index.is_some() {
        anyhow::bail!("Specify only one of --comment-id or --comment-index");
    }

    if let Some(id) = comment_id {
        let selected = comments
            .iter()
            .find(|comment| comment.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment id not found: {}", id))?;
        return Ok(selected);
    }

    if let Some(index) = comment_index {
        if index == 0 {
            anyhow::bail!("comment-index is 1-based");
        }
        let selected = comments
            .get(index - 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Comment index out of range: {}", index))?;
        return Ok(selected);
    }

    comments
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No comments available"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_discussion_comment_empty_comments() {
        // Should return an error, not panic
        let result = select_discussion_comment(&[], None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_discussion_comment_defaults_to_first() {
        let comment = core::Comment {
            id: "cmt_1".to_string(),
            file_path: PathBuf::from("test.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: core::comment::Severity::Info,
            category: core::comment::Category::BestPractice,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        };
        let result = select_discussion_comment(&[comment.clone()], None, None).unwrap();
        assert_eq!(result.id, "cmt_1");
    }

    #[test]
    fn test_feedback_stats_not_double_counted() {
        // Simulate accepting the same comment twice — stats should only increment once
        let mut store = review::FeedbackStore::default();
        let comment = core::Comment {
            id: "cmt_dup".to_string(),
            file_path: PathBuf::from("test.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: core::comment::FixEffort::Low,
            feedback: None,
        };

        let comments = vec![comment];

        // Accept the same batch of comments twice
        for _ in 0..2 {
            apply_feedback_accept(&mut store, &comments);
        }

        let key = review::classify_comment_type(&comments[0])
            .as_str()
            .to_string();
        let stats = &store.by_comment_type[&key];
        assert_eq!(
            stats.accepted, 1,
            "Stats should only count 1 acceptance, not 2 (double-counting bug)"
        );
    }
}

fn load_discussion_thread(path: Option<&std::path::Path>, comment_id: &str) -> DiscussionThread {
    let Some(path) = path else {
        return DiscussionThread {
            comment_id: comment_id.to_string(),
            turns: Vec::new(),
        };
    };

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => {
            return DiscussionThread {
                comment_id: comment_id.to_string(),
                turns: Vec::new(),
            };
        }
    };

    let parsed: DiscussionThread = serde_json::from_str(&content).unwrap_or_default();
    if parsed.comment_id == comment_id {
        parsed
    } else {
        DiscussionThread {
            comment_id: comment_id.to_string(),
            turns: Vec::new(),
        }
    }
}

fn save_discussion_thread(path: &std::path::Path, thread: &DiscussionThread) -> Result<()> {
    let content = serde_json::to_string_pretty(thread)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn read_follow_up_question() -> Result<Option<String>> {
    use std::io::Write;

    print!("question> ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("exit") {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

async fn answer_discussion_question(
    adapter: &dyn adapters::llm::LLMAdapter,
    comment: &core::Comment,
    thread: &DiscussionThread,
    question: &str,
) -> Result<String> {
    let mut history = String::new();
    for turn in thread.turns.iter().rev().take(8).rev() {
        history.push_str(&format!("{}: {}\n", turn.role, turn.message));
    }

    let mut prompt = String::new();
    prompt.push_str("Review comment context:\n");
    prompt.push_str(&format!(
        "- id: {}\n- file: {}\n- line: {}\n- severity: {}\n- category: {}\n- confidence: {:.0}%\n- comment: {}\n",
        comment.id,
        comment.file_path.display(),
        comment.line_number,
        comment.severity,
        comment.category,
        comment.confidence * 100.0,
        comment.content
    ));
    if let Some(suggestion) = &comment.suggestion {
        prompt.push_str(&format!("- suggested fix: {}\n", suggestion));
    }

    if !history.trim().is_empty() {
        prompt.push_str("\nPrevious follow-up thread:\n");
        prompt.push_str(&history);
    }

    prompt.push_str(&format!("\nNew question:\n{}\n", question));

    let request = adapters::llm::LLMRequest {
        system_prompt: "You are an expert reviewer assisting with follow-up questions on a specific code review comment. Answer directly, cite tradeoffs, and suggest concrete next steps. If the comment appears weak, say so and explain why.".to_string(),
        user_prompt: prompt,
        temperature: Some(0.2),
        max_tokens: Some(1200),
    };

    let response = adapter.complete(request).await?;
    Ok(response.content)
}
