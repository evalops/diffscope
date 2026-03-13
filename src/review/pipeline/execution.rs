use anyhow::Result;
use std::collections::HashMap;

#[path = "execution/dispatcher.rs"]
mod dispatcher;
#[path = "execution/metrics.rs"]
mod metrics;
#[path = "execution/responses.rs"]
mod responses;

use crate::core;

use super::contracts::{ExecutionSummary, FileReviewJob, ReviewExecutionContext};
use super::types::{AgentActivity, ProgressUpdate};

use dispatcher::{dispatch_jobs, DispatchedJobResult};
use metrics::{record_file_metric, UsageTotals};
use responses::process_job_result;

pub(super) async fn execute_review_jobs(
    jobs: Vec<FileReviewJob>,
    context: ReviewExecutionContext<'_>,
) -> Result<ExecutionSummary> {
    let dispatched_results = dispatch_jobs(jobs, &context).await;
    reduce_job_results(dispatched_results, context)
}

fn reduce_job_results(
    mut dispatched_results: Vec<DispatchedJobResult>,
    context: ReviewExecutionContext<'_>,
) -> Result<ExecutionSummary> {
    let files_skipped_snapshot = context.files_skipped;

    dispatched_results.sort_by_key(|result| result.job_order);

    let mut all_comments = context.initial_comments;
    let mut files_completed = context.files_completed;
    let mut usage_totals = UsageTotals::default();
    let mut file_metrics = Vec::new();
    let mut comments_by_pass: HashMap<String, usize> = HashMap::new();
    let mut aggregate_agent_iterations = 0usize;
    let mut aggregate_agent_tool_calls: Vec<core::agent_loop::AgentToolCallLog> = Vec::new();
    let mut has_agent_activity = false;

    for result in dispatched_results {
        let diff = &context.session.diffs[result.diff_index];
        let processed = process_job_result(result, diff, context.services.is_local)?;

        usage_totals.add(&processed);
        record_file_metric(&mut file_metrics, &processed);

        if let Some(pass_tag) = processed.pass_tag.as_ref() {
            *comments_by_pass.entry(pass_tag.clone()).or_insert(0) += processed.comment_count;
        }

        let current_file = processed.file_path.display().to_string();
        let mark_file_complete = processed.mark_file_complete;

        if let Some(activity) = processed.agent_data {
            has_agent_activity = true;
            aggregate_agent_iterations += activity.total_iterations;
            aggregate_agent_tool_calls.extend(activity.tool_calls);
        }

        all_comments.extend(processed.comments);

        if mark_file_complete {
            files_completed += 1;
            if let Some(ref callback) = context.session.on_progress {
                callback(ProgressUpdate {
                    current_file,
                    files_total: context.session.files_total,
                    files_completed,
                    files_skipped: files_skipped_snapshot,
                    comments_so_far: all_comments.clone(),
                });
            }
        }
    }

    Ok(ExecutionSummary {
        all_comments,
        total_prompt_tokens: usage_totals.prompt_tokens,
        total_completion_tokens: usage_totals.completion_tokens,
        total_tokens: usage_totals.total_tokens,
        file_metrics,
        comments_by_pass,
        agent_activity: if has_agent_activity {
            Some(AgentActivity {
                total_iterations: aggregate_agent_iterations,
                tool_calls: aggregate_agent_tool_calls,
            })
        } else {
            None
        },
    })
}
