use std::collections::{HashMap, HashSet};

use super::{Category, Comment, ReviewSummary, Severity};

pub(super) fn generate_summary(comments: &[Comment]) -> ReviewSummary {
    let mut by_severity = HashMap::new();
    let mut by_category = HashMap::new();
    let mut files = HashSet::new();
    let mut critical_issues = 0;

    for comment in comments {
        let severity_str = comment.severity.to_string();
        *by_severity.entry(severity_str).or_insert(0) += 1;

        let category_str = comment.category.to_string();
        *by_category.entry(category_str).or_insert(0) += 1;

        files.insert(comment.file_path.clone());

        if matches!(comment.severity, Severity::Error) {
            critical_issues += 1;
        }
    }

    ReviewSummary {
        total_comments: comments.len(),
        by_severity,
        by_category,
        critical_issues,
        files_reviewed: files.len(),
        overall_score: calculate_overall_score(comments),
        recommendations: generate_recommendations(comments),
    }
}

fn calculate_overall_score(comments: &[Comment]) -> f32 {
    if comments.is_empty() {
        return 10.0;
    }

    let mut score: f32 = 10.0;
    for comment in comments {
        let penalty = match comment.severity {
            Severity::Error => 2.0,
            Severity::Warning => 1.0,
            Severity::Info => 0.3,
            Severity::Suggestion => 0.1,
        };
        score -= penalty;
    }

    score.clamp(0.0, 10.0)
}

fn generate_recommendations(comments: &[Comment]) -> Vec<String> {
    let mut recommendations = Vec::new();
    let mut security_count = 0;
    let mut performance_count = 0;
    let mut style_count = 0;

    for comment in comments {
        match comment.category {
            Category::Security => security_count += 1,
            Category::Performance => performance_count += 1,
            Category::Style => style_count += 1,
            _ => {}
        }
    }

    if security_count > 0 {
        recommendations.push(format!(
            "Address {security_count} security issue(s) immediately"
        ));
    }
    if performance_count > 2 {
        recommendations.push(
            "Consider a performance audit - multiple optimization opportunities found".to_string(),
        );
    }
    if style_count > 5 {
        recommendations
            .push("Consider setting up automated linting to catch style issues".to_string());
    }

    recommendations
}
