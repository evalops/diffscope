use anyhow::Result;
use serde::Deserialize;
use std::process::Command;

use crate::core;
use crate::review;

use super::super::gh::GhPrMetadata;
use super::post_pr_comment;

#[derive(Debug, Deserialize)]
struct GhIssueComment {
    id: u64,
    body: String,
}

pub(super) fn upsert_pr_summary_comment(
    pr_number: &str,
    repo: Option<&str>,
    metadata: &GhPrMetadata,
    comments: &[core::Comment],
    rule_priority: &[String],
) -> Result<()> {
    const SUMMARY_MARKER: &str = "<!-- diffscope:summary -->";
    let summary_body = review::build_pr_summary_comment_body(comments, rule_priority);
    let full_body = format!("{SUMMARY_MARKER}\n\n{summary_body}");

    let comments_endpoint = format!(
        "repos/{}/issues/{}/comments?per_page=100",
        metadata.base_repository.name_with_owner, pr_number
    );
    let mut args = vec!["api".to_string(), comments_endpoint];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.to_string());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api list issue comments failed: {}", stderr.trim());
    }

    let issue_comments: Vec<GhIssueComment> = serde_json::from_slice(&output.stdout)?;
    if let Some(existing) = issue_comments
        .iter()
        .find(|comment| comment.body.contains(SUMMARY_MARKER))
    {
        let patch_endpoint = format!(
            "repos/{}/issues/comments/{}",
            metadata.base_repository.name_with_owner, existing.id
        );
        let mut patch_args = vec![
            "api".to_string(),
            "-X".to_string(),
            "PATCH".to_string(),
            patch_endpoint,
            "-f".to_string(),
            format!("body={}", full_body),
        ];
        if let Some(repo) = repo {
            patch_args.push("--repo".to_string());
            patch_args.push(repo.to_string());
        }

        let patch_output = Command::new("gh").args(&patch_args).output()?;
        if !patch_output.status.success() {
            let stderr = String::from_utf8_lossy(&patch_output.stderr);
            anyhow::bail!("gh api patch summary comment failed: {}", stderr.trim());
        }
        return Ok(());
    }

    post_pr_comment(pr_number, repo, &full_body)
}
