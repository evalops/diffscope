//! Server-side cost estimation for review events (tokens × model price).
//! Used for wide-event cost_estimate_usd and aggregated total_cost_estimate in stats.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostBreakdownRow {
    pub workload: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cost_estimate_usd: f64,
}

impl CostBreakdownRow {
    pub fn new(
        workload: impl Into<String>,
        role: impl Into<String>,
        provider: Option<String>,
        model: impl Into<String>,
        prompt_tokens: usize,
        completion_tokens: usize,
        total_tokens: usize,
    ) -> Self {
        let model = model.into();
        Self {
            workload: workload.into(),
            role: role.into(),
            provider,
            cost_estimate_usd: estimate_cost_usd(&model, total_tokens),
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }
    }
}

pub struct CostBreakdownRequest<'a> {
    pub workload: &'a str,
    pub role: &'a str,
    pub provider: Option<String>,
    pub model: &'a str,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

pub fn aggregate_cost_breakdowns<I>(rows: I) -> Vec<CostBreakdownRow>
where
    I: IntoIterator<Item = CostBreakdownRow>,
{
    let mut aggregated =
        HashMap::<(String, String, Option<String>, String), CostBreakdownRow>::new();
    for row in rows {
        let key = (
            row.workload.clone(),
            row.role.clone(),
            row.provider.clone(),
            row.model.clone(),
        );
        let entry = aggregated.entry(key).or_insert_with(|| CostBreakdownRow {
            workload: row.workload.clone(),
            role: row.role.clone(),
            provider: row.provider.clone(),
            model: row.model.clone(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_estimate_usd: 0.0,
        });
        entry.prompt_tokens += row.prompt_tokens;
        entry.completion_tokens += row.completion_tokens;
        entry.total_tokens += row.total_tokens;
        entry.cost_estimate_usd += row.cost_estimate_usd;
    }

    let mut rows = aggregated.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .cost_estimate_usd
            .total_cmp(&left.cost_estimate_usd)
            .then_with(|| right.total_tokens.cmp(&left.total_tokens))
            .then_with(|| left.workload.cmp(&right.workload))
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.model.cmp(&right.model))
    });
    rows
}

pub fn review_cost_breakdowns(
    generation: CostBreakdownRequest<'_>,
    verification_workload: &str,
    verification_report: Option<&crate::review::verification::VerificationReport>,
) -> Vec<CostBreakdownRow> {
    let mut rows = Vec::new();
    if generation.total_tokens > 0 {
        rows.push(CostBreakdownRow::new(
            generation.workload,
            generation.role,
            generation.provider,
            generation.model,
            generation.prompt_tokens,
            generation.completion_tokens,
            generation.total_tokens,
        ));
    }

    if let Some(report) = verification_report {
        for judge in &report.judges {
            if judge.total_tokens == 0 {
                continue;
            }
            rows.push(CostBreakdownRow::new(
                verification_workload,
                judge.role.as_str(),
                judge.provider.clone(),
                judge.model.as_str(),
                judge.prompt_tokens,
                judge.completion_tokens,
                judge.total_tokens,
            ));
        }
    }

    aggregate_cost_breakdowns(rows)
}

/// Price per million tokens (input+output blended), USD. Model ID fragment -> price.
const MODEL_PRICE_PER_M: &[(&str, f64)] = &[
    ("gpt-5.4-pro", 30.0),
    ("gpt-5.4", 2.5),
    ("claude-opus-4", 5.0),
    ("claude-sonnet-4", 3.0),
    ("claude-opus", 5.0),
    ("claude-sonnet", 3.0),
    ("gemini-3.1-pro", 2.0),
    ("gpt-5.3-codex", 1.75),
    ("gpt-5.2-codex", 1.75),
    ("gpt-4o", 2.5),
    ("gpt-4-turbo", 10.0),
    ("gpt-4", 30.0),
    ("devstral", 0.4),
    ("qwen3-coder", 0.12),
    ("deepseek-v3", 0.25),
    ("llama-4-maverick", 0.15),
    ("llama-4-scout", 0.08),
    ("gemini-3-flash", 0.5),
    ("gemini-3.1-flash", 0.25),
    ("nemotron", 0.0),
];

/// Fallback when no model match: USD per million tokens (conservative).
const FALLBACK_PRICE_PER_M: f64 = 1.0;

/// Estimate cost in USD for a given model and total token count.
/// Used when building ReviewEvent so storage can aggregate total_cost_estimate.
pub fn estimate_cost_usd(model: &str, tokens_total: usize) -> f64 {
    if tokens_total == 0 {
        return 0.0;
    }
    let model_lower = model.to_lowercase();
    for (fragment, price_per_m) in MODEL_PRICE_PER_M {
        if model_lower.contains(&fragment.to_lowercase()) {
            return (tokens_total as f64 / 1_000_000.0) * price_per_m;
        }
    }
    (tokens_total as f64 / 1_000_000.0) * FALLBACK_PRICE_PER_M
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_zero_tokens() {
        assert_eq!(estimate_cost_usd("gpt-4o", 0), 0.0);
    }

    #[test]
    fn test_estimate_known_model() {
        let cost = estimate_cost_usd("claude-sonnet-4.5", 1_000_000);
        assert!(cost > 2.0 && cost < 4.0);
    }

    #[test]
    fn test_estimate_fallback() {
        let cost = estimate_cost_usd("unknown-model-xyz", 1_000_000);
        assert_eq!(cost, FALLBACK_PRICE_PER_M);
    }

    #[test]
    fn test_estimate_gpt4o_exact() {
        // 1M tokens × $2.5/M = $2.5
        assert_eq!(estimate_cost_usd("gpt-4o", 1_000_000), 2.5);
    }

    #[test]
    fn test_estimate_case_insensitive() {
        assert_eq!(estimate_cost_usd("GPT-4O", 1_000_000), 2.5);
        assert_eq!(estimate_cost_usd("Claude-Opus-4", 2_000_000), 10.0);
    }

    #[test]
    fn test_estimate_free_model() {
        assert_eq!(estimate_cost_usd("nemotron-3-nano", 1_000_000), 0.0);
    }

    #[test]
    fn test_estimate_fallback_exact() {
        let cost = estimate_cost_usd("custom-xyz", 500_000);
        assert_eq!(cost, 0.5);
    }

    #[test]
    fn aggregate_cost_breakdowns_sums_matching_rows() {
        let rows = aggregate_cost_breakdowns(vec![
            CostBreakdownRow::new(
                "review_generation",
                "primary",
                Some("anthropic".to_string()),
                "anthropic/claude-opus-4.5",
                100,
                50,
                150,
            ),
            CostBreakdownRow::new(
                "review_generation",
                "primary",
                Some("anthropic".to_string()),
                "anthropic/claude-opus-4.5",
                20,
                10,
                30,
            ),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].prompt_tokens, 120);
        assert_eq!(rows[0].completion_tokens, 60);
        assert_eq!(rows[0].total_tokens, 180);
        assert!(rows[0].cost_estimate_usd > 0.0);
    }
}
