use std::collections::HashSet;

use crate::core;

use super::super::super::pattern::summarize_for_eval;
use super::super::super::EvalExpectations;

pub(super) struct UnexpectedMatchResults {
    pub(super) failures: Vec<String>,
    pub(super) unexpected_comment_indices: HashSet<usize>,
}

pub(super) fn collect_unexpected_matches(
    expectations: &EvalExpectations,
    comments: &[core::Comment],
) -> UnexpectedMatchResults {
    let mut failures = Vec::new();
    let mut unexpected_comment_indices = HashSet::new();

    for unexpected in &expectations.must_not_find {
        if let Some((comment_idx, comment)) = comments
            .iter()
            .enumerate()
            .find(|(_, comment)| unexpected.matches(comment))
        {
            unexpected_comment_indices.insert(comment_idx);
            failures.push(format!(
                "Unexpected finding matched {}:{} '{}'",
                comment.file_path.display(),
                comment.line_number,
                summarize_for_eval(&comment.content)
            ));
        }
    }

    UnexpectedMatchResults {
        failures,
        unexpected_comment_indices,
    }
}
