use anyhow::Result;

use crate::core;

use super::super::gh::GhPrMetadata;
use super::{
    build_github_comment_body, post_inline_pr_comment, post_pr_comment, upsert_pr_summary_comment,
};

pub(in super::super) struct PostingStats {
    pub(in super::super) inline_posted: usize,
    pub(in super::super) fallback_posted: usize,
}

pub(in super::super) fn post_review_comments(
    pr_number: &str,
    repo: Option<&str>,
    metadata: &GhPrMetadata,
    comments: &[core::Comment],
    rule_priority: &[String],
) -> Result<PostingStats> {
    let mut inline_posted = 0usize;
    let mut fallback_posted = 0usize;

    for comment in comments {
        let body = build_github_comment_body(comment);
        let inline_result = post_inline_pr_comment(pr_number, repo, metadata, comment, &body);

        if inline_result.is_ok() {
            inline_posted += 1;
            continue;
        }

        if let Err(err) = inline_result {
            tracing::warn!(
                "Inline comment failed for {}:{} (falling back to PR comment): {}",
                comment.file_path.display(),
                comment.line_number,
                err
            );
        }
        post_pr_comment(pr_number, repo, &body)?;
        fallback_posted += 1;
    }

    upsert_pr_summary_comment(pr_number, repo, metadata, comments, rule_priority)?;

    Ok(PostingStats {
        inline_posted,
        fallback_posted,
    })
}
