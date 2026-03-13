use anyhow::Result;
use std::path::Path;

use crate::core;

pub(in super::super) async fn load_discussion_comments(
    review_path: &Path,
) -> Result<Vec<core::Comment>> {
    let content = tokio::fs::read_to_string(review_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;
    if comments.is_empty() {
        anyhow::bail!("No comments found in {}", review_path.display());
    }

    normalize_comment_ids(&mut comments);
    Ok(comments)
}

fn normalize_comment_ids(comments: &mut [core::Comment]) {
    for comment in comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }
}
