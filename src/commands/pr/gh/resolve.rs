use anyhow::Result;
use std::process::Command;

pub(in super::super) fn resolve_pr_number(
    number: Option<u32>,
    repo: Option<&str>,
) -> Result<String> {
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
