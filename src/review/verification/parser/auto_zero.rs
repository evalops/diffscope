use crate::core::Comment;

use super::super::VerificationResult;

const AUTO_ZERO_PATTERNS: &[&str] = &[
    "docstring",
    "doc comment",
    "documentation comment",
    "type hint",
    "type annotation",
    "import order",
    "import sorting",
    "unused import",
    "trailing whitespace",
    "trailing newline",
];

pub fn is_auto_zero(content: &str) -> bool {
    let lower = content.to_lowercase();
    AUTO_ZERO_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

pub(super) fn apply_auto_zero(
    mut results: Vec<VerificationResult>,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    for comment in comments {
        if !is_auto_zero(&comment.content) {
            continue;
        }

        if let Some(existing) = results
            .iter_mut()
            .find(|result| result.comment_id == comment.id)
        {
            existing.accurate = false;
            existing.line_correct = false;
            existing.score = 0;
            existing.reason = "Auto-zero: noise category".to_string();
        } else {
            results.push(auto_zero_result(comment));
        }
    }

    results
}

fn auto_zero_result(comment: &Comment) -> VerificationResult {
    VerificationResult {
        comment_id: comment.id.clone(),
        accurate: false,
        line_correct: false,
        suggestion_sound: false,
        score: 0,
        reason: "Auto-zero: noise category".to_string(),
    }
}
