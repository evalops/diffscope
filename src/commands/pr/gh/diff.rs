use anyhow::Result;
use std::process::Command;

pub(in super::super) fn fetch_pr_diff(pr_number: &str, repo: Option<&str>) -> Result<String> {
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
