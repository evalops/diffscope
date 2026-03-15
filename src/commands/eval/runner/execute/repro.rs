use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::adapters;
use crate::adapters::llm::{LLMRequest, StructuredOutputSchema};
use crate::config;
use crate::core;
use crate::core::agent_loop::AgentToolCallLog;
use crate::core::agent_tools::{build_review_tools, ReviewTool, ReviewToolContext};
use crate::core::diff_parser::{ChangeType, DiffParser, UnifiedDiff};

use super::super::super::{EvalReproductionCheck, EvalReproductionSummary};
use super::loading::PreparedFixtureExecution;
use super::result::convert_agent_activity;

const REPRODUCTION_SYSTEM_PROMPT: &str = r#"You are a code review reproduction validator.

Your job is to independently confirm or refute a single review finding using the repository tools.
Start from the reported file and line, inspect the diff, and pull in supporting code only as needed.
Do not trust the review comment by itself; verify it from evidence.

Return JSON only with this schema:
{"reproduced":true,"confidence":0.91,"reason":"brief evidence-based explanation"}

Rules:
- `reproduced=true` when the finding is clearly supported by the code and diff.
- `reproduced=false` when the finding is contradicted by the evidence.
- `reproduced=null` when the evidence is inconclusive.
- `confidence` must be between 0.0 and 1.0.
- Keep `reason` short and evidence-based.
"#;

#[derive(Debug, Deserialize)]
struct ReproductionResponse {
    reproduced: Option<bool>,
    confidence: Option<f32>,
    reason: String,
}

pub(super) async fn maybe_run_reproduction_validation(
    config: &config::Config,
    prepared: &PreparedFixtureExecution,
    comments: &[core::Comment],
    max_comments: usize,
) -> Result<Option<EvalReproductionSummary>> {
    if comments.is_empty() || max_comments == 0 {
        return Ok(None);
    }

    let model_config = config.to_model_config_for_role(config.auditing_model_role);
    let model_name = model_config.model_name.clone();
    let adapter: Arc<dyn adapters::llm::LLMAdapter> =
        Arc::from(adapters::llm::create_adapter(&model_config)?);
    let workspace = prepare_reproduction_workspace(prepared)?;
    let tool_context = Arc::new(ReviewToolContext {
        repo_path: workspace.repo_path.clone(),
        context_fetcher: Arc::new(core::ContextFetcher::new(workspace.repo_path.clone())),
        symbol_index: None,
        symbol_graph: None,
        git_history: None,
    });
    let tools = build_review_tools(tool_context, None);

    let mut checks = Vec::new();
    for comment in comments.iter().take(max_comments) {
        let (tool_evidence, tool_logs, tool_warnings) =
            gather_reproduction_evidence(&tools, comment, workspace.include_git_tools).await;
        let user_prompt = build_reproduction_prompt(prepared, comment, &tool_evidence);
        let request = LLMRequest {
            system_prompt: REPRODUCTION_SYSTEM_PROMPT.to_string(),
            user_prompt,
            temperature: Some(0.0),
            max_tokens: Some(400),
            response_schema: Some(reproduction_response_schema()),
        };
        match adapter.complete(request).await {
            Ok(response) => {
                let parsed = parse_reproduction_response(&response.content);
                let agent_activity = convert_agent_activity(Some(crate::review::AgentActivity {
                    total_iterations: usize::from(!tool_logs.is_empty()),
                    tool_calls: tool_logs,
                }));
                match parsed {
                    Ok(parsed) => checks.push(EvalReproductionCheck {
                        comment_id: comment.id.clone(),
                        model: response.model,
                        reproduced: parsed.reproduced,
                        confidence: parsed.confidence.map(|value| value.clamp(0.0, 1.0)),
                        reason: parsed.reason,
                        warning: (!tool_warnings.is_empty()).then(|| tool_warnings.join(" | ")),
                        agent_activity,
                    }),
                    Err(error) => checks.push(EvalReproductionCheck {
                        comment_id: comment.id.clone(),
                        model: response.model,
                        reproduced: None,
                        confidence: None,
                        reason: String::new(),
                        warning: Some(format!(
                            "reproduction validator output was unparseable: {}{}",
                            error,
                            if tool_warnings.is_empty() {
                                String::new()
                            } else {
                                format!(" | tool warnings: {}", tool_warnings.join(" | "))
                            }
                        )),
                        agent_activity,
                    }),
                }
            }
            Err(error) => checks.push(EvalReproductionCheck {
                comment_id: comment.id.clone(),
                model: model_name.clone(),
                reproduced: None,
                confidence: None,
                reason: String::new(),
                warning: Some(format!(
                    "reproduction validator failed: {}{}",
                    error,
                    if tool_warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" | tool warnings: {}", tool_warnings.join(" | "))
                    }
                )),
                agent_activity: convert_agent_activity(Some(crate::review::AgentActivity {
                    total_iterations: usize::from(!tool_logs.is_empty()),
                    tool_calls: tool_logs,
                })),
            }),
        }
    }

    Ok(Some(build_reproduction_summary(checks)))
}

