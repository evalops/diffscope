use crate::core;
use crate::parsing::parse_smart_category;

pub fn parse_rule_severity_override(value: &str) -> Option<core::comment::Severity> {
    match value.trim().to_ascii_lowercase().as_str() {
        "critical" | "error" => Some(core::comment::Severity::Error),
        "high" | "warning" | "warn" => Some(core::comment::Severity::Warning),
        "medium" | "info" | "informational" => Some(core::comment::Severity::Info),
        "low" | "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}

pub fn parse_rule_category_override(value: &str) -> Option<core::comment::Category> {
    parse_smart_category(value)
}
