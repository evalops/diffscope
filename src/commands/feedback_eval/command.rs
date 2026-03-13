#[path = "command/load.rs"]
mod load;
#[path = "command/report.rs"]
mod report;

use anyhow::Result;
use std::path::PathBuf;

use load::{load_eval_report_for_feedback, load_feedback_eval_or_bail};
use report::emit_feedback_eval_report;

pub async fn feedback_eval_command(
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    confidence_threshold: f32,
    eval_report_path: Option<PathBuf>,
) -> Result<()> {
    let loaded = load_feedback_eval_or_bail(&input_path).await?;
    let eval_report = match eval_report_path.as_deref() {
        Some(path) => Some(load_eval_report_for_feedback(path)?),
        None => None,
    };
    emit_feedback_eval_report(
        &loaded,
        output_path.as_deref(),
        confidence_threshold,
        eval_report.as_ref(),
    )
    .await
}
