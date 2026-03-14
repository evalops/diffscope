use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core;
use crate::plugins::PreAnalysis;
use crate::review::triage::{triage_diff_with_options, TriageOptions};

use super::super::super::comments::{filter_comments_for_diff, synthesize_analyzer_comments};

pub(super) enum DiffPreparationDecision {
    Skip,
    CompleteWithComments(Vec<core::Comment>),
    Review(PreparedDiffAnalysis),
}

pub(super) struct PreparedDiffAnalysis {
    pub context_chunks: Vec<core::LLMContextChunk>,
    pub deterministic_comments: Vec<core::Comment>,
}

pub(super) fn prepare_diff_analysis(
    diff: &core::UnifiedDiff,
    batched_pre_analysis: &mut HashMap<PathBuf, PreAnalysis>,
    triage_skip_deletion_only: bool,
) -> Result<DiffPreparationDecision> {
    let pre_analysis = batched_pre_analysis
        .remove(&diff.file_path)
        .unwrap_or_default();
    let deterministic_comments = filter_comments_for_diff(
        diff,
        synthesize_analyzer_comments(pre_analysis.findings.clone())?,
    );

    let triage_result = triage_diff_with_options(
        diff,
        TriageOptions {
            skip_deletion_only: triage_skip_deletion_only,
        },
    );
    if triage_result.should_skip() {
        if deterministic_comments.is_empty() {
            tracing::info!(
                "Skipping {} (triage: {})",
                diff.file_path.display(),
                triage_result.reason()
            );
            return Ok(DiffPreparationDecision::Skip);
        }

        tracing::info!(
            "Skipping expensive LLM review for {} (triage: {}), keeping {} analyzer finding(s)",
            diff.file_path.display(),
            triage_result.reason(),
            deterministic_comments.len()
        );
        return Ok(DiffPreparationDecision::CompleteWithComments(
            deterministic_comments,
        ));
    }

    Ok(DiffPreparationDecision::Review(PreparedDiffAnalysis {
        context_chunks: pre_analysis.context_chunks,
        deterministic_comments,
    }))
}
