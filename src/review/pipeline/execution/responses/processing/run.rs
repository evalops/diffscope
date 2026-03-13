use anyhow::Result;
use tracing::warn;

use crate::core;

use super::super::super::dispatcher::DispatchedJobResult;
use super::super::validation::validate_llm_response;
use super::comments::extract_processed_comments;
use super::fallback::fallback_result;
use super::merge::merge_processed_comments;
use super::usage::response_usage;
use super::{ProcessedJobResult, ResponseUsage};

pub(in super::super::super) fn process_job_result(
    result: DispatchedJobResult,
    diff: &core::UnifiedDiff,
    is_local: bool,
) -> Result<ProcessedJobResult> {
    let DispatchedJobResult {
        active_rules,
        path_config,
        file_path,
        deterministic_comments,
        pass_kind,
        mark_file_complete,
        response,
        latency_ms,
        agent_data,
        ..
    } = result;

    match response {
        Err(error) => {
            warn!("LLM request failed for {}: {}", file_path.display(), error);
            Ok(fallback_result(
                file_path,
                deterministic_comments,
                mark_file_complete,
                latency_ms,
                ResponseUsage::default(),
                agent_data,
            ))
        }
        Ok(response) => {
            let usage = response_usage(&response);
            let validation_error = validate_llm_response(&response.content).err();

            let extracted_comments = extract_processed_comments(
                &response.content,
                diff,
                &active_rules,
                path_config.as_ref(),
                pass_kind,
            )?;

            if let Some(validation_error) = validation_error.as_ref() {
                eprintln!(
                    "Warning: LLM response validation failed for {}: {}",
                    file_path.display(),
                    validation_error
                );
                if is_local {
                    eprintln!(
                        "Hint: Try a larger model or reduce diff size for better results with local models."
                    );
                }
            }

            let Some(comments) = extracted_comments else {
                if let Some(validation_error) = validation_error.as_ref() {
                    warn!(
                        "Falling back for {} after validation failure: {}",
                        file_path.display(),
                        validation_error
                    );
                }
                return Ok(fallback_result(
                    file_path,
                    deterministic_comments,
                    mark_file_complete,
                    latency_ms,
                    usage,
                    agent_data,
                ));
            };

            if let Some(validation_error) = validation_error.as_ref() {
                warn!(
                    "Salvaging parseable comments for {} despite validation warning: {}",
                    file_path.display(),
                    validation_error
                );
            }

            let comments = merge_processed_comments(diff, comments, deterministic_comments);

            Ok(ProcessedJobResult {
                file_path,
                comment_count: comments.len(),
                comments,
                latency_ms,
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                pass_tag: Some(
                    pass_kind
                        .map(|kind| kind.tag().to_string())
                        .unwrap_or_else(|| "default".to_string()),
                ),
                mark_file_complete,
                agent_data,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::llm::LLMResponse;
    use std::path::PathBuf;

    fn test_diff() -> core::UnifiedDiff {
        core::UnifiedDiff {
            file_path: PathBuf::from("src/lib.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![core::diff_parser::DiffHunk {
                old_start: 14,
                old_lines: 0,
                new_start: 14,
                new_lines: 1,
                context: "@@ -14,0 +14,1 @@".to_string(),
                changes: vec![core::diff_parser::DiffLine {
                    old_line_no: None,
                    new_line_no: Some(14),
                    change_type: core::diff_parser::ChangeType::Added,
                    content: "let sql = format!(\"SELECT * FROM users WHERE id = {}\", user_id);"
                        .to_string(),
                }],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    fn test_result(content: &str) -> DispatchedJobResult {
        DispatchedJobResult {
            job_order: 0,
            diff_index: 0,
            active_rules: Vec::new(),
            path_config: None,
            file_path: PathBuf::from("src/lib.rs"),
            deterministic_comments: Vec::new(),
            pass_kind: None,
            mark_file_complete: true,
            response: Ok(LLMResponse {
                content: content.to_string(),
                model: "test-model".to_string(),
                usage: None,
            }),
            latency_ms: 42,
            agent_data: None,
        }
    }

    #[test]
    fn process_job_result_salvages_parseable_comments_despite_validation_warning() {
        let repetitive_response = format!(
            "Line 14: SQL query interpolates user input and attackers can inject arbitrary SQL.\n{}",
            "The code looks fine. ".repeat(10)
        );

        let processed =
            process_job_result(test_result(&repetitive_response), &test_diff(), false).unwrap();

        assert_eq!(processed.comment_count, 1);
        assert!(processed.comments[0]
            .content
            .contains("attackers can inject arbitrary SQL"));
    }

    #[test]
    fn process_job_result_falls_back_when_validation_warns_and_nothing_is_parseable() {
        let repetitive_response = "The code looks fine. ".repeat(10);

        let processed =
            process_job_result(test_result(&repetitive_response), &test_diff(), false).unwrap();

        assert_eq!(processed.comment_count, 0);
        assert!(processed.comments.is_empty());
    }
}
