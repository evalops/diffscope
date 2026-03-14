use anyhow::Result;

use crate::config;
use crate::core;

use super::request::complete_suggestion;

pub(in super::super) async fn suggest_commit_message(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let diff_content = git.get_staged_diff()?;

    if diff_content.is_empty() {
        println!("No staged changes found. Stage your changes with 'git add' first.");
        return Ok(());
    }

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_commit_prompt(&diff_content);
    let response = complete_suggestion(&config, system_prompt, user_prompt, 500).await?;
    let commit_message = core::CommitPromptBuilder::extract_commit_message(&response);

    println!("\nSuggested commit message:");
    println!("{commit_message}");

    if commit_message.len() > 72 {
        println!(
            "\nWarning: Commit message exceeds 72 characters ({})",
            commit_message.len()
        );
    }

    Ok(())
}
