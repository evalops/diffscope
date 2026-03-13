use tracing::info;

use crate::core;

pub(super) fn apply_convention_suppression(
    comments: Vec<core::Comment>,
    convention_store: &core::convention_learner::ConventionStore,
) -> (Vec<core::Comment>, usize) {
    let suppression_patterns = convention_store.suppression_patterns();
    if suppression_patterns.is_empty() {
        return (comments, 0);
    }

    let before_count = comments.len();
    let filtered: Vec<core::Comment> = comments
        .into_iter()
        .filter(|comment| {
            let category_str = comment.category.to_string();
            let score = convention_store.score_comment(&comment.content, &category_str);
            score > -0.25
        })
        .collect();

    let suppressed = before_count.saturating_sub(filtered.len());
    if suppressed > 0 {
        info!(
            "Convention learning suppressed {} comment(s) based on team feedback patterns",
            suppressed
        );
    }

    (filtered, suppressed)
}
