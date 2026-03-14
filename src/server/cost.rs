//! Server-side cost estimation for review events (tokens × model price).
//! Used for wide-event cost_estimate_usd and aggregated total_cost_estimate in stats.

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
}
