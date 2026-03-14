use crate::config;

pub(super) fn collect_guidance_sections(
    config: &config::Config,
    path_config: Option<&config::PathConfig>,
) -> Vec<String> {
    let mut sections = vec![strictness_section(config)];

    push_section(&mut sections, comment_types_section(config));
    push_section(&mut sections, review_profile_section(config));
    push_section(&mut sections, global_instructions_section(config));
    push_section(&mut sections, path_instructions_section(path_config));
    push_section(&mut sections, output_language_section(config));
    sections.push(fix_suggestion_section(config));

    sections
}

fn strictness_section(config: &config::Config) -> String {
    let strictness_guidance = match config.strictness {
        1 => "Prefer high-signal findings only. Avoid low-impact nitpicks and optional suggestions.",
        3 => {
            "Be exhaustive. Surface meaningful edge cases and maintainability concerns, including lower-severity findings."
        }
        _ => "Balance precision and coverage; prioritize clear, actionable findings.",
    };

    format!(
        "Strictness ({}): {}",
        config.strictness, strictness_guidance
    )
}

fn comment_types_section(config: &config::Config) -> Option<String> {
    if config.comment_types.is_empty() {
        None
    } else {
        Some(format!(
            "Enabled comment types: {}. Do not emit findings outside these types.",
            config.comment_types.join(", ")
        ))
    }
}

fn review_profile_section(config: &config::Config) -> Option<String> {
    let profile = config.review_profile.as_deref()?;
    let guidance = match profile {
        "chill" => Some(
            "Be conservative and only surface high-confidence, high-impact issues. Avoid nitpicks and redundant comments.",
        ),
        "assertive" => Some(
            "Be thorough and proactive. Surface edge cases, latent risks, and maintainability concerns even if they are subtle.",
        ),
        _ => None,
    }?;

    Some(format!("Review profile ({profile}): {guidance}"))
}

fn global_instructions_section(config: &config::Config) -> Option<String> {
    let instructions = config.review_instructions.as_deref()?.trim();
    if instructions.is_empty() {
        None
    } else {
        Some(format!("Global review instructions:\n{instructions}"))
    }
}

fn path_instructions_section(path_config: Option<&config::PathConfig>) -> Option<String> {
    let instructions = path_config?.review_instructions.as_deref()?.trim();
    if instructions.is_empty() {
        None
    } else {
        Some(format!("Path-specific instructions:\n{instructions}"))
    }
}

fn output_language_section(config: &config::Config) -> Option<String> {
    let lang = config.output_language.as_deref()?;
    if lang == "en" || lang.starts_with("en-") {
        None
    } else {
        Some(format!(
            "Write all review comments and suggestions in {lang}."
        ))
    }
}

fn fix_suggestion_section(config: &config::Config) -> String {
    if !config.include_fix_suggestions {
        "Do not include code fix suggestions. Only describe the issue. Do not include <<<ORIGINAL/>>>SUGGESTED blocks.".to_string()
    } else {
        "For every finding where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:\n\n<<<ORIGINAL\n<the problematic code>\n===\n<the fixed code>\n>>>SUGGESTED\n\nAlways copy the original code verbatim from the diff. Only omit the block when no concrete fix can be expressed in code.".to_string()
    }
}

fn push_section(sections: &mut Vec<String>, section: Option<String>) {
    if let Some(section) = section {
        sections.push(section);
    }
}
