use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

use crate::adapters::llm::{
    ChatMessage, ChatRequest, ChatRole, ContentBlock, LLMAdapter, StopReason, Usage,
};
use crate::core::agent_tools::ReviewTool;

/// Configuration for the agent loop.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    /// Maximum number of LLM round-trips (default 10).
    pub max_iterations: usize,
    /// Optional total token budget across all iterations.
    pub max_total_tokens: Option<usize>,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            max_total_tokens: None,
        }
    }
}

/// Events emitted during agent loop execution for observability.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgentEvent {
    TurnCompleted {
        iteration: usize,
        stop_reason: StopReason,
        tool_calls_count: usize,
    },
    ToolCalled {
        iteration: usize,
        tool_name: String,
        duration_ms: u64,
    },
    LoopFinished {
        total_iterations: usize,
        total_tokens: usize,
        reason: String,
    },
}

/// Result of running the agent loop.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentLoopResult {
    /// Accumulated text content from all LLM responses.
    pub content: String,
    /// Model that produced the final response.
    pub model: String,
    /// Total token usage across all iterations.
    pub total_usage: Usage,
    /// Number of LLM round-trips performed.
    pub iterations: usize,
}

/// Run an iterative agent loop: LLM call → tool execution → repeat until done.
///
/// The loop continues until:
/// - The LLM returns `StopReason::EndTurn` (done reviewing)
/// - `max_iterations` is reached
/// - `max_total_tokens` budget is exceeded
/// - The adapter doesn't support tools (single iteration, same as one-shot)
pub async fn run_agent_loop(
    adapter: &dyn LLMAdapter,
    initial_request: ChatRequest,
    tools: &[Box<dyn ReviewTool>],
    config: &AgentLoopConfig,
    on_event: Option<Arc<dyn Fn(AgentEvent) + Send + Sync>>,
) -> Result<AgentLoopResult> {
    // If adapter doesn't support tools, do a single iteration
    if !adapter.supports_tools() {
        let response = adapter.chat(initial_request).await?;
        let content = extract_text_content(&response.content);
        if let Some(ref cb) = on_event {
            cb(AgentEvent::LoopFinished {
                total_iterations: 1,
                total_tokens: response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
                reason: "no_tool_support".to_string(),
            });
        }
        return Ok(AgentLoopResult {
            content,
            model: response.model,
            total_usage: response.usage.unwrap_or(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
            iterations: 1,
        });
    }

    let mut messages = initial_request.messages.clone();
    let mut total_usage = Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };
    let mut accumulated_text = String::new();
    let mut model = String::new();

    for iteration in 0..config.max_iterations {
        let request = ChatRequest {
            system_prompt: initial_request.system_prompt.clone(),
            messages: messages.clone(),
            tools: initial_request.tools.clone(),
            temperature: initial_request.temperature,
            max_tokens: initial_request.max_tokens,
        };

        let response = adapter.chat(request).await?;

        // Accumulate usage
        if let Some(ref usage) = response.usage {
            total_usage.prompt_tokens += usage.prompt_tokens;
            total_usage.completion_tokens += usage.completion_tokens;
            total_usage.total_tokens += usage.total_tokens;
        }
        model = response.model.clone();

        // Extract text content from this turn
        let turn_text = extract_text_content(&response.content);
        if !turn_text.is_empty() {
            if !accumulated_text.is_empty() {
                accumulated_text.push('\n');
            }
            accumulated_text.push_str(&turn_text);
        }

        // Count tool calls in this response
        let tool_calls: Vec<_> = response
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();

        if let Some(ref cb) = on_event {
            cb(AgentEvent::TurnCompleted {
                iteration,
                stop_reason: response.stop_reason,
                tool_calls_count: tool_calls.len(),
            });
        }

        // If end_turn or max_tokens, we're done
        if response.stop_reason == StopReason::EndTurn
            || response.stop_reason == StopReason::MaxTokens
        {
            if let Some(ref cb) = on_event {
                cb(AgentEvent::LoopFinished {
                    total_iterations: iteration + 1,
                    total_tokens: total_usage.total_tokens,
                    reason: format!("{:?}", response.stop_reason),
                });
            }
            return Ok(AgentLoopResult {
                content: accumulated_text,
                model,
                total_usage,
                iterations: iteration + 1,
            });
        }

        // StopReason::ToolUse — execute tools and continue
        if tool_calls.is_empty() {
            // Model said tool_use but didn't include any tool calls — treat as done
            warn!("Model returned ToolUse stop_reason but no tool calls; ending loop");
            if let Some(ref cb) = on_event {
                cb(AgentEvent::LoopFinished {
                    total_iterations: iteration + 1,
                    total_tokens: total_usage.total_tokens,
                    reason: "empty_tool_calls".to_string(),
                });
            }
            return Ok(AgentLoopResult {
                content: accumulated_text,
                model,
                total_usage,
                iterations: iteration + 1,
            });
        }

        // Push the assistant's response (with tool_use blocks) into messages
        messages.push(ChatMessage {
            role: ChatRole::Assistant,
            content: response.content.clone(),
        });

        // Execute each tool sequentially
        let mut tool_results = Vec::new();
        for (call_id, call_name, call_input) in &tool_calls {
            let start = Instant::now();

            let result = match tools.iter().find(|t| t.name() == call_name) {
                Some(tool) => match tool.execute(call_input.clone()).await {
                    Ok(output) => ContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: output,
                        is_error: false,
                    },
                    Err(e) => ContentBlock::ToolResult {
                        tool_use_id: call_id.clone(),
                        content: format!("Error: {}", e),
                        is_error: true,
                    },
                },
                None => ContentBlock::ToolResult {
                    tool_use_id: call_id.clone(),
                    content: format!("Error: unknown tool '{}'", call_name),
                    is_error: true,
                },
            };

            let duration_ms = start.elapsed().as_millis() as u64;
            if let Some(ref cb) = on_event {
                cb(AgentEvent::ToolCalled {
                    iteration,
                    tool_name: call_name.clone(),
                    duration_ms,
                });
            }
            info!(
                tool = %call_name,
                duration_ms = duration_ms,
                iteration = iteration,
                "Agent tool executed"
            );

            tool_results.push(result);
        }

        // Push tool results as a user message
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: tool_results,
        });

        // Check token budget
        if let Some(budget) = config.max_total_tokens {
            if total_usage.total_tokens >= budget {
                warn!(
                    "Agent loop token budget exceeded ({} >= {})",
                    total_usage.total_tokens, budget
                );
                if let Some(ref cb) = on_event {
                    cb(AgentEvent::LoopFinished {
                        total_iterations: iteration + 1,
                        total_tokens: total_usage.total_tokens,
                        reason: "token_budget_exceeded".to_string(),
                    });
                }
                return Ok(AgentLoopResult {
                    content: accumulated_text,
                    model,
                    total_usage,
                    iterations: iteration + 1,
                });
            }
        }
    }

    // Max iterations reached
    if let Some(ref cb) = on_event {
        cb(AgentEvent::LoopFinished {
            total_iterations: config.max_iterations,
            total_tokens: total_usage.total_tokens,
            reason: "max_iterations".to_string(),
        });
    }
    warn!(
        "Agent loop reached max iterations ({})",
        config.max_iterations
    );

    Ok(AgentLoopResult {
        content: accumulated_text,
        model,
        total_usage,
        iterations: config.max_iterations,
    })
}

