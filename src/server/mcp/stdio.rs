use anyhow::{Context, Result};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufReader, BufWriter};
use std::sync::Arc;
use tracing::info;

use super::super::{api, state::AppState};
use super::prompts::{prompt_specs, render_prompt};
use super::protocol::{
    error_response, read_message, success_response, write_message, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, SERVER_NOT_INITIALIZED,
};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_INSTRUCTIONS: &str = "DiffScope exposes repository review, PR readiness, analytics, feedback-management tools, and reusable agent workflows over MCP. Review-starting tools are asynchronous; poll get_review, list_reviews, or get_pr_readiness after starting a review.";

pub(crate) async fn start_mcp_server(config: crate::config::Config) -> Result<()> {
    let state = Arc::new(AppState::new(config).await?);
    info!(repo_path = %state.repo_path.display(), "DiffScope MCP server listening on stdio");
    StdioMcpServer::new(state).serve().await
}

#[derive(Clone)]
struct ToolSpec {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

impl ToolSpec {
    fn as_mcp_tool(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }
}

#[derive(Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize)]
struct PromptGetParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize)]
struct ReviewIdInput {
    id: String,
}

#[derive(Deserialize)]
struct ReviewPrInput {
    repo: String,
    pr_number: u32,
    #[serde(default)]
    post_results: bool,
}

#[derive(Deserialize)]
struct ReviewFeedbackInput {
    review_id: String,
    comment_id: String,
    action: String,
}

#[derive(Deserialize)]
struct ReviewLifecycleInput {
    review_id: String,
    comment_id: String,
    status: String,
}

struct StdioMcpServer {
    state: Arc<AppState>,
    initialized: bool,
}

