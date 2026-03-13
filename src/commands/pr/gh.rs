#[path = "gh/diff.rs"]
mod diff;
#[path = "gh/metadata.rs"]
mod metadata;
#[path = "gh/resolve.rs"]
mod resolve;

pub(super) use diff::fetch_pr_diff;
pub(super) use metadata::{fetch_pr_metadata, GhPrMetadata};
pub(super) use resolve::resolve_pr_number;
