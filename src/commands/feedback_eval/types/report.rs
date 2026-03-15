use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalBucket {
    #[serde(default)]
    pub(in super::super) name: String,
    #[serde(default)]
    pub(in super::super) total: usize,
    #[serde(default)]
    pub(in super::super) accepted: usize,
    #[serde(default)]
    pub(in super::super) rejected: usize,
    #[serde(default)]
    pub(in super::super) acceptance_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalCategoryCorrelation {
    #[serde(default)]
    pub(in super::super) name: String,
    #[serde(default)]
    pub(in super::super) feedback_total: usize,
    #[serde(default)]
    pub(in super::super) feedback_acceptance_rate: f32,
    #[serde(default)]
    pub(in super::super) high_confidence_total: usize,
    #[serde(default)]
    pub(in super::super) high_confidence_acceptance_rate: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_fixture_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_micro_f1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_weighted_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) feedback_vs_eval_gap: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) high_confidence_vs_eval_gap: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalRuleCorrelation {
    #[serde(default)]
    pub(in super::super) rule_id: String,
    #[serde(default)]
    pub(in super::super) feedback_total: usize,
    #[serde(default)]
    pub(in super::super) feedback_acceptance_rate: f32,
    #[serde(default)]
    pub(in super::super) high_confidence_total: usize,
    #[serde(default)]
    pub(in super::super) high_confidence_acceptance_rate: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_precision: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_recall: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_f1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) feedback_vs_eval_gap: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) high_confidence_vs_eval_gap: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalCorrelationReport {
    #[serde(default)]
    pub(in super::super) by_category: Vec<FeedbackEvalCategoryCorrelation>,
    #[serde(default)]
    pub(in super::super) by_rule: Vec<FeedbackEvalRuleCorrelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in super::super) attention_by_category: Vec<FeedbackEvalCategoryCorrelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in super::super) attention_by_rule: Vec<FeedbackEvalRuleCorrelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalTrendGap {
    #[serde(default)]
    pub(in super::super) name: String,
    #[serde(default)]
    pub(in super::super) feedback_total: usize,
    #[serde(default)]
    pub(in super::super) high_confidence_total: usize,
    #[serde(default)]
    pub(in super::super) high_confidence_acceptance_rate: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) gap: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(in super::super) struct FeedbackEvalTrendEntry {
    #[serde(default)]
    pub(in super::super) timestamp: String,
    #[serde(default)]
    pub(in super::super) labeled_comments: usize,
    #[serde(default)]
    pub(in super::super) accepted: usize,
    #[serde(default)]
    pub(in super::super) rejected: usize,
    #[serde(default)]
    pub(in super::super) acceptance_rate: f32,
    #[serde(default)]
    pub(in super::super) confidence_threshold: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_agreement_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_precision: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_recall: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_f1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in super::super) attention_by_category: Vec<FeedbackEvalTrendGap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in super::super) attention_by_rule: Vec<FeedbackEvalTrendGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(in super::super) struct FeedbackEvalTrend {
    #[serde(default)]
    pub(in super::super) entries: Vec<FeedbackEvalTrendEntry>,
}

impl FeedbackEvalTrend {
    pub(in super::super) fn new() -> Self {
        Self::default()
    }

    pub(in super::super) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub(in super::super) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub(in super::super) struct FeedbackThresholdMetrics {
    #[serde(default)]
    pub(in super::super) total_scored: usize,
    #[serde(default)]
    pub(in super::super) true_positive: usize,
    #[serde(default)]
    pub(in super::super) false_positive: usize,
    #[serde(default)]
    pub(in super::super) true_negative: usize,
    #[serde(default)]
    pub(in super::super) false_negative: usize,
    #[serde(default)]
    pub(in super::super) precision: f32,
    #[serde(default)]
    pub(in super::super) recall: f32,
    #[serde(default)]
    pub(in super::super) f1: f32,
    #[serde(default)]
    pub(in super::super) agreement_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalExample {
    #[serde(default)]
    pub(in super::super) source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) pr_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) file_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) line_number: Option<usize>,
    #[serde(default)]
    pub(in super::super) category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence: Option<f32>,
    #[serde(default)]
    pub(in super::super) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in super::super) struct FeedbackEvalReport {
    #[serde(default)]
    pub(in super::super) total_comments_seen: usize,
    #[serde(default)]
    pub(in super::super) total_reviews_seen: usize,
    #[serde(default)]
    pub(in super::super) labeled_comments: usize,
    #[serde(default)]
    pub(in super::super) labeled_reviews: usize,
    #[serde(default)]
    pub(in super::super) accepted: usize,
    #[serde(default)]
    pub(in super::super) rejected: usize,
    #[serde(default)]
    pub(in super::super) acceptance_rate: f32,
    #[serde(default)]
    pub(in super::super) feedback_coverage_rate: f32,
    #[serde(default)]
    pub(in super::super) confidence_threshold: f32,
    pub(in super::super) vague_comments: FeedbackEvalBucket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_metrics: Option<FeedbackThresholdMetrics>,
    #[serde(default)]
    pub(in super::super) by_category: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_rule: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) high_confidence_by_category: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) high_confidence_by_rule: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_severity: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_repo: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_file_pattern: Vec<FeedbackEvalBucket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) eval_correlation: Option<FeedbackEvalCorrelationReport>,
    #[serde(default)]
    pub(in super::super) showcase_candidates: Vec<FeedbackEvalExample>,
    #[serde(default)]
    pub(in super::super) vague_rejections: Vec<FeedbackEvalExample>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in super::super) threshold_failures: Vec<String>,
}
