use std::collections::{HashMap, HashSet};

use super::{
    Category, Comment, CommentStatus, MergeReadiness, ReviewCompletenessSummary, ReviewSummary,
    ReviewVerificationState, ReviewVerificationSummary, Severity,
};

pub(super) fn generate_summary(comments: &[Comment]) -> ReviewSummary {
    let mut by_severity = HashMap::new();
    let mut by_category = HashMap::new();
    let mut open_by_severity = HashMap::new();
    let mut files = HashSet::new();
    let mut critical_issues = 0;
    let mut open_comments = 0;
    let mut open_blocking_comments = 0;
    let mut open_informational_comments = 0;
    let mut resolved_comments = 0;
    let mut dismissed_comments = 0;
    let mut open_blockers = 0;

    for comment in comments {
        let severity_str = comment.severity.to_string();
        *by_severity.entry(severity_str).or_insert(0) += 1;

        let category_str = comment.category.to_string();
        *by_category.entry(category_str).or_insert(0) += 1;

        files.insert(comment.file_path.clone());

        if matches!(comment.severity, Severity::Error) {
            critical_issues += 1;
        }

        match comment.status {
            CommentStatus::Open => {
                open_comments += 1;
                *open_by_severity
                    .entry(comment.severity.to_string())
                    .or_insert(0) += 1;
                if comment.severity.is_blocking() {
                    open_blocking_comments += 1;
                    open_blockers += 1;
                }
                if comment.severity.is_informational() {
                    open_informational_comments += 1;
                }
            }
            CommentStatus::Resolved => resolved_comments += 1,
            CommentStatus::Dismissed => dismissed_comments += 1,
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
        open_comments,
        open_by_severity,
        open_blocking_comments,
        open_informational_comments,
        resolved_comments,
        dismissed_comments,
        open_blockers,
        completeness: build_completeness_summary(
            comments.len(),
            resolved_comments,
            dismissed_comments,
            0,
        ),
        merge_readiness: default_merge_readiness(open_blockers),
        verification: ReviewVerificationSummary::default(),
        readiness_reasons: Vec::new(),
        loop_telemetry: None,
    }
}

pub(super) fn inherit_review_state(
    mut summary: ReviewSummary,
    previous: Option<&ReviewSummary>,
) -> ReviewSummary {
    if let Some(previous) = previous {
        summary.verification = previous.verification.clone();
        summary.loop_telemetry = previous.loop_telemetry.clone();
    }
    apply_review_runtime_state(summary, false)
}

pub(super) fn apply_verification(
    mut summary: ReviewSummary,
    verification: ReviewVerificationSummary,
) -> ReviewSummary {
    summary.verification = verification;
    apply_review_runtime_state(summary, false)
}

pub(super) fn apply_review_runtime_state(
    mut summary: ReviewSummary,
    stale_review: bool,
) -> ReviewSummary {
    summary.completeness = build_completeness_summary(
        summary.total_comments,
        summary.resolved_comments,
        summary.dismissed_comments,
        if stale_review {
            summary.open_comments
        } else {
            0
        },
    );

    let mut reasons = Vec::new();
    if matches!(
        summary.verification.state,
        ReviewVerificationState::Inconclusive
    ) {
        reasons.push("verification was inconclusive or fail-open; rerun this review".to_string());
    }
    if stale_review {
        reasons.push("new commits landed after this review".to_string());
    }
    summary.readiness_reasons = reasons;
    summary.merge_readiness = if !summary.readiness_reasons.is_empty() {
        MergeReadiness::NeedsReReview
    } else {
        default_merge_readiness(summary.open_blockers)
    };
    summary
}

fn build_completeness_summary(
    total_findings: usize,
    resolved_comments: usize,
    dismissed_comments: usize,
    stale_findings: usize,
) -> ReviewCompletenessSummary {
    ReviewCompletenessSummary {
        total_findings,
        acknowledged_findings: resolved_comments + dismissed_comments,
        fixed_findings: resolved_comments,
        stale_findings,
    }
}

fn default_merge_readiness(open_blockers: usize) -> MergeReadiness {
    if open_blockers == 0 {
        MergeReadiness::Ready
    } else {
        MergeReadiness::NeedsAttention
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
        if comment.status != CommentStatus::Open {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::core::comment::{Category, FixEffort, FixLoopTelemetry};

    fn make_comment(
        id: &str,
        severity: Severity,
        category: Category,
        status: CommentStatus,
    ) -> Comment {
        Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
            status,
            resolved_at: None,
        }
    }

    #[test]
    fn summary_tracks_lifecycle_and_merge_readiness() {
        let comments = vec![
            make_comment(
                "open-error",
                Severity::Error,
                Category::Security,
                CommentStatus::Open,
            ),
            make_comment(
                "resolved-warning",
                Severity::Warning,
                Category::Bug,
                CommentStatus::Resolved,
            ),
            make_comment(
                "dismissed-info",
                Severity::Info,
                Category::Style,
                CommentStatus::Dismissed,
            ),
        ];

        let summary = generate_summary(&comments);
        assert_eq!(summary.total_comments, 3);
        assert_eq!(summary.open_comments, 1);
        assert_eq!(summary.open_blocking_comments, 1);
        assert_eq!(summary.open_informational_comments, 0);
        assert_eq!(summary.resolved_comments, 1);
        assert_eq!(summary.dismissed_comments, 1);
        assert_eq!(summary.open_blockers, 1);
        assert_eq!(summary.completeness.total_findings, 3);
        assert_eq!(summary.completeness.acknowledged_findings, 2);
        assert_eq!(summary.completeness.fixed_findings, 1);
        assert_eq!(summary.completeness.stale_findings, 0);
        assert_eq!(summary.open_by_severity.get("Error"), Some(&1));
        assert_eq!(summary.merge_readiness, MergeReadiness::NeedsAttention);
        assert_eq!(
            summary.recommendations,
            vec!["Address 1 security issue(s) immediately".to_string()]
        );
    }

    #[test]
    fn summary_is_ready_when_only_resolved_or_dismissed_comments_remain() {
        let comments = vec![
            make_comment(
                "resolved-error",
                Severity::Error,
                Category::Security,
                CommentStatus::Resolved,
            ),
            make_comment(
                "dismissed-warning",
                Severity::Warning,
                Category::Bug,
                CommentStatus::Dismissed,
            ),
        ];

        let summary = generate_summary(&comments);
        assert_eq!(summary.open_blockers, 0);
        assert_eq!(summary.open_blocking_comments, 0);
        assert_eq!(summary.open_informational_comments, 0);
        assert_eq!(summary.merge_readiness, MergeReadiness::Ready);
        assert!(summary.recommendations.is_empty());
    }

    #[test]
    fn summary_distinguishes_blocking_and_informational_open_findings() {
        let comments = vec![
            make_comment(
                "open-warning",
                Severity::Warning,
                Category::Bug,
                CommentStatus::Open,
            ),
            make_comment(
                "open-info",
                Severity::Info,
                Category::Documentation,
                CommentStatus::Open,
            ),
            make_comment(
                "open-suggestion",
                Severity::Suggestion,
                Category::Style,
                CommentStatus::Open,
            ),
        ];

        let summary = generate_summary(&comments);
        assert_eq!(summary.open_comments, 3);
        assert_eq!(summary.open_blocking_comments, 1);
        assert_eq!(summary.open_informational_comments, 2);
        assert_eq!(summary.open_by_severity.get("Warning"), Some(&1));
        assert_eq!(summary.open_by_severity.get("Info"), Some(&1));
        assert_eq!(summary.open_by_severity.get("Suggestion"), Some(&1));
    }

    #[test]
    fn summary_needs_rereview_when_verification_is_inconclusive() {
        let comments = vec![make_comment(
            "open-warning",
            Severity::Warning,
            Category::Bug,
            CommentStatus::Open,
        )];

        let summary = apply_verification(
            generate_summary(&comments),
            ReviewVerificationSummary {
                state: ReviewVerificationState::Inconclusive,
                judge_count: 1,
                required_votes: 1,
                warning_count: 1,
                filtered_comments: 0,
                abstained_comments: 1,
            },
        );

        assert_eq!(summary.merge_readiness, MergeReadiness::NeedsReReview);
        assert_eq!(
            summary.verification.state,
            ReviewVerificationState::Inconclusive
        );
        assert_eq!(summary.readiness_reasons.len(), 1);
    }

    #[test]
    fn stale_review_forces_needs_rereview_even_without_blockers() {
        let comments = vec![make_comment(
            "resolved-warning",
            Severity::Warning,
            Category::Bug,
            CommentStatus::Resolved,
        )];

        let summary = apply_review_runtime_state(generate_summary(&comments), true);
        assert_eq!(summary.open_blockers, 0);
        assert_eq!(summary.merge_readiness, MergeReadiness::NeedsReReview);
        assert_eq!(summary.completeness.stale_findings, 0);
        assert_eq!(
            summary.readiness_reasons,
            vec!["new commits landed after this review".to_string()]
        );
    }

    #[test]
    fn stale_review_counts_open_findings_in_completeness() {
        let comments = vec![
            make_comment(
                "open-warning",
                Severity::Warning,
                Category::Bug,
                CommentStatus::Open,
            ),
            make_comment(
                "resolved-info",
                Severity::Info,
                Category::Documentation,
                CommentStatus::Resolved,
            ),
        ];

        let summary = apply_review_runtime_state(generate_summary(&comments), true);
        assert_eq!(summary.completeness.total_findings, 2);
        assert_eq!(summary.completeness.acknowledged_findings, 1);
        assert_eq!(summary.completeness.fixed_findings, 1);
        assert_eq!(summary.completeness.stale_findings, 1);
    }

    #[test]
    fn inherit_review_state_preserves_fix_loop_telemetry() {
        let previous = ReviewSummary {
            loop_telemetry: Some(FixLoopTelemetry {
                iterations: 3,
                fixes_attempted: 2,
                findings_cleared: 4,
                findings_reopened: 1,
            }),
            ..generate_summary(&[make_comment(
                "open-warning",
                Severity::Warning,
                Category::Bug,
                CommentStatus::Open,
            )])
        };

        let inherited = inherit_review_state(generate_summary(&[]), Some(&previous));

        assert_eq!(inherited.loop_telemetry, previous.loop_telemetry);
    }
}
