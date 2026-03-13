use std::cmp::Ordering;

use super::super::super::FeedbackEvalExample;

const MAX_EXAMPLES: usize = 10;

pub(super) fn rank_feedback_examples(examples: &mut Vec<FeedbackEvalExample>) {
    examples.sort_by(compare_feedback_examples);
    examples.truncate(MAX_EXAMPLES);
}

fn compare_feedback_examples(left: &FeedbackEvalExample, right: &FeedbackEvalExample) -> Ordering {
    right
        .confidence
        .partial_cmp(&left.confidence)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            severity_rank(right.severity.as_deref()).cmp(&severity_rank(left.severity.as_deref()))
        })
        .then_with(|| left.content.cmp(&right.content))
}

fn severity_rank(severity: Option<&str>) -> usize {
    match severity.map(|value| value.to_ascii_lowercase()) {
        Some(value) if value == "error" => 3,
        Some(value) if value == "warning" => 2,
        Some(value) if value == "info" => 1,
        _ => 0,
    }
}
