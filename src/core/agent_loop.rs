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

/// Log entry for a single tool call during the agent loop.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentToolCallLog {
    pub iteration: usize,
    pub tool_name: String,
    pub duration_ms: u64,
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
    /// Log of all tool calls made during the loop.
    pub tool_calls: Vec<AgentToolCallLog>,
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
            tool_calls: Vec::new(),
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
    let mut tool_call_log: Vec<AgentToolCallLog> = Vec::new();

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
                tool_calls: tool_call_log,
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
                tool_calls: tool_call_log,
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
            tool_call_log.push(AgentToolCallLog {
                iteration,
                tool_name: call_name.clone(),
                duration_ms,
            });
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
                    tool_calls: tool_call_log,
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
        tool_calls: tool_call_log,
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
        received_requests: Mutex<Vec<ChatRequest>>,
        supports_tools: bool,
    }

    impl MockChatAdapter {
        fn new(responses: Vec<ChatResponse>, supports_tools: bool) -> Self {
            // Reverse so we can pop from the front
            let mut responses = responses;
            responses.reverse();
            Self {
                responses: Mutex::new(responses),
                received_requests: Mutex::new(Vec::new()),
                supports_tools,
            }
        }

        /// Return all ChatRequests that were sent to this adapter.
        fn received_requests(&self) -> Vec<ChatRequest> {
            self.received_requests.lock().unwrap().clone()
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

        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
            self.received_requests.lock().unwrap().push(request);
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

    /// Mutation: || → && in `StopReason::EndTurn || StopReason::MaxTokens`
    /// MaxTokens should also terminate the loop (not continue).
    #[tokio::test]
    async fn test_max_tokens_terminates_loop() {
        let adapter = MockChatAdapter::new(
            vec![ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "Partial output...".to_string(),
                }],
                model: "test-model".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                }),
                stop_reason: StopReason::MaxTokens,
            }],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "content".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 1);
        assert_eq!(result.content, "Partial output...");
    }

    /// Mutation: == → != in tool name matching.
    /// When the correct tool exists, it should be called (not the error path).
    #[tokio::test]
    async fn test_correct_tool_is_called_by_name() {
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
                    model: "m".to_string(),
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

        // Two tools with distinctive responses to verify the correct one is called
        let tools: Vec<Box<dyn ReviewTool>> = vec![
            Box::new(MockTool {
                name: "read_file".to_string(),
                response: "CORRECT_TOOL_RESPONSE".to_string(),
            }),
            Box::new(MockTool {
                name: "other_tool".to_string(),
                response: "WRONG_TOOL".to_string(),
            }),
        ];
        let config = AgentLoopConfig::default();

        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 2);

        // Verify the tool result fed back to the adapter contains the correct tool's output.
        // The second request (iteration 2) should have messages including the tool result.
        let requests = adapter.received_requests();
        assert_eq!(requests.len(), 2, "should have sent 2 requests to adapter");

        // The second request's messages should contain the tool result from read_file
        let second_req = &requests[1];
        let tool_result_content = second_req
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .find_map(|block| match block {
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    assert!(!is_error, "tool result should not be an error");
                    Some(content.as_str())
                }
                _ => None,
            })
            .expect("should have a ToolResult in the second request's messages");

        assert_eq!(
            tool_result_content, "CORRECT_TOOL_RESPONSE",
            "should have called read_file (not other_tool)"
        );
    }

    /// Mutation: + → * or - in token accumulation.
    /// Verify exact arithmetic on prompt_tokens and completion_tokens.
    #[tokio::test]
    async fn test_token_accumulation_arithmetic() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "c1".to_string(),
                        name: "t".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 7,
                        completion_tokens: 3,
                        total_tokens: 10,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "c2".to_string(),
                        name: "t".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 11,
                        completion_tokens: 5,
                        total_tokens: 16,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Done".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 13,
                        completion_tokens: 2,
                        total_tokens: 15,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "t".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        // 7 + 11 + 13 = 31 (not 7 * 11 * 13 = 1001, or 7 - 11 - 13 = -17)
        assert_eq!(result.total_usage.prompt_tokens, 31);
        // 3 + 5 + 2 = 10 (not 3 * 5 * 2 = 30, or 3 - 5 - 2 = -4)
        assert_eq!(result.total_usage.completion_tokens, 10);
        // 10 + 16 + 15 = 41 (not 10 * 16 * 15 = 2400)
        assert_eq!(result.total_usage.total_tokens, 41);
        assert_eq!(result.iterations, 3);
    }

    /// Mutation: text join `\n` separator.
    /// Multi-turn text should be joined with newlines, not concatenated.
    #[tokio::test]
    async fn test_text_accumulation_across_turns() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![
                        ContentBlock::Text {
                            text: "First observation.".to_string(),
                        },
                        ContentBlock::ToolUse {
                            id: "c1".to_string(),
                            name: "t".to_string(),
                            input: serde_json::json!({}),
                        },
                    ],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Second observation.".to_string(),
                    }],
                    model: "m".to_string(),
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

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "t".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        // Both observations should be present, separated by newline
        assert_eq!(result.content, "First observation.\nSecond observation.");
    }

    /// Mutation: default `supports_tools() -> true`.
    /// Verify the default trait impl returns false (adapters opt-in).
    #[tokio::test]
    async fn test_default_supports_tools_is_false() {
        // A basic adapter using the default trait impl
        struct BasicAdapter;
        #[async_trait]
        impl LLMAdapter for BasicAdapter {
            async fn complete(&self, _request: LLMRequest) -> Result<LLMResponse> {
                Ok(LLMResponse {
                    content: "basic response".to_string(),
                    model: "basic".to_string(),
                    usage: None,
                })
            }
            fn model_name(&self) -> &str {
                "basic"
            }
        }

        let adapter = BasicAdapter;
        assert!(
            !adapter.supports_tools(),
            "Default supports_tools() should return false"
        );

        // When used in the agent loop, it should fallback to single iteration
        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "content".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 1);
        assert_eq!(result.content, "basic response");
    }

    /// MaxTokens stop_reason with tool_use content should exit without executing tools.
    /// Catches mutation: `|| → &&` at the EndTurn/MaxTokens check.
    /// If the mutation turns `||` into `&&`, the early exit is skipped and tools get executed.
    #[tokio::test]
    async fn test_max_tokens_stop_reason_exits_without_running_tools() {
        let adapter = MockChatAdapter::new(
            vec![
                // Response has MaxTokens but also contains a ToolUse block.
                // Correct behavior: exit immediately because MaxTokens, don't execute the tool.
                ChatResponse {
                    content: vec![
                        ContentBlock::Text {
                            text: "Partial review".to_string(),
                        },
                        ContentBlock::ToolUse {
                            id: "c1".to_string(),
                            name: "read_file".to_string(),
                            input: serde_json::json!({}),
                        },
                    ],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 100,
                        completion_tokens: 50,
                        total_tokens: 150,
                    }),
                    stop_reason: StopReason::MaxTokens,
                },
                // If the early exit is missed, the tool executes, results are pushed,
                // and this second response would be consumed.
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "SHOULD NOT REACH THIS".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "read_file".to_string(),
            response: "file contents".to_string(),
        })];
        let config = AgentLoopConfig::default();
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        assert_eq!(result.iterations, 1, "should exit after 1 iteration");
        assert!(
            !result.content.contains("SHOULD NOT REACH THIS"),
            "should not have continued to a second iteration"
        );
        // Only 1 request should have been sent (no second iteration)
        assert_eq!(adapter.received_requests().len(), 1);
    }

    /// Empty tool_calls with ToolUse stop_reason should exit and report correct iteration count.
    /// Catches mutations: `+ → *` and `+ → -` at the empty_tool_calls exit path.
    #[tokio::test]
    async fn test_empty_tool_calls_iterations_count() {
        // First response: a tool call, second: ToolUse stop_reason but no actual tool calls
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
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                // Second response: ToolUse stop reason but empty content (no tool calls)
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Hmm.".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 20,
                        completion_tokens: 5,
                        total_tokens: 25,
                    }),
                    stop_reason: StopReason::ToolUse,
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

        // 2 iterations: first had a real tool call, second had empty tool_calls and exited
        assert_eq!(result.iterations, 2);
        assert_eq!(result.total_usage.total_tokens, 40); // 15 + 25
    }

    /// LoopFinished event should report correct total_iterations and reason.
    /// Catches mutations: `+ → *` and `+ → -` on event `total_iterations` fields.
    #[tokio::test]
    async fn test_loop_finished_event_fields() {
        // 2-iteration scenario: tool call → end turn
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "c1".to_string(),
                        name: "t".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "Done".to_string(),
                    }],
                    model: "m".to_string(),
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

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "t".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig::default();

        let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let callback: Arc<dyn Fn(AgentEvent) + Send + Sync> =
            Arc::new(move |event| events_clone.lock().unwrap().push(event));

        let _result = run_agent_loop(
            &adapter,
            make_initial_request(),
            &tools,
            &config,
            Some(callback),
        )
        .await
        .unwrap();

        let events = events.lock().unwrap();
        let finished = events.iter().find_map(|e| match e {
            AgentEvent::LoopFinished {
                total_iterations,
                total_tokens,
                reason,
            } => Some((*total_iterations, *total_tokens, reason.clone())),
            _ => None,
        });

        let (iters, tokens, reason) = finished.expect("should have a LoopFinished event");
        assert_eq!(iters, 2, "total_iterations should be 2 (iteration 1 + 1)");
        assert_eq!(tokens, 40, "total_tokens should be 15 + 25 = 40");
        assert!(
            reason.contains("EndTurn"),
            "reason should contain EndTurn, got: {}",
            reason
        );
    }

    /// LoopFinished event in empty_tool_calls path should have correct total_iterations.
    /// Catches mutations: `+ → *` and `+ → -` on empty_tool_calls path event fields.
    #[tokio::test]
    async fn test_empty_tool_calls_event_iterations() {
        // Single response with ToolUse stop reason but no tool calls in content
        let adapter = MockChatAdapter::new(
            vec![ChatResponse {
                content: vec![ContentBlock::Text {
                    text: "Hmm.".to_string(),
                }],
                model: "m".to_string(),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                stop_reason: StopReason::ToolUse,
            }],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "t".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig::default();

        let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let callback: Arc<dyn Fn(AgentEvent) + Send + Sync> =
            Arc::new(move |event| events_clone.lock().unwrap().push(event));

        let _result = run_agent_loop(
            &adapter,
            make_initial_request(),
            &tools,
            &config,
            Some(callback),
        )
        .await
        .unwrap();

        let events = events.lock().unwrap();
        let finished = events.iter().find_map(|e| match e {
            AgentEvent::LoopFinished {
                total_iterations,
                reason,
                ..
            } => Some((*total_iterations, reason.clone())),
            _ => None,
        });

        let (iters, reason) = finished.expect("should have a LoopFinished event");
        assert_eq!(iters, 1, "total_iterations should be 1 (iteration 0 + 1)");
        assert_eq!(reason, "empty_tool_calls");
    }

    /// LoopFinished event in token_budget path should have correct total_iterations.
    /// Catches mutations: `+ → *` and `+ → -` on token_budget_exceeded path event fields.
    #[tokio::test]
    async fn test_token_budget_event_iterations() {
        let adapter = MockChatAdapter::new(
            vec![
                ChatResponse {
                    content: vec![ContentBlock::ToolUse {
                        id: "c1".to_string(),
                        name: "t".to_string(),
                        input: serde_json::json!({}),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 50,
                        completion_tokens: 50,
                        total_tokens: 100,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "unreachable".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    }),
                    stop_reason: StopReason::EndTurn,
                },
            ],
            true,
        );

        let tools: Vec<Box<dyn ReviewTool>> = vec![Box::new(MockTool {
            name: "t".to_string(),
            response: "ok".to_string(),
        })];
        let config = AgentLoopConfig {
            max_iterations: 10,
            max_total_tokens: Some(50),
        };

        let events: Arc<Mutex<Vec<AgentEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let callback: Arc<dyn Fn(AgentEvent) + Send + Sync> =
            Arc::new(move |event| events_clone.lock().unwrap().push(event));

        let _result = run_agent_loop(
            &adapter,
            make_initial_request(),
            &tools,
            &config,
            Some(callback),
        )
        .await
        .unwrap();

        let events = events.lock().unwrap();
        let finished = events.iter().find_map(|e| match e {
            AgentEvent::LoopFinished {
                total_iterations,
                reason,
                ..
            } => Some((*total_iterations, reason.clone())),
            _ => None,
        });

        let (iters, reason) = finished.expect("should have a LoopFinished event");
        assert_eq!(iters, 1, "total_iterations should be 1 (iteration 0 + 1)");
        assert_eq!(reason, "token_budget_exceeded");
    }

    /// Token budget exit path should report correct iteration count.
    /// Catches mutations: `+ → *` and `+ → -` at the token_budget exit.
    #[tokio::test]
    async fn test_token_budget_iterations_count() {
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
                        prompt_tokens: 50,
                        completion_tokens: 50,
                        total_tokens: 100,
                    }),
                    stop_reason: StopReason::ToolUse,
                },
                // This response won't be reached — budget exceeded after first
                ChatResponse {
                    content: vec![ContentBlock::Text {
                        text: "unreachable".to_string(),
                    }],
                    model: "m".to_string(),
                    usage: Some(Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
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
        let config = AgentLoopConfig {
            max_iterations: 10,
            max_total_tokens: Some(50), // Budget of 50 — exceeded by first response (100)
        };
        let result = run_agent_loop(&adapter, make_initial_request(), &tools, &config, None)
            .await
            .unwrap();

        // Should exit after 1 iteration due to token budget
        assert_eq!(result.iterations, 1);
        assert_eq!(result.total_usage.total_tokens, 100);
    }
}
