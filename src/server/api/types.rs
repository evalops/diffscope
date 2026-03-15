use std::collections::HashSet;

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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiComment {
    #[serde(flatten)]
    pub comment: crate::core::comment::Comment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<crate::core::comment::CommentOutcome>,
}

impl ApiComment {
    pub(crate) fn from_comment(
        comment: crate::core::comment::Comment,
        stale_review: bool,
        addressed_by_follow_up: bool,
    ) -> Self {
        let outcomes = crate::core::comment::derive_comment_outcomes(
            &comment,
            crate::core::comment::CommentOutcomeContext {
                stale_review,
                addressed_by_follow_up,
                auto_fixed: false,
            },
        );
        Self { comment, outcomes }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiReviewSession {
    pub id: String,
    pub status: crate::server::state::ReviewStatus,
    pub diff_source: String,
    #[serde(default)]
    pub github_head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_post_results_requested: Option<bool>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub comments: Vec<ApiComment>,
    pub summary: Option<crate::core::comment::ReviewSummary>,
    pub files_reviewed: usize,
    pub error: Option<String>,
    #[serde(default)]
    pub pr_summary_text: Option<String>,
    #[serde(default)]
    pub diff_content: Option<String>,
    #[serde(default)]
    pub event: Option<crate::server::state::ReviewEvent>,
    #[serde(default)]
    pub progress: Option<crate::server::state::ReviewProgress>,
}

pub(crate) fn build_api_review_session(
    session: crate::server::state::ReviewSession,
    stale_review: bool,
    addressed_by_follow_up_comment_ids: &HashSet<String>,
) -> ApiReviewSession {
    ApiReviewSession {
        id: session.id,
        status: session.status,
        diff_source: session.diff_source,
        github_head_sha: session.github_head_sha,
        github_post_results_requested: session.github_post_results_requested,
        started_at: session.started_at,
        completed_at: session.completed_at,
        comments: session
            .comments
            .into_iter()
            .map(|comment| {
                let addressed_by_follow_up =
                    addressed_by_follow_up_comment_ids.contains(&comment.id);
                ApiComment::from_comment(comment, stale_review, addressed_by_follow_up)
            })
            .collect(),
        summary: session.summary,
        files_reviewed: session.files_reviewed,
        error: session.error,
        pr_summary_text: session.pr_summary_text,
        diff_content: session.diff_content,
        event: session.event,
        progress: session.progress,
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn build_api_review_session_derives_comment_outcomes() {
        let session = crate::server::state::ReviewSession {
            id: "review-1".to_string(),
            status: crate::server::state::ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("abc123".to_string()),
            github_post_results_requested: Some(true),
            started_at: 1,
            completed_at: Some(2),
            comments: vec![crate::core::comment::Comment {
                id: "comment-1".to_string(),
                file_path: PathBuf::from("src/lib.rs"),
                line_number: 10,
                content: "test".to_string(),
                rule_id: None,
                severity: crate::core::comment::Severity::Warning,
                category: crate::core::comment::Category::Bug,
                suggestion: None,
                confidence: 0.9,
                code_suggestion: None,
                tags: Vec::new(),
                fix_effort: crate::core::comment::FixEffort::Low,
                feedback: Some("accept".to_string()),
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            }],
            summary: None,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };

        let api_session =
            build_api_review_session(session, true, &HashSet::from(["comment-1".to_string()]));

        assert_eq!(api_session.comments.len(), 1);
        assert_eq!(
            api_session.comments[0].outcomes,
            vec![
                crate::core::comment::CommentOutcome::Accepted,
                crate::core::comment::CommentOutcome::Addressed,
                crate::core::comment::CommentOutcome::Stale
            ]
        );
    }
}
