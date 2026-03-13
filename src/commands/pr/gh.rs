use anyhow::Result;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub(super) struct GhPrMetadata {
    #[serde(rename = "headRefOid")]
    pub(super) head_ref_oid: String,
    #[serde(rename = "baseRepository")]
    pub(super) base_repository: GhBaseRepository,
}

#[derive(Debug, Deserialize)]
pub(super) struct GhBaseRepository {
    #[serde(rename = "nameWithOwner")]
    pub(super) name_with_owner: String,
}

pub(super) fn resolve_pr_number(number: Option<u32>, repo: Option<&str>) -> Result<String> {
    if let Some(num) = number {
        return Ok(num.to_string());
    }

    let mut args = vec![
        "pr".to_string(),
        "view".to_string(),
        "--json".to_string(),
        "number".to_string(),
        "-q".to_string(),
        ".number".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.to_string());
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
    Ok(pr_number)
}

pub(super) fn fetch_pr_diff(pr_number: &str, repo: Option<&str>) -> Result<String> {
    let mut diff_args = vec!["pr".to_string(), "diff".to_string(), pr_number.to_string()];
    if let Some(repo) = repo {
        diff_args.push("--repo".to_string());
        diff_args.push(repo.to_string());
    }
    let diff_output = Command::new("gh").args(&diff_args).output()?;
    if !diff_output.status.success() {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        anyhow::bail!("gh pr diff failed: {}", stderr.trim());
    }

    Ok(String::from_utf8(diff_output.stdout)?)
}

pub(super) fn fetch_pr_metadata(pr_number: &str, repo: Option<&str>) -> Result<GhPrMetadata> {
    let mut args = vec![
        "pr".to_string(),
        "view".to_string(),
        pr_number.to_string(),
        "--json".to_string(),
        "headRefOid,baseRepository".to_string(),
    ];
    if let Some(repo) = repo {
        args.push("--repo".to_string());
        args.push(repo.to_string());
    }

    let output = Command::new("gh").args(&args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view metadata failed: {}", stderr.trim());
    }

    let metadata: GhPrMetadata = serde_json::from_slice(&output.stdout)?;
    Ok(metadata)
}
