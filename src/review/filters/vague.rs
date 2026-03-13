use tracing::info;

use crate::core;

const VAGUE_COMMENT_PREFIXES: &[&str] = &[
    "ensure",
    "verify",
    "validate",
    "consider",
    "review",
    "confirm",
    "check",
    "make sure",
];

const VAGUE_COMMENT_PHRASES: &[&str] = &[
    "ensure that",
    "verify that",
    "validate that",
    "consider adding",
    "consider using",
    "make sure",
    "double-check",
    "it may be worth",
];

pub fn is_vague_comment_text(text: &str) -> bool {
    let trimmed = text
        .trim()
        .trim_start_matches(|ch: char| ch == '-' || ch == '*' || ch == ':' || ch.is_whitespace())
        .trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if VAGUE_COMMENT_PREFIXES
        .iter()
        .any(|prefix| lower == *prefix || lower.starts_with(&format!("{} ", prefix)))
    {
        return true;
    }

    VAGUE_COMMENT_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

pub fn is_vague_review_comment(comment: &core::Comment) -> bool {
    is_vague_comment_text(&comment.content)
}

pub fn apply_vague_comment_filter(comments: Vec<core::Comment>) -> Vec<core::Comment> {
    let total = comments.len();
    let kept: Vec<_> = comments
        .into_iter()
        .filter(|comment| !is_vague_review_comment(comment))
        .collect();

    if kept.len() != total {
        info!(
            "Dropped {} vague review comment(s) after generation",
            total.saturating_sub(kept.len())
        );
    }

    kept
}
