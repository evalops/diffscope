mod format;

pub use format::{
    build_change_walkthrough, format_diff_as_unified, format_smart_review_output, output_comments,
    OutputFormat,
};

// Used by sibling modules (commands, review) and their tests
#[allow(unused_imports)]
pub(crate) use format::{
    format_as_markdown, format_as_patch, format_detailed_comment, format_pr_summary_section,
};
