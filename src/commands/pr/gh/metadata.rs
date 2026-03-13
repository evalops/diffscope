use anyhow::Result;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub(in super::super) struct GhPrMetadata {
    #[serde(rename = "headRefOid")]
    pub(in super::super) head_ref_oid: String,
    #[serde(rename = "baseRepository")]
    pub(in super::super) base_repository: GhBaseRepository,
}

#[derive(Debug, Deserialize)]
pub(in super::super) struct GhBaseRepository {
    #[serde(rename = "nameWithOwner")]
    pub(in super::super) name_with_owner: String,
}

pub(in super::super) fn fetch_pr_metadata(
    pr_number: &str,
    repo: Option<&str>,
) -> Result<GhPrMetadata> {
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
