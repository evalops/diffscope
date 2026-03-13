use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::core;
use crate::review;

use super::apply::{apply_feedback_accept, apply_feedback_reject};
use super::conventions::record_convention_feedback;

pub async fn feedback_command(
    mut config: config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<()> {
    let (action, input_path) = match (accept, reject) {
        (Some(path), None) => ("accept", path),
        (None, Some(path)) => ("reject", path),
        _ => {
            anyhow::bail!("Specify exactly one of --accept or --reject");
        }
    };

    let feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    config.feedback_path = feedback_path.clone();
    let content = tokio::fs::read_to_string(&input_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;

    for comment in &mut comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }

    let mut store = review::load_feedback_store_from_path(&feedback_path);
    let updated = if action == "accept" {
        apply_feedback_accept(&mut store, &comments)
    } else {
        apply_feedback_reject(&mut store, &comments)
    };

    review::save_feedback_store(&feedback_path, &store)?;
    println!(
        "Updated feedback store at {} ({} {} comment(s))",
        feedback_path.display(),
        updated,
        action
    );

    let is_accepted = action == "accept";
    let _ = review::record_semantic_feedback_examples(&config, &comments, is_accepted).await;
    record_convention_feedback(&config, &comments, is_accepted);

    Ok(())
}
