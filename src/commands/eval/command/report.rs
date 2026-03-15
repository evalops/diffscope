use anyhow::Result;
use std::path::Path;

use super::super::report::{
    build_eval_report, evaluation_failure_message, print_eval_report, update_eval_quality_trend,
    write_eval_report,
};
use super::super::{EvalFixtureResult, EvalReport, EvalRunMetadata};
use super::options::PreparedEvalOptions;

pub(super) async fn emit_eval_report(
    config: &crate::config::Config,
    results: Vec<EvalFixtureResult>,
    output_path: Option<&Path>,
    prepared_options: PreparedEvalOptions,
    run_metadata: EvalRunMetadata,
) -> Result<()> {
    let report = materialize_eval_report(
        config,
        results,
        output_path,
        prepared_options,
        run_metadata,
        true,
    )
    .await?;

    if let Some(message) = evaluation_failure_message(&report) {
        anyhow::bail!("{}", message);
    }

    Ok(())
}

pub(super) async fn materialize_eval_report(
    config: &crate::config::Config,
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

    if report.fixtures_failed > 0
        || !report.warnings.is_empty()
        || !report.threshold_failures.is_empty()
    {
        let trigger = if report.fixtures_failed > 0 {
            "eval_failures"
        } else if !report.threshold_failures.is_empty() {
            "eval_threshold_failures"
        } else {
            "eval_warnings"
        };
        let artifact_dir = report
            .run
            .artifact_dir
            .as_ref()
            .map(std::path::PathBuf::from);
        let manifest = crate::forensics::write_eval_forensics_bundle(
            config,
            crate::forensics::EvalForensicsBundleInput {
                trigger: trigger.to_string(),
                report: report.clone(),
                artifact_dir,
            },
        )
        .await?;
        println!("Forensics bundle: {}", manifest.root_path);
    }

    Ok(report)
}
