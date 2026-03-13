use std::collections::{HashMap, HashSet};

use super::super::super::super::FeedbackEvalComment;
use super::super::stats::add_bucket_count;

pub(super) struct FeedbackBucketCounts {
    pub(super) category_counts: HashMap<String, (usize, usize)>,
    pub(super) rule_counts: HashMap<String, (usize, usize)>,
    pub(super) severity_counts: HashMap<String, (usize, usize)>,
    pub(super) repo_counts: HashMap<String, (usize, usize)>,
    pub(super) file_pattern_counts: HashMap<String, (usize, usize)>,
}

pub(super) fn collect_feedback_bucket_counts(
    comments: &[FeedbackEvalComment],
) -> FeedbackBucketCounts {
    collect_feedback_bucket_counts_with_predicate(comments, |_| true)
}

pub(super) fn collect_high_confidence_bucket_counts(
    comments: &[FeedbackEvalComment],
    confidence_threshold: f32,
) -> FeedbackBucketCounts {
    collect_feedback_bucket_counts_with_predicate(comments, |comment| {
        comment
            .confidence
            .map(|confidence| confidence >= confidence_threshold)
            .unwrap_or(false)
    })
}

fn collect_feedback_bucket_counts_with_predicate<F>(
    comments: &[FeedbackEvalComment],
    predicate: F,
) -> FeedbackBucketCounts
where
    F: Fn(&FeedbackEvalComment) -> bool,
{
    let mut category_counts = HashMap::new();
    let mut rule_counts = HashMap::new();
    let mut severity_counts = HashMap::new();
    let mut repo_counts = HashMap::new();
    let mut file_pattern_counts = HashMap::new();

    for comment in comments {
        if !predicate(comment) {
            continue;
        }

        add_bucket_count(&mut category_counts, &comment.category, comment.accepted);
        if let Some(rule_id) = comment.rule_id.as_deref() {
            add_bucket_count(&mut rule_counts, rule_id, comment.accepted);
        }

        let severity = comment.severity.as_deref().unwrap_or("unknown");
        add_bucket_count(&mut severity_counts, severity, comment.accepted);

        if let Some(repo) = comment.repo.as_deref() {
            add_bucket_count(&mut repo_counts, repo, comment.accepted);
        }

        let unique_patterns = comment
            .file_patterns
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for pattern in unique_patterns {
            add_bucket_count(&mut file_pattern_counts, pattern, comment.accepted);
        }
    }

    FeedbackBucketCounts {
        category_counts,
        rule_counts,
        severity_counts,
        repo_counts,
        file_pattern_counts,
    }
}
