use std::collections::HashSet;

use tracing::info;

use crate::core;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewCommentType {
    Logic,
    Syntax,
    Style,
    Informational,
}

impl ReviewCommentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logic => "logic",
            Self::Syntax => "syntax",
            Self::Style => "style",
            Self::Informational => "informational",
        }
    }
}

pub fn classify_comment_type(comment: &core::Comment) -> ReviewCommentType {
    if matches!(comment.category, core::comment::Category::Style) {
        return ReviewCommentType::Style;
    }

    if matches!(
        comment.category,
        core::comment::Category::Documentation | core::comment::Category::BestPractice
    ) {
        return ReviewCommentType::Informational;
    }

    let content = comment.content.to_lowercase();
    if content.contains("syntax")
        || content.contains("parse error")
        || content.contains("compilation")
        || content.contains("compile")
        || content.contains("token")
    {
        return ReviewCommentType::Syntax;
    }

    ReviewCommentType::Logic
}

pub fn apply_comment_type_filter(
    comments: Vec<core::Comment>,
    enabled_types: &[String],
) -> Vec<core::Comment> {
    if enabled_types.is_empty() {
        return comments;
    }

    let enabled: HashSet<&str> = enabled_types.iter().map(String::as_str).collect();
    let total = comments.len();
    let mut kept = Vec::with_capacity(total);

    for comment in comments {
        let comment_type = classify_comment_type(&comment);
        if enabled.contains(comment_type.as_str()) {
            kept.push(comment);
        }
    }

    if kept.len() != total {
        let dropped = total.saturating_sub(kept.len());
        info!(
            "Dropped {} comment(s) due to comment type filters [{}]",
            dropped,
            enabled_types.join(", ")
        );
    }

    kept
}
