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
    pub(in super::super) confidence_threshold: f32,
    pub(in super::super) vague_comments: FeedbackEvalBucket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) confidence_metrics: Option<FeedbackThresholdMetrics>,
    #[serde(default)]
    pub(in super::super) by_category: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_severity: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_repo: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) by_file_pattern: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(in super::super) showcase_candidates: Vec<FeedbackEvalExample>,
    #[serde(default)]
    pub(in super::super) vague_rejections: Vec<FeedbackEvalExample>,
}
