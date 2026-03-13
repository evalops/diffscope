use super::signals::{
    has_architecture_signal, has_bug_signal, has_documentation_signal, has_maintainability_signal,
    has_performance_signal, has_security_signal, has_style_signal, has_testing_signal,
};
use super::{Category, FixEffort, Severity};

/// `lower` must already be lowercased.
pub(super) fn determine_severity(lower: &str) -> Severity {
    if lower.contains("error") || lower.contains("critical") {
        Severity::Error
    } else if lower.contains("warning") || lower.contains("issue") {
        Severity::Warning
    } else if lower.contains("consider") || lower.contains("suggestion") {
        Severity::Suggestion
    } else {
        Severity::Info
    }
}

/// `lower` must already be lowercased.
pub(super) fn determine_category(lower: &str) -> Category {
    if has_security_signal(lower) || lower.contains("cwe-") {
        Category::Security
    } else if has_performance_signal(lower) {
        Category::Performance
    } else if has_bug_signal(lower) {
        Category::Bug
    } else if has_style_signal(lower) {
        Category::Style
    } else if has_documentation_signal(lower) {
        Category::Documentation
    } else if has_testing_signal(lower) {
        Category::Testing
    } else if has_maintainability_signal(lower) {
        Category::Maintainability
    } else if has_architecture_signal(lower) {
        Category::Architecture
    } else {
        Category::BestPractice
    }
}

/// `lower` must already be lowercased.
pub(super) fn determine_fix_effort(lower: &str, category: &Category) -> FixEffort {
    if lower.contains("architecture") || lower.contains("refactor") || lower.contains("redesign") {
        return FixEffort::High;
    }

    if matches!(category, Category::Security)
        && (lower.contains("injection") || lower.contains("vulnerability"))
    {
        return FixEffort::Medium;
    }

    if matches!(category, Category::Performance) && lower.contains("n+1") {
        return FixEffort::Medium;
    }

    if matches!(category, Category::Style | Category::Documentation) {
        return FixEffort::Low;
    }

    FixEffort::Medium
}
