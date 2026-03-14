#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageResult {
    NeedsReview,
    SkipLockFile,
    SkipWhitespaceOnly,
    SkipGenerated,
    SkipCommentOnly,
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
        }
    }
}
