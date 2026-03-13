use anyhow::Result;
use std::path::Path;

use super::super::input::load_feedback_eval_input;
use super::super::LoadedFeedbackEvalInput;
use crate::commands::eval::EvalReport;

pub(super) async fn load_feedback_eval_or_bail(path: &Path) -> Result<LoadedFeedbackEvalInput> {
    let loaded = load_feedback_eval_input(path).await?;
    if loaded.comments.is_empty() {
        anyhow::bail!(
            "No accepted/rejected feedback examples found in {}",
            path.display()
        );
    }

    Ok(loaded)
}

pub(super) fn load_eval_report_for_feedback(path: &Path) -> Result<EvalReport> {
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}