fn build_reproduction_prompt(
    prepared: &PreparedFixtureExecution,
    comment: &core::Comment,
    tool_evidence: &str,
) -> String {
    format!(
        "Validate this review finding against the repository.\n\nFixture: {}\nSuite: {}\nRepo path: {}\n\nChanged diff:\n{}\n\nTool evidence:\n{}\n\nReported finding:\n- file: {}\n- line: {}\n- severity: {}\n- category: {}\n- content: {}\n- suggestion: {}\n",
        prepared.fixture_name,
        prepared.suite_name.as_deref().unwrap_or("standalone"),
        prepared.repo_path.display(),
        prepared.diff_content,
        tool_evidence,
        comment.file_path.display(),
        comment.line_number,
        comment.severity,
        comment.category,
        comment.content,
        comment.suggestion.as_deref().unwrap_or(""),
    )
}

fn build_reproduction_summary(checks: Vec<EvalReproductionCheck>) -> EvalReproductionSummary {
    let mut summary = EvalReproductionSummary::default();
    for check in &checks {
        match check.reproduced {
            Some(true) => summary.confirmed += 1,
            Some(false) => summary.rejected += 1,
            None => summary.inconclusive += 1,
        }
    }
    summary.checks = checks;
    summary
}

fn parse_reproduction_response(content: &str) -> Result<ReproductionResponse> {
    let trimmed = strip_code_fences(content.trim());
    if let Ok(parsed) = serde_json::from_str::<ReproductionResponse>(trimmed) {
        return Ok(parsed);
    }

    let Some(start_index) = trimmed.find('{') else {
        return parse_reproduction_response_fallback(trimmed);
    };
    let Some(end_index) = trimmed.rfind('}') else {
        return parse_reproduction_response_fallback(trimmed);
    };
    serde_json::from_str::<ReproductionResponse>(&trimmed[start_index..=end_index])
        .map_err(anyhow::Error::from)
}

fn strip_code_fences(content: &str) -> &str {
    let stripped = content.trim();
    let stripped = stripped.strip_prefix("```json").unwrap_or(stripped);
    let stripped = stripped.strip_prefix("```").unwrap_or(stripped);
    stripped.strip_suffix("```").unwrap_or(stripped).trim()
}

fn parse_reproduction_response_fallback(content: &str) -> Result<ReproductionResponse> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        anyhow::bail!("no JSON object found");
    }

    let normalized = trimmed.to_ascii_lowercase();
    let reproduced = if normalized.contains("cannot reproduce")
        || normalized.contains("could not reproduce")
        || normalized.contains("not reproduced")
        || normalized.contains("does not support the finding")
        || normalized.contains("doesn't support the finding")
        || normalized.contains("not supported by the diff")
        || normalized.contains("not present")
        || normalized.contains("doesn't exist")
    {
        Some(false)
    } else if normalized.contains("reproduced")
        || normalized.contains("can reproduce")
        || normalized.contains("confirmed")
        || normalized.contains("supports the finding")
        || normalized.contains("finding is valid")
        || normalized.contains("issue is present")
    {
        Some(true)
    } else {
        None
    };

    Ok(ReproductionResponse {
        reproduced,
        confidence: None,
        reason: trimmed.to_string(),
    })
}

