use anyhow::Result;
use std::path::Path;

use crate::core;
use crate::review;

use super::super::apply::{apply_feedback_accept, apply_feedback_reject};
use super::input::FeedbackAction;

pub(super) fn apply_feedback_store_update(
    feedback_path: &Path,
    action: FeedbackAction,
    comments: &[core::Comment],
) -> Result<usize> {
    let mut store = review::load_feedback_store_from_path(feedback_path);
    let updated = match action {
        FeedbackAction::Accept => apply_feedback_accept(&mut store, comments),
        FeedbackAction::Reject => apply_feedback_reject(&mut store, comments),
    };

    review::save_feedback_store(feedback_path, &store)?;
    Ok(updated)
}
