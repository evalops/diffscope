use std::collections::{HashMap, HashSet};

use super::super::super::super::FeedbackEvalComment;
use super::super::stats::add_bucket_count;

pub(super) struct FeedbackBucketCounts {
    pub(super) category_counts: HashMap<String, (usize, usize)>,
    pub(super) severity_counts: HashMap<String, (usize, usize)>,
    pub(super) repo_counts: HashMap<String, (usize, usize)>,
    pub(super) file_pattern_counts: HashMap<String, (usize, usize)>,
}

pub(super) fn collect_feedback_bucket_counts(
    comments: &[FeedbackEvalComment],
) -> FeedbackBucketCounts {
    let mut category_counts = HashMap::new();
    let mut severity_counts = HashMap::new();
    let mut repo_counts = HashMap::new();
    let mut file_pattern_counts = HashMap::new();

    for comment in comments {
        add_bucket_count(&mut category_counts, &comment.category, comment.accepted);

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
        severity_counts,
        repo_counts,
        file_pattern_counts,
    }
}