#[derive(Debug)]
struct ReproductionWorkspace {
    repo_path: PathBuf,
    include_git_tools: bool,
    _scratch_dir: Option<ScratchDir>,
}

fn prepare_reproduction_workspace(
    prepared: &PreparedFixtureExecution,
) -> Result<ReproductionWorkspace> {
    let parsed_diffs = DiffParser::parse_unified_diff(&prepared.diff_content)?;
    let should_use_fixture_repo = prepared.fixture.repo_path.is_some();
    if should_use_fixture_repo && diff_paths_exist(&prepared.repo_path, &parsed_diffs) {
        return Ok(ReproductionWorkspace {
            repo_path: prepared.repo_path.clone(),
            include_git_tools: prepared.repo_path.join(".git").exists(),
            _scratch_dir: None,
        });
    }

    let scratch_dir = ScratchDir::new()?;
    materialize_diff_workspace(&scratch_dir.path, &parsed_diffs)?;
    Ok(ReproductionWorkspace {
        repo_path: scratch_dir.path.clone(),
        include_git_tools: false,
        _scratch_dir: Some(scratch_dir),
    })
}

fn diff_paths_exist(repo_path: &Path, diffs: &[UnifiedDiff]) -> bool {
    !diffs.is_empty()
        && diffs
            .iter()
            .filter(|diff| !diff.is_binary && !diff.is_deleted)
            .all(|diff| repo_path.join(&diff.file_path).exists())
}

fn materialize_diff_workspace(root: &Path, diffs: &[UnifiedDiff]) -> Result<()> {
    for diff in diffs
        .iter()
        .filter(|diff| !diff.is_binary && !diff.is_deleted)
    {
        let full_path = root.join(&diff.file_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full_path, reconstruct_new_file_contents(diff))?;
    }
    Ok(())
}

fn reconstruct_new_file_contents(diff: &UnifiedDiff) -> String {
    let max_line = diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.changes.iter().filter_map(|line| line.new_line_no))
        .max()
        .unwrap_or_default();
    if max_line == 0 {
        return String::new();
    }

    let mut lines = vec![String::new(); max_line];
    for hunk in &diff.hunks {
        for line in &hunk.changes {
            if line.change_type == ChangeType::Removed {
                continue;
            }
            if let Some(line_number) = line.new_line_no {
                lines[line_number.saturating_sub(1)] = line.content.clone();
            }
        }
    }

    let content = lines.join("\n");
    if content.is_empty() {
        content
    } else {
        format!("{content}\n")
    }
}

#[derive(Debug)]
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "diffscope-eval-repro-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

async fn gather_reproduction_evidence(
    tools: &[Box<dyn ReviewTool>],
    comment: &core::Comment,
    include_git_tools: bool,
) -> (String, Vec<AgentToolCallLog>, Vec<String>) {
    let mut evidence_sections = Vec::new();
    let mut tool_logs = Vec::new();
    let mut warnings = Vec::new();
    let file_path = comment.file_path.to_string_lossy().to_string();
    let start_line = comment.line_number.saturating_sub(8).max(1);
    let end_line = comment.line_number + 8;

    match execute_tool(
        tools,
        "read_file",
        json!({
            "file_path": file_path,
            "start_line": start_line,
            "end_line": end_line
        }),
    )
    .await
    {
        Ok((output, log)) => {
            if output.starts_with("Error:") {
                warnings.push(format!("read_file returned '{output}'"));
            }
            evidence_sections.push(format!("read_file\n{output}"));
            tool_logs.push(log);
        }
        Err(error) => warnings.push(format!("read_file failed: {error}")),
    }

    if include_git_tools {
        match execute_tool(
            tools,
            "get_blame",
            json!({
                "file_path": comment.file_path.to_string_lossy(),
                "start_line": start_line,
                "end_line": end_line
            }),
        )
        .await
        {
            Ok((output, log)) => {
                if output.starts_with("Error:") {
                    warnings.push(format!("get_blame returned '{output}'"));
                }
                evidence_sections.push(format!("get_blame\n{output}"));
                tool_logs.push(log);
            }
            Err(error) => warnings.push(format!("get_blame failed: {error}")),
        }
    }

    (evidence_sections.join("\n\n"), tool_logs, warnings)
}

