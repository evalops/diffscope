use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

use crate::adapters::llm::{LLMAdapter, LLMRequest};
use crate::config::VerificationConsensusMode;
use crate::core::{Comment, LLMContextChunk, UnifiedDiff};

#[path = "verification/parser.rs"]
mod parser;
#[path = "verification/prompt.rs"]
mod prompt;
#[cfg(test)]
#[path = "verification/tests.rs"]
mod tests;

#[cfg(test)]
use parser::parse_verification_response;
use parser::{try_parse_verification_response, verification_response_schema};
use prompt::build_verification_prompt;

/// Result of verifying a single review comment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub comment_id: String,
    pub accurate: bool,
    pub line_correct: bool,
    pub suggestion_sound: bool,
    pub score: u8, // 0-10
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct VerificationSummary {
    pub comments: Vec<Comment>,
    pub warnings: Vec<String>,
    pub report: Option<VerificationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationJudgeRun {
    pub model: String,
    pub total_comments: usize,
    pub passed_comments: usize,
    pub filtered_comments: usize,
    pub abstained_comments: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationReport {
    pub consensus_mode: String,
    pub required_votes: usize,
    pub judge_count: usize,
    pub judges: Vec<VerificationJudgeRun>,
}

const VERIFICATION_BATCH_SIZE: usize = 6;

#[derive(Debug, Clone)]
struct JudgeDecision {
    comment_id: String,
    kept_comment: Option<Comment>,
    passed_vote: bool,
    abstained: bool,
}

#[derive(Debug, Clone)]
struct SingleJudgeSummary {
    model: String,
    decisions: Vec<JudgeDecision>,
    warnings: Vec<String>,
}

pub(crate) struct VerificationJudgeConfig<'a> {
    pub adapters: &'a [Arc<dyn LLMAdapter>],
    pub min_score: u8,
    pub fail_open: bool,
    pub consensus_mode: VerificationConsensusMode,
}

const VERIFICATION_SYSTEM_PROMPT: &str = r#"You are a code review verifier. Your job is to validate review findings against the exact code snippets provided.

For each finding, assess:
1. Does the referenced file and line exist in the supplied evidence?
2. Does the comment accurately describe the code shown in the diff, nearby file context, and any supporting cross-file context?
3. Is the suggestion sound, including fixes that belong in a related supporting-context file instead of the changed file?
4. Is the finding a false positive or hallucinated issue?
5. Cross-file findings are valid when the changed line introduces a call path or tainted data flow into a vulnerable helper shown in supporting context.
6. Mark `line_correct=true` when the changed line is the introduction point or risky call site, even if the sink or flawed helper implementation is in another file shown in supporting context.
7. Treat supporting context with graph or semantic provenance as first-class evidence, not as a weak hint.
If the evidence is missing, ambiguous, or the file/line cannot be confirmed, return a result anyway with accurate=false, line_correct=false, and a low score.

Score each finding 0-10:
- 8-10: Critical bugs or security issues that are clearly present
- 5-7: Valid issues that exist but may be minor
- 1-4: Questionable issues, possibly hallucinated or too trivial
- 0: Noise (docstrings, type hints, import ordering, trailing whitespace)

Respond with JSON only. Return exactly one object per finding, in order, with this schema:
[{"index":1,"accurate":true,"line_correct":true,"suggestion_sound":true,"score":8,"reason":"brief reason"}]
"#;

/// Verify a batch of review comments with a single judge model.
#[cfg(test)]
pub async fn verify_comments(
    comments: Vec<Comment>,
    diffs: &[UnifiedDiff],
    source_files: &HashMap<std::path::PathBuf, String>,
    extra_context: &HashMap<std::path::PathBuf, Vec<LLMContextChunk>>,
    adapter: &dyn LLMAdapter,
    min_score: u8,
    fail_open: bool,
) -> VerificationSummary {
    if comments.is_empty() {
        return VerificationSummary::default();
    }

    let judge_summary = verify_comments_single(
        &comments,
        diffs,
        source_files,
        extra_context,
        adapter,
        min_score,
        fail_open,
    )
    .await;
    build_verification_summary(
        comments,
        vec![judge_summary],
        VerificationConsensusMode::Any,
        fail_open,
    )
}

pub(crate) async fn verify_comments_with_judges(
    comments: Vec<Comment>,
    diffs: &[UnifiedDiff],
    source_files: &HashMap<std::path::PathBuf, String>,
    extra_context: &HashMap<std::path::PathBuf, Vec<LLMContextChunk>>,
    judge_config: VerificationJudgeConfig<'_>,
) -> VerificationSummary {
    if comments.is_empty() {
        return VerificationSummary::default();
    }

    if judge_config.adapters.is_empty() {
        return VerificationSummary {
            comments,
            warnings: vec![
                "verification skipped because no judge models were configured".to_string(),
            ],
            report: None,
        };
    }

    let mut judge_summaries = Vec::new();
    for adapter in judge_config.adapters {
        judge_summaries.push(
            verify_comments_single(
                &comments,
                diffs,
                source_files,
                extra_context,
                adapter.as_ref(),
                judge_config.min_score,
                judge_config.fail_open,
            )
            .await,
        );
    }

    build_verification_summary(
        comments,
        judge_summaries,
        judge_config.consensus_mode,
        judge_config.fail_open,
    )
}

async fn verify_comments_single(
    comments: &[Comment],
    diffs: &[UnifiedDiff],
    source_files: &HashMap<std::path::PathBuf, String>,
    extra_context: &HashMap<std::path::PathBuf, Vec<LLMContextChunk>>,
    adapter: &dyn LLMAdapter,
    min_score: u8,
    fail_open: bool,
) -> SingleJudgeSummary {
    let total_count = comments.len();
    let mut decisions = Vec::new();
    let mut warnings = Vec::new();
    for batch in comments.chunks(VERIFICATION_BATCH_SIZE) {
        let prompt = build_verification_prompt(batch, diffs, source_files, extra_context);
        let request = LLMRequest {
            system_prompt: VERIFICATION_SYSTEM_PROMPT.to_string(),
            user_prompt: prompt,
            temperature: Some(0.0),
            max_tokens: Some((batch.len() * 220).max(400)),
            response_schema: Some(verification_response_schema()),
        };

        let response = match adapter.complete(request).await {
            Ok(response) => response,
            Err(error) => {
                info!(
                    "Verification batch failed for {} comment(s) with {}: {}",
                    batch.len(),
                    adapter.model_name(),
                    error
                );
                if fail_open {
                    warnings.push(format!(
                        "verification judge {} fail-open kept {} comment(s) after verifier request error: {}",
                        adapter.model_name(),
                        batch.len(),
                        error
                    ));
                }
                decisions.extend(batch.iter().cloned().map(|comment| JudgeDecision {
                    comment_id: comment.id.clone(),
                    kept_comment: if fail_open { Some(comment) } else { None },
                    passed_vote: false,
                    abstained: true,
                }));
                continue;
            }
        };

        let Some(parsed_results) = try_parse_verification_response(&response.content, batch) else {
            info!(
                "Verification batch returned an unparseable response for {} comment(s) with {}",
                batch.len(),
                adapter.model_name()
            );
            if fail_open {
                warnings.push(format!(
                    "verification judge {} fail-open kept {} comment(s) after unparseable verifier output",
                    adapter.model_name(),
                    batch.len()
                ));
            }
            decisions.extend(batch.iter().cloned().map(|comment| JudgeDecision {
                comment_id: comment.id.clone(),
                kept_comment: if fail_open { Some(comment) } else { None },
                passed_vote: false,
                abstained: true,
            }));
            continue;
        };

        let results = parsed_results
            .into_iter()
            .map(|result| (result.comment_id.clone(), result))
            .collect::<HashMap<_, _>>();

        for mut comment in batch.iter().cloned() {
            match results.get(&comment.id) {
                Some(result)
                    if result.score >= min_score && result.accurate && result.line_correct =>
                {
                    comment.confidence = (result.score as f32 / 10.0).min(1.0);
                    if !result.suggestion_sound {
                        comment.suggestion = None;
                        comment.code_suggestion = None;
                    }
                    decisions.push(JudgeDecision {
                        comment_id: comment.id.clone(),
                        kept_comment: Some(comment),
                        passed_vote: true,
                        abstained: false,
                    });
                }
                Some(result) => {
                    info!(
                        "Verification filtered comment {} by {} (score: {}, accurate: {}, line_correct: {})",
                        comment.id,
                        adapter.model_name(),
                        result.score,
                        result.accurate,
                        result.line_correct
                    );
                    decisions.push(JudgeDecision {
                        comment_id: comment.id.clone(),
                        kept_comment: None,
                        passed_vote: false,
                        abstained: false,
                    });
                }
                None => {
                    info!(
                        "Verification dropped comment {} because {} returned no result",
                        comment.id,
                        adapter.model_name()
                    );
                    decisions.push(JudgeDecision {
                        comment_id: comment.id.clone(),
                        kept_comment: None,
                        passed_vote: false,
                        abstained: false,
                    });
                }
            }
        }
    }

    let verified_count = decisions
        .iter()
        .filter(|decision| decision.passed_vote)
        .count();
    info!(
        "Verification judge {}: {}/{} comments passed",
        adapter.model_name(),
        verified_count,
        total_count
    );

    SingleJudgeSummary {
        model: adapter.model_name().to_string(),
        decisions,
        warnings,
    }
}

fn build_verification_summary(
    comments: Vec<Comment>,
    judge_summaries: Vec<SingleJudgeSummary>,
    consensus_mode: VerificationConsensusMode,
    fail_open: bool,
) -> VerificationSummary {
    let configured_required_votes = required_votes(consensus_mode, judge_summaries.len());
    let warnings = judge_summaries
        .iter()
        .flat_map(|summary| summary.warnings.iter().cloned())
        .collect::<Vec<_>>();
    let decision_maps = judge_summaries
        .iter()
        .map(|summary| {
            summary
                .decisions
                .iter()
                .map(|decision| (decision.comment_id.clone(), decision))
                .collect::<HashMap<_, _>>()
        })
        .collect::<Vec<_>>();

    let mut verified = Vec::new();
    for original_comment in comments {
        let mut decisive_votes = 0usize;
        let mut positive_comments = Vec::new();
        let mut abstained_comments = Vec::new();

        for decision_map in &decision_maps {
            let Some(decision) = decision_map.get(&original_comment.id) else {
                continue;
            };
            if decision.abstained {
                if let Some(comment) = &decision.kept_comment {
                    abstained_comments.push(comment.clone());
                }
                continue;
            }

            decisive_votes += 1;
            if decision.passed_vote {
                if let Some(comment) = &decision.kept_comment {
                    positive_comments.push(comment.clone());
                }
            }
        }

        if decisive_votes == 0 {
            if fail_open {
                verified.push(
                    abstained_comments
                        .into_iter()
                        .next()
                        .unwrap_or(original_comment),
                );
            }
            continue;
        }

        if positive_comments.len() >= required_votes(consensus_mode, decisive_votes) {
            verified.push(select_best_verified_comment(
                positive_comments,
                &original_comment,
            ));
        }
    }

    info!(
        "Verification consensus ({}) kept {}/{} comments across {} judge(s)",
        consensus_mode.as_str(),
        verified.len(),
        decision_maps
            .first()
            .map(|map| map.len())
            .unwrap_or_default(),
        judge_summaries.len()
    );

    VerificationSummary {
        comments: verified,
        warnings,
        report: Some(VerificationReport {
            consensus_mode: consensus_mode.as_str().to_string(),
            required_votes: configured_required_votes,
            judge_count: judge_summaries.len(),
            judges: judge_summaries
                .into_iter()
                .map(|summary| VerificationJudgeRun {
                    total_comments: summary.decisions.len(),
                    passed_comments: summary
                        .decisions
                        .iter()
                        .filter(|decision| decision.passed_vote)
                        .count(),
                    filtered_comments: summary
                        .decisions
                        .iter()
                        .filter(|decision| !decision.abstained && !decision.passed_vote)
                        .count(),
                    abstained_comments: summary
                        .decisions
                        .iter()
                        .filter(|decision| decision.abstained)
                        .count(),
                    model: summary.model,
                    warnings: summary.warnings,
                })
                .collect(),
        }),
    }
}

fn required_votes(consensus_mode: VerificationConsensusMode, judge_count: usize) -> usize {
    match consensus_mode {
        VerificationConsensusMode::Any => 1,
        VerificationConsensusMode::Majority => (judge_count / 2).saturating_add(1),
        VerificationConsensusMode::All => judge_count.max(1),
    }
}

fn select_best_verified_comment(candidates: Vec<Comment>, fallback: &Comment) -> Comment {
    candidates
        .into_iter()
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or_else(|| fallback.clone())
}
