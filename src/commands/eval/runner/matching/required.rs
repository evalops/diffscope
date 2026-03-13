use std::collections::HashSet;

use crate::core;

use super::super::super::EvalExpectations;

pub(super) struct RequiredMatchResults {
    pub(super) failures: Vec<String>,
    pub(super) required_matches: usize,
    pub(super) used_comment_indices: HashSet<usize>,
    pub(super) matched_pairs: Vec<(usize, usize)>,
}

pub(super) fn collect_required_matches(
    expectations: &EvalExpectations,
    comments: &[core::Comment],
) -> RequiredMatchResults {
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let mut used_comment_indices = HashSet::new();
    let mut matched_pairs = Vec::new();

    for (expected_idx, expected) in expectations.must_find.iter().enumerate() {
        let found = comments
            .iter()
            .enumerate()
            .find(|(comment_idx, comment)| {
                !used_comment_indices.contains(comment_idx) && expected.matches(comment)
            })
            .map(|(comment_idx, _)| comment_idx);

        if let Some(comment_idx) = found {
            used_comment_indices.insert(comment_idx);
            matched_pairs.push((expected_idx, comment_idx));
            required_matches = required_matches.saturating_add(1);
        } else {
            failures.push(format!("Missing expected finding: {}", expected.describe()));
        }
    }

    RequiredMatchResults {
        failures,
        required_matches,
        used_comment_indices,
        matched_pairs,
    }
}