async fn execute_tool(
    tools: &[Box<dyn ReviewTool>],
    tool_name: &str,
    input: serde_json::Value,
) -> Result<(String, AgentToolCallLog)> {
    let tool = tools
        .iter()
        .find(|tool| tool.name() == tool_name)
        .ok_or_else(|| anyhow::anyhow!("tool '{}' not available", tool_name))?;
    let start = Instant::now();
    let output = tool.execute(input).await?;
    Ok((
        output,
        AgentToolCallLog {
            iteration: 0,
            tool_name: tool_name.to_string(),
            duration_ms: start.elapsed().as_millis() as u64,
        },
    ))
}

fn reproduction_response_schema() -> StructuredOutputSchema {
    StructuredOutputSchema::json_schema(
        "reproduction_validation",
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "reproduced": {"type": ["boolean", "null"]},
                "confidence": {"type": ["number", "null"], "minimum": 0.0, "maximum": 1.0},
                "reason": {"type": "string"}
            },
            "required": ["reproduced", "confidence", "reason"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reproduction_response_accepts_plain_json() {
        let parsed = parse_reproduction_response(
            r#"{"reproduced":true,"confidence":0.82,"reason":"confirmed from repository evidence"}"#,
        )
        .unwrap();

        assert_eq!(parsed.reproduced, Some(true));
        assert_eq!(parsed.confidence, Some(0.82));
    }

    #[test]
    fn parse_reproduction_response_accepts_fenced_json() {
        let parsed = parse_reproduction_response(
            "```json\n{\"reproduced\":null,\"confidence\":0.4,\"reason\":\"inconclusive\"}\n```",
        )
        .unwrap();

        assert_eq!(parsed.reproduced, None);
        assert_eq!(parsed.reason, "inconclusive");
    }

    #[test]
    fn parse_reproduction_response_falls_back_to_plain_text() {
        let parsed = parse_reproduction_response(
            "Confirmed: the issue is present in the diff and supports the finding.",
        )
        .unwrap();

        assert_eq!(parsed.reproduced, Some(true));
        assert_eq!(
            parsed.reason,
            "Confirmed: the issue is present in the diff and supports the finding."
        );
    }

    #[test]
    fn reconstruct_new_file_contents_keeps_new_side_lines() {
        let diff = UnifiedDiff {
            file_path: PathBuf::from("src/example.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![crate::core::diff_parser::DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 3,
                context: "@@ -1,1 +1,3 @@".to_string(),
                changes: vec![
                    crate::core::diff_parser::DiffLine {
                        old_line_no: Some(1),
                        new_line_no: Some(1),
                        change_type: ChangeType::Context,
                        content: "fn demo() {".to_string(),
                    },
                    crate::core::diff_parser::DiffLine {
                        old_line_no: None,
                        new_line_no: Some(2),
                        change_type: ChangeType::Added,
                        content: "    println!(\"hi\");".to_string(),
                    },
                    crate::core::diff_parser::DiffLine {
                        old_line_no: Some(2),
                        new_line_no: Some(3),
                        change_type: ChangeType::Context,
                        content: "}".to_string(),
                    },
                ],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        };

        let rendered = reconstruct_new_file_contents(&diff);
        assert!(rendered.contains("fn demo() {"));
        assert!(rendered.contains("println!(\"hi\")"));
        assert!(rendered.contains("}"));
    }
}
