mod format;

pub use format::{
    output_comments, format_smart_review_output,
    format_diff_as_unified, build_change_walkthrough, OutputFormat,
};

// Used by sibling modules (commands, review) and their tests
#[allow(unused_imports)]
pub(crate) use format::{
    format_as_patch, format_as_markdown,
    format_detailed_comment, format_pr_summary_section,
};
