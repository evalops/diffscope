use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;
use crate::output::OutputFormat;
use crate::review;

pub async fn pr_command(
    number: Option<u32>,
    repo: Option<String>,
    post_comments: bool,
    summary: bool,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    use std::process::Command;

    let pr_number = if let Some(num) = number {
        num.to_string()
    } else {
        let mut args = vec![
            "pr".to_string(),
            "view".to_string(),
            "--json".to_string(),
            "number".to_string(),
            "-q".to_string(),
            ".number".to_string(),
        ];
        if let Some(repo) = repo.as_ref() {
            args.push("--repo".to_string());
            args.push(repo.clone());
        }

        let output = Command::new("gh").args(&args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gh pr view failed: {}", stderr.trim());
        }

        let pr_number = String::from_utf8(output.stdout)?.trim().to_string();
        if pr_number.is_empty() {
            anyhow::bail!("Unable to determine PR number from gh output");
        }
        pr_number
    };

    info!("Reviewing PR #{}", pr_number);

    let git = core::GitIntegration::new(".")?;
    let repo_root = git.workdir().unwrap_or_else(|| PathBuf::from("."));
    if let Ok(branch) = git.get_current_branch() {
        info!("Current branch: {}", branch);
    }
    if let Ok(Some(remote)) = git.get_remote_url() {
        info!("Remote URL: {}", remote);
    }

    let mut diff_args = vec!["pr".to_string(), "diff".to_string(), pr_number.clone()];
    if let Some(repo) = repo.as_ref() {
        diff_args.push("--repo".to_string());
        diff_args.push(repo.clone());
    }
    let diff_output = Command::new("gh").args(&diff_args).output()?;
    if !diff_output.status.success() {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        anyhow::bail!("gh pr diff failed: {}", stderr.trim());
    }

    let diff_content = String::from_utf8(diff_output.stdout)?;

    if diff_content.is_empty() {
        println!("No changes in PR");
        return Ok(());
    }

    if summary {
        let diffs = core::DiffParser::parse_unified_diff(&diff_content)?;
        let git = core::GitIntegration::new(".")?;

        // Use Fast model for PR summary generation (lightweight task)
        let fast_config = config.to_model_config_for_role(config::ModelRole::Fast);
        let adapter = adapters::llm::create_adapter(&fast_config)?;
        let options = core::SummaryOptions {
            include_diagram: config.smart_review_diagram,
        };
        let pr_summary = core::PRSummaryGenerator::generate_summary_with_options(
            &diffs,
            &git,
            adapter.as_ref(),
            options,
        )
        .await?;

        println!("{}", pr_summary.to_markdown());
        return Ok(());
    }

    let review_result =
        review::review_diff_content_raw(&diff_content, config.clone(), &repo_root).await?;
    let comments = review_result.comments;

    if post_comments {
        info!("Posting {} comments to PR", comments.len());
        let metadata = fetch_pr_metadata(&pr_number, repo.as_ref())?;
        let mut inline_posted = 0usize;
        let mut fallback_posted = 0usize;

        for comment in &comments {
            let body = build_github_comment_body(comment);
            let inline_result =
                post_inline_pr_comment(&pr_number, repo.as_ref(), &metadata, comment, &body);

            if inline_result.is_ok() {
                inline_posted += 1;
                continue;
            }

            if let Err(err) = inline_result {
                warn!(
                    "Inline comment failed for {}:{} (falling back to PR comment): {}",
                    comment.file_path.display(),
                    comment.line_number,
                    err
                );
            }
            post_pr_comment(&pr_number, repo.as_ref(), &body)?;
            fallback_posted += 1;
        }
        upsert_pr_summary_comment(
            &pr_number,
            repo.as_ref(),
            &metadata,
            &comments,
            &config.rule_priority,
        )?;

        println!(
            "Posted {} comments to PR #{} (inline: {}, fallback: {}, summary: updated)",
            comments.len(),
            pr_number,
            inline_posted,
            fallback_posted
        );
    } else {
        crate::output::output_comments(&comments, None, format, &config.rule_priority).await?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GhPrMetadata {
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    #[serde(rename = "baseRepository")]
    base_repository: GhBaseRepository,
}

#[derive(Debug, Deserialize)]
struct GhBaseRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

fn fetch_pr_metadata(pr_number: &str, repo: Option<&String>) -> Result<GhPrMetadata> {
    use std::process::Command;

    let mut args = vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "headRefOid,baseRepository".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view metadata failed: {}", stderr.trim());
    }

    let metadata: GhPrMetadata = serde_json::from_slice(&output.stdout)?;
    Ok(metadata)
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
    repo: Option<&String>,
    metadata: &GhPrMetadata,
    comment: &core::Comment,
    body: &str,
) -> Result<()> {
    use std::process::Command;

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
        args.push(repo.clone());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api inline comment failed: {}", stderr.trim());
    }

    Ok(())
}

fn post_pr_comment(pr_number: &str, repo: Option<&String>, body: &str) -> Result<()> {
    use std::process::Command;

    let mut args = vec![
        "pr".to_string(),
        "comment".to_string(),
        pr_number.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.clone());
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
    repo: Option<&String>,
    metadata: &GhPrMetadata,
    comments: &[core::Comment],
    rule_priority: &[String],
) -> Result<()> {
    use std::process::Command;

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
        args.push(repo.clone());
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
            patch_args.push(repo.clone());
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
