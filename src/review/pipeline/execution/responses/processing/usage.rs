use crate::adapters::llm::LLMResponse;

use super::ResponseUsage;

pub(super) fn response_usage(response: &LLMResponse) -> ResponseUsage {
    response
        .usage
        .as_ref()
        .map(|usage| ResponseUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        })
        .unwrap_or_default()
}
