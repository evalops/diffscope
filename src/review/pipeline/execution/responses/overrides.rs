use crate::config;
use crate::core;

pub(super) fn apply_specialized_pass_tags(
    comments: &mut [core::Comment],
    pass_kind: Option<core::SpecializedPassKind>,
) {
    let Some(pass_kind) = pass_kind else {
        return;
    };

    let tag = pass_kind.tag().to_string();
    for comment in comments {
        if !comment.tags.contains(&tag) {
            comment.tags.push(tag.clone());
        }
    }
}

pub(super) fn apply_path_severity_overrides(
    comments: &mut [core::Comment],
    path_config: Option<&config::PathConfig>,
) {
    let Some(path_config) = path_config else {
        return;
    };

    for comment in comments {
        for (category, severity) in &path_config.severity_overrides {
            if comment.category.as_str() == category.to_lowercase() {
                if let Some(severity) = parse_path_severity_override(severity) {
                    comment.severity = severity;
                }
            }
        }
    }
}

fn parse_path_severity_override(value: &str) -> Option<core::comment::Severity> {
    match value.to_lowercase().as_str() {
        "error" => Some(core::comment::Severity::Error),
        "warning" => Some(core::comment::Severity::Warning),
        "info" => Some(core::comment::Severity::Info),
        "suggestion" => Some(core::comment::Severity::Suggestion),
        _ => None,
    }
}
