#[path = "command/input.rs"]
mod input;
#[path = "command/store.rs"]
mod store;

use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::review;

use super::backfill::backfill_feedback_store;
use super::conventions::record_convention_feedback;
use input::{load_feedback_command_input, FeedbackAction};
use store::apply_feedback_store_update;

pub async fn feedback_command(
    mut config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
    backfill: Option<PathBuf>,
) -> Result<()> {
    let action_count = usize::from(accept.is_some())
        + usize::from(reject.is_some())
        + usize::from(backfill.is_some());
    if action_count != 1 {
        anyhow::bail!("Specify exactly one of --accept, --reject, or --backfill");
    }

    let resolved_feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    config.feedback_path = resolved_feedback_path.clone();

    if let Some(input_path) = backfill {
        let summary = backfill_feedback_store(&input_path, &resolved_feedback_path).await?;
        println!(
            "Backfilled feedback store at {} from {} ({} reviews, {} comments, {} accepted, {} rejected, {} dismissed, {} not-addressed)",
            resolved_feedback_path.display(),
            input_path.display(),
            summary.reviews_seen,
            summary.comments_seen,
            summary.accepted,
            summary.rejected,
            summary.dismissed,
            summary.not_addressed,
        );
        return Ok(());
    }

    let command_input =
        load_feedback_command_input(&config, accept, reject, Some(resolved_feedback_path)).await?;
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
