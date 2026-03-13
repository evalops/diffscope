#[path = "comments/api.rs"]
mod api;
#[path = "comments/body.rs"]
mod body;
#[path = "comments/posting.rs"]
mod posting;
#[path = "comments/summary.rs"]
mod summary;

use api::{post_inline_pr_comment, post_pr_comment};
use body::build_github_comment_body;
use summary::upsert_pr_summary_comment;

pub(super) use posting::post_review_comments;
