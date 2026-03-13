use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::eval_benchmarks::{BenchmarkThresholds, Difficulty};

use super::pattern::EvalExpectations;

#[derive(Debug, Clone, Deserialize, Default)]
pub(in super::super) struct EvalFixture {
    #[serde(default)]
    pub(in super::super) name: Option<String>,
    #[serde(default)]
    pub(in super::super) diff: Option<String>,
    #[serde(default)]
    pub(in super::super) diff_file: Option<PathBuf>,
    #[serde(default)]
    pub(in super::super) repo_path: Option<PathBuf>,
    #[serde(default)]
    pub(in super::super) expect: EvalExpectations,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(in super::super) struct EvalFixtureMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in super::super) description: Option<String>,
}

#[derive(Debug, Clone)]
pub(in super::super) struct LoadedEvalFixture {
    pub(in super::super) fixture_path: PathBuf,
    pub(in super::super) fixture: EvalFixture,
    pub(in super::super) suite_name: Option<String>,
    pub(in super::super) suite_thresholds: Option<BenchmarkThresholds>,
    pub(in super::super) difficulty: Option<Difficulty>,
    pub(in super::super) metadata: Option<EvalFixtureMetadata>,
}
