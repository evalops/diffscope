use anyhow::Result;

use crate::adapters;
use crate::config;

pub(super) async fn complete_suggestion(
    config: &config::Config,
    system_prompt: String,
    user_prompt: String,
    max_tokens: usize,
) -> Result<String> {
    let model_config = config.to_model_config_for_role(config::ModelRole::Fast);
    let adapter = adapters::llm::create_adapter(&model_config)?;
    let request = adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: Some(0.3),
        max_tokens: Some(max_tokens),
        response_schema: None,
    };

    let response = adapter.complete(request).await?;
    Ok(response.content)
}
