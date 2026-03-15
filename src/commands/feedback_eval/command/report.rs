use anyhow::Result;
use std::path::Path;

use crate::commands::eval::EvalReport;

use super::super::report::{
    build_feedback_eval_report, print_feedback_eval_report, update_feedback_eval_trend,
    write_feedback_eval_report,
};
use super::super::LoadedFeedbackEvalInput;

pub(super) async fn emit_feedback_eval_report(
    loaded: &LoadedFeedbackEvalInput,
    output_path: Option<&Path>,
    trend_path: Option<&Path>,
    trend_max_entries: usize,
    confidence_threshold: f32,
    eval_report: Option<&EvalReport>,
) -> Result<()> {
    let report =
        build_feedback_eval_report(loaded, confidence_threshold.clamp(0.0, 1.0), eval_report);
    print_feedback_eval_report(&report);

    if let Some(path) = output_path {
        write_feedback_eval_report(&report, path).await?;
    }
    if let Some(path) = trend_path {
        update_feedback_eval_trend(&report, eval_report, path, trend_max_entries).await?;
    }

    Ok(())
}
