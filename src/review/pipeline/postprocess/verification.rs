use tracing::{info, warn};

use crate::core;
use crate::review::verification::VerificationJudgeConfig;

use super::super::comments::is_analyzer_comment;
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;

pub(super) struct VerificationPassOutput {
    pub(super) comments: Vec<core::Comment>,
    pub(super) warnings: Vec<String>,
    pub(super) report: Option<crate::review::verification::VerificationReport>,
}

pub(super) async fn apply_verification_pass(
    comments: Vec<core::Comment>,
    services: &PipelineServices,
    session: &ReviewSession,
) -> VerificationPassOutput {
    let (analyzer_comments, llm_comments): (Vec<_>, Vec<_>) =
        comments.into_iter().partition(is_analyzer_comment);

    let (verified_llm_comments, warnings, report) = if services.config.verification.enabled
        && !llm_comments.is_empty()
        && llm_comments.len() <= services.config.verification.max_comments
    {
        let comment_count_before = llm_comments.len();
        let summary = super::super::super::verification::verify_comments_with_judges(
            llm_comments,
            &session.diffs,
            &session.source_files,
            &session.verification_context,
            VerificationJudgeConfig {
                adapters: &services.verification_adapters,
                min_score: services.config.verification.min_score,
                fail_open: services.config.verification.fail_open,
                consensus_mode: services.config.verification.consensus_mode,
            },
        )
        .await;

        for warning_message in &summary.warnings {
            warn!("{}", warning_message);
        }

        info!(
            "Verification pass: {}/{} comments passed",
            summary.comments.len(),
            comment_count_before
        );
        (summary.comments, summary.warnings, summary.report)
    } else {
        (llm_comments, Vec::new(), None)
    };

    let mut processed_comments = analyzer_comments;
    processed_comments.extend(verified_llm_comments);
    VerificationPassOutput {
        comments: processed_comments,
        warnings,
        report,
    }
}
