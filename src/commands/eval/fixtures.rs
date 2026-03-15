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
    fn test_checked_in_repo_regression_contract_edge_fixture_loads_expected_repo_path() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/trait_impl_sql_runner.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - trait impl sql runner usage")
        );
        assert_eq!(
            fixtures[0].fixture.repo_path,
            Some(std::path::PathBuf::from("graph_contract_edge_repo"))
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].file.as_deref(),
            Some("routes.rs")
        );
        assert!(fixtures[0].fixture.expect.must_find[0]
            .contains_any
            .iter()
            .any(|phrase| phrase.contains("trait implementation")));
    }

    #[test]
    fn test_checked_in_fix_loop_convergence_fixture_loads_expected_fields() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/fix_loop_premature_convergence.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - fix loop premature convergence")
        );
        assert_eq!(
            fixtures[0].fixture.repo_path,
            Some(std::path::PathBuf::from("../../.."))
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].file.as_deref(),
            Some("src/server/api/gh.rs")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.fix-loop.premature-convergence")
        );
    }

    #[test]
    fn test_checked_in_fix_loop_reopened_fixture_loads_expected_fields() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/fix_loop_reopened_findings.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - reopened finding telemetry broken")
        );
        assert_eq!(
            fixtures[0].fixture.repo_path,
            Some(std::path::PathBuf::from("../../.."))
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].file.as_deref(),
            Some("src/server/api/gh.rs")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.fix-loop.reopened-finding-telemetry")
        );
    }

    #[test]
    fn test_checked_in_readiness_blocker_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "eval/fixtures/repo_regressions/readiness_informational_blocker_classification.yml",
        );

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - informational findings counted as blockers")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.readiness.informational-blocker-classification")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
        );
    }

    #[test]
    fn test_checked_in_current_head_stale_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/readiness_current_head_stale.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - current head staleness ignored")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.readiness.current-head-staleness")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
        );
    }

    #[test]
    fn test_checked_in_inconclusive_verification_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/readiness_inconclusive_verification.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - inconclusive verification no longer blocks readiness")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.readiness.inconclusive-verification")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
        );
    }

    #[test]
    fn test_checked_in_lifecycle_context_only_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/lifecycle_context_only_addressed.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - context-only edits marked addressed")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.lifecycle.context-only-addressed")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
        );
    }

    #[test]
    fn test_checked_in_lifecycle_persistence_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "eval/fixtures/repo_regressions/lifecycle_not_addressed_persistence_inversion.yml",
        );

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - persistent findings dropped from not-addressed inference")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.lifecycle.not-addressed-persistence-inversion")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
        );
    }

    #[test]
    fn test_checked_in_lifecycle_api_fixture_loads_summary_expectations() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/repo_regressions/lifecycle_api_drops_followup_addressed.yml");

        let fixtures = load_eval_fixtures_from_path(&fixture_path).unwrap();

        assert_eq!(fixtures.len(), 1);
        assert_eq!(
            fixtures[0].fixture.name.as_deref(),
            Some("repo regression - API drops follow-up addressed outcome")
        );
        assert_eq!(
            fixtures[0].fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.lifecycle.api-drops-followup-addressed")
        );
        assert_eq!(
            fixtures[0]
                .fixture
                .expect
                .summary
                .merge_readiness
                .as_deref(),
            Some("NeedsAttention")
        );
        assert_eq!(
            fixtures[0].fixture.expect.summary.min_open_blockers,
            Some(1)
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

    #[test]
    fn test_checked_in_supply_chain_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_supply_chain.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 7);
        let typosquat = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-supply-chain/npm-typosquat-package")
            })
            .unwrap();
        assert_eq!(
            typosquat.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.supply-chain.new-dependency")
        );

        let unpinned = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-supply-chain/python-unpinned-dependency")
            })
            .unwrap();
        assert_eq!(
            unpinned.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.supply-chain.unpinned-version")
        );

        let downgraded = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-supply-chain/rust-yanked-crate-version")
            })
            .unwrap();
        assert_eq!(
            downgraded.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("sec.supply-chain.version-downgrade")
        );

        let replace_directive = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-supply-chain/go-replace-directive-remote")
            })
            .unwrap();
        assert_eq!(
            replace_directive.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("sec.supply-chain.override-directive")
        );
    }

    #[test]
    fn test_checked_in_async_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_async.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 6);
        let foreach = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-async/typescript-async-foreach-not-awaited")
            })
            .unwrap();
        assert_eq!(
            foreach.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.async.foreach-no-await")
        );

        let blocking = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-async/rust-blocking-in-async-runtime")
            })
            .unwrap();
        assert_eq!(
            blocking.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.async.blocking-runtime-call")
        );

        let nested_loop = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-async/python-asyncio-run-in-running-loop")
            })
            .unwrap();
        assert_eq!(
            nested_loop.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.async.nested-event-loop")
        );

        let cancel_order = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-async/go-context-cancel-defer-order")
            })
            .unwrap();
        assert_eq!(
            cancel_order.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.async.context-cancel-order")
        );
    }

    #[test]
    fn test_checked_in_error_handling_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_error_handling.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 4);
        let unwrap_request = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-error-handling/rust-unwrap-in-request-handler")
            })
            .unwrap();
        assert_eq!(
            unwrap_request.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("bug.error-handling.unwrap-request")
        );

        let ignored_error = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-error-handling/go-ignored-error-return")
            })
            .unwrap();
        assert_eq!(
            ignored_error.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.error-handling.ignored-error")
        );

        let bare_except = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-error-handling/python-bare-except-silences-error")
            })
            .unwrap();
        assert_eq!(
            bare_except.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.error-handling.bare-except")
        );

        let unhandled_promise = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-error-handling/ts-unhandled-promise-rejection")
            })
            .unwrap();
        assert_eq!(
            unhandled_promise.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("bug.error-handling.unhandled-promise")
        );
    }

    #[test]
    fn test_checked_in_language_footguns_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_language_footguns.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 4);
        let nil_interface = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-language-footguns/go-nil-interface-comparison")
            })
            .unwrap();
        assert_eq!(
            nil_interface.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("bug.lang.nil-interface")
        );

        let mutable_default = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-language-footguns/python-mutable-default-arg")
            })
            .unwrap();
        assert_eq!(
            mutable_default.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("bug.lang.mutable-default")
        );

        let dangling_reference = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-language-footguns/rust-lifetime-dangling-ref")
            })
            .unwrap();
        assert_eq!(
            dangling_reference.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("bug.lang.dangling-reference")
        );

        let loose_equality = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-language-footguns/ts-equality-coercion-trap")
            })
            .unwrap();
        assert_eq!(
            loose_equality.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("bug.lang.loose-equality")
        );
    }

    #[test]
    fn test_checked_in_api_design_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_api_design.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 4);
        let field_rename = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-api-design/ts-breaking-api-field-rename")
            })
            .unwrap();
        assert_eq!(
            field_rename.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("design.api.field-rename")
        );

        let input_validation = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-api-design/python-missing-input-validation")
            })
            .unwrap();
        assert_eq!(
            input_validation.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("design.api.input-validation")
        );

        let removed_field = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-api-design/go-removed-required-field")
            })
            .unwrap();
        assert_eq!(
            removed_field.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("design.api.required-field-removal")
        );

        let error_type_change = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-api-design/rust-public-api-error-type-change")
            })
            .unwrap();
        assert_eq!(
            error_type_change.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("design.api.error-type-change")
        );
    }

    #[test]
    fn test_checked_in_performance_pack_loads_expected_fixtures() {
        let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("eval/fixtures/deep_review_suite/review_depth_performance.json");

        let fixtures = load_eval_fixtures_from_path(&pack_path).unwrap();

        assert_eq!(fixtures.len(), 5);
        let n_plus_one = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-performance/python-n-plus-1-orm-query")
            })
            .unwrap();
        assert_eq!(
            n_plus_one.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("perf.query.n-plus-one")
        );

        let goroutines = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-performance/go-unbounded-goroutine-spawn")
            })
            .unwrap();
        assert_eq!(
            goroutines.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("perf.concurrency.unbounded-goroutines")
        );

        let clone_hot_loop = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-performance/rust-clone-in-hot-loop")
            })
            .unwrap();
        assert_eq!(
            clone_hot_loop.fixture.expect.must_find[0]
                .rule_id
                .as_deref(),
            Some("perf.allocation.clone-hot-loop")
        );

        let listener_leak = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-performance/ts-memory-leak-event-listener")
            })
            .unwrap();
        assert_eq!(
            listener_leak.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("perf.memory.event-listener-leak")
        );

        let file_handle = fixtures
            .iter()
            .find(|fixture| {
                fixture.fixture.name.as_deref()
                    == Some("review-depth-performance/python-file-handle-no-close")
            })
            .unwrap();
        assert_eq!(
            file_handle.fixture.expect.must_find[0].rule_id.as_deref(),
            Some("perf.resource.file-handle-leak")
        );
    }
}
