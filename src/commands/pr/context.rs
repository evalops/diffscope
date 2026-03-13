use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::core;

use super::gh::{fetch_pr_diff, resolve_pr_number};

pub(super) struct PrCommandContext {
    pub(super) pr_number: String,
    pub(super) repo_root: PathBuf,
    pub(super) diff_content: String,
}

pub(super) fn prepare_pr_context(
    number: Option<u32>,
    repo: Option<&str>,
) -> Result<PrCommandContext> {
    let pr_number = resolve_pr_number(number, repo)?;
    let git = core::GitIntegration::new(".")?;
    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));

    if let Ok(branch) = git.get_current_branch() {
        info!("Current branch: {}", branch);
    }
    if let Ok(Some(remote)) = git.get_remote_url() {
        info!("Remote URL: {}", remote);
    }

    let diff_content = fetch_pr_diff(&pr_number, repo)?;

    Ok(PrCommandContext {
        pr_number,
        repo_root,
        diff_content,
    })
}
