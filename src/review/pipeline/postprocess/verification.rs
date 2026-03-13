use tracing::{info, warn};

use crate::core;

use super::super::comments::is_analyzer_comment;
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;

pub(super) async fn apply_verification_pass(
    comments: Vec<core::Comment>,
    services: &PipelineServices,
    session: &ReviewSession,
) -> Vec<core::Comment> {
    let (analyzer_comments, llm_comments): (Vec<_>, Vec<_>) =
        comments.into_iter().partition(is_analyzer_comment);

    let verified_llm_comments = if services.config.verification_pass
        && !llm_comments.is_empty()
        && llm_comments.len() <= services.config.verification_max_comments
    {
        let comment_count_before = llm_comments.len();
        match super::super::super::verification::verify_comments(
            llm_comments,
            &session.diffs,
            &session.source_files,
            &session.verification_context,
            services.verification_adapter.as_ref(),
            services.config.verification_min_score,
        )
        .await
        {
            Ok(verified) => {
                info!(
                    "Verification pass: {}/{} comments passed",
                    verified.len(),
                    comment_count_before
                );
                verified
            }
            Err(error) => {
                warn!(
                    "Verification pass failed, dropping unverified LLM comments: {}",
                    error
                );
                Vec::new()
            }
        }
    } else {
        llm_comments
    };

    let mut processed_comments = analyzer_comments;
    processed_comments.extend(verified_llm_comments);
    processed_comments
}
