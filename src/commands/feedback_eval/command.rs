#[path = "command/load.rs"]
mod load;
#[path = "command/report.rs"]
mod report;

use anyhow::Result;
use std::path::PathBuf;

use load::load_feedback_eval_or_bail;
use report::emit_feedback_eval_report;

pub async fn feedback_eval_command(
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    confidence_threshold: f32,
) -> Result<()> {
    let loaded = load_feedback_eval_or_bail(&input_path).await?;
    emit_feedback_eval_report(&loaded, output_path.as_deref(), confidence_threshold).await
}
