pub(crate) mod compression;
mod context_helpers;
mod feedback;
mod filters;
mod pipeline;
mod rule_helpers;
pub mod triage;
pub(crate) mod verification;

pub use context_helpers::{
    inject_custom_context, inject_pattern_repository_context, rank_and_trim_context_chunks,
    resolve_pattern_repositories,
};
pub use feedback::{load_feedback_store, load_feedback_store_from_path, save_feedback_store};
pub use filters::apply_review_filters;
pub use pipeline::{
    build_review_guidance, build_symbol_index, extract_symbols_from_diff, filter_comments_for_diff,
    review_diff_content, review_diff_content_raw, review_diff_content_raw_with_progress,
    review_diff_content_with_repo, ProgressCallback, ProgressUpdate,
};
#[allow(unused_imports)]
pub(crate) use pipeline::{AgentActivity, FileMetric, ReviewResult};
pub use rule_helpers::{
    apply_rule_overrides, build_pr_summary_comment_body, inject_rule_context, load_review_rules,
    normalize_rule_id, summarize_rule_hits,
};

// Used by sibling modules (commands, output) and their tests
#[allow(unused_imports)]
pub(crate) use context_helpers::PatternRepositoryMap;
#[allow(unused_imports)]
pub(crate) use feedback::{FeedbackStore, FeedbackTypeStats};
#[allow(unused_imports)]
pub(crate) use filters::{classify_comment_type, ReviewCommentType};
#[allow(unused_imports)]
pub(crate) use pipeline::is_line_in_diff;
#[allow(unused_imports)]
pub(crate) use rule_helpers::{
    build_rule_priority_rank, format_top_findings_by_file, severity_rank, RuleHitBreakdown,
};
