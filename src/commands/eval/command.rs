#[path = "command/fixtures.rs"]
mod fixtures;
#[path = "command/options.rs"]
mod options;
#[path = "command/report.rs"]
mod report;

use anyhow::Result;
use std::path::PathBuf;

use crate::config;

use super::EvalRunOptions;
use fixtures::run_eval_fixtures;
use options::prepare_eval_options;
use report::emit_eval_report;

pub async fn eval_command(
    config: config::Config,
    fixtures_dir: PathBuf,
    output_path: Option<PathBuf>,
    options: EvalRunOptions,
) -> Result<()> {
    let results = run_eval_fixtures(&config, &fixtures_dir).await?;
    let prepared_options = prepare_eval_options(&options)?;
    emit_eval_report(results, output_path.as_deref(), prepared_options).await
}
