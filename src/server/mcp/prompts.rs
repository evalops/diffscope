use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use super::super::{api, pr_readiness, state::AppState};
use crate::core::comment::{Comment, CommentStatus};
use crate::core::context::ContextFetcher;

#[derive(Clone, Serialize)]
pub(super) struct PromptArgumentSpec {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Serialize)]
pub(super) struct PromptSpec {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgumentSpec>,
}

#[derive(Clone, Serialize)]
pub(super) struct PromptTextContent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

#[derive(Clone, Serialize)]
pub(super) struct PromptMessage {
    pub role: &'static str,
    pub content: PromptTextContent,
}

#[derive(Clone, Serialize)]
pub(super) struct PromptResult {
    pub description: &'static str,
    pub messages: Vec<PromptMessage>,
}

#[derive(Deserialize)]
struct CheckPrReadinessArgs {
    repo: String,
    pr_number: u32,
    #[serde(default)]
    rerun_if_stale: bool,
}

#[derive(Deserialize)]
struct FixUntilCleanArgs {
    repo: String,
    pr_number: u32,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
}

#[derive(Deserialize)]
struct ReplayIssueArgs {
    repo: String,
    pr_number: u32,
    comment_id: String,
}

fn default_max_iterations() -> usize {
    3
}

pub(super) fn prompt_specs() -> Vec<PromptSpec> {
    vec![
        PromptSpec {
            name: "check_pr_readiness",
            description: "Guide an agent through DiffScope PR readiness checks, reruns, and blocker triage.",
            arguments: vec![
                PromptArgumentSpec {
                    name: "repo",
                    description: "GitHub repo in owner/repo format.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "pr_number",
                    description: "GitHub pull request number.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "rerun_if_stale",
                    description: "Whether to rerun the PR review when DiffScope marks the latest review stale.",
                    required: false,
                },
            ],
        },
        PromptSpec {
            name: "fix_until_clean",
            description: "Guide an agent through an iterative DiffScope fix loop until PR blockers are cleared or the iteration budget is exhausted.",
            arguments: vec![
                PromptArgumentSpec {
                    name: "repo",
                    description: "GitHub repo in owner/repo format.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "pr_number",
                    description: "GitHub pull request number.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "max_iterations",
                    description: "Maximum fix-loop iterations before stopping.",
                    required: false,
                },
                PromptArgumentSpec {
                    name: "profile",
                    description: "Optional loop policy profile (`conservative_auditor`, `high_autonomy_fixer`, or `report_only`).",
                    required: false,
                },
            ],
        },
        PromptSpec {
            name: "replay_issue",
            description: "Hand a single unresolved DiffScope finding back to a coding agent with file-local context and concrete fix instructions.",
            arguments: vec![
                PromptArgumentSpec {
                    name: "repo",
                    description: "GitHub repo in owner/repo format.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "pr_number",
                    description: "GitHub pull request number.",
                    required: true,
                },
                PromptArgumentSpec {
                    name: "comment_id",
                    description: "Comment id for the unresolved finding to replay.",
                    required: true,
                },
            ],
        },
    ]
}

pub(super) async fn render_prompt(
    state: Option<&Arc<AppState>>,
    name: &str,
    arguments: Value,
) -> Result<PromptResult> {
    match name {
        "check_pr_readiness" => render_check_pr_readiness(arguments),
        "fix_until_clean" => render_fix_until_clean(arguments),
        "replay_issue" => {
            let state = state.context("replay_issue prompt requires server state")?;
            render_replay_issue(state, arguments).await
        }
        _ => anyhow::bail!("Unknown DiffScope prompt: {name}"),
    }
}