/// Extract concatenated text from content blocks.
fn extract_text_content(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::llm::{ChatResponse, LLMRequest, LLMResponse, ToolDefinition};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// A mock adapter that returns a scripted sequence of ChatResponses.
    struct MockChatAdapter {
        responses: Mutex<Vec<ChatResponse>>,
        supports_tools: bool,
    }

    impl MockChatAdapter {
        fn new(responses: Vec<ChatResponse>, supports_tools: bool) -> Self {
            // Reverse so we can pop from the front
            let mut responses = responses;
            responses.reverse();
            Self {
                responses: Mutex::new(responses),
                supports_tools,
            }
        }
    }

    #[async_trait]
    impl LLMAdapter for MockChatAdapter {
        async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
            Ok(LLMResponse {
                content: "fallback".to_string(),
                model: "mock".to_string(),
                usage: None,
            })
        }

        fn model_name(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse> {
            let mut responses = self.responses.lock().unwrap();
            responses
                .pop()
                .ok_or_else(|| anyhow::anyhow!("MockChatAdapter: no more scripted responses"))
        }

        fn supports_tools(&self) -> bool {
            self.supports_tools
        }
    }

    /// A simple mock tool.
    struct MockTool {
        name: String,
        response: String,
    }

    #[async_trait]
    impl ReviewTool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.clone(),
                description: "Mock tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(&self, _input: serde_json::Value) -> Result<String> {
            Ok(self.response.clone())
        }
    }

    fn make_initial_request() -> ChatRequest {
        ChatRequest {
            system_prompt: "You are a reviewer.".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: vec![ContentBlock::Text {
                    text: "Review this diff.".to_string(),
                }],
            }],
            tools: vec![],
            temperature: None,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn test_single_iteration_end_turn() {
        let adapter = MockChatAdapter::new(
            vec![ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "LGTM!".to_string(),
                }],
                model: "test-model".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 100,
                    completion_tokens: 10,
                    total_tokens: 110,
                }),
                stop_reason: StopReason::EndTurn,
            }],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.content, "LGTM!");
        assert_eq!(result.iterations, 1);
        assert_eq!(result.total_usage.total_tokens, 110);
    }

    #[tokio::test]
    async fn test_no_tool_support_single_iteration() {
        let adapter = MockChatAdapter::new(
            vec![ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "One-shot review.".to_string(),
                }],
                model: "test-model".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 50,
                    completion_tokens: 5,
                    total_tokens: 55,
                }),
                stop_reason: StopReason::EndTurn,
            }],
            false, // no tool support
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "contents".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.content, "One-shot review.");
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn test_tool_use_then_end_turn() {
        let adapter = MockChatAdapter::new(
            vec![
                // First response: tool call
                ChatResponse {
                    content: vec![
                        ContentBlock::Text {
                            text: "Let me check that file.".to_string(),
                        },
                        ContentBlock::ToolUse {
                            id: "call_1".to_string(),
                            name: "read_file".to_string(),
                            input: serde_json::json!({"file_path": "src/main.rs"}),
                        },
                    ],
                    model: "test-model".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 100,
                        completion_tokens: 20,
                        total_tokens: 120,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                // Second response: final review
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Found a bug on line 42.".to_string(),
                    }],
                    model: "test-model".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 200,
                        completion_tokens: 15,
                        total_tokens: 215,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "fn main() { panic!() }".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert!(result.content.contains("Let me check that file."));
        assert!(result.content.contains("Found a bug on line 42."));
        assert_eq!(result.iterations, 2);
        assert_eq!(result.total_usage.total_tokens, 335);
    }

    #[tokio::test]
    async fn test_max_iterations_guard() {
        // Adapter always returns tool calls — loop should terminate at max_iterations
        let mut responses = Vec::new();
        for i in 0..5 {
            responses.push(ChatResponse {
                content: vec![ContentBlock::ToolUse {
                    id: format!("call_{}", i),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"file_path": "test.rs"}),
                }],
                model: "test-model".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                stop_reason: StopReason::ToolUse,
            });
        }

        let adapter = MockChatAdapter::new(responses, true);
        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "file content".to_string(),
        })];
        let config = AgentLoopConfig {
            max_iterations: 3,
            max_total_tokens: None,
        };
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 3);
    }

    #[tokio::test]
    async fn test_token_budget_guard() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "read_file".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "test-model".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 500,
                        completion_tokens: 500,
                        total_tokens: 1000,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                // This response shouldn't be reached
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Should not see this.".to_string(),
                    }],
                    model: "test-model".to_string(),
                    usage: None,
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "content".to_string(),
        })];
        let config = AgentLoopConfig {
            max_iterations: 10,
            max_total_tokens: Some(500), // Budget is 500, first call uses 1000
        };
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 1);
        assert_eq!(result.total_usage.total_tokens, 1000);
    }

    #[tokio::test]
    async fn test_unknown_tool_returns_error_result() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        name: "nonexistent_tool".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "test-model".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Done.".to_string(),
                    }],
                    model: "test-model".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 20,
                        completion_tokens: 5,
                        total_tokens: 25,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        // Should still complete — unknown tool returns error message to the model
        assert_eq!(result.content, "Done.");
        assert_eq!(result.iterations, 2);
    }

    #[tokio::test]
    async fn test_events_are_emitted() {
        let adapter = MockChatAdapter::new(
            vec![ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "Done.".to_string(),
                }],
                model: "test-model".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                stop_reason: StopReason::EndTurn,
            }],
            true,
        );

        let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let callback: Arc<dyn Fn(AgentEvent) + Send + Sync> =
            Arc::new(move |event| events_clone.lock().unwrap().push(event));

        let tools: Vec<Box<dyn ReviewTool>> = vec![];
        let config = AgentLoopConfig::default();
        run_agent_loop(
            &adapter,
            make_initial_request(),
            &tools,
            &config,
            Some(callback),
        )
        .await
        .unwrap();

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2); // TurnCompleted + LoopFinished
    }

    #[tokio::test]
    async fn test_usage_accumulation_across_iterations() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "c1".to_string(),
                        name: "read_file".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 100,
                        completion_tokens: 50,
                        total_tokens: 150,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Done".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 200,
                        completion_tokens: 30,
                        total_tokens: 230,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.total_usage.prompt_tokens, 300);
        assert_eq!(result.total_usage.completion_tokens, 80);
        assert_eq!(result.total_usage.total_tokens, 380);
    }
}
