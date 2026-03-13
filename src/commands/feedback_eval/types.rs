use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub(super) struct LoadedFeedbackEvalInput {
    pub(super) total_comments_seen: usize,
    pub(super) total_reviews_seen: usize,
    pub(super) comments: Vec<FeedbackEvalComment>,
}

#[derive(Debug, Clone)]
pub(super) struct FeedbackEvalComment {
    pub(super) source_kind: String,
    pub(super) review_id: Option<String>,
    pub(super) repo: Option<String>,
    pub(super) pr_number: Option<u32>,
    pub(super) title: Option<String>,
    pub(super) file_path: Option<PathBuf>,
    pub(super) line_number: Option<usize>,
    pub(super) file_patterns: Vec<String>,
    pub(super) content: String,
    pub(super) category: String,
    pub(super) severity: Option<String>,
    pub(super) confidence: Option<f32>,
    pub(super) accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeedbackEvalBucket {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) total: usize,
    #[serde(default)]
    pub(super) accepted: usize,
    #[serde(default)]
    pub(super) rejected: usize,
    #[serde(default)]
    pub(super) acceptance_rate: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub(super) struct FeedbackThresholdMetrics {
    #[serde(default)]
    pub(super) total_scored: usize,
    #[serde(default)]
    pub(super) true_positive: usize,
    #[serde(default)]
    pub(super) false_positive: usize,
    #[serde(default)]
    pub(super) true_negative: usize,
    #[serde(default)]
    pub(super) false_negative: usize,
    #[serde(default)]
    pub(super) precision: f32,
    #[serde(default)]
    pub(super) recall: f32,
    #[serde(default)]
    pub(super) f1: f32,
    #[serde(default)]
    pub(super) agreement_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeedbackEvalExample {
    #[serde(default)]
    pub(super) source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) pr_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) file_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) line_number: Option<usize>,
    #[serde(default)]
    pub(super) category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) confidence: Option<f32>,
    #[serde(default)]
    pub(super) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FeedbackEvalReport {
    #[serde(default)]
    pub(super) total_comments_seen: usize,
    #[serde(default)]
    pub(super) total_reviews_seen: usize,
    #[serde(default)]
    pub(super) labeled_comments: usize,
    #[serde(default)]
    pub(super) labeled_reviews: usize,
    #[serde(default)]
    pub(super) accepted: usize,
    #[serde(default)]
    pub(super) rejected: usize,
    #[serde(default)]
    pub(super) acceptance_rate: f32,
    #[serde(default)]
    pub(super) confidence_threshold: f32,
    pub(super) vague_comments: FeedbackEvalBucket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) confidence_metrics: Option<FeedbackThresholdMetrics>,
    #[serde(default)]
    pub(super) by_category: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(super) by_severity: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(super) by_repo: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(super) by_file_pattern: Vec<FeedbackEvalBucket>,
    #[serde(default)]
    pub(super) showcase_candidates: Vec<FeedbackEvalExample>,
    #[serde(default)]
    pub(super) vague_rejections: Vec<FeedbackEvalExample>,
}