fn render_check_pr_readiness(arguments: Value) -> Result<PromptResult> {
    let args: CheckPrReadinessArgs = parse_arguments(arguments)?;
    let rerun_guidance = if args.rerun_if_stale {
        "If the latest review is stale (`NeedsReReview`, stale head SHA, or no fresh latest review), call `rerun_pr_review` with the latest review id and then poll `get_review` until it reaches `Complete` or `Failed`."
    } else {
        "If the latest review is stale, do not rerun automatically; report the stale state and explain why a rerun is recommended."
    };

    Ok(PromptResult {
        description: "Run a reusable DiffScope PR readiness workflow.",
        messages: vec![PromptMessage {
            role: "user",
            content: PromptTextContent {
                kind: "text",
                text: format!(
                    "You are checking DiffScope readiness for GitHub PR `{repo}#{pr_number}`.\n\nUse these steps:\n1. Call `get_pr_readiness` with `repo={repo}` and `pr_number={pr_number}`.\n2. If there is no `latest_review`, call `review_pr` with `post_results=false`, then poll `get_review` until the new review reaches `Complete` or `Failed`.\n3. {rerun_guidance}\n4. Once you have a fresh completed review, call `get_pr_findings` with `group_by=severity` and `get_pr_comments` with `status=open` to inspect unresolved blockers and the most urgent findings.\n5. Summarize the final `merge_readiness`, `open_blockers`, `readiness_reasons`, the freshest review id, and the top files or rules that still need attention.\n6. If the latest review fails, stop and report the failure payload instead of guessing.\n\nAlways quote concrete evidence from DiffScope fields such as `rule_id`, `file_path`, `line_number`, `content`, and `suggestion` when describing blockers.",
                    repo = args.repo,
                    pr_number = args.pr_number,
                    rerun_guidance = rerun_guidance,
                ),
            },
        }],
    })
}

fn render_fix_until_clean(arguments: Value) -> Result<PromptResult> {
    let args: FixUntilCleanArgs = parse_arguments(arguments)?;
    let max_iterations = args.max_iterations.max(1);
    let profile_argument = args
        .profile
        .as_deref()
        .map(|profile| format!(", `profile={profile}`"))
        .unwrap_or_default();

    Ok(PromptResult {
        description: "Run a reusable DiffScope fix loop workflow.",
        messages: vec![PromptMessage {
            role: "user",
            content: PromptTextContent {
                kind: "text",
                text: format!(
                    "You are driving a DiffScope fix loop for GitHub PR `{repo}#{pr_number}` with an iteration budget of {max_iterations}.\n\nProfiles:\n- `conservative_auditor`: tighter replay surface, no automatic stale reruns.\n- `high_autonomy_fixer`: current autonomous defaults with automatic review starts and stale reruns.\n- `report_only`: never auto-start or auto-rerun reviews; only report the current state.\n\nLoop instructions:\n1. Start by calling `run_fix_until_clean` with `repo={repo}`, `pr_number={pr_number}`, and `max_iterations={max_iterations}`{profile_argument}.\n2. If the loop returns `review_pending`, wait for the referenced review id to finish and call `run_fix_until_clean` again.\n3. If the loop returns `needs_review`, follow the suggested `next_action` (`start_review` or `rerun_review`) or rerun the tool with `auto_start_review=true` / `auto_rerun_stale=true` when your chosen profile allows extra automation.\n4. If the loop returns `needs_fixes`, use `fix_handoff` as the machine-friendly edit contract and use each `replay_candidates[*]` entry with the `replay_issue` prompt when you want a file-local handoff for a specific finding.\n5. Use your normal workspace edit/test tools to fix the highest-signal unresolved blockers first. Run the repository validators after each meaningful edit batch, push the updated PR head, and call `run_fix_until_clean` again to measure blocker deltas.\n6. Stop when the loop returns `converged`, `failed`, `exhausted`, or `stalled`. `stalled` means two consecutive review iterations showed no improvement.\n\nEvery loop summary must include the review id, validator outcome, blocker delta, and the remaining files/rules still preventing merge readiness.",
                    repo = args.repo,
                    pr_number = args.pr_number,
                    max_iterations = max_iterations,
                    profile_argument = profile_argument,
                ),
            },
        }],
    })
}

