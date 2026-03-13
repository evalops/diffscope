use anyhow::Result;

use super::EvalRuleThreshold;

pub(in super::super) fn parse_rule_threshold_args(
    values: &[String],
    label: &str,
) -> Result<Vec<EvalRuleThreshold>> {
    let mut parsed = Vec::new();
    for raw in values {
        let Some((rule_id, value)) = raw.split_once('=') else {
            anyhow::bail!("Invalid {} entry '{}': expected rule_id=value", label, raw);
        };
        let rule_id = rule_id.trim().to_ascii_lowercase();
        if rule_id.is_empty() {
            anyhow::bail!("Invalid {} entry '{}': empty rule id", label, raw);
        }
        let value: f32 = value
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid {} entry '{}': invalid float", label, raw))?;
        if !(0.0..=1.0).contains(&value) {
            anyhow::bail!(
                "Invalid {} entry '{}': value must be between 0.0 and 1.0",
                label,
                raw
            );
        }
        parsed.push(EvalRuleThreshold { rule_id, value });
    }
    Ok(parsed)
}
