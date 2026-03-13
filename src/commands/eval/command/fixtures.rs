use anyhow::Result;
use std::path::Path;

use crate::config;

use super::super::fixtures::collect_eval_fixtures;
use super::super::runner::run_eval_fixture;
use super::super::EvalFixtureResult;

pub(super) async fn run_eval_fixtures(
    config: &config::Config,
    fixtures_dir: &Path,
) -> Result<Vec<EvalFixtureResult>> {
    let fixtures = collect_eval_fixtures(fixtures_dir)?;
    if fixtures.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let mut results = Vec::new();
    for fixture in fixtures {
        results.push(run_eval_fixture(config, fixture).await?);
    }

    Ok(results)
}
