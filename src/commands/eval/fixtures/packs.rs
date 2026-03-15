use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::core::eval_benchmarks::CommunityFixturePack;

use super::super::{
    EvalExpectations, EvalFixture, EvalFixtureMetadata, EvalPattern, LoadedEvalFixture,
};
use super::validation::validate_eval_fixture;

pub(super) fn expand_community_fixture_pack(
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
                repo_path: fixture.repo_path.map(PathBuf::from),
                expect: EvalExpectations {
                    must_find: fixture
                        .expected_findings
                        .into_iter()
                        .map(|finding| EvalPattern {
                            file: finding.file_pattern,
                            line: finding.line_hint,
                            contains: finding.contains,
                            contains_any: finding.contains_any,
                            severity: finding.severity,
                            category: finding.category,
                            tags_any: finding.tags_any,
                            confidence_at_least: finding.confidence_at_least,
                            confidence_at_most: finding.confidence_at_most,
                            fix_effort: finding.fix_effort,
                            rule_id: finding.rule_id.clone(),
                            rule_id_aliases: finding.rule_id_aliases,
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
                            contains_any: finding.contains_any,
                            ..Default::default()
                        })
                        .collect(),
                    min_total: fixture.min_total,
                    max_total: fixture.max_total,
                    summary: Default::default(),
                },
            };
            validate_eval_fixture(&eval_fixture)?;

            Ok(LoadedEvalFixture {
                fixture_path: path.to_path_buf(),
                fixture: eval_fixture,
                suite_name: Some(pack_name.clone()),
                suite_thresholds: thresholds.clone(),
                difficulty: Some(difficulty),
                metadata: Some(EvalFixtureMetadata {
                    category: Some(fixture.category),
                    language: Some(fixture.language),
                    source: fixture.source,
                    description: fixture.description,
                }),
            })
        })
        .collect::<Result<Vec<_>>>()
}
