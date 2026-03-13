use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::core::eval_benchmarks::CommunityFixturePack;

use super::{EvalExpectations, EvalFixture, EvalPattern, EvalReport, LoadedEvalFixture};

pub(super) fn collect_fixture_paths(fixtures_dir: &Path) -> Result<Vec<PathBuf>> {
    if !fixtures_dir.exists() {
        anyhow::bail!("Fixtures directory not found: {}", fixtures_dir.display());
    }
    if !fixtures_dir.is_dir() {
        anyhow::bail!(
            "Fixtures path is not a directory: {}",
            fixtures_dir.display()
        );
    }

    let mut paths = Vec::new();
    let mut stack = vec![fixtures_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if matches!(extension.as_deref(), Some("json" | "yml" | "yaml")) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

pub(super) fn collect_eval_fixtures(fixtures_dir: &Path) -> Result<Vec<LoadedEvalFixture>> {
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

fn expand_community_fixture_pack(
    path: &Path,
    pack: CommunityFixturePack,
) -> Result<Vec<LoadedEvalFixture>> {
    let pack_name = pack.name;
    let thresholds = pack.thresholds;
    pack.fixtures
        .into_iter()
        .map(|fixture| {
            let difficulty = fixture.difficulty.clone();
            let eval_fixture = EvalFixture {
                name: Some(format!("{}/{}", pack_name, fixture.name)),
                diff: Some(fixture.diff_content),
                diff_file: None,
                repo_path: None,
                expect: EvalExpectations {
                    must_find: fixture
                        .expected_findings
                        .into_iter()
                        .map(|finding| EvalPattern {
                            file: finding.file_pattern,
                            line: finding.line_hint,
                            contains: finding.contains,
                            severity: finding.severity,
                            category: finding.category,
                            rule_id: finding.rule_id.clone(),
                            require_rule_id: finding.rule_id.is_some(),
                            ..Default::default()
                        })
                        .collect(),
                    must_not_find: fixture
                        .negative_findings
                        .into_iter()
                        .map(|finding| EvalPattern {
                            file: finding.file_pattern,
                            contains: finding.contains,
                            ..Default::default()
                        })
                        .collect(),
                    min_total: None,
                    max_total: None,
                },
            };
            validate_eval_fixture(&eval_fixture)?;

            Ok(LoadedEvalFixture {
                fixture_path: path.to_path_buf(),
                fixture: eval_fixture,
                suite_name: Some(pack_name.clone()),
                suite_thresholds: thresholds.clone(),
                difficulty: Some(difficulty),
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn validate_eval_fixture(fixture: &EvalFixture) -> Result<()> {
    for pattern in fixture
        .expect
        .must_find
        .iter()
        .chain(fixture.expect.must_not_find.iter())
    {
        if let Some(pattern_text) = pattern.matches_regex.as_deref().map(str::trim) {
            if !pattern_text.is_empty() {
                Regex::new(pattern_text).map_err(|error| {
                    anyhow::anyhow!(
                        "Invalid regex '{}' in fixture '{}': {}",
                        pattern_text,
                        fixture.name.as_deref().unwrap_or("<unnamed>"),
                        error
                    )
                })?;
            }
        }
    }
    Ok(())
}

pub(super) fn load_eval_report(path: &Path) -> Result<EvalReport> {
    let content = std::fs::read_to_string(path)?;
    let report: EvalReport = serde_json::from_str(&content)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::eval_benchmarks::{
        BenchmarkFixture, BenchmarkThresholds, Difficulty, ExpectedFinding, NegativeFinding,
    };
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn test_load_eval_fixtures_from_path_expands_benchmark_pack() {
        let dir = tempdir().unwrap();
        let pack_path = dir.path().join("pack.json");
        let pack = CommunityFixturePack {
            name: "owasp-top10".to_string(),
            author: "community".to_string(),
            version: "1.0.0".to_string(),
            description: "security regressions".to_string(),
            languages: vec!["python".to_string()],
            categories: vec!["security".to_string()],
            thresholds: Some(BenchmarkThresholds {
                min_precision: 0.8,
                min_recall: 0.7,
                min_f1: 0.75,
                max_false_positive_rate: 0.1,
                min_weighted_score: 0.77,
            }),
            metadata: HashMap::new(),
            fixtures: vec![BenchmarkFixture {
                name: "sql-injection".to_string(),
                category: "security".to_string(),
                language: "python".to_string(),
                difficulty: Difficulty::Easy,
                diff_content: "diff --git a/app.py b/app.py".to_string(),
                expected_findings: vec![ExpectedFinding {
                    description: "detect sql injection".to_string(),
                    severity: Some("error".to_string()),
                    category: Some("security".to_string()),
                    file_pattern: Some("app.py".to_string()),
                    line_hint: Some(12),
                    contains: Some("sql injection".to_string()),
                    rule_id: Some("sec.sql.injection".to_string()),
                }],
                negative_findings: vec![NegativeFinding {
                    description: "no false positive on sanitizer".to_string(),
                    file_pattern: Some("app.py".to_string()),
                    contains: Some("sanitized".to_string()),
                }],
                description: None,
                source: None,
            }],
        };
        std::fs::write(&pack_path, serde_json::to_string(&pack).unwrap()).unwrap();

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        let fixture = &fixtures[0];
        assert_eq!(
            fixture.fixture.name.as_deref(),
            Some("owasp-top10/sql-injection")
        );
        assert_eq!(fixture.suite_name.as_deref(), Some("owasp-top10"));
        assert_eq!(
            fixture.fixture.diff.as_deref(),
            Some("diff --git a/app.py b/app.py")
        );
        assert_eq!(fixture.fixture.expect.must_find.len(), 1);
        assert_eq!(fixture.fixture.expect.must_not_find.len(), 1);
        assert!(fixture.fixture.expect.must_find[0].require_rule_id);
        assert_eq!(fixture.difficulty.as_ref(), Some(&Difficulty::Easy));
        assert_eq!(
            fixture.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.sql.injection")
        );
        assert_eq!(
            fixture.suite_thresholds.as_ref().map(|value| value.min_f1),
            Some(0.75)
        );
    }

    #[test]
    fn test_load_eval_fixtures_from_path_keeps_standard_fixture_shape() {
        let dir = tempdir().unwrap();
        let fixture_path = dir.path().join("standard.yml");
        std::fs::write(
            &fixture_path,
            r#"name: standard
diff: |
  diff --git a/lib.rs b/lib.rs
expect:
  must_find:
    - contains: injection
      severity: error
"#,
        )
        .unwrap();

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(fixtures[0].fixture.name.as_deref(), Some("standard"));
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].contains.as_deref(),
            Some("injection")
        );
    }

    #[test]
    fn test_collect_eval_fixtures_expands_pack_entries_in_sorted_order() {
        let dir = tempdir().unwrap();
        let standard_path = dir.path().join("b-standard.yml");
        std::fs::write(
            &standard_path,
            r#"name: standard
diff: |
  diff --git a/lib.rs b/lib.rs
expect:
  must_find:
    - contains: unwrap
"#,
        )
        .unwrap();

        let pack_path = dir.path().join("a-pack.json");
        let pack = CommunityFixturePack {
            name: "community".to_string(),
            author: "tester".to_string(),
            version: "1.0.0".to_string(),
            description: "regressions".to_string(),
            languages: vec!["rust".to_string()],
            categories: vec!["correctness".to_string()],
            thresholds: None,
            metadata: HashMap::new(),
            fixtures: vec![BenchmarkFixture {
                name: "panic".to_string(),
                category: "correctness".to_string(),
                language: "rust".to_string(),
                difficulty: Difficulty::Medium,
                diff_content: "diff --git a/lib.rs b/lib.rs".to_string(),
                expected_findings: vec![],
                negative_findings: vec![],
                description: None,
                source: None,
            }],
        };
        std::fs::write(&pack_path, serde_json::to_string(&pack).unwrap()).unwrap();

        let fixtures = collect_eval_fixtures(dir.path()).unwrap();

        assert_eq!(fixtures.len(), 2);
        assert_eq!(fixtures[0].fixture.name.as_deref(), Some("community/panic"));
        assert_eq!(fixtures[1].fixture.name.as_deref(), Some("standard"));
    }
}
