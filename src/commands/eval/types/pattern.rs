use serde::Deserialize;

use crate::core::comment::{MergeReadiness, ReviewSummary};

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalExpectations {
    #[serde(default)]
    pub(in super::super) must_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(in super::super) must_not_find: Vec<EvalPattern>,
    #[serde(default)]
    pub(in super::super) min_total: Option<usize>,
    #[serde(default)]
    pub(in super::super) max_total: Option<usize>,
    #[serde(default)]
    pub(in super::super) summary: EvalSummaryExpectations,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalSummaryExpectations {
    #[serde(default)]
    pub(in super::super) merge_readiness: Option<String>,
    #[serde(default)]
    pub(in super::super) open_blockers: Option<usize>,
    #[serde(default)]
    pub(in super::super) min_open_blockers: Option<usize>,
    #[serde(default)]
    pub(in super::super) max_open_blockers: Option<usize>,
}

impl EvalSummaryExpectations {
    pub(in super::super) fn append_failures(
        &self,
        failures: &mut Vec<String>,
        summary: &ReviewSummary,
    ) {
        if let Some(expected) = self.merge_readiness.as_deref() {
            let actual = canonical_merge_readiness_for_enum(summary.merge_readiness);
            if canonical_merge_readiness(expected) != Some(actual) {
                failures.push(format!(
                    "Expected merge_readiness {}, got {}",
                    expected, summary.merge_readiness
                ));
            }
        }

        if let Some(expected) = self.open_blockers {
            if summary.open_blockers != expected {
                failures.push(format!(
                    "Expected open_blockers to equal {expected}, got {}",
                    summary.open_blockers
                ));
            }
        }
        if let Some(min_open_blockers) = self.min_open_blockers {
            if summary.open_blockers < min_open_blockers {
                failures.push(format!(
                    "Expected open_blockers to be at least {min_open_blockers}, got {}",
                    summary.open_blockers
                ));
            }
        }
        if let Some(max_open_blockers) = self.max_open_blockers {
            if summary.open_blockers > max_open_blockers {
                failures.push(format!(
                    "Expected open_blockers to be at most {max_open_blockers}, got {}",
                    summary.open_blockers
                ));
            }
        }
    }

    pub(in super::super) fn validate(&self, fixture_name: &str) -> anyhow::Result<()> {
        if let Some(expected) = self.merge_readiness.as_deref() {
            if canonical_merge_readiness(expected).is_none() {
                anyhow::bail!(
                    "Invalid merge_readiness '{}' in fixture '{}': expected Ready, NeedsAttention, or NeedsReReview",
                    expected,
                    fixture_name
                );
            }
        }
        Ok(())
    }
}

fn canonical_merge_readiness(value: &str) -> Option<&'static str> {
    match normalize_merge_readiness(value).as_str() {
        "ready" => Some("Ready"),
        "needsattention" => Some("NeedsAttention"),
        "needsrereview" => Some("NeedsReReview"),
        _ => None,
    }
}

fn canonical_merge_readiness_for_enum(value: MergeReadiness) -> &'static str {
    match value {
        MergeReadiness::Ready => "Ready",
        MergeReadiness::NeedsAttention => "NeedsAttention",
        MergeReadiness::NeedsReReview => "NeedsReReview",
    }
}

fn normalize_merge_readiness(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalPattern {
    #[serde(default)]
    pub(in super::super) file: Option<String>,
    #[serde(default)]
    pub(in super::super) line: Option<usize>,
    #[serde(default)]
    pub(in super::super) contains: Option<String>,
    #[serde(default)]
    pub(in super::super) contains_any: Vec<String>,
    #[serde(default)]
    pub(in super::super) matches_regex: Option<String>,
    #[serde(default)]
    pub(in super::super) severity: Option<String>,
    #[serde(default)]
    pub(in super::super) category: Option<String>,
    #[serde(default)]
    pub(in super::super) tags_any: Vec<String>,
    #[serde(default)]
    pub(in super::super) confidence_at_least: Option<f32>,
    #[serde(default)]
    pub(in super::super) confidence_at_most: Option<f32>,
    #[serde(default)]
    pub(in super::super) fix_effort: Option<String>,
    #[serde(default)]
    pub(in super::super) rule_id: Option<String>,
    #[serde(default)]
    pub(in super::super) rule_id_aliases: Vec<String>,
    #[serde(default)]
    pub(in super::super) require_rule_id: bool,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::core::comment::{ReviewCompletenessSummary, ReviewVerificationSummary};

    fn build_summary(merge_readiness: MergeReadiness, open_blockers: usize) -> ReviewSummary {
        ReviewSummary {
            total_comments: open_blockers,
            by_severity: HashMap::new(),
            by_category: HashMap::new(),
            critical_issues: 0,
            files_reviewed: 1,
            overall_score: 8.0,
            recommendations: Vec::new(),
            open_comments: open_blockers,
            open_by_severity: HashMap::new(),
            open_blocking_comments: open_blockers,
            open_informational_comments: 0,
            resolved_comments: 0,
            dismissed_comments: 0,
            open_blockers,
            completeness: ReviewCompletenessSummary::default(),
            merge_readiness,
            verification: ReviewVerificationSummary::default(),
            readiness_reasons: Vec::new(),
            loop_telemetry: None,
        }
    }

    #[test]
    fn summary_expectations_accept_flexible_merge_readiness_labels() {
        let expectations = EvalSummaryExpectations {
            merge_readiness: Some("needs_attention".to_string()),
            min_open_blockers: Some(1),
            ..Default::default()
        };
        let summary = build_summary(MergeReadiness::NeedsAttention, 2);
        let mut failures = Vec::new();

        expectations.append_failures(&mut failures, &summary);

        assert!(failures.is_empty());
    }

    #[test]
    fn summary_expectations_report_mismatched_values() {
        let expectations = EvalSummaryExpectations {
            merge_readiness: Some("Ready".to_string()),
            open_blockers: Some(0),
            ..Default::default()
        };
        let summary = build_summary(MergeReadiness::NeedsAttention, 2);
        let mut failures = Vec::new();

        expectations.append_failures(&mut failures, &summary);

        assert_eq!(failures.len(), 2);
        assert!(failures[0].contains("Expected merge_readiness Ready"));
        assert!(failures[1].contains("Expected open_blockers to equal 0"));
    }

    #[test]
    fn summary_expectations_reject_unknown_merge_readiness_values() {
        let error = EvalSummaryExpectations {
            merge_readiness: Some("ship-it".to_string()),
            ..Default::default()
        }
        .validate("fixture-name")
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Invalid merge_readiness 'ship-it' in fixture 'fixture-name'"));
    }
}
