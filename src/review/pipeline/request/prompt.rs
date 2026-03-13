use anyhow::Result;

use crate::adapters;
use crate::config;
use crate::core;
use crate::core::offline::optimize_prompt_for_local;

use super::super::guidance::build_review_guidance;
use super::super::services::PipelineServices;
use super::super::session::ReviewSession;

pub(in super::super) fn build_review_request(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &[core::LLMContextChunk],
    path_config: Option<&config::PathConfig>,
    pass_kind: Option<core::SpecializedPassKind>,
) -> Result<adapters::llm::LLMRequest> {
    let local_prompt_builder = core::PromptBuilder::new(build_prompt_config(
        services,
        session,
        path_config,
        pass_kind,
    ));
    let (system_prompt, user_prompt) = local_prompt_builder.build_prompt(diff, context_chunks)?;

    let (system_prompt, user_prompt) = if services.is_local {
        let context_window = services.config.context_window.unwrap_or(8192);
        optimize_prompt_for_local(&system_prompt, &user_prompt, context_window)
    } else {
        (system_prompt, user_prompt)
    };

    Ok(adapters::llm::LLMRequest {
        system_prompt,
        user_prompt,
        temperature: None,
        max_tokens: None,
        response_schema: Some(super::schema::review_comments_response_schema()),
    })
}

fn build_prompt_config(
    services: &PipelineServices,
    session: &ReviewSession,
    path_config: Option<&config::PathConfig>,
    pass_kind: Option<core::SpecializedPassKind>,
) -> core::prompt::PromptConfig {
    let mut local_prompt_config = services.base_prompt_config.clone();

    if let Some(pass_kind) = pass_kind {
        local_prompt_config.system_prompt = pass_kind.system_prompt();

        if !session.enhanced_guidance.is_empty() {
            local_prompt_config.system_prompt.push_str("\n\n");
            local_prompt_config
                .system_prompt
                .push_str(&session.enhanced_guidance);
        }
        if let Some(instructions) = session.auto_instructions.as_ref() {
            local_prompt_config
                .system_prompt
                .push_str("\n\n# Project-specific instructions (auto-detected):\n");
            local_prompt_config.system_prompt.push_str(instructions);
        }

        return local_prompt_config;
    }

    if let Some(custom_prompt) = &services.config.system_prompt {
        local_prompt_config.system_prompt = custom_prompt.clone();
    }
    if let Some(path_config) = path_config {
        if let Some(ref prompt) = path_config.system_prompt {
            local_prompt_config.system_prompt = prompt.clone();
        }
    }
    if let Some(guidance) = build_review_guidance(&services.config, path_config) {
        local_prompt_config.system_prompt.push_str("\n\n");
        local_prompt_config.system_prompt.push_str(&guidance);
    }
    if !session.enhanced_guidance.is_empty() {
        local_prompt_config.system_prompt.push_str("\n\n");
        local_prompt_config
            .system_prompt
            .push_str(&session.enhanced_guidance);
    }
    if !services.feedback_context.is_empty() {
        local_prompt_config.system_prompt.push_str("\n\n");
        local_prompt_config
            .system_prompt
            .push_str(&services.feedback_context);
    }
    if let Some(instructions) = session.auto_instructions.as_ref() {
        local_prompt_config
            .system_prompt
            .push_str("\n\n# Project-specific instructions (auto-detected):\n");
        local_prompt_config.system_prompt.push_str(instructions);
    }

    local_prompt_config
}
