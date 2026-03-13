use anyhow::Result;
use std::path::PathBuf;

use crate::config;
use crate::core;

#[derive(Clone, Copy)]
pub(super) enum FeedbackAction {
    Accept,
    Reject,
}

impl FeedbackAction {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
        }
    }
}

pub(super) struct FeedbackCommandInput {
    pub(super) action: FeedbackAction,
    pub(super) feedback_path: PathBuf,
    pub(super) comments: Vec<core::Comment>,
}

pub(super) async fn load_feedback_command_input(
    config: &config::Config,
    accept: Option<PathBuf>,
    reject: Option<PathBuf>,
    feedback_path: Option<PathBuf>,
) -> Result<FeedbackCommandInput> {
    let (action, input_path) = match (accept, reject) {
        (Some(path), None) => (FeedbackAction::Accept, path),
        (None, Some(path)) => (FeedbackAction::Reject, path),
        _ => {
            anyhow::bail!("Specify exactly one of --accept or --reject");
        }
    };

    let feedback_path = feedback_path.unwrap_or_else(|| config.feedback_path.clone());
    let content = tokio::fs::read_to_string(&input_path).await?;
    let mut comments: Vec<core::Comment> = serde_json::from_str(&content)?;
    normalize_comment_ids(&mut comments);

    Ok(FeedbackCommandInput {
        action,
        feedback_path,
        comments,
    })
}

fn normalize_comment_ids(comments: &mut [core::Comment]) {
    for comment in comments {
        if comment.id.trim().is_empty() {
            comment.id = core::comment::compute_comment_id(
                &comment.file_path,
                &comment.content,
                &comment.category,
            );
        }
    }
}
