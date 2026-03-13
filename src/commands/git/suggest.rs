#[path = "suggest/commit.rs"]
mod commit;
#[path = "suggest/pr_title.rs"]
mod pr_title;
#[path = "suggest/request.rs"]
mod request;
#[path = "suggest/response.rs"]
mod response;

pub(super) use commit::suggest_commit_message;
pub(super) use pr_title::suggest_pr_title;
