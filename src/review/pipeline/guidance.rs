#[path = "guidance/format.rs"]
mod format;
#[path = "guidance/sections.rs"]
mod sections;

use crate::config;

use format::format_guidance_sections;
use sections::collect_guidance_sections;

pub fn build_review_guidance(
    config: &config::Config,
    path_config: Option<&config::PathConfig>,
) -> Option<String> {
    format_guidance_sections(collect_guidance_sections(config, path_config))
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

    #[test]
    fn build_review_guidance_includes_prose_rules() {
        // #12: natural language custom rules — injected as bullets into guidance
        let config = config::Config {
            review_rules_prose: Some(vec![
                "Always use parameterized queries.".to_string(),
                "No direct SQL string concatenation.".to_string(),
            ]),
            ..config::Config::default()
        };
        let guidance = build_review_guidance(&config, None).unwrap();
        assert!(
            guidance.contains("Custom rules (natural language)"),
            "guidance should include prose rules section"
        );
        assert!(guidance.contains("Always use parameterized queries"));
        assert!(guidance.contains("No direct SQL string concatenation"));
    }
}
