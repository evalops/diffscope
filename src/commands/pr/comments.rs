use anyhow::Result;
use serde::Deserialize;
use std::process::Command;

use crate::core;
use crate::review;

use super::gh::GhPrMetadata;

pub(super) struct PostingStats {
    pub(super) inline_posted: usize,
    pub(super) fallback_posted: usize,
}

pub(super) fn post_review_comments(
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

fn build_github_comment_body(comment: &core::Comment) -> String {
    let mut body = format!(
        "**{:?} ({:?})**\n\n{}",
        comment.severity, comment.category, comment.content
    );
    if let Some(rule_id) = &comment.rule_id {
        body.push_str(&format!("\n\n**Rule:** `{}`", rule_id));
    }
    if let Some(suggestion) = &comment.suggestion {
        body.push_str("\n\n**Suggested fix:** ");
        body.push_str(suggestion);
    }
    body.push_str(&format!(
        "\n\n_Confidence: {:.0}%_",
        comment.confidence * 100.0
    ));
    body
}

fn post_inline_pr_comment(
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

fn post_pr_comment(pr_number: &str, repo: Option<&str>, body: &str) -> Result<()> {
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

#[derive(Debug, Deserialize)]
struct GhIssueComment {
    id: u64,
    body: String,
}

fn upsert_pr_summary_comment(
    pr_number: &str,
    repo: Option<&str>,
    metadata: &GhPrMetadata,
    comments: &[core::Comment],
    rule_priority: &[String],
) -> Result<()> {
    const SUMMARY_MARKER: &str = "<!-- diffscope:summary -->";
    let summary_body = review::build_pr_summary_comment_body(comments, rule_priority);
    let full_body = format!("{}\n\n{}", SUMMARY_MARKER, summary_body);

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
