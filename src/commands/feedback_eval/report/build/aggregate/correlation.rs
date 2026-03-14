use std::collections::HashMap;

use crate::commands::eval::EvalReport;

use super::super::super::super::{
    FeedbackEvalBucket, FeedbackEvalCategoryCorrelation, FeedbackEvalCorrelationReport,
    FeedbackEvalRuleCorrelation,
};

const ATTENTION_GAP_THRESHOLD: f32 = 0.15;
const ATTENTION_ACCEPTANCE_THRESHOLD: f32 = 0.5;

pub(super) fn build_feedback_eval_correlation(
    eval_report: Option<&EvalReport>,
    by_category: &[FeedbackEvalBucket],
    high_confidence_by_category: &[FeedbackEvalBucket],
    by_rule: &[FeedbackEvalBucket],
    high_confidence_by_rule: &[FeedbackEvalBucket],
) -> Option<FeedbackEvalCorrelationReport> {
    let eval_report = eval_report?;
    let high_confidence_categories = bucket_map(high_confidence_by_category);
    let high_confidence_rules = bucket_map(high_confidence_by_rule);
    let eval_categories = eval_report
        .benchmark_by_category
        .iter()
        .map(|(name, metrics)| (normalize_key(name), metrics))
        .collect::<HashMap<_, _>>();
    let eval_rules = eval_report
        .rule_metrics
        .iter()
        .map(|metric| (normalize_key(&metric.rule_id), metric))
        .collect::<HashMap<_, _>>();

    let by_category = by_category
        .iter()
        .map(|bucket| {
            let high_confidence = high_confidence_categories.get(&normalize_key(&bucket.name));
            let eval_metrics = eval_categories.get(&normalize_key(&bucket.name));
            let eval_micro_f1 = eval_metrics.map(|metrics| metrics.micro_f1);
            let high_confidence_acceptance_rate = high_confidence
                .map(|bucket| bucket.acceptance_rate)
                .unwrap_or(0.0);
            FeedbackEvalCategoryCorrelation {
                name: bucket.name.clone(),
                feedback_total: bucket.total,
                feedback_acceptance_rate: bucket.acceptance_rate,
                high_confidence_total: high_confidence.map(|bucket| bucket.total).unwrap_or(0),
                high_confidence_acceptance_rate,
                eval_fixture_count: eval_metrics.map(|metrics| metrics.fixture_count),
                eval_micro_f1,
                eval_weighted_score: eval_metrics.map(|metrics| metrics.weighted_score),
                feedback_vs_eval_gap: metric_gap(eval_micro_f1, bucket.acceptance_rate),
                high_confidence_vs_eval_gap: metric_gap(
                    eval_micro_f1,
                    high_confidence_acceptance_rate,
                ),
            }
        })
        .collect::<Vec<_>>();
    let by_rule = by_rule
        .iter()
        .map(|bucket| {
            let high_confidence = high_confidence_rules.get(&normalize_key(&bucket.name));
            let eval_metric = eval_rules.get(&normalize_key(&bucket.name));
            let eval_f1 = eval_metric.map(|metric| metric.f1);
            let high_confidence_acceptance_rate = high_confidence
                .map(|bucket| bucket.acceptance_rate)
                .unwrap_or(0.0);
            FeedbackEvalRuleCorrelation {
                rule_id: bucket.name.clone(),
                feedback_total: bucket.total,
                feedback_acceptance_rate: bucket.acceptance_rate,
                high_confidence_total: high_confidence.map(|bucket| bucket.total).unwrap_or(0),
                high_confidence_acceptance_rate,
                eval_precision: eval_metric.map(|metric| metric.precision),
                eval_recall: eval_metric.map(|metric| metric.recall),
                eval_f1,
                feedback_vs_eval_gap: metric_gap(eval_f1, bucket.acceptance_rate),
                high_confidence_vs_eval_gap: metric_gap(eval_f1, high_confidence_acceptance_rate),
            }
        })
        .collect::<Vec<_>>();

    Some(FeedbackEvalCorrelationReport {
        attention_by_category: build_attention_categories(&by_category),
        attention_by_rule: build_attention_rules(&by_rule),
        by_category,
        by_rule,
    })
}

fn bucket_map(buckets: &[FeedbackEvalBucket]) -> HashMap<String, &FeedbackEvalBucket> {
    buckets
        .iter()
        .map(|bucket| (normalize_key(&bucket.name), bucket))
        .collect()
}

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn metric_gap(eval_metric: Option<f32>, acceptance_rate: f32) -> Option<f32> {
    eval_metric.map(|metric| (metric - acceptance_rate).max(0.0))
}

