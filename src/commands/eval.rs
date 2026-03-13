#[path = "eval/command.rs"]
mod command;
#[path = "eval/fixtures.rs"]
mod fixtures;
#[path = "eval/metrics.rs"]
mod metrics;
#[path = "eval/pattern.rs"]
mod pattern;
#[path = "eval/report.rs"]
mod report;
#[path = "eval/runner.rs"]
mod runner;
#[path = "eval/thresholds.rs"]
mod thresholds;
#[path = "eval/types.rs"]
mod types;

pub use command::eval_command;
pub use types::EvalRunOptions;
pub use types::{
    EvalAgentActivity, EvalAgentToolCall, EvalFixtureMetadata, EvalReport, EvalReproductionCheck,
    EvalReproductionSummary, EvalRuleMetrics, EvalRunMetadata, EvalVerificationJudgeReport,
    EvalVerificationReport,
};

#[allow(unused_imports)]
use types::{
    EvalExpectations, EvalFixture, EvalFixtureResult, EvalNamedMetricComparison, EvalPattern,
    EvalRuleScoreSummary, EvalRunFilters, EvalSuiteResult, EvalVerificationHealth,
    LoadedEvalFixture,
};

pub(crate) fn describe_eval_fixture_graph(
    repro_validate: bool,
) -> crate::core::dag::DagGraphContract {
    runner::describe_eval_fixture_graph(repro_validate)
}
