use anyhow::Result;
use std::path::Path;

use super::super::report::{
    build_eval_report, evaluation_failure_message, print_eval_report, write_eval_report,
};
use super::super::EvalFixtureResult;
use super::options::PreparedEvalOptions;

pub(super) async fn emit_eval_report(
    results: Vec<EvalFixtureResult>,
    output_path: Option<&Path>,
    prepared_options: PreparedEvalOptions,
) -> Result<()> {
    let report = build_eval_report(
        results,
        prepared_options.baseline.as_ref(),
        &prepared_options.threshold_options,
    );
    print_eval_report(&report);

    if let Some(path) = output_path {
        write_eval_report(&report, path).await?;
    }

    if let Some(message) = evaluation_failure_message(&report) {
        anyhow::bail!("{}", message);
    }

    Ok(())
}
