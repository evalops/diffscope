use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::adapters;
use crate::config;
use crate::core;

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

fn load_discussion_thread(path: Option<&Path>, comment_id: &str) -> DiscussionThread {
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

fn save_discussion_thread(path: &Path, thread: &DiscussionThread) -> Result<()> {
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
        response_schema: None,
    };

    let response = adapter.complete(request).await?;
    Ok(response.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_discussion_comment_empty_comments() {
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
        let result = select_discussion_comment(std::slice::from_ref(&comment), None, None).unwrap();
        assert_eq!(result.id, "cmt_1");
    }
}
