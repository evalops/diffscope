use super::store::FeedbackStore;

const MIN_EXPLANATION_OBSERVATIONS: usize = 2;
const MAX_EXPLANATION_GUIDANCE_ITEMS: usize = 4;

#[derive(Default)]
struct ExplanationGuidanceBucket {
    accepted: usize,
    rejected: usize,
    snippets: Vec<String>,
}

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
                "- {} findings usually produce positive outcomes ({:.0}% positive reinforcement rate) — be thorough on {} issues",
                category, rate * 100.0, category.to_lowercase()
            ));
        } else if rate < 0.3 {
            patterns.push(format!(
                "- {} findings rarely produce positive outcomes ({:.0}% positive reinforcement rate) — only flag clear {} issues",
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
                "- Comments on {} files rarely produce positive outcomes ({:.0}% positive reinforcement rate) — be more conservative",
                pattern, rate * 100.0
            ));
        }
    }

    let mut explanation_buckets =
        std::collections::HashMap::<String, ExplanationGuidanceBucket>::new();
    for explanation in store.explanations_by_comment.values() {
        let bucket_key = explanation
            .rule_id
            .as_deref()
            .map(|rule_id| format!("rule::{rule_id}"))
            .unwrap_or_else(|| format!("category::{}", explanation.category));
        let bucket = explanation_buckets.entry(bucket_key).or_default();
        if explanation.action == "accept" {
            bucket.accepted += 1;
        } else {
            bucket.rejected += 1;
        }

        if let Some(snippet) = explanation_snippet(&explanation.text) {
            let already_seen = bucket
                .snippets
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&snippet));
            if !already_seen {
                bucket.snippets.push(snippet);
            }
        }
    }

    let mut explanation_guidance = explanation_buckets
        .into_iter()
        .filter_map(|(bucket_key, bucket)| {
            let total = bucket.accepted + bucket.rejected;
            if total < MIN_EXPLANATION_OBSERVATIONS
                || bucket.snippets.is_empty()
                || bucket.accepted == bucket.rejected
            {
                return None;
            }

            let snippets = bucket
                .snippets
                .into_iter()
                .take(2)
                .collect::<Vec<_>>()
                .join("; ");
            if snippets.is_empty() {
                return None;
            }

            let label = format_explanation_bucket_label(&bucket_key);
            if bucket.accepted >= bucket.rejected {
                Some((
                    total,
                    format!(
                        "- Reviewers accepted similar {label} findings when they noted: {snippets}",
                    ),
                ))
            } else {
                Some((
                    total,
                    format!(
                        "- Reviewers rejected similar {label} findings when they noted: {snippets} — avoid flagging them unless the same concern clearly applies",
                    ),
                ))
            }
        })
        .collect::<Vec<_>>();
    explanation_guidance
        .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    patterns.extend(
        explanation_guidance
            .into_iter()
            .take(MAX_EXPLANATION_GUIDANCE_ITEMS)
            .map(|(_, guidance)| guidance),
    );

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

fn explanation_snippet(text: &str) -> Option<String> {
    let snippet = text
        .replace("\r\n", "\n")
        .split(['\n', '.', '!', '?'])
        .map(str::trim)
        .find(|segment| !segment.is_empty())?
        .chars()
        .take(140)
        .collect::<String>();

    if snippet.is_empty() {
        None
    } else {
        Some(snippet)
    }
}

fn format_explanation_bucket_label(bucket_key: &str) -> String {
    if let Some(rule_id) = bucket_key.strip_prefix("rule::") {
        format!("rule `{rule_id}`")
    } else if let Some(category) = bucket_key.strip_prefix("category::") {
        match category {
            "BestPractice" => "best practice".to_string(),
            _ => category.to_lowercase(),
        }
    } else {
        "feedback".to_string()
    }
}
