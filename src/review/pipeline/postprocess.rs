use anyhow::Result;
use tracing::{info, warn};

use crate::adapters;
use crate::config;
use crate::core;

use super::super::feedback::derive_file_patterns;
use super::super::filters::{apply_feedback_confidence_adjustment, apply_review_filters};
use super::comments::is_analyzer_comment;
use super::contracts::ExecutionSummary;
use super::session::{save_convention_store, PipelineServices, ReviewSession};
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

    let (analyzer_comments, llm_comments): (Vec<_>, Vec<_>) = processed_comments
        .into_iter()
        .partition(is_analyzer_comment);

    let verified_llm_comments = if services.config.verification_pass
        && !llm_comments.is_empty()
        && llm_comments.len() <= services.config.verification_max_comments
    {
        let comment_count_before = llm_comments.len();
        match super::super::verification::verify_comments(
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

pub(super) fn deduplicate_specialized_comments(
    mut comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    if comments.len() <= 1 {
        return comments;
    }

    comments.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });

    let mut deduped: Vec<core::Comment> = Vec::with_capacity(comments.len());
    for comment in comments {
        let dominated = deduped.iter_mut().find(|existing| {
            existing.file_path == comment.file_path
                && existing.line_number == comment.line_number
                && core::multi_pass::content_similarity(&existing.content, &comment.content) > 0.6
        });
        if let Some(existing) = dominated {
            if comment.confidence > existing.confidence {
                existing.content = comment.content;
                existing.confidence = comment.confidence;
                existing.severity = comment.severity;
            }
            for tag in &comment.tags {
                if !existing.tags.contains(tag) {
                    existing.tags.push(tag.clone());
                }
            }
        } else {
            deduped.push(comment);
        }
    }

    deduped
}

pub(super) async fn apply_semantic_feedback_adjustment(
    comments: Vec<core::Comment>,
    store: Option<&core::SemanticFeedbackStore>,
    embedding_adapter: Option<&dyn adapters::llm::LLMAdapter>,
    config: &config::Config,
) -> Vec<core::Comment> {
    let Some(store) = store else {
        return comments;
    };
    if store.examples.len() < config.semantic_feedback_min_examples {
        return comments;
    }

    let embedding_texts = comments
        .iter()
        .map(|comment| {
            core::build_feedback_embedding_text(&comment.content, comment.category.as_str())
        })
        .collect::<Vec<_>>();
    let embeddings = core::embed_texts_with_fallback(embedding_adapter, &embedding_texts).await;

    comments
        .into_iter()
        .zip(embeddings)
        .map(|(mut comment, embedding)| {
            if is_analyzer_comment(&comment) {
                return comment;
            }

            let file_patterns = derive_file_patterns(&comment.file_path);
            let matches = core::find_similar_feedback_examples(
                store,
                &embedding,
                comment.category.as_str(),
                &file_patterns,
                config.semantic_feedback_similarity,
                config.semantic_feedback_max_neighbors,
            );
            let accepted = matches
                .iter()
                .filter(|(example, _)| example.accepted)
                .count();
            let rejected = matches
                .iter()
                .filter(|(example, _)| !example.accepted)
                .count();
            let observations = accepted + rejected;

            if observations < config.semantic_feedback_min_examples {
                return comment;
            }

            if rejected > accepted {
                let delta = ((rejected - accepted) as f32 * 0.15).min(0.45);
                comment.confidence = (comment.confidence - delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:rejected")
                {
                    comment.tags.push("semantic-feedback:rejected".to_string());
                }
            } else if accepted > rejected {
                let delta = ((accepted - rejected) as f32 * 0.10).min(0.25);
                comment.confidence = (comment.confidence + delta).clamp(0.0, 1.0);
                if !comment
                    .tags
                    .iter()
                    .any(|tag| tag == "semantic-feedback:accepted")
                {
                    comment.tags.push("semantic-feedback:accepted".to_string());
                }
            }

            comment
        })
        .collect()
}

pub(super) fn apply_convention_suppression(
    comments: Vec<core::Comment>,
    convention_store: &core::convention_learner::ConventionStore,
) -> (Vec<core::Comment>, usize) {
    let suppression_patterns = convention_store.suppression_patterns();
    if suppression_patterns.is_empty() {
        return (comments, 0);
    }

    let before_count = comments.len();
    let filtered: Vec<core::Comment> = comments
        .into_iter()
        .filter(|comment| {
            let category_str = comment.category.to_string();
            let score = convention_store.score_comment(&comment.content, &category_str);
            score > -0.25
        })
        .collect();

    let suppressed = before_count.saturating_sub(filtered.len());
    if suppressed > 0 {
        info!(
            "Convention learning suppressed {} comment(s) based on team feedback patterns",
            suppressed
        );
    }

    (filtered, suppressed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_comment(file: &str, line: usize, content: &str, tag: &str) -> core::Comment {
        core::Comment {
            id: format!("cmt_{}", line),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::BestPractice,
            suggestion: None,
            confidence: 0.7,
            code_suggestion: None,
            tags: vec![tag.to_string()],
            fix_effort: core::comment::FixEffort::Medium,
            feedback: None,
        }
    }

    #[test]
    fn dedup_removes_similar_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                10,
                "Missing null check on user input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].tags.contains(&"security-pass".to_string()));
    }

    #[test]
    fn dedup_keeps_different_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "SQL injection vulnerability", "security-pass"),
            make_comment("a.rs", 10, "Off-by-one error in loop", "correctness-pass"),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_keeps_similar_comments_on_different_lines() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                20,
                "Missing null check on input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_handles_empty_input() {
        let deduped = deduplicate_specialized_comments(vec![]);
        assert!(deduped.is_empty());
    }
}
