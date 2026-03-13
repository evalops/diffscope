#[path = "repo_support/conventions.rs"]
mod conventions;
#[path = "repo_support/diff.rs"]
mod diff;
#[path = "repo_support/git.rs"]
mod git;
#[path = "repo_support/instructions.rs"]
mod instructions;

pub(super) use conventions::{resolve_convention_store_path, save_convention_store};
pub(super) use diff::chunk_diff_for_context;
pub(super) use git::gather_git_log;
pub(super) use instructions::detect_instruction_files;
