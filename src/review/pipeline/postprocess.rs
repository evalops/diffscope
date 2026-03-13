#[path = "postprocess/dag.rs"]
mod dag;
#[path = "postprocess/dedup.rs"]
mod dedup;
#[path = "postprocess/feedback.rs"]
mod feedback;
#[path = "postprocess/suppression.rs"]
mod suppression;
#[path = "postprocess/verification.rs"]
mod verification;

use anyhow::Result;

use super::contracts::ExecutionSummary;
use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::ReviewResult;

pub(crate) fn describe_review_postprocess_graph(
    config: &crate::config::Config,
    has_convention_store_path: bool,
) -> crate::core::dag::DagGraphContract {
    dag::describe_review_postprocess_graph(config, has_convention_store_path)
}

pub(super) async fn run_postprocess(
    execution: ExecutionSummary,
    services: &PipelineServices,
    session: &mut ReviewSession,
) -> Result<ReviewResult> {
    dag::run_postprocess_dag(execution, services, session).await
}
