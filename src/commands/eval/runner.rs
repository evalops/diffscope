use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config;
use crate::core::eval_benchmarks::FixtureResult as BenchmarkFixtureResult;
use crate::review::review_diff_content_raw;

use super::metrics::{compute_rule_metrics, summarize_rule_metrics};
use super::pattern::summarize_for_eval;
use super::{EvalFixtureResult, LoadedEvalFixture};

pub(super) async fn run_eval_fixture(
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
    let fixture_name = fixture.name.unwrap_or_else(|| {
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

    let diff_content = match (fixture.diff, fixture.diff_file) {
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
    let mut failures = Vec::new();
    let mut required_matches = 0usize;
    let required_total = fixture.expect.must_find.len();
    let mut used_comment_indices = HashSet::new();
    let mut unexpected_comment_indices = HashSet::new();
    let mut matched_pairs = Vec::new();

    for (expected_idx, expected) in fixture.expect.must_find.iter().enumerate() {
        let found = comments
            .iter()
            .enumerate()
            .find(|(comment_idx, comment)| {
                !used_comment_indices.contains(comment_idx) && expected.matches(comment)
            })
            .map(|(comment_idx, _)| comment_idx);

        if let Some(comment_idx) = found {
            used_comment_indices.insert(comment_idx);
            matched_pairs.push((expected_idx, comment_idx));
            required_matches = required_matches.saturating_add(1);
        } else {
            failures.push(format!("Missing expected finding: {}", expected.describe()));
        }
    }

    for unexpected in &fixture.expect.must_not_find {
        if let Some((comment_idx, comment)) = comments
            .iter()
            .enumerate()
            .find(|(_, comment)| unexpected.matches(comment))
        {
            unexpected_comment_indices.insert(comment_idx);
            failures.push(format!(
                "Unexpected finding matched {}:{} '{}'",
                comment.file_path.display(),
                comment.line_number,
                summarize_for_eval(&comment.content)
            ));
        }
    }

    let rule_metrics = compute_rule_metrics(&fixture.expect.must_find, &comments, &matched_pairs);
    let rule_summary = summarize_rule_metrics(&rule_metrics);

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
        let accounted_for = used_comment_indices
            .union(&unexpected_comment_indices)
            .count();
        let extra_findings = total_comments.saturating_sub(accounted_for);
        let mut result = BenchmarkFixtureResult::compute(
            &fixture_name,
            fixture.expect.must_find.len(),
            fixture.expect.must_not_find.len(),
            required_matches,
            unexpected_comment_indices.len(),
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
        required_matches,
        required_total,
        benchmark_metrics,
        suite_thresholds,
        difficulty,
        rule_metrics,
        rule_summary,
        failures,
    })
}