impl StdioMcpServer {
    fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            initialized: false,
        }
    }

    async fn serve(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut reader = BufReader::new(stdin.lock());
        let mut writer = BufWriter::new(stdout.lock());

        while let Some(message) = read_message(&mut reader)? {
            if let Some(response) = self.handle_message(message).await {
                write_message(&mut writer, &response)?;
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, message: Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error_response(
                None,
                INVALID_REQUEST,
                "MCP messages must be JSON objects",
                None,
            ));
        };

        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return id.map(|id| {
                error_response(
                    Some(id),
                    INVALID_REQUEST,
                    "MCP requests must include a method",
                    None,
                )
            });
        };

        let params = object.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "notifications/initialized" | "$/cancelRequest" => return None,
            _ => {}
        }

        let id = match id {
            Some(id) => id,
            None => return None,
        };

        let response = match method {
            "initialize" => self.handle_initialize(id),
            _ if !self.initialized => error_response(
                Some(id),
                SERVER_NOT_INITIALIZED,
                "Server not initialized",
                None,
            ),
            "ping" => success_response(id, json!({})),
            "prompts/list" => self.handle_prompts_list(id),
            "prompts/get" => self.handle_prompts_get(id, params).await,
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, params).await,
            _ => error_response(
                Some(id),
                METHOD_NOT_FOUND,
                format!("Unknown MCP method: {method}"),
                None,
            ),
        };

        Some(response)
    }

    fn handle_initialize(&mut self, id: Value) -> Value {
        self.initialized = true;
        success_response(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "prompts": {
                        "listChanged": false,
                    },
                    "tools": {
                        "listChanged": false,
                    }
                },
                "serverInfo": {
                    "name": "diffscope",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": SERVER_INSTRUCTIONS,
            }),
        )
    }

    fn handle_prompts_list(&self, id: Value) -> Value {
        success_response(
            id,
            json!({
                "prompts": prompt_specs(),
            }),
        )
    }

    async fn handle_prompts_get(&self, id: Value, params: Value) -> Value {
        let params: PromptGetParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(err) => {
                return error_response(
                    Some(id),
                    INVALID_PARAMS,
                    format!("Invalid prompts/get params: {err}"),
                    None,
                )
            }
        };

        let arguments = match normalize_object_arguments(params.arguments, "Prompt arguments") {
            Ok(arguments) => arguments,
            Err(err) => return error_response(Some(id), INVALID_PARAMS, err.to_string(), None),
        };

        match render_prompt(Some(&self.state), &params.name, arguments).await {
            Ok(prompt) => match to_value(prompt) {
                Ok(prompt) => success_response(id, prompt),
                Err(err) => error_response(Some(id), INVALID_PARAMS, err.to_string(), None),
            },
            Err(err) => error_response(Some(id), INVALID_PARAMS, err.to_string(), None),
        }
    }

    fn handle_tools_list(&self, id: Value) -> Value {
        success_response(
            id,
            json!({
                "tools": tool_specs()
                    .into_iter()
                    .map(|tool| tool.as_mcp_tool())
                    .collect::<Vec<_>>(),
            }),
        )
    }

    async fn handle_tools_call(&self, id: Value, params: Value) -> Value {
        let params: ToolCallParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(err) => {
                return error_response(
                    Some(id),
                    INVALID_PARAMS,
                    format!("Invalid tools/call params: {err}"),
                    None,
                )
            }
        };

        let arguments = match normalize_object_arguments(params.arguments, "Tool arguments") {
            Ok(arguments) => arguments,
            Err(err) => return error_response(Some(id), INVALID_PARAMS, err.to_string(), None),
        };

        let result = match self.call_tool(&params.name, arguments).await {
            Ok(value) => tool_success(value),
            Err(err) => tool_error(err.to_string(), None),
        };

        success_response(id, result)
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            "get_status" => self.get_status().await,
            "list_reviews" => self.list_reviews(arguments).await,
            "get_review" => self.get_review(arguments).await,
            "start_review" => self.start_review(arguments).await,
            "review_pr" => self.review_pr(arguments).await,
            "rerun_pr_review" => self.rerun_pr_review(arguments).await,
            "get_pr_readiness" => self.get_pr_readiness(arguments).await,
            "get_pr_comments" => self.get_pr_comments(arguments).await,
            "get_pr_findings" => self.get_pr_findings(arguments).await,
            "run_fix_until_clean" => self.run_fix_until_clean(arguments).await,
            "get_fix_handoff" => self.get_fix_handoff(arguments).await,
            "list_events" => self.list_events(arguments).await,
            "get_event_stats" => self.get_event_stats(arguments).await,
            "get_analytics_trends" => self.get_analytics_trends().await,
            "get_learned_rules" => self.get_learned_rules().await,
            "get_attention_gaps" => self.get_attention_gaps().await,
            "get_rejected_patterns" => self.get_rejected_patterns().await,
            "submit_comment_feedback" => self.submit_comment_feedback(arguments).await,
            "update_comment_lifecycle" => self.update_comment_lifecycle(arguments).await,
            _ => anyhow::bail!("Unknown DiffScope tool: {name}"),
        }
    }

    async fn get_status(&self) -> Result<Value> {
        to_value(api::get_status(State(self.state.clone())).await.0)
    }

    async fn list_reviews(&self, arguments: Value) -> Result<Value> {
        let params: api::ListReviewsParams = parse_arguments(arguments)?;
        to_value(
            api::list_reviews(State(self.state.clone()), Query(params))
                .await
                .0,
        )
    }

    async fn get_review(&self, arguments: Value) -> Result<Value> {
        let input: ReviewIdInput = parse_arguments(arguments)?;
        match api::get_review(State(self.state.clone()), Path(input.id.clone())).await {
            Ok(response) => to_value(response.0),
            Err(StatusCode::NOT_FOUND) => anyhow::bail!("Review '{}' not found", input.id),
            Err(status) => anyhow::bail!("Failed to load review '{}': {}", input.id, status),
        }
    }

    async fn start_review(&self, arguments: Value) -> Result<Value> {
        let request: api::StartReviewRequest = parse_arguments(arguments)?;
        match api::start_review(State(self.state.clone()), Json(request)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn review_pr(&self, arguments: Value) -> Result<Value> {
        let input: ReviewPrInput = parse_arguments(arguments)?;
        let request = api::StartPrReviewRequest {
            repo: input.repo,
            pr_number: input.pr_number,
            post_results: input.post_results,
        };

        match api::dispatch_pr_review(&self.state, request).await {
            Ok(response) => to_value(response),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn rerun_pr_review(&self, arguments: Value) -> Result<Value> {
        let request: api::RerunPrReviewRequest = parse_arguments(arguments)?;
        match api::rerun_pr_review(State(self.state.clone()), Json(request)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn get_pr_readiness(&self, arguments: Value) -> Result<Value> {
        let params: api::PrReadinessParams = parse_arguments(arguments)?;
        match api::get_gh_pr_readiness(State(self.state.clone()), Query(params)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn get_pr_comments(&self, arguments: Value) -> Result<Value> {
        let params: api::PrCommentSearchParams = parse_arguments(arguments)?;
        match api::get_gh_pr_comments(State(self.state.clone()), Query(params)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn get_pr_findings(&self, arguments: Value) -> Result<Value> {
        let params: api::PrFindingsParams = parse_arguments(arguments)?;
        match api::get_gh_pr_findings(State(self.state.clone()), Query(params)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn run_fix_until_clean(&self, arguments: Value) -> Result<Value> {
        let request: api::PrFixLoopRequest = parse_arguments(arguments)?;
        match api::run_gh_pr_fix_loop(State(self.state.clone()), Json(request)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn get_fix_handoff(&self, arguments: Value) -> Result<Value> {
        let params: api::PrFixHandoffParams = parse_arguments(arguments)?;
        match api::get_gh_pr_fix_handoff(State(self.state.clone()), Query(params)).await {
            Ok(response) => to_value(response.0),
            Err((_, message)) => anyhow::bail!(message),
        }
    }

    async fn list_events(&self, arguments: Value) -> Result<Value> {
        let params: api::ListEventsParams = parse_arguments(arguments)?;
        to_value(
            api::list_events(State(self.state.clone()), Query(params))
                .await
                .0,
        )
    }

    async fn get_event_stats(&self, arguments: Value) -> Result<Value> {
        let params: api::ListEventsParams = parse_arguments(arguments)?;
        to_value(
            api::get_event_stats(State(self.state.clone()), Query(params))
                .await
                .0,
        )
    }

    async fn get_analytics_trends(&self) -> Result<Value> {
        to_value(api::get_analytics_trends(State(self.state.clone())).await.0)
    }

    async fn get_learned_rules(&self) -> Result<Value> {
        to_value(
            api::get_analytics_learned_rules(State(self.state.clone()))
                .await
                .0,
        )
    }

    async fn get_attention_gaps(&self) -> Result<Value> {
        to_value(
            api::get_analytics_attention_gaps(State(self.state.clone()))
                .await
                .0,
        )
    }

    async fn get_rejected_patterns(&self) -> Result<Value> {
        to_value(
            api::get_analytics_rejected_patterns(State(self.state.clone()))
                .await
                .0,
        )
    }

    async fn submit_comment_feedback(&self, arguments: Value) -> Result<Value> {
        let input: ReviewFeedbackInput = parse_arguments(arguments)?;
        let request = api::FeedbackRequest {
            comment_id: input.comment_id.clone(),
            action: input.action.clone(),
        };

        match api::submit_feedback(
            State(self.state.clone()),
            Path(input.review_id.clone()),
            Json(request),
        )
        .await
        {
            Ok(response) => to_value(response.0),
            Err(StatusCode::BAD_REQUEST) => {
                anyhow::bail!("Invalid feedback action. Use 'accept' or 'reject'.")
            }
            Err(StatusCode::NOT_FOUND) => anyhow::bail!(
                "Review '{}' or comment '{}' was not found",
                input.review_id,
                input.comment_id
            ),
            Err(status) => anyhow::bail!("Failed to submit feedback: {}", status),
        }
    }

    async fn update_comment_lifecycle(&self, arguments: Value) -> Result<Value> {
        let input: ReviewLifecycleInput = parse_arguments(arguments)?;
        let request = api::CommentLifecycleRequest {
            comment_id: input.comment_id.clone(),
            status: input.status.clone(),
        };

        match api::update_comment_lifecycle(
            State(self.state.clone()),
            Path(input.review_id.clone()),
            Json(request),
        )
        .await
        {
            Ok(response) => to_value(response.0),
            Err(StatusCode::BAD_REQUEST) => {
                anyhow::bail!("Invalid lifecycle status. Use 'open', 'resolved', or 'dismissed'.")
            }
            Err(StatusCode::NOT_FOUND) => anyhow::bail!(
                "Review '{}' or comment '{}' was not found",
                input.review_id,
                input.comment_id
            ),
            Err(status) => anyhow::bail!("Failed to update comment lifecycle: {}", status),
        }
    }
}

fn normalize_object_arguments(arguments: Value, label: &str) -> Result<Value> {
    match arguments {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(arguments),
        _ => anyhow::bail!("{label} must be a JSON object"),
    }
}

fn parse_arguments<T>(arguments: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(arguments).context("invalid tool arguments")
}

fn to_value<T>(value: T) -> Result<Value>
where
    T: serde::Serialize,
{
    serde_json::to_value(value).context("failed to serialize MCP tool response")
}

fn tool_success(value: Value) -> Value {
    let text = render_tool_text(&value);
    json!({
        "content": [
            {
                "type": "text",
                "text": text,
            }
        ],
        "structuredContent": value,
        "isError": false,
    })
}

fn tool_error(message: String, data: Option<Value>) -> Value {
    let structured = data.unwrap_or_else(|| json!({ "error": message }));
    json!({
        "content": [
            {
                "type": "text",
                "text": render_tool_text(&structured),
            }
        ],
        "structuredContent": structured,
        "isError": true,
    })
}

fn render_tool_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}

fn list_event_filter_properties() -> Value {
    json!({
        "source": {
            "type": "string",
            "description": "Filter by diff source or PR source"
        },
        "model": {
            "type": "string",
            "description": "Filter by model name"
        },
        "status": {
            "type": "string",
            "description": "Filter by review status"
        },
        "time_from": {
            "type": "string",
            "description": "Inclusive RFC3339 start timestamp"
        },
        "time_to": {
            "type": "string",
            "description": "Inclusive RFC3339 end timestamp"
        },
        "github_repo": {
            "type": "string",
            "description": "Filter by GitHub repo in owner/repo format"
        },
        "limit": {
            "type": "integer",
            "description": "Maximum number of rows to return"
        },
        "offset": {
            "type": "integer",
            "description": "Number of rows to skip"
        }
    })
}

fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "get_status",
            description: "Return the current repository path, branch, active review count, and configured model/provider.",
            input_schema: empty_schema(),
        },
        ToolSpec {
            name: "list_reviews",
            description: "List stored review summaries with pagination.",
            input_schema: object_schema(
                json!({
                    "page": { "type": "integer", "description": "1-based page number" },
                    "per_page": { "type": "integer", "description": "Page size" }
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "get_review",
            description: "Fetch the full stored review session for a given review id.",
            input_schema: object_schema(
                json!({
                    "id": { "type": "string", "description": "Review id" }
                }),
                &["id"],
            ),
        },
        ToolSpec {
            name: "start_review",
            description: "Start a repository review over head, staged, branch, or raw diff input.",
            input_schema: object_schema(
                json!({
                    "diff_source": {
                        "type": "string",
                        "enum": ["head", "staged", "branch", "raw"],
                        "description": "Diff source to review"
                    },
                    "base_branch": {
                        "type": "string",
                        "description": "Base branch when diff_source is branch"
                    },
                    "diff_content": {
                        "type": "string",
                        "description": "Raw diff content when diff_source is raw"
                    },
                    "title": {
                        "type": "string",
                        "description": "Optional display title for raw diffs"
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional per-review model override"
                    },
                    "strictness": {
                        "type": "integer",
                        "description": "Optional review strictness override (1-3)"
                    },
                    "review_profile": {
                        "type": "string",
                        "description": "Optional review profile override"
                    }
                }),
                &["diff_source"],
            ),
        },
        ToolSpec {
            name: "review_pr",
            description: "Start a GitHub PR review and return the new review id.",
            input_schema: object_schema(
                json!({
                    "repo": {
                        "type": "string",
                        "description": "GitHub repo in owner/repo format"
                    },
                    "pr_number": {
                        "type": "integer",
                        "description": "GitHub pull request number"
                    },
                    "post_results": {
                        "type": "boolean",
                        "description": "Whether DiffScope should post review results back to GitHub"
                    }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "rerun_pr_review",
            description: "Re-run an existing GitHub PR review using the stored review metadata.",
            input_schema: object_schema(
                json!({
                    "review_id": {
                        "type": "string",
                        "description": "Existing review id tied to a GitHub PR"
                    },
                    "post_results": {
                        "type": "boolean",
                        "description": "Optional override for whether to post results back to GitHub"
                    }
                }),
                &["review_id"],
            ),
        },
        ToolSpec {
            name: "get_pr_readiness",
            description: "Get the latest PR readiness snapshot for a GitHub pull request.",
            input_schema: object_schema(
                json!({
                    "repo": { "type": "string", "description": "GitHub repo in owner/repo format" },
                    "pr_number": { "type": "integer", "description": "GitHub pull request number" }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "get_pr_comments",
            description: "Fetch the latest PR comments for a GitHub pull request, optionally filtered by lifecycle state.",
            input_schema: object_schema(
                json!({
                    "repo": { "type": "string", "description": "GitHub repo in owner/repo format" },
                    "pr_number": { "type": "integer", "description": "GitHub pull request number" },
                    "status": {
                        "type": "string",
                        "enum": ["all", "open", "unresolved", "resolved", "dismissed"],
                        "description": "Optional lifecycle filter"
                    }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "get_pr_findings",
            description: "Fetch grouped findings for the latest PR review by severity, file, or lifecycle.",
            input_schema: object_schema(
                json!({
                    "repo": { "type": "string", "description": "GitHub repo in owner/repo format" },
                    "pr_number": { "type": "integer", "description": "GitHub pull request number" },
                    "group_by": {
                        "type": "string",
                        "enum": ["severity", "file", "lifecycle"],
                        "description": "Grouping key for findings"
                    }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "run_fix_until_clean",
            description: "Drive a first-class PR fix loop by starting or rerunning DiffScope reviews, detecting convergence, and returning replay candidates for unresolved findings.",
            input_schema: object_schema(
                json!({
                    "repo": { "type": "string", "description": "GitHub repo in owner/repo format" },
                    "pr_number": { "type": "integer", "description": "GitHub pull request number" },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Maximum completed review iterations before the loop stops"
                    },
                    "replay_limit": {
                        "type": "integer",
                        "description": "Maximum unresolved findings to surface as replay candidates"
                    },
                    "auto_start_review": {
                        "type": "boolean",
                        "description": "Whether to automatically start a PR review when no completed review exists"
                    },
                    "auto_rerun_stale": {
                        "type": "boolean",
                        "description": "Whether to automatically rerun when the latest review is stale against the current PR head"
                    }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "get_fix_handoff",
            description: "Return a machine-friendly fix-agent handoff contract with rule IDs, evidence, and suggested diffs for the latest PR review.",
            input_schema: object_schema(
                json!({
                    "repo": { "type": "string", "description": "GitHub repo in owner/repo format" },
                    "pr_number": { "type": "integer", "description": "GitHub pull request number" },
                    "include_resolved": {
                        "type": "boolean",
                        "description": "Whether resolved and dismissed findings should also be included in the handoff contract"
                    }
                }),
                &["repo", "pr_number"],
            ),
        },
        ToolSpec {
            name: "list_events",
            description: "List wide review events with optional analytics filters.",
            input_schema: object_schema(list_event_filter_properties(), &[]),
        },
        ToolSpec {
            name: "get_event_stats",
            description: "Return aggregated review-event analytics with optional filters.",
            input_schema: object_schema(list_event_filter_properties(), &[]),
        },
        ToolSpec {
            name: "get_analytics_trends",
            description: "Load eval and feedback-eval trend history artifacts.",
            input_schema: empty_schema(),
        },
        ToolSpec {
            name: "get_learned_rules",
            description: "Return learned boost and suppression patterns derived from historical review feedback.",
            input_schema: empty_schema(),
        },
        ToolSpec {
            name: "get_attention_gaps",
            description: "Return the latest feedback-vs-eval attention gap snapshot.",
            input_schema: empty_schema(),
        },
        ToolSpec {
            name: "get_rejected_patterns",
            description: "Return the most frequently rejected feedback patterns by category, rule, and file pattern.",
            input_schema: empty_schema(),
        },
        ToolSpec {
            name: "submit_comment_feedback",
            description: "Mark a review comment as accepted or rejected and update learned feedback stores.",
            input_schema: object_schema(
                json!({
                    "review_id": { "type": "string", "description": "Review id" },
                    "comment_id": { "type": "string", "description": "Comment id" },
                    "action": {
                        "type": "string",
                        "enum": ["accept", "reject"],
                        "description": "Feedback action"
                    }
                }),
                &["review_id", "comment_id", "action"],
            ),
        },
        ToolSpec {
            name: "update_comment_lifecycle",
            description: "Change a review comment lifecycle state to open, resolved, or dismissed.",
            input_schema: object_schema(
                json!({
                    "review_id": { "type": "string", "description": "Review id" },
                    "comment_id": { "type": "string", "description": "Comment id" },
                    "status": {
                        "type": "string",
                        "enum": ["open", "resolved", "dismissed"],
                        "description": "Lifecycle status"
                    }
                }),
                &["review_id", "comment_id", "status"],
            ),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};
    use crate::core::{Comment, CommentSynthesizer};
    use crate::server::state::{AppState, ReviewSession, ReviewStatus, MAX_CONCURRENT_REVIEWS};
    use crate::server::storage::StorageBackend;
    use crate::server::storage_json::JsonStorageBackend;
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
            code_suggestion: None,
            tags: Vec::new(),
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

    async fn initialize_server(server: &mut StdioMcpServer) {
        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn initialize_exposes_tool_capabilities() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut server = StdioMcpServer::new(state);

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {}
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["serverInfo"]["name"], "diffscope");
        assert_eq!(
            response["result"]["capabilities"]["prompts"]["listChanged"],
            false
        );
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
    }

    #[tokio::test]
    async fn prompts_list_returns_reusable_workflows() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut server = StdioMcpServer::new(state);
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "prompts/list",
                "params": {}
            }))
            .await
            .unwrap();

        let prompts = response["result"]["prompts"].as_array().unwrap();
        assert!(prompts
            .iter()
            .any(|prompt| prompt["name"] == "check_pr_readiness"));
        assert!(prompts
            .iter()
            .any(|prompt| prompt["name"] == "fix_until_clean"));
        assert!(prompts
            .iter()
            .any(|prompt| prompt["name"] == "replay_issue"));
    }

    #[tokio::test]
    async fn prompts_get_renders_fix_loop_workflow() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut server = StdioMcpServer::new(state);
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "prompts/get",
                "params": {
                    "name": "fix_until_clean",
                    "arguments": {
                        "repo": "owner/repo",
                        "pr_number": 42,
                        "max_iterations": 4
                    }
                }
            }))
            .await
            .unwrap();

        let text = response["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("iteration budget of 4"));
        assert!(text.contains("run_fix_until_clean"));
        assert!(text.contains("replay_issue"));
        assert!(text.contains("owner/repo#42"));
    }

    #[tokio::test]
    async fn prompts_get_renders_issue_replay_workflow() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            [
                "pub fn replay_issue(input: &str) {",
                "    if input.is_empty() {",
                "        return;",
                "    }",
                "",
                "    let raw = input.trim();",
                "    println!(\"{}\", raw);",
                "}",
                "",
                "pub fn wrapper(input: &str) {",
                "    replay_issue(input);",
                "}",
            ]
            .join("\n"),
        )
        .unwrap();

        let state = test_state(dir.path().to_path_buf());
        let review = make_review("review-replay", make_comment("comment-replay"));
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review);

        let mut server = StdioMcpServer::new(state);
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "prompts/get",
                "params": {
                    "name": "replay_issue",
                    "arguments": {
                        "repo": "owner/repo",
                        "pr_number": 42,
                        "comment_id": "comment-replay"
                    }
                }
            }))
            .await
            .unwrap();

        let text = response["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("comment_id: comment-replay"));
        assert!(text.contains("File-local context from `src/lib.rs`"));
        assert!(text.contains("owner/repo#42"));
    }

    #[test]
    fn tool_catalog_covers_review_analytics_and_rule_management() {
        let tool_names: Vec<&str> = tool_specs().iter().map(|tool| tool.name).collect();
        assert!(tool_names.contains(&"review_pr"));
        assert!(tool_names.contains(&"get_event_stats"));
        assert!(tool_names.contains(&"run_fix_until_clean"));
        assert!(tool_names.contains(&"get_fix_handoff"));
        assert!(tool_names.contains(&"get_learned_rules"));
        assert!(tool_names.contains(&"submit_comment_feedback"));
        assert!(tool_names.contains(&"update_comment_lifecycle"));
    }

    #[tokio::test]
    async fn get_status_tool_returns_repo_metadata() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut server = StdioMcpServer::new(state.clone());
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "get_status",
                    "arguments": {}
                }
            }))
            .await
            .unwrap();

        let content = &response["result"]["structuredContent"];
        assert_eq!(content["repo_path"], state.repo_path.display().to_string());
        assert_eq!(content["active_reviews"], 0);
    }

    #[tokio::test]
    async fn get_fix_handoff_tool_returns_structured_contract() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut review = make_review("review-handoff", make_comment("comment-handoff"));
        review.summary.as_mut().unwrap().open_blockers = 1;
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review.clone());

        let mut server = StdioMcpServer::new(state);
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "get_fix_handoff",
                    "arguments": {
                        "repo": "owner/repo",
                        "pr_number": 42
                    }
                }
            }))
            .await
            .unwrap();

        let contract = &response["result"]["structuredContent"];
        assert_eq!(contract["contract_version"], 1);
        assert_eq!(contract["latest_review_id"], "review-handoff");
        assert_eq!(contract["findings"][0]["rule_id"], "sec.missing-guard");
        assert!(contract["findings"][0]["evidence"]["content"]
            .as_str()
            .unwrap()
            .contains("missing input"));
    }

    #[tokio::test]
    async fn run_fix_until_clean_tool_returns_replay_plan() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let mut review = make_review("review-fix-loop", make_comment("comment-fix-loop"));
        review.summary.as_mut().unwrap().open_blockers = 1;
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review);

        let mut server = StdioMcpServer::new(state);
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "run_fix_until_clean",
                    "arguments": {
                        "repo": "owner/repo",
                        "pr_number": 42,
                        "max_iterations": 3,
                        "replay_limit": 2
                    }
                }
            }))
            .await
            .unwrap();

        let plan = &response["result"]["structuredContent"];
        assert_eq!(plan["status"], "needs_fixes");
        assert_eq!(plan["next_action"], "apply_fixes");
        assert_eq!(plan["loop_telemetry"]["iterations"], 1);
        assert_eq!(plan["replay_candidates"][0]["prompt_name"], "replay_issue");
        assert_eq!(plan["fix_handoff"]["contract_version"], 1);
    }

    #[tokio::test]
    async fn submit_comment_feedback_tool_updates_review_and_feedback_store() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let review = make_review("review-1", make_comment("comment-1"));
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review.clone());

        let mut server = StdioMcpServer::new(state.clone());
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "submit_comment_feedback",
                    "arguments": {
                        "review_id": "review-1",
                        "comment_id": "comment-1",
                        "action": "accept"
                    }
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], false);

        let stored = state.reviews.read().await;
        let session = stored.get("review-1").unwrap();
        assert_eq!(session.comments[0].feedback.as_deref(), Some("accept"));
        drop(stored);

        let feedback_path = state.config.read().await.feedback_path.clone();
        let feedback_json = std::fs::read_to_string(&feedback_path).unwrap();
        let feedback_store: crate::review::FeedbackStore =
            serde_json::from_str(&feedback_json).unwrap();
        assert_eq!(feedback_store.by_category["Security"].accepted, 1);
    }

    #[tokio::test]
    async fn update_comment_lifecycle_tool_marks_comment_resolved() {
        let dir = tempdir().unwrap();
        let state = test_state(dir.path().to_path_buf());
        let review = make_review("review-2", make_comment("comment-2"));
        state
            .reviews
            .write()
            .await
            .insert(review.id.clone(), review.clone());

        let mut server = StdioMcpServer::new(state.clone());
        initialize_server(&mut server).await;

        let response = server
            .handle_message(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "update_comment_lifecycle",
                    "arguments": {
                        "review_id": "review-2",
                        "comment_id": "comment-2",
                        "status": "resolved"
                    }
                }
            }))
            .await
            .unwrap();

        assert_eq!(response["result"]["isError"], false);

        let stored = state.reviews.read().await;
        let session = stored.get("review-2").unwrap();
        assert_eq!(session.comments[0].status, CommentStatus::Resolved);
        assert!(session.comments[0].resolved_at.is_some());
    }
}
