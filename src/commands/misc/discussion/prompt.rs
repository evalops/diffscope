use anyhow::Result;

use crate::adapters;
use crate::core;

use super::types::DiscussionThread;

pub(super) async fn answer_discussion_question(
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
