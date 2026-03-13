use super::store::FeedbackStore;

/// Generate feedback context to inject into the review prompt.
///
/// Scans the feedback store for statistically significant patterns
/// and generates guidance text for the LLM reviewer.
pub fn generate_feedback_context(store: &FeedbackStore) -> String {
    let min_observations = 5;
    let mut patterns: Vec<String> = Vec::new();

    for (category, stats) in &store.by_category {
        if stats.total() < min_observations {
            continue;
        }
        let rate = stats.acceptance_rate();
        if rate >= 0.7 {
            patterns.push(format!(
                "- {} findings are usually accepted ({:.0}% acceptance rate) — be thorough on {} issues",
                category, rate * 100.0, category.to_lowercase()
            ));
        } else if rate < 0.3 {
            patterns.push(format!(
                "- {} findings are frequently rejected ({:.0}% acceptance rate) — only flag clear {} issues",
                category, rate * 100.0, category.to_lowercase()
            ));
        }
    }

    for (pattern, stats) in &store.by_file_pattern {
        if stats.total() < min_observations {
            continue;
        }
        let rate = stats.acceptance_rate();
        if rate < 0.3 {
            patterns.push(format!(
                "- Comments on {} files are usually rejected ({:.0}% acceptance rate) — be more conservative",
                pattern, rate * 100.0
            ));
        }
    }

    patterns.truncate(10);

    if patterns.is_empty() {
        return String::new();
    }

    let mut context = String::from(
        "## Learned Feedback Patterns\nBased on historical feedback from this project:\n",
    );
    for pattern in &patterns {
        context.push_str(pattern);
        context.push('\n');
    }
    context
}
