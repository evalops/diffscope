use super::super::EvalFixtureResult;

const LIFECYCLE_RULE_PREFIX: &str = "bug.lifecycle.";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in super::super) struct EvalPassRate {
    pub(in super::super) passed: usize,
    pub(in super::super) total: usize,
    pub(in super::super) rate: f32,
}

pub(in super::super) fn build_lifecycle_accuracy(
    results: &[EvalFixtureResult],
) -> Option<EvalPassRate> {
    let total = results
        .iter()
        .filter(|result| is_lifecycle_fixture(result))
        .count();
    if total == 0 {
        return None;
    }

    let passed = results
        .iter()
        .filter(|result| is_lifecycle_fixture(result) && result.passed)
        .count();

    Some(EvalPassRate {
        passed,
        total,
        rate: passed as f32 / total as f32,
    })
}

fn is_lifecycle_fixture(result: &EvalFixtureResult) -> bool {
    result
        .rule_metrics
        .iter()
        .any(|metric| metric.expected > 0 && metric.rule_id.starts_with(LIFECYCLE_RULE_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::eval::EvalRuleMetrics;

    fn fixture_result(passed: bool, rule_id: &str) -> EvalFixtureResult {
        EvalFixtureResult {
            passed,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: rule_id.to_string(),
                expected: 1,
                predicted: 1,
                true_positives: usize::from(passed),
                false_positives: usize::from(!passed),
                false_negatives: usize::from(!passed),
                precision: if passed { 1.0 } else { 0.0 },
                recall: if passed { 1.0 } else { 0.0 },
                f1: if passed { 1.0 } else { 0.0 },
            }],
            ..Default::default()
        }
    }

    #[test]
    fn build_lifecycle_accuracy_aggregates_lifecycle_fixture_pass_rate() {
        let accuracy = build_lifecycle_accuracy(&[
            fixture_result(true, "bug.lifecycle.context-only-addressed"),
            fixture_result(false, "bug.lifecycle.api-drops-followup-addressed"),
            fixture_result(true, "bug.readiness.current-head-staleness"),
        ])
        .unwrap();

        assert_eq!(accuracy.passed, 1);
        assert_eq!(accuracy.total, 2);
        assert!((accuracy.rate - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn build_lifecycle_accuracy_returns_none_without_lifecycle_rules() {
        assert!(build_lifecycle_accuracy(&[fixture_result(
            true,
            "bug.readiness.current-head-staleness"
        )])
        .is_none());
    }
}
