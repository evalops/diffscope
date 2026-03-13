use anyhow::Result;
use tracing::info;

#[path = "postprocess/dedup.rs"]
mod dedup;
#[path = "postprocess/feedback.rs"]
mod feedback;
#[path = "postprocess/suppression.rs"]
mod suppression;
#[path = "postprocess/verification.rs"]
mod verification;

use crate::core;

use dedup::deduplicate_specialized_comments;
use feedback::apply_semantic_feedback_adjustment;
use suppression::apply_convention_suppression;
use verification::apply_verification_pass;

use super::super::filters::{apply_feedback_confidence_adjustment, apply_review_filters};
use super::contracts::ExecutionSummary;
use super::repo_support::save_convention_store;
use super::services::PipelineServices;
use super::session::ReviewSession;
use super::types::ReviewResult;

pub(super) async fn run_postprocess(
    execution: ExecutionSummary,
    services: &PipelineServices,
    session: &mut ReviewSession,
) -> Result<ReviewResult> {
    let ExecutionSummary {
        mut all_comments,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        file_metrics,
        comments_by_pass,
        agent_activity,
    } = execution;

    if services.config.multi_pass_specialized {
        let before = all_comments.len();
        all_comments = deduplicate_specialized_comments(all_comments);
        let after = all_comments.len();
        if before != after {
            info!(
                "Deduplicated {} comment(s) across specialized passes ({} -> {})",
                before - after,
                before,
                after
            );
        }
    }

    let repo_path_str = services.repo_path_str();
    let processed_comments = services
        .plugin_manager
        .run_post_processors(all_comments, &repo_path_str)
        .await?;
    let processed_comments = apply_verification_pass(processed_comments, services, session).await;

    let processed_comments = if services.config.semantic_feedback {
        apply_semantic_feedback_adjustment(
            processed_comments,
            session.semantic_feedback_store.as_ref(),
            services.embedding_adapter.as_deref(),
            &services.config,
        )
        .await
    } else {
        processed_comments
    };

    let processed_comments = if services.config.enhanced_feedback {
        apply_feedback_confidence_adjustment(
            processed_comments,
            &services.feedback,
            services.config.feedback_min_observations,
        )
    } else {
        processed_comments
    };

    let processed_comments =
        apply_review_filters(processed_comments, &services.config, &services.feedback);
    let processed_comments =
        core::apply_enhanced_filters(&mut session.enhanced_ctx, processed_comments);
    let (processed_comments, convention_suppressed_count) =
        apply_convention_suppression(processed_comments, &session.enhanced_ctx.convention_store);

    if let Some(ref store_path) = services.convention_store_path {
        save_convention_store(&session.enhanced_ctx.convention_store, store_path);
    }

    Ok(ReviewResult {
        comments: processed_comments,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        file_metrics,
        convention_suppressed_count,
        comments_by_pass,
        hotspots: session.enhanced_ctx.hotspots.clone(),
        agent_activity,
    })
}
