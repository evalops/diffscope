#[path = "command/input.rs"]
mod input;
#[path = "command/store.rs"]
mod store;

use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::review;

use super::conventions::record_convention_feedback;
use input::{load_feedback_command_input, FeedbackAction};
use store::apply_feedback_store_update;

pub async fn feedback_command(
    mut config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<()> {
    let command_input = load_feedback_command_input(&config, accept, reject, feedback_path).await?;
    config.feedback_path = command_input.feedback_path.clone();

    let updated = apply_feedback_store_update(
        &command_input.feedback_path,
        command_input.action,
        &command_input.comments,
    )?;

    println!(
        "Updated feedback store at {} ({} {} comment(s))",
        command_input.feedback_path.display(),
        updated,
        command_input.action.as_str()
    );

    let is_accepted = matches!(command_input.action, FeedbackAction::Accept);
    let _ =
        review::record_semantic_feedback_examples(&config, &command_input.comments, is_accepted)
            .await;
    record_convention_feedback(&config, &command_input.comments, is_accepted);

    Ok(())
}
