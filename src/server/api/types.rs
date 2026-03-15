use super::*;

// === Request/Response types ===

#[derive(Deserialize)]
pub(crate) struct StartReviewRequest {
    pub diff_source: String,
    pub base_branch: Option<String>,
    /// Raw diff content (used when diff_source is "raw", e.g. from a GitHub PR)
    pub diff_content: Option<String>,
    /// Optional title for the review (e.g. "owner/repo#123: PR title")
    pub title: Option<String>,
    // --- per-review overrides ---
    pub model: Option<String>,
    pub strictness: Option<u8>,
    pub review_profile: Option<String>,
}

/// Per-review config overrides from the start request.
#[derive(Clone, Default)]
pub(crate) struct ReviewOverrides {
    pub(crate) model: Option<String>,
    pub(crate) strictness: Option<u8>,
    pub(crate) review_profile: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct StartReviewResponse {
    pub id: String,
    pub status: ReviewStatus,
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    pub repo_path: String,
    pub branch: Option<String>,
    pub model: String,
    pub adapter: Option<String>,
    pub base_url: Option<String>,
    pub active_reviews: usize,
}

#[derive(Deserialize)]
pub(crate) struct FeedbackRequest {
    pub comment_id: String,
    pub action: String,
}

#[derive(Serialize)]
pub(crate) struct FeedbackResponse {
    pub ok: bool,
}

#[derive(Deserialize)]
pub(crate) struct CommentLifecycleRequest {
    pub comment_id: String,
    pub status: String,
}

#[derive(Serialize)]
pub(crate) struct CommentLifecycleResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FeedbackEvalTrendGapResponse {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub feedback_total: usize,
    #[serde(default)]
    pub high_confidence_total: usize,
    #[serde(default)]
    pub high_confidence_acceptance_rate: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FeedbackEvalTrendEntryResponse {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub labeled_comments: usize,
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub acceptance_rate: f32,
    #[serde(default)]
    pub confidence_threshold: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_agreement_rate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_precision: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_recall: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_f1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_by_category: Vec<FeedbackEvalTrendGapResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_by_rule: Vec<FeedbackEvalTrendGapResponse>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct FeedbackEvalTrendResponse {
    #[serde(default)]
    pub entries: Vec<FeedbackEvalTrendEntryResponse>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AnalyticsTrendsResponse {
    pub eval_trend_path: String,
    pub feedback_eval_trend_path: String,
    #[serde(default)]
    pub eval_trend: crate::core::eval_benchmarks::QualityTrend,
    #[serde(default)]
    pub feedback_eval_trend: FeedbackEvalTrendResponse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct LearnedRuleResponse {
    #[serde(default)]
    pub pattern_text: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub accepted_count: usize,
    #[serde(default)]
    pub rejected_count: usize,
    #[serde(default)]
    pub total_observations: usize,
    #[serde(default)]
    pub acceptance_rate: f32,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_patterns: Vec<String>,
    #[serde(default)]
    pub first_seen: String,
    #[serde(default)]
    pub last_seen: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct LearnedRulesResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convention_store_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boost: Vec<LearnedRuleResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppress: Vec<LearnedRuleResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AttentionGapSnapshotResponse {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_category: Vec<FeedbackEvalTrendGapResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_rule: Vec<FeedbackEvalTrendGapResponse>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AttentionGapsResponse {
    pub feedback_eval_trend_path: String,
    #[serde(default)]
    pub latest: AttentionGapSnapshotResponse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct RejectedPatternResponse {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: usize,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub acceptance_rate: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct RejectedPatternsResponse {
    pub feedback_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_category: Vec<RejectedPatternResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_rule: Vec<RejectedPatternResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_file_pattern: Vec<RejectedPatternResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ListReviewsParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct ListEventsParams {
    pub source: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub github_repo: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

use crate::server::storage::EventFilters;

impl ListEventsParams {
    pub(crate) fn into_filters(self) -> EventFilters {
        EventFilters {
            source: self.source,
            model: self.model,
            status: self.status,
            time_from: self.time_from.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|t| t.with_timezone(&chrono::Utc))
            }),
            time_to: self.time_to.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|t| t.with_timezone(&chrono::Utc))
            }),
            github_repo: self.github_repo,
            limit: self.limit,
            offset: self.offset,
        }
    }
}
