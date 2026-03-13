#[path = "misc/changelog.rs"]
mod changelog;
#[path = "misc/discussion.rs"]
mod discussion;
#[path = "misc/feedback.rs"]
mod feedback;
#[path = "misc/lsp_check.rs"]
mod lsp_check;

pub use changelog::changelog_command;
pub use discussion::discuss_command;
pub use feedback::feedback_command;
pub use lsp_check::lsp_check_command;
