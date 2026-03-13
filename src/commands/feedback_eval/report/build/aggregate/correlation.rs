use std::collections::HashMap;

use crate::commands::eval::EvalReport;

use super::super::super::super::{
    FeedbackEvalBucket, FeedbackEvalCategoryCorrelation, FeedbackEvalCorrelationReport,
    FeedbackEvalRuleCorrelation,
};

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

    Some(FeedbackEvalCorrelationReport {
        by_category: by_category
            .iter()
            .map(|bucket| {
                let high_confidence = high_confidence_categories.get(&normalize_key(&bucket.name));
                let eval_metrics = eval_categories.get(&normalize_key(&bucket.name));
                FeedbackEvalCategoryCorrelation {
                    name: bucket.name.clone(),
                    feedback_total: bucket.total,
                    feedback_acceptance_rate: bucket.acceptance_rate,
                    high_confidence_total: high_confidence.map(|bucket| bucket.total).unwrap_or(0),
                    high_confidence_acceptance_rate: high_confidence
                        .map(|bucket| bucket.acceptance_rate)
                        .unwrap_or(0.0),
                    eval_fixture_count: eval_metrics.map(|metrics| metrics.fixture_count),
                    eval_micro_f1: eval_metrics.map(|metrics| metrics.micro_f1),
                    eval_weighted_score: eval_metrics.map(|metrics| metrics.weighted_score),
                }
            })
            .collect(),
        by_rule: by_rule
            .iter()
            .map(|bucket| {
                let high_confidence = high_confidence_rules.get(&normalize_key(&bucket.name));
                let eval_metric = eval_rules.get(&normalize_key(&bucket.name));
                FeedbackEvalRuleCorrelation {
                    rule_id: bucket.name.clone(),
                    feedback_total: bucket.total,
                    feedback_acceptance_rate: bucket.acceptance_rate,
                    high_confidence_total: high_confidence.map(|bucket| bucket.total).unwrap_or(0),
                    high_confidence_acceptance_rate: high_confidence
                        .map(|bucket| bucket.acceptance_rate)
                        .unwrap_or(0.0),
                    eval_precision: eval_metric.map(|metric| metric.precision),
                    eval_recall: eval_metric.map(|metric| metric.recall),
                    eval_f1: eval_metric.map(|metric| metric.f1),
                }
            })
            .collect(),
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
        assert_eq!(correlation.by_rule[0].eval_f1, Some(1.0));
        assert_eq!(correlation.by_rule[0].high_confidence_total, 2);
    }
}
