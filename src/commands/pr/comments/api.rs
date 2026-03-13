use anyhow::Result;
use std::process::Command;

use crate::core;

use super::super::gh::GhPrMetadata;

pub(super) fn post_inline_pr_comment(
    pr_number: &str,
    repo: Option<&str>,
    metadata: &GhPrMetadata,
    comment: &core::Comment,
    body: &str,
) -> Result<()> {
    if comment.line_number == 0 {
        anyhow::bail!("line number is 0");
    }

    let endpoint = format!(
        "repos/{}/pulls/{}/comments",
        metadata.base_repository.name_with_owner, pr_number
    );
    let mut args = vec![
        "api".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        endpoint,
        "-f".to_string(),
        format!("body={}", body),
        "-f".to_string(),
        format!("commit_id={}", metadata.head_ref_oid),
        "-f".to_string(),
        format!("path={}", comment.file_path.display()),
        "-F".to_string(),
        format!("line={}", comment.line_number),
        "-f".to_string(),
        "side=RIGHT".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.to_string());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api inline comment failed: {}", stderr.trim());
    }

    Ok(())
}

pub(super) fn post_pr_comment(pr_number: &str, repo: Option<&str>, body: &str) -> Result<()> {
    let mut args = vec![
        "pr".to_string(),
        "comment".to_string(),
        pr_number.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.to_string());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr comment failed: {}", stderr.trim());
    }
    Ok(())
}
