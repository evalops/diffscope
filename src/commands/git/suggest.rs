use anyhow::Result;

use crate::adapters;
use crate::config;
use crate::core;

pub(super) async fn suggest_commit_message(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let diff_content = git.get_staged_diff()?;

    if diff_content.is_empty() {
        println!("No staged changes found. Stage your changes with 'git add' first.");
        return Ok(());
    }

    let model_config = config.to_model_config_for_role(config::ModelRole::Fast);
    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_commit_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(500),
        response_schema: None,
    };

    let response = adapter.complete(request).await?;
    let commit_message = core::CommitPromptBuilder::extract_commit_message(&response.content);

    println!("\nSuggested commit message:");
    println!("{}", commit_message);

    if commit_message.len() > 72 {
        println!(
            "\n⚠️  Warning: Commit message exceeds 72 characters ({})",
            commit_message.len()
        );
    }

    Ok(())
}

pub(super) async fn suggest_pr_title(config: config::Config) -> Result<()> {
    let git = core::GitIntegration::new(".")?;
    let base_branch = git
        .get_default_branch()
        .unwrap_or_else(|_| "main".to_string());
    let diff_content = git.get_branch_diff(&base_branch)?;

    if diff_content.is_empty() {
        println!("No changes found compared to {} branch.", base_branch);
        return Ok(());
    }

    let model_config = config.to_model_config_for_role(config::ModelRole::Fast);
    let adapter = adapters::llm::create_adapter(&model_config)?;

    let (system_prompt, user_prompt) =
        core::CommitPromptBuilder::build_pr_title_prompt(&diff_content);

    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(200),
        response_schema: None,
    };

    let response = adapter.complete(request).await?;
    let title = extract_title_from_response(&response.content);

    println!("\nSuggested PR title:");
    println!("{}", title);

    if title.len() > 65 {
        println!(
            "\n⚠️  Warning: PR title exceeds 65 characters ({})",
            title.len()
        );
    }

    Ok(())
}

fn extract_title_from_response(content: &str) -> String {
    if let Some(start) = content.find("<title>") {
        let after_tag = start + 7;
        if let Some(end) = content[after_tag..].find("</title>") {
            content[after_tag..after_tag + end].trim().to_string()
        } else {
            content.trim().to_string()
        }
    } else {
        content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title_normal() {
        let content = "<title>Fix login bug</title>";
        assert_eq!(extract_title_from_response(content), "Fix login bug");
    }

    #[test]
    fn test_extract_title_malformed_closing_before_opening() {
        let content = "Some text</title> more <title>Real Title</title>";
        let title = extract_title_from_response(content);
        assert!(!title.is_empty());
    }

    #[test]
    fn test_extract_title_no_tags() {
        let content = "Just a plain title\nSecond line";
        assert_eq!(extract_title_from_response(content), "Just a plain title");
    }
}
