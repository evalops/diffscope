use anyhow::Result;
use std::path::PathBuf;

use super::input::load_feedback_eval_input;
use super::report::{
    build_feedback_eval_report, print_feedback_eval_report, write_feedback_eval_report,
};

pub async fn feedback_eval_command(
    input_path: PathBuf,
    output_path: Option<PathBuf>,
    confidence_threshold: f32,
) -> Result<()> {
    let loaded = load_feedback_eval_input(&input_path).await?;
    if loaded.comments.is_empty() {
        anyhow::bail!(
            "No accepted/rejected feedback examples found in {}",
            input_path.display()
        );
    }

    let report = build_feedback_eval_report(&loaded, confidence_threshold.clamp(0.0, 1.0));
    print_feedback_eval_report(&report);

    if let Some(path) = output_path.as_deref() {
        write_feedback_eval_report(&report, path).await?;
    }

    Ok(())
}
