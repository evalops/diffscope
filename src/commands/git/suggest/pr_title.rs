use anyhow::Result;

use crate::config;
use crate::core;

use super::request::complete_suggestion;
use super::response::extract_title_from_response;

pub(in super::super) async fn suggest_pr_title(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let base_branch = git
        .get_default_branch()
        .unwrap_or_else(|_| "main".to_string());
    let diff_content = git.get_branch_diff(&base_branch)?;

    if diff_content.is_empty() {
        println!("No changes found compared to {base_branch} branch.");
        return Ok(());
    }

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_pr_title_prompt(&diff_content);
    let response = complete_suggestion(&config, system_prompt, user_prompt, 200).await?;
    let title = extract_title_from_response(&response);

    println!("\nSuggested PR title:");
    println!("{title}");

    if title.len() > 65 {
        println!(
            "\nWarning: PR title exceeds 65 characters ({})",
            title.len()
        );
    }

    Ok(())
}
