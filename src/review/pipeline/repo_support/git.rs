use std::path::Path;
use tracing::info;

pub(in super::super) fn gather_git_log(repo_path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--numstat",
            "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ai%n%n    %s%n",
            "-100",
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let log_text = String::from_utf8_lossy(&out.stdout).to_string();
            if log_text.trim().is_empty() {
                None
            } else {
                info!("Gathered git log ({} bytes)", log_text.len());
                Some(log_text)
            }
        }
        _ => {
            info!("Git log unavailable (not a git repo or git not found)");
            None
        }
    }
}
