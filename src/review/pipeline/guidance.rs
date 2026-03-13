use crate::config;

pub fn build_review_guidance(
    config: &config::Config,
    path_config: Option<&config::PathConfig>,
) -> Option<String> {
    let mut sections = Vec::new();

    let strictness_guidance = match config.strictness {
        1 => "Prefer high-signal findings only. Avoid low-impact nitpicks and optional suggestions.",
        3 => {
            "Be exhaustive. Surface meaningful edge cases and maintainability concerns, including lower-severity findings."
        }
        _ => "Balance precision and coverage; prioritize clear, actionable findings.",
    };
    sections.push(format!(
        "Strictness ({}): {}",
        config.strictness, strictness_guidance
    ));
    if !config.comment_types.is_empty() {
        sections.push(format!(
            "Enabled comment types: {}. Do not emit findings outside these types.",
            config.comment_types.join(", ")
        ));
    }

    if let Some(profile) = config.review_profile.as_deref() {
        let guidance = match profile {
            "chill" => Some(
                "Be conservative and only surface high-confidence, high-impact issues. Avoid nitpicks and redundant comments.",
            ),
            "assertive" => Some(
                "Be thorough and proactive. Surface edge cases, latent risks, and maintainability concerns even if they are subtle.",
            ),
            _ => None,
        };
        if let Some(text) = guidance {
            sections.push(format!("Review profile ({}): {}", profile, text));
        }
    }

    if let Some(instructions) = config.review_instructions.as_deref() {
        let trimmed = instructions.trim();
        if !trimmed.is_empty() {
            sections.push(format!("Global review instructions:\n{}", trimmed));
        }
    }

    if let Some(pc) = path_config {
        if let Some(instructions) = pc.review_instructions.as_deref() {
            let trimmed = instructions.trim();
            if !trimmed.is_empty() {
                sections.push(format!("Path-specific instructions:\n{}", trimmed));
            }
        }
    }

    if let Some(ref lang) = config.output_language {
        if lang != "en" && !lang.starts_with("en-") {
            sections.push(format!(
                "Write all review comments and suggestions in {}.",
                lang
            ));
        }
    }

    if !config.include_fix_suggestions {
        sections.push("Do not include code fix suggestions. Only describe the issue. Do not include <<<ORIGINAL/>>>SUGGESTED blocks.".to_string());
    } else {
        sections.push(
            "For every finding where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:\n\n<<<ORIGINAL\n<the problematic code>\n===\n<the fixed code>\n>>>SUGGESTED\n\nAlways copy the original code verbatim from the diff. Only omit the block when no concrete fix can be expressed in code.".to_string(),
        );
    }

    if sections.is_empty() {
        None
    } else {
        Some(format!(
            "Additional review guidance:\n{}",
            sections.join("\n\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_review_guidance_includes_strictness() {
        let config = config::Config::default();
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Strictness"));
    }

    #[test]
    fn build_review_guidance_includes_comment_types() {
        let config = config::Config {
            comment_types: vec!["logic".to_string(), "security".to_string()],
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Enabled comment types: logic, security"));
    }

    #[test]
    fn build_review_guidance_includes_profile() {
        let config = config::Config {
            review_profile: Some("assertive".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Review profile (assertive)"));
    }

    #[test]
    fn build_review_guidance_includes_path_instructions() {
        let path_config = config::PathConfig {
            review_instructions: Some("Focus on transaction safety".to_string()),
            ..config::PathConfig::default()
        };
        let guidance =
            build_review_guidance(&config::Config::default(), Some(&path_config)).unwrap();
        assert!(guidance.contains("Path-specific instructions"));
    }

    #[test]
    fn build_review_guidance_includes_output_language() {
        let config = config::Config {
            output_language: Some("de".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Write all review comments and suggestions in de."));
    }

    #[test]
    fn build_review_guidance_skips_en_language() {
        let config = config::Config {
            output_language: Some("en".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(!guidance.contains("Write all review comments"));
    }

    #[test]
    fn build_review_guidance_skips_en_us_language() {
        let config = config::Config {
            output_language: Some("en-us".to_string()),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(!guidance.contains("Write all review comments"));
    }

    #[test]
    fn build_review_guidance_no_fix_suggestions() {
        let config = config::Config {
            include_fix_suggestions: false,
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(guidance.contains("Do not include code fix suggestions"));
    }
}
