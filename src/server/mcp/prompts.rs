use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
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
            ],
        },
    ]
}

pub(super) fn render_prompt(name: &str, arguments: Value) -> Result<PromptResult> {
    match name {
        "check_pr_readiness" => render_check_pr_readiness(arguments),
        "fix_until_clean" => render_fix_until_clean(arguments),
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

    Ok(PromptResult {
        description: "Run a reusable DiffScope fix loop workflow.",
        messages: vec![PromptMessage {
            role: "user",
            content: PromptTextContent {
                kind: "text",
                text: format!(
                    "You are driving a DiffScope fix loop for GitHub PR `{repo}#{pr_number}` with an iteration budget of {max_iterations}.\n\nLoop instructions:\n1. Start by calling `get_pr_readiness`. If there is no completed review, call `review_pr` with `post_results=false`; otherwise reuse the freshest review id.\n2. If the latest review is stale or incomplete, call `rerun_pr_review` and poll `get_review` until the rerun completes.\n3. Inspect the current findings with `get_pr_findings` (`group_by=file`) and `get_pr_comments` (`status=open`). When a file needs more detail, call `get_review` and extract each finding's `rule_id`, `file_path`, `line_number`, `content`, and `suggestion` as the handoff contract for your code edits.\n4. Use your normal workspace edit/test tools to fix the highest-signal unresolved blockers first. Run the repository validators after each meaningful edit batch.\n5. Call `rerun_pr_review` on the freshest review id, wait for completion with `get_review`, and compare blocker counts and readiness against the previous iteration.\n6. Stop early when `merge_readiness` is `Ready`, `open_blockers` is `0`, there are no unresolved comments, the review fails, or two consecutive iterations show no improvement.\n\nEvery loop summary must include the review id, validator outcome, blocker delta, and the remaining files/rules still preventing merge readiness.",
                    repo = args.repo,
                    pr_number = args.pr_number,
                    max_iterations = max_iterations,
                ),
            },
        }],
    })
}

fn parse_arguments<T>(arguments: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(arguments).context("invalid MCP prompt arguments")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_catalog_lists_readiness_and_fix_loop_workflows() {
        let prompts = prompt_specs();
        let names: Vec<&str> = prompts.iter().map(|prompt| prompt.name).collect();
        assert!(names.contains(&"check_pr_readiness"));
        assert!(names.contains(&"fix_until_clean"));
    }

    #[test]
    fn readiness_prompt_mentions_rerun_flow_and_findings() {
        let prompt = render_prompt(
            "check_pr_readiness",
            json!({
                "repo": "owner/repo",
                "pr_number": 42,
                "rerun_if_stale": true,
            }),
        )
        .unwrap();

        let text = &prompt.messages[0].content.text;
        assert!(text.contains("get_pr_readiness"));
        assert!(text.contains("rerun_pr_review"));
        assert!(text.contains("get_pr_findings"));
        assert!(text.contains("owner/repo#42"));
    }

    #[test]
    fn fix_loop_prompt_uses_iteration_budget_and_stop_conditions() {
        let prompt = render_prompt(
            "fix_until_clean",
            json!({
                "repo": "owner/repo",
                "pr_number": 7,
                "max_iterations": 5,
            }),
        )
        .unwrap();

        let text = &prompt.messages[0].content.text;
        assert!(text.contains("iteration budget of 5"));
        assert!(text.contains("rerun_pr_review"));
        assert!(text.contains("merge_readiness"));
        assert!(text.contains("no improvement"));
    }
}
