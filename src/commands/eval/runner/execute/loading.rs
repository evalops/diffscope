#[path = "loading/diff.rs"]
mod diff;
#[path = "loading/repo.rs"]
mod repo;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::core::eval_benchmarks::{BenchmarkThresholds, Difficulty};

use super::super::super::{EvalFixture, EvalFixtureMetadata, LoadedEvalFixture};
use diff::load_diff_content;
use repo::resolve_repo_path;

pub(super) struct PreparedFixtureExecution {
    pub(super) fixture_name: String,
    pub(super) fixture: EvalFixture,
    pub(super) suite_name: Option<String>,
    pub(super) suite_thresholds: Option<BenchmarkThresholds>,
    pub(super) difficulty: Option<Difficulty>,
    pub(super) metadata: Option<EvalFixtureMetadata>,
    pub(super) diff_content: String,
    pub(super) repo_path: PathBuf,
}

pub(super) fn prepare_fixture_execution(
    loaded_fixture: LoadedEvalFixture,
) -> Result<PreparedFixtureExecution> {
    let LoadedEvalFixture {
        fixture_path,
        fixture,
        suite_name,
        suite_thresholds,
        difficulty,
        metadata,
    } = loaded_fixture;
    let fixture_name = fixture.name.clone().unwrap_or_else(|| {
        fixture_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("fixture")
            .to_string()
    });
    let fixture_dir = fixture_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let diff_content = load_diff_content(&fixture_name, &fixture_dir, &fixture)?;
    let repo_path = resolve_repo_path(&fixture_dir, &fixture);

    Ok(PreparedFixtureExecution {
        fixture_name,
        fixture,
        suite_name,
        suite_thresholds,
        difficulty,
        metadata,
        diff_content,
        repo_path,
    })
}
