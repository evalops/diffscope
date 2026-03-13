use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub(in super::super) struct LoadedFeedbackEvalInput {
    pub(in super::super) total_comments_seen: usize,
    pub(in super::super) total_reviews_seen: usize,
    pub(in super::super) comments: Vec<FeedbackEvalComment>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct FeedbackEvalComment {
    pub(in super::super) source_kind: String,
    pub(in super::super) review_id: Option<String>,
    pub(in super::super) repo: Option<String>,
    pub(in super::super) pr_number: Option<u32>,
    pub(in super::super) title: Option<String>,
    pub(in super::super) file_path: Option<PathBuf>,
    pub(in super::super) line_number: Option<usize>,
    pub(in super::super) file_patterns: Vec<String>,
    pub(in super::super) content: String,
    pub(in super::super) category: String,
    pub(in super::super) severity: Option<String>,
    pub(in super::super) confidence: Option<f32>,
    pub(in super::super) accepted: bool,
}
