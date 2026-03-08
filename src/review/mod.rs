mod filters;
mod pipeline;
mod feedback;
mod context_helpers;
mod rule_helpers;

pub use filters::apply_review_filters;
pub use pipeline::{
    review_diff_content, review_diff_content_with_repo, review_diff_content_raw,
    review_diff_content_raw_with_progress,
    extract_symbols_from_diff, filter_comments_for_diff,
    build_symbol_index, build_review_guidance,
    ProgressCallback, ProgressUpdate,
};
pub use feedback::{
    load_feedback_store, load_feedback_store_from_path, save_feedback_store,
};
pub use context_helpers::{
    inject_custom_context, inject_pattern_repository_context,
    resolve_pattern_repositories, rank_and_trim_context_chunks,
};
pub use rule_helpers::{
    load_review_rules, inject_rule_context, apply_rule_overrides,
    summarize_rule_hits, build_pr_summary_comment_body,
    normalize_rule_id,
};

// Used by sibling modules (commands, output) and their tests
#[allow(unused_imports)]
pub(crate) use filters::{
    classify_comment_type, ReviewCommentType,
};
#[allow(unused_imports)]
pub(crate) use feedback::{FeedbackStore, FeedbackTypeStats};
#[allow(unused_imports)]
pub(crate) use context_helpers::PatternRepositoryMap;
#[allow(unused_imports)]
pub(crate) use rule_helpers::{
    RuleHitBreakdown, build_rule_priority_rank,
    format_top_findings_by_file, severity_rank,
};
#[allow(unused_imports)]
pub(crate) use pipeline::is_line_in_diff;
