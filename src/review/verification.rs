use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

use crate::adapters::llm::{LLMAdapter, LLMRequest};
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
}

const VERIFICATION_BATCH_SIZE: usize = 6;

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

/// Verify a batch of review comments by asking the LLM to validate each one.
/// Returns only comments that pass verification (score >= min_score), unless
/// fail-open mode keeps the original batch after verifier failures.
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

    let total_count = comments.len();
    let mut verified = Vec::new();
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
                    "Verification batch failed for {} comment(s): {}",
                    batch.len(),
                    error
                );
                if fail_open {
                    warnings.push(format!(
                        "verification fail-open kept {} comment(s) after verifier request error: {}",
                        batch.len(),
                        error
                    ));
                    verified.extend(batch.iter().cloned());
                }
                continue;
            }
        };

        let Some(parsed_results) = try_parse_verification_response(&response.content, batch) else {
            info!(
                "Verification batch returned an unparseable response for {} comment(s)",
                batch.len()
            );
            if fail_open {
                warnings.push(format!(
                    "verification fail-open kept {} comment(s) after unparseable verifier output",
                    batch.len()
                ));
                verified.extend(batch.iter().cloned());
            }
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
                    verified.push(comment);
                }
                Some(result) => {
                    info!(
                        "Verification filtered comment {} (score: {}, accurate: {}, line_correct: {})",
                        comment.id, result.score, result.accurate, result.line_correct
                    );
                }
                None => {
                    info!(
                        "Verification dropped comment {} because the verifier returned no result",
                        comment.id
                    );
                }
            }
        }
    }

    info!(
        "Verification: {}/{} comments passed",
        verified.len(),
        total_count
    );

    VerificationSummary {
        comments: verified,
        warnings,
    }
}
