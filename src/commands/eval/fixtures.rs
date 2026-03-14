#[path = "fixtures/discovery.rs"]
mod discovery;
#[path = "fixtures/loading.rs"]
mod loading;
#[path = "fixtures/packs.rs"]
mod packs;
#[path = "fixtures/validation.rs"]
mod validation;

pub(super) use loading::{collect_eval_fixtures, load_eval_report};

#[cfg(test)]
use loading::load_eval_fixtures_from_path;

#[cfg(test)]
mod tests {
    use super::{collect_eval_fixtures, load_eval_fixtures_from_path};
    use crate::core::eval_benchmarks::{
        BenchmarkFixture, BenchmarkThresholds, CommunityFixturePack, Difficulty, ExpectedFinding,
        NegativeFinding,
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
                repo_path: Some("fixtures_repo".to_string()),
                expected_findings: vec![ExpectedFinding {
                    description: "detect sql injection".to_string(),
                    severity: Some("error".to_string()),
                    category: Some("security".to_string()),
                    file_pattern: Some("app.py".to_string()),
                    line_hint: Some(12),
                    contains: Some("sql injection".to_string()),
                    contains_any: vec!["unsafe sql".to_string()],
                    tags_any: vec!["sql-injection".to_string()],
                    confidence_at_least: Some(0.7),
                    confidence_at_most: None,
                    fix_effort: Some("medium".to_string()),
                    rule_id: Some("sec.sql.injection".to_string()),
                    rule_id_aliases: vec!["security.sql-injection".to_string()],
                }],
                negative_findings: vec![NegativeFinding {
                    description: "no false positive on sanitizer".to_string(),
                    file_pattern: Some("app.py".to_string()),
                    contains: Some("sanitized".to_string()),
                    contains_any: vec!["escaped".to_string()],
                }],
                min_total: Some(1),
                max_total: Some(5),
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
        assert_eq!(
            fixture.fixture.repo_path,
            Some(std::path::PathBuf::from("fixtures_repo"))
        );
        assert_eq!(fixture.fixture.expect.must_find.len(), 1);
        assert_eq!(fixture.fixture.expect.must_not_find.len(), 1);
        assert!(fixture.fixture.expect.must_find[0].require_rule_id);
        assert_eq!(fixture.fixture.expect.must_find[0].contains_any.len(), 1);
        assert_eq!(fixture.fixture.expect.must_find[0].tags_any.len(), 1);
        assert_eq!(fixture.fixture.expect.min_total, Some(1));
        assert_eq!(fixture.fixture.expect.max_total, Some(5));
        assert_eq!(fixture.difficulty.as_ref(), Some(&Difficulty::Easy));
        assert_eq!(
            fixture
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.category.as_deref()),
            Some("security")
        );
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
                repo_path: None,
                expected_findings: vec![],
                negative_findings: vec![],
                min_total: None,
                max_total: None,
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

    #[test]
    fn test_checked_in_infra_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_infra.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 4);
        assert_eq!(
            fixtures[0].suite_name.as_deref(),
            Some("review-depth-infra")
        );
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("review-depth-infra/docker-user-root")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.infra.docker-root")
        );
        assert_eq!(
            fixtures[1]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.language.as_deref()),
            Some("hcl")
        );
        assert_eq!(
            fixtures[2].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.supply-chain.ci-injection")
        );
        assert_eq!(
            fixtures[3].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.infra.k8s-privileged")
        );
    }
}