fn build_attention_categories(
    categories: &[FeedbackEvalCategoryCorrelation],
) -> Vec<FeedbackEvalCategoryCorrelation> {
    let mut attention = categories
        .iter()
        .filter(|category| {
            category.high_confidence_total > 0
                && category.high_confidence_acceptance_rate <= ATTENTION_ACCEPTANCE_THRESHOLD
                && category
                    .high_confidence_vs_eval_gap
                    .is_some_and(|gap| gap >= ATTENTION_GAP_THRESHOLD)
        })
        .cloned()
        .collect::<Vec<_>>();
    attention.sort_by(|left, right| {
        right
            .high_confidence_vs_eval_gap
            .unwrap_or_default()
            .total_cmp(&left.high_confidence_vs_eval_gap.unwrap_or_default())
            .then_with(|| right.high_confidence_total.cmp(&left.high_confidence_total))
            .then_with(|| right.feedback_total.cmp(&left.feedback_total))
    });
    attention
}

fn build_attention_rules(
    rules: &[FeedbackEvalRuleCorrelation],
) -> Vec<FeedbackEvalRuleCorrelation> {
    let mut attention = rules
        .iter()
        .filter(|rule| {
            rule.high_confidence_total > 0
                && rule.high_confidence_acceptance_rate <= ATTENTION_ACCEPTANCE_THRESHOLD
                && rule
                    .high_confidence_vs_eval_gap
                    .is_some_and(|gap| gap >= ATTENTION_GAP_THRESHOLD)
        })
        .cloned()
        .collect::<Vec<_>>();
    attention.sort_by(|left, right| {
        right
            .high_confidence_vs_eval_gap
            .unwrap_or_default()
            .total_cmp(&left.high_confidence_vs_eval_gap.unwrap_or_default())
            .then_with(|| right.high_confidence_total.cmp(&left.high_confidence_total))
            .then_with(|| right.feedback_total.cmp(&left.feedback_total))
    });
    attention
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::commands::eval::{EvalReport, EvalRuleMetrics, EvalRunMetadata};
    use crate::core::eval_benchmarks::AggregateMetrics;

    use super::*;

    #[test]
    fn build_feedback_eval_correlation_joins_feedback_and_eval_metrics() {
        let eval_report = EvalReport {
            run: EvalRunMetadata::default(),
            fixtures_total: 1,
            fixtures_passed: 1,
            fixtures_failed: 0,
            rule_metrics: vec![EvalRuleMetrics {
                rule_id: "sec.sql.injection".to_string(),
                expected: 1,
                predicted: 1,
                true_positives: 1,
                false_positives: 0,
                false_negatives: 0,
                precision: 1.0,
                recall: 1.0,
                f1: 1.0,
            }],
            rule_summary: None,
            benchmark_summary: None,
            suite_results: vec![],
            benchmark_by_category: HashMap::from([(
                "security".to_string(),
                AggregateMetrics {
                    fixture_count: 2,
                    micro_f1: 0.8,
                    weighted_score: 0.82,
                    ..Default::default()
                },
            )]),
            benchmark_by_language: HashMap::new(),
            benchmark_by_difficulty: HashMap::new(),
            suite_comparisons: vec![],
            category_comparisons: vec![],
            language_comparisons: vec![],
            verification_health: None,
            warnings: vec![],
            threshold_failures: vec![],
            results: vec![],
        };
        let by_category = vec![FeedbackEvalBucket {
            name: "Security".to_string(),
            total: 4,
            accepted: 1,
            rejected: 3,
            acceptance_rate: 0.25,
        }];
        let high_confidence_by_category = vec![FeedbackEvalBucket {
            name: "Security".to_string(),
            total: 2,
            accepted: 0,
            rejected: 2,
            acceptance_rate: 0.0,
        }];
        let by_rule = vec![FeedbackEvalBucket {
            name: "sec.sql.injection".to_string(),
            total: 3,
            accepted: 1,
            rejected: 2,
            acceptance_rate: 0.33,
        }];
        let high_confidence_by_rule = vec![FeedbackEvalBucket {
            name: "sec.sql.injection".to_string(),
            total: 2,
            accepted: 0,
            rejected: 2,
            acceptance_rate: 0.0,
        }];

        let correlation = build_feedback_eval_correlation(
            Some(&eval_report),
            &by_category,
            &high_confidence_by_category,
            &by_rule,
            &high_confidence_by_rule,
        )
        .unwrap();

        assert_eq!(correlation.by_category.len(), 1);
        assert_eq!(correlation.by_category[0].eval_fixture_count, Some(2));
        assert!((correlation.by_category[0].feedback_vs_eval_gap.unwrap() - 0.55).abs() < 0.001);
        assert!(
            (correlation.by_category[0]
                .high_confidence_vs_eval_gap
                .unwrap()
                - 0.8)
                .abs()
                < 0.001
        );
        assert_eq!(correlation.by_rule[0].eval_f1, Some(1.0));
        assert_eq!(correlation.by_rule[0].high_confidence_total, 2);
        assert!((correlation.by_rule[0].feedback_vs_eval_gap.unwrap() - 0.67).abs() < 0.001);
        assert_eq!(correlation.attention_by_category.len(), 1);
        assert_eq!(correlation.attention_by_rule.len(), 1);
    }
}
