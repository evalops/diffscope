use anyhow::Result;
use std::path::Path;

use crate::config;

use super::super::fixtures::collect_eval_fixtures;
use super::super::runner::{run_eval_fixture, EvalFixtureArtifactContext};
use super::super::{EvalFixtureResult, EvalRunOptions, LoadedEvalFixture};

pub(super) struct EvalFixtureExecution {
    pub(super) discovered_count: usize,
    pub(super) selected_count: usize,
    pub(super) results: Vec<EvalFixtureResult>,
}

pub(super) async fn run_eval_fixtures(
    config: &config::Config,
    fixtures_dir: &Path,
    options: &EvalRunOptions,
) -> Result<EvalFixtureExecution> {
    let fixtures = collect_eval_fixtures(fixtures_dir)?;
    let discovered_count = fixtures.len();
    if fixtures.is_empty() {
        anyhow::bail!(
            "No fixture files found in {} (expected .json/.yml/.yaml)",
            fixtures_dir.display()
        );
    }

    let fixtures = filter_fixtures(fixtures, options);
    if fixtures.is_empty() {
        anyhow::bail!(
            "No fixtures matched the selected filters in {}",
            fixtures_dir.display()
        );
    }

    let selected_count = fixtures.len();
    let artifact_context =
        options
            .artifact_dir
            .as_ref()
            .map(|artifact_dir| EvalFixtureArtifactContext {
                artifact_dir: artifact_dir.clone(),
                run_label: options.label.clone(),
                model: config.model.clone(),
            });

    let mut results = Vec::new();
    for fixture in fixtures {
        results.push(run_eval_fixture(config, fixture, artifact_context.as_ref()).await?);
    }

    Ok(EvalFixtureExecution {
        discovered_count,
        selected_count,
        results,
    })
}

fn filter_fixtures(
    fixtures: Vec<LoadedEvalFixture>,
    options: &EvalRunOptions,
) -> Vec<LoadedEvalFixture> {
    let suite_filters = normalized_filters(&options.suite_filters);
    let category_filters = normalized_filters(&options.category_filters);
    let language_filters = normalized_filters(&options.language_filters);
    let fixture_name_filters = normalized_filters(&options.fixture_name_filters);

    let mut filtered = fixtures
        .into_iter()
        .filter(|fixture| {
            matches_fixture_filters(
                fixture,
                &suite_filters,
                &category_filters,
                &language_filters,
                &fixture_name_filters,
            )
        })
        .collect::<Vec<_>>();

    if let Some(max_fixtures) = options.max_fixtures {
        filtered.truncate(max_fixtures);
    }

    filtered
}

fn matches_fixture_filters(
    fixture: &LoadedEvalFixture,
    suite_filters: &[String],
    category_filters: &[String],
    language_filters: &[String],
    fixture_name_filters: &[String],
) -> bool {
    matches_exact_filter(suite_filters, fixture.suite_name.as_deref())
        && matches_exact_filter(
            category_filters,
            fixture
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.category.as_deref()),
        )
        && matches_exact_filter(
            language_filters,
            fixture
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.language.as_deref()),
        )
        && matches_substring_filter(fixture_name_filters, fixture.fixture.name.as_deref())
}

fn matches_exact_filter(filters: &[String], value: Option<&str>) -> bool {
    if filters.is_empty() {
        return true;
    }

    let Some(value) = value else {
        return false;
    };
    let normalized = value.trim().to_ascii_lowercase();
    filters.iter().any(|filter| filter == &normalized)
}

fn matches_substring_filter(filters: &[String], value: Option<&str>) -> bool {
    if filters.is_empty() {
        return true;
    }

    let Some(value) = value else {
        return false;
    };
    let normalized = value.trim().to_ascii_lowercase();
    filters.iter().any(|filter| normalized.contains(filter))
}

fn normalized_filters(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::commands::eval::{
        EvalExpectations, EvalFixture, EvalFixtureMetadata, EvalRunOptions, LoadedEvalFixture,
    };
    use crate::core::eval_benchmarks::{BenchmarkThresholds, Difficulty};

    use super::filter_fixtures;

    fn loaded_fixture(
        name: &str,
        suite: Option<&str>,
        category: Option<&str>,
        language: Option<&str>,
    ) -> LoadedEvalFixture {
        LoadedEvalFixture {
            fixture_path: PathBuf::from(format!("{}.yml", name)),
            fixture: EvalFixture {
                name: Some(name.to_string()),
                diff: Some("diff --git a/a b/b".to_string()),
                diff_file: None,
                repo_path: None,
                expect: EvalExpectations::default(),
            },
            suite_name: suite.map(|value| value.to_string()),
            suite_thresholds: Some(BenchmarkThresholds::default()),
            difficulty: Some(Difficulty::Medium),
            metadata: Some(EvalFixtureMetadata {
                category: category.map(|value| value.to_string()),
                language: language.map(|value| value.to_string()),
                source: None,
                description: None,
            }),
        }
    }

    #[test]
    fn filter_fixtures_applies_suite_category_language_and_name_filters() {
        let fixtures = vec![
            loaded_fixture(
                "review-depth-core/rust-shell-command-injection",
                Some("review-depth-core"),
                Some("security"),
                Some("rust"),
            ),
            loaded_fixture(
                "review-depth-core/python-n-plus-one-query",
                Some("review-depth-core"),
                Some("performance"),
                Some("python"),
            ),
        ];

        let filtered = filter_fixtures(
            fixtures,
            &EvalRunOptions {
                baseline_report: None,
                max_micro_f1_drop: None,
                max_suite_f1_drop: None,
                max_category_f1_drop: None,
                max_language_f1_drop: None,
                min_micro_f1: None,
                min_macro_f1: None,
                min_rule_f1: Vec::new(),
                max_rule_f1_drop: Vec::new(),
                matrix_models: Vec::new(),
                repeat: 1,
                suite_filters: vec!["review-depth-core".to_string()],
                category_filters: vec!["security".to_string()],
                language_filters: vec!["rust".to_string()],
                fixture_name_filters: vec!["shell".to_string()],
                max_fixtures: None,
                label: None,
                trend_file: None,
                artifact_dir: None,
            },
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].fixture.name.as_deref(),
            Some("review-depth-core/rust-shell-command-injection")
        );
    }

    #[test]
    fn filter_fixtures_respects_max_fixture_limit() {
        let fixtures = vec![
            loaded_fixture("a", Some("suite"), Some("security"), Some("rust")),
            loaded_fixture("b", Some("suite"), Some("security"), Some("rust")),
        ];

        let filtered = filter_fixtures(
            fixtures,
            &EvalRunOptions {
                baseline_report: None,
                max_micro_f1_drop: None,
                max_suite_f1_drop: None,
                max_category_f1_drop: None,
                max_language_f1_drop: None,
                min_micro_f1: None,
                min_macro_f1: None,
                min_rule_f1: Vec::new(),
                max_rule_f1_drop: Vec::new(),
                matrix_models: Vec::new(),
                repeat: 1,
                suite_filters: vec![],
                category_filters: vec![],
                language_filters: vec![],
                fixture_name_filters: vec![],
                max_fixtures: Some(1),
                label: None,
                trend_file: None,
                artifact_dir: None,
            },
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].fixture.name.as_deref(), Some("a"));
    }
}
