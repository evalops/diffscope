use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

use crate::core::eval_benchmarks::CommunityFixturePack;

use super::super::{EvalFixture, EvalReport, LoadedEvalFixture};
use super::discovery::collect_fixture_paths;
use super::packs::expand_community_fixture_pack;
use super::validation::validate_eval_fixture;

pub(in super::super) fn collect_eval_fixtures(
    fixtures_dir: &Path,
) -> Result<Vec<LoadedEvalFixture>> {
    let mut fixtures = Vec::new();
    for path in collect_fixture_paths(fixtures_dir)? {
        fixtures.extend(load_eval_fixtures_from_path(&path)?);
    }
    fixtures.sort_by(|left, right| {
        left.fixture_path
            .cmp(&right.fixture_path)
            .then_with(|| left.fixture.name.cmp(&right.fixture.name))
    });
    Ok(fixtures)
}

pub(super) fn load_eval_fixtures_from_path(path: &Path) -> Result<Vec<LoadedEvalFixture>> {
    let content = std::fs::read_to_string(path)?;

    if let Ok(pack) = load_fixture_file::<CommunityFixturePack>(path, &content) {
        return expand_community_fixture_pack(path, pack);
    }

    let fixture = load_eval_fixture_from_content(path, &content)?;
    Ok(vec![LoadedEvalFixture {
        fixture_path: path.to_path_buf(),
        fixture,
        suite_name: None,
        suite_thresholds: None,
        difficulty: None,
    }])
}

pub(in super::super) fn load_eval_report(path: &Path) -> Result<EvalReport> {
    let content = std::fs::read_to_string(path)?;
    let report: EvalReport = serde_json::from_str(&content)?;
    Ok(report)
}

fn load_eval_fixture_from_content(path: &Path, content: &str) -> Result<EvalFixture> {
    let fixture = load_fixture_file::<EvalFixture>(path, content)?;
    validate_eval_fixture(&fixture)?;
    Ok(fixture)
}

fn load_fixture_file<T>(path: &Path, content: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Ok(serde_json::from_str(content)?),
        _ => match serde_yaml::from_str(content) {
            Ok(parsed) => Ok(parsed),
            Err(_) => Ok(serde_json::from_str(content)?),
        },
    }
}
