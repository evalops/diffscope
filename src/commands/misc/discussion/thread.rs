use anyhow::Result;
use std::path::Path;

use super::types::DiscussionThread;

pub(super) fn load_discussion_thread(path: Option<&Path>, comment_id: &str) -> DiscussionThread {
    let Some(path) = path else {
        return empty_discussion_thread(comment_id);
    };

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return empty_discussion_thread(comment_id),
    };

    let parsed: DiscussionThread = serde_json::from_str(&content).unwrap_or_default();
    if parsed.comment_id == comment_id {
        parsed
    } else {
        empty_discussion_thread(comment_id)
    }
}

pub(super) fn save_discussion_thread(path: &Path, thread: &DiscussionThread) -> Result<()> {
    let content = serde_json::to_string_pretty(thread)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub(super) fn read_follow_up_question() -> Result<Option<String>> {
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

fn empty_discussion_thread(comment_id: &str) -> DiscussionThread {
    DiscussionThread {
        comment_id: comment_id.to_string(),
        turns: Vec::new(),
    }
}
