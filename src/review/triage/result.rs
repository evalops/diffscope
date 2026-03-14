#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageResult {
    NeedsReview,
    SkipLockFile,
    SkipWhitespaceOnly,
    SkipGenerated,
    SkipCommentOnly,
    /// File has only removal hunks; skip when config triage_skip_deletion_only is true (#29).
    SkipDeletionOnly,
}

impl TriageResult {
    pub fn should_skip(&self) -> bool {
        !matches!(self, TriageResult::NeedsReview)
    }

    pub fn reason(&self) -> &'static str {
        match self {
            TriageResult::NeedsReview => "needs review",
            TriageResult::SkipLockFile => "lock file",
            TriageResult::SkipWhitespaceOnly => "whitespace-only changes",
            TriageResult::SkipGenerated => "generated file",
            TriageResult::SkipCommentOnly => "comment-only changes",
            TriageResult::SkipDeletionOnly => "deletion-only changes",
        }
    }
}
