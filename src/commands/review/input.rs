use anyhow::Result;
use std::io::IsTerminal;
use std::path::PathBuf;

use crate::core;

pub(crate) async fn load_review_input(diff_path: Option<PathBuf>) -> Result<(PathBuf, String)> {
    let repo_root = core::GitIntegration::new(".")
        .ok()
        .and_then(|git| git.workdir())
        .unwrap_or_else(|| PathBuf::from("."));

    let diff_content = if let Some(path) = diff_path {
        tokio::fs::read_to_string(path).await?
    } else if std::io::stdin().is_terminal() {
        if let Ok(git) = core::GitIntegration::new(".") {
            let diff = git.get_uncommitted_diff()?;
            if diff.is_empty() {
                println!("No changes found");
                return Ok((repo_root, String::new()));
            }
            diff
        } else {
            println!("No diff provided and not in a git repository.");
            return Ok((repo_root, String::new()));
        }
    } else {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    Ok((repo_root, diff_content))
}
