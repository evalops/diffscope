use anyhow::Result;
use std::path::Path;

use super::super::report::{
    build_feedback_eval_report, print_feedback_eval_report, write_feedback_eval_report,
};
use super::super::LoadedFeedbackEvalInput;

pub(super) async fn emit_feedback_eval_report(
    loaded: &LoadedFeedbackEvalInput,
    output_path: Option<&Path>,
    confidence_threshold: f32,
) -> Result<()> {
    let report = build_feedback_eval_report(loaded, confidence_threshold.clamp(0.0, 1.0));
    print_feedback_eval_report(&report);

    if let Some(path) = output_path {
        write_feedback_eval_report(&report, path).await?;
    }

    Ok(())
}
