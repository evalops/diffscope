use anyhow::Result;
use std::path::Path;

use super::super::report::{
    build_eval_report, evaluation_failure_message, print_eval_report, update_eval_quality_trend,
    write_eval_report,
};
use super::super::{EvalFixtureResult, EvalReport, EvalRunMetadata};
use super::options::PreparedEvalOptions;

pub(super) async fn emit_eval_report(
    results: Vec<EvalFixtureResult>,
    output_path: Option<&Path>,
    prepared_options: PreparedEvalOptions,
    run_metadata: EvalRunMetadata,
) -> Result<()> {
    let report =
        materialize_eval_report(results, output_path, prepared_options, run_metadata, true).await?;

    if let Some(message) = evaluation_failure_message(&report) {
        anyhow::bail!("{}", message);
    }

    Ok(())
}

pub(super) async fn materialize_eval_report(
    results: Vec<EvalFixtureResult>,
    output_path: Option<&Path>,
    prepared_options: PreparedEvalOptions,
    run_metadata: EvalRunMetadata,
    print_report: bool,
) -> Result<EvalReport> {
    let report = build_eval_report(
        results,
        prepared_options.baseline.as_ref(),
        &prepared_options.threshold_options,
        run_metadata,
    );
    if print_report {
        print_eval_report(&report);
    }

    if let Some(path) = output_path {
        write_eval_report(&report, path).await?;
    }
    if let Some(path) = prepared_options.trend_path.as_deref() {
        update_eval_quality_trend(&report, path, prepared_options.trend_max_entries).await?;
    }

    Ok(report)
}