async fn render_replay_issue(state: &Arc<AppState>, arguments: Value) -> Result<PromptResult> {
    let args: ReplayIssueArgs = parse_arguments(arguments)?;

    if !api::is_valid_repo_name(&args.repo) {
        anyhow::bail!("invalid repo format; expected 'owner/repo'");
    }

    if args.pr_number == 0 {
        anyhow::bail!("pr_number must be greater than zero");
    }

    let inventory = pr_readiness::load_review_inventory(state).await;
    let review = pr_readiness::latest_pr_review_session(&inventory, &args.repo, args.pr_number)
        .with_context(|| {
            format!(
                "no completed DiffScope review found for {}#{}",
                args.repo, args.pr_number
            )
        })?;
    let comment = review
        .comments
        .iter()
        .find(|comment| comment.id == args.comment_id.as_str())
        .with_context(|| {
            format!(
                "comment '{}' was not found in the latest review '{}' for {}#{}",
                args.comment_id,
                review.id.as_str(),
                args.repo,
                args.pr_number
            )
        })?;

    if comment.status != CommentStatus::Open {
        anyhow::bail!(
            "comment '{}' is {} in review '{}'; replay_issue only supports unresolved findings",
            comment.id.as_str(),
            comment.status,
            review.id.as_str()
        );
    }

    let file_context = format_file_local_context(&state.repo_path, comment).await?;
    let summary = review.summary.as_ref();
    let rule_id = comment.rule_id.as_deref().unwrap_or("unscoped");
    let suggestion = comment
        .suggestion
        .as_deref()
        .unwrap_or("No explicit reviewer suggestion was provided.");
    let severity = comment.severity.to_string();
    let category = comment.category.to_string();
    let lifecycle_status = comment.status.to_string();
    let fix_effort = format_fix_effort(&comment.fix_effort);
    let readiness_reasons = summary
        .map(|summary| {
            if summary.readiness_reasons.is_empty() {
                "none".to_string()
            } else {
                summary.readiness_reasons.join("; ")
            }
        })
        .unwrap_or_else(|| "none".to_string());
    let tags = if comment.tags.is_empty() {
        "none".to_string()
    } else {
        comment.tags.join(", ")
    };

    let mut text = format!(
        "You are replaying a single unresolved DiffScope finding for GitHub PR `{repo}#{pr_number}`.\n\nReview snapshot:\n- latest_review_id: {review_id}\n- merge_readiness: {merge_readiness}\n- open_blockers: {open_blockers}\n- readiness_reasons: {readiness_reasons}\n\nFinding contract:\n- comment_id: {comment_id}\n- rule_id: {rule_id}\n- severity: {severity}\n- category: {category}\n- lifecycle_status: {lifecycle_status}\n- fix_effort: {fix_effort}\n- confidence: {confidence:.2}\n- tags: {tags}\n- file_path: {file_path}\n- line_number: {line_number}\n\nReviewer evidence:\n- content: {content}\n- suggestion: {suggestion}\n\n{file_context}\n\nReplay instructions:\n1. Start from the file-local context above and fix this finding before touching unrelated code.\n2. Keep the patch minimal and preserve behavior outside the affected path unless a small supporting refactor is required.\n3. If nearby helpers or symbols control the bug, inspect them before editing, but keep the final change scoped to the finding.\n4. Run the repository validators after the edit batch.\n5. Report the patch, validator outcome, and whether the finding should be rerun, challenged, or marked resolved.\n\nAlways quote the DiffScope evidence (`rule_id`, `file_path`, `line_number`, `content`) in your final handoff summary.",
        repo = args.repo,
        pr_number = args.pr_number,
        review_id = review.id.as_str(),
        merge_readiness = summary
            .map(|summary| summary.merge_readiness.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        open_blockers = summary
            .map(|summary| summary.open_blockers.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        readiness_reasons = readiness_reasons,
        comment_id = comment.id.as_str(),
        rule_id = rule_id,
        severity = severity,
        category = category,
        lifecycle_status = lifecycle_status,
        fix_effort = fix_effort,
        confidence = comment.confidence,
        tags = tags,
        file_path = comment.file_path.display(),
        line_number = comment.line_number,
        content = comment.content.as_str(),
        suggestion = suggestion,
        file_context = file_context,
    );

    if let Some(code_suggestion) = comment.code_suggestion.as_ref() {
        text.push_str(&format!(
            "\n\nSuggested patch hint:\n- explanation: {}\n```diff\n{}\n```",
            code_suggestion.explanation, code_suggestion.diff
        ));
    }

    Ok(PromptResult {
        description: "Replay a single unresolved DiffScope finding with local file context.",
        messages: vec![PromptMessage {
            role: "user",
            content: PromptTextContent { kind: "text", text },
        }],
    })
}

async fn format_file_local_context(repo_path: &Path, comment: &Comment) -> Result<String> {
    let file_path = comment.file_path.clone();
    let focus_line = comment.line_number.max(1);
    let fetcher = ContextFetcher::new(repo_path.to_path_buf());
    let context_chunks = fetcher
        .fetch_context_for_file(&file_path, &[(focus_line, focus_line)])
        .await?;
    let (start_line, end_line) = context_chunks
        .first()
        .and_then(|chunk| chunk.line_range)
        .unwrap_or((focus_line.saturating_sub(5).max(1), focus_line + 2));

    match read_numbered_excerpt(repo_path, &file_path, start_line, end_line).await? {
        Some(excerpt) => Ok(format!(
            "File-local context from `{}` lines {}-{}:\n```text\n{}```",
            file_path.display(),
            start_line,
            end_line,
            excerpt
        )),
        None => Ok(format!(
            "File-local context from `{}` is unavailable because the file is not present in the current worktree.",
            file_path.display()
        )),
    }
}

async fn read_numbered_excerpt(
    repo_path: &Path,
    file_path: &Path,
    start_line: usize,
    end_line: usize,
) -> Result<Option<String>> {
    let full_path = repo_path.join(file_path);
    if !full_path.exists() {
        return Ok(None);
    }

    let repo_root = repo_path.canonicalize()?;
    let canonical_path = full_path.canonicalize()?;
    if !canonical_path.starts_with(&repo_root) {
        anyhow::bail!(
            "file-local context path escapes the repository root: {}",
            file_path.display()
        );
    }

    let content = tokio::fs::read_to_string(&canonical_path).await?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Ok(Some(String::new()));
    }

    let start = start_line.max(1).min(lines.len());
    let end = end_line.max(start).min(lines.len());
    let width = end.to_string().len().max(2);
    let mut excerpt = String::new();

    for (index, line) in lines[start - 1..end].iter().enumerate() {
        let line_number = start + index;
        excerpt.push_str(&format!("{line_number:>width$} | {line}\n", width = width));
    }

    Ok(Some(excerpt))
}

fn parse_arguments<T>(arguments: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(arguments).context("invalid MCP prompt arguments")
}

fn format_fix_effort(fix_effort: &crate::core::comment::FixEffort) -> &'static str {
    match fix_effort {
        crate::core::comment::FixEffort::Low => "Low",
        crate::core::comment::FixEffort::Medium => "Medium",
        crate::core::comment::FixEffort::High => "High",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::comment::{Category, CodeSuggestion, FixEffort, Severity};
    use crate::core::CommentSynthesizer;
    use crate::server::state::{ReviewSession, ReviewStatus, MAX_CONCURRENT_REVIEWS};
    use crate::server::storage::StorageBackend;
    use crate::server::storage_json::JsonStorageBackend;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio::sync::{RwLock, Semaphore};

    fn test_state(repo_path: PathBuf) -> Arc<AppState> {
        let feedback_path = repo_path.join("feedback.json");
        let convention_store_path = repo_path.join("conventions.json");
        let storage_path = repo_path.join("reviews.json");
        let config_path = repo_path.join("config.json");

        let config = Config {
            feedback_path,
            convention_store_path: Some(convention_store_path.display().to_string()),
            ..Config::default()
        };

        let storage: Arc<dyn StorageBackend> = Arc::new(JsonStorageBackend::new(&storage_path));

        Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            repo_path,
            reviews: Arc::new(RwLock::new(HashMap::new())),
            storage,
            storage_path,
            config_path,
            http_client: reqwest::Client::new(),
            review_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_REVIEWS)),
            last_reviewed_shas: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    fn make_comment(id: &str) -> Comment {
        Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 12,
            content: "Add a guard for missing input".to_string(),
            rule_id: Some("sec.missing-guard".to_string()),
            severity: Severity::Warning,
            category: Category::Security,
            suggestion: Some("Validate input before use".to_string()),
            confidence: 0.82,
            code_suggestion: Some(CodeSuggestion {
                original_code: "run(input);".to_string(),
                suggested_code: "run(validated_input);".to_string(),
                explanation: "Validate or normalize the input before running.".to_string(),
                diff: "- run(input);\n+ run(validated_input);".to_string(),
            }),
            tags: vec!["security".to_string(), "input".to_string()],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    fn make_review(id: &str, comment: Comment) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-123".to_string()),
            github_post_results_requested: Some(false),
            started_at: 100,
            completed_at: Some(101),
            comments: vec![comment.clone()],
            summary: Some(CommentSynthesizer::generate_summary(&[comment])),
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn prompt_catalog_lists_readiness_and_fix_loop_workflows() {
        let prompts = prompt_specs();
        let names: Vec<&str> = prompts.iter().map(|prompt| prompt.name).collect();
        assert!(names.contains(&"check_pr_readiness"));
        assert!(names.contains(&"fix_until_clean"));
        assert!(names.contains(&"replay_issue"));
    }

    #[tokio::test]
    async fn readiness_prompt_mentions_rerun_flow_and_findings() {
        let prompt = render_prompt(
            None,
            "check_pr_readiness",
            json!({
                "repo": "owner/repo",
                "pr_number": 42,
                "rerun_if_stale": true,
            }),
        )
        .await
        .unwrap();

        let text = &prompt.messages[0].content.text;
        assert!(text.contains("get_pr_readiness"));
        assert!(text.contains("rerun_pr_review"));
        assert!(text.contains("get_pr_findings"));
        assert!(text.contains("owner/repo#42"));
    }

    #[tokio::test]
    async fn fix_loop_prompt_uses_iteration_budget_and_stop_conditions() {
        let prompt = render_prompt(
            None,
            "fix_until_clean",
            json!({
                "repo": "owner/repo",
                "pr_number": 7,
                "max_iterations": 5,
                "profile": "conservative_auditor",
            }),
        )
        .await
        .unwrap();

        let text = &prompt.messages[0].content.text;
        assert!(text.contains("iteration budget of 5"));
        assert!(text.contains("run_fix_until_clean"));
        assert!(text.contains("conservative_auditor"));
        assert!(text.contains("high_autonomy_fixer"));
        assert!(text.contains("report_only"));
        assert!(text.contains("fix_handoff"));
        assert!(text.contains("replay_issue"));
        assert!(text.contains("stalled"));
    }

    #[tokio::test]
    async fn replay_issue_prompt_includes_finding_contract_and_file_context() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            [
                "pub fn replay_issue(input: &str) {",
                "    let prefix = \"value:\";",
                "    println!(\"{}\", prefix);",
                "}",
                "",
                "pub fn run(input: &str) {",
                "    if input.is_empty() {",
                "        return;",
                "    }",
                "",
                "    let normalized = input.trim();",
                "    println!(\"{}\", normalized);",
                "}",
            ]
            .join("\n"),
        )
        .unwrap();

        let state = test_state(dir.path().to_path_buf());
        let review = make_review("review-1", make_comment("comment-1"));
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review);

        let prompt = render_prompt(
            Some(&state),
            "replay_issue",
            json!({
                "repo": "owner/repo",
                "pr_number": 42,
                "comment_id": "comment-1"
            }),
        )
        .await
        .unwrap();

        let text = &prompt.messages[0].content.text;
        assert!(text.contains("comment_id: comment-1"));
        assert!(text.contains("rule_id: sec.missing-guard"));
        assert!(text.contains("File-local context from `src/lib.rs`"));
        assert!(text.contains("12 |     println!(\"{}\", normalized);"));
        assert!(text.contains("```diff"));
    }
}
