#[path = "loading/format.rs"]
mod format;
#[path = "loading/parse.rs"]
mod parse;

use anyhow::Result;
use std::path::Path;

use super::super::LoadedFeedbackEvalInput;
use format::detect_feedback_eval_input_format;
use parse::load_feedback_eval_input_from_value;

pub(in super::super) async fn load_feedback_eval_input(
    path: &Path,
) -> Result<LoadedFeedbackEvalInput> {
    let content = tokio::fs::read_to_string(path).await?;
    load_feedback_eval_input_from_str(&content)
}

pub(in super::super) fn load_feedback_eval_input_from_str(
    content: &str,
) -> Result<LoadedFeedbackEvalInput> {
    let value = serde_json::from_str(content).map_err(|_| {
        anyhow::anyhow!(
            "Unsupported feedback eval input format: expected reviews.json, a comments array, or semantic feedback store JSON"
        )
    })?;
    let Some(input_format) = detect_feedback_eval_input_format(&value) else {
        anyhow::bail!(
            "Unsupported feedback eval input format: expected reviews.json, a comments array, or semantic feedback store JSON"
        );
    };
    load_feedback_eval_input_from_value(value, input_format)
}
