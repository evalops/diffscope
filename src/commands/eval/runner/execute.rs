use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config;
use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;
use crate::review::review_diff_content_raw;

use super::super::{EvalFixtureResult, LoadedEvalFixture};
use super::matching::evaluate_fixture_expectations;

pub(in super::super) async fn run_eval_fixture(
    config: &config::Config,
    loaded_fixture: LoadedEvalFixture,
) -> Result<EvalFixtureResult> {
    let LoadedEvalFixture {
        fixture_path,
        fixture,
        suite_name,
        suite_thresholds,
        difficulty,
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

    let diff_content = match (fixture.diff.clone(), fixture.diff_file.clone()) {
        (Some(diff), _) => diff,
        (None, Some(diff_file)) => {
            let path = if diff_file.is_absolute() {
                diff_file
            } else {
                fixture_dir.join(diff_file)
            };
            std::fs::read_to_string(path)?
        }
        (None, None) => anyhow::bail!(
            "Fixture '{}' must define either diff or diff_file",
            fixture_name
        ),
    };

    let repo_path = fixture
        .repo_path
        .clone()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                fixture_dir.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let review_result = review_diff_content_raw(&diff_content, config.clone(), &repo_path).await?;
    let comments = review_result.comments;
    let total_comments = comments.len();
    let match_summary = evaluate_fixture_expectations(&fixture.expect, &comments);
    let mut failures = match_summary.failures;

    if let Some(min_total) = fixture.expect.min_total {
        if total_comments < min_total {
            failures.push(format!(
                "Expected at least {} comments, got {}",
                min_total, total_comments
            ));
        }
    }
    if let Some(max_total) = fixture.expect.max_total {
        if total_comments > max_total {
            failures.push(format!(
                "Expected at most {} comments, got {}",
                max_total, total_comments
            ));
        }
    }

    let benchmark_metrics = suite_name.as_ref().map(|_| {
        let accounted_for = match_summary
            .used_comment_indices
            .union(&match_summary.unexpected_comment_indices)
            .count();
        let extra_findings = total_comments.saturating_sub(accounted_for);
        let mut result = BenchmarkFixtureResult::compute(
            &fixture_name,
            fixture.expect.must_find.len(),
            fixture.expect.must_not_find.len(),
            match_summary.required_matches,
            match_summary.unexpected_comment_indices.len(),
            extra_findings,
        );
        result.details = failures.clone();
        result
    });

    Ok(EvalFixtureResult {
        fixture: fixture_name,
        suite: suite_name,
        passed: failures.is_empty(),
        total_comments,
        required_matches: match_summary.required_matches,
        required_total: match_summary.required_total,
        benchmark_metrics,
        suite_thresholds,
        difficulty,
        rule_metrics: match_summary.rule_metrics,
        rule_summary: match_summary.rule_summary,
        failures,
    })
}
