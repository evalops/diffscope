use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::core::{diff_parser::ChangeType, UnifiedDiff};

use super::{Comment, CommentOutcome, CommentOutcomeContext, CommentStatus};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FollowUpCommentResolutionOutcomes {
    pub addressed_comment_ids: HashSet<String>,
    pub not_addressed_comment_ids: HashSet<String>,
}

fn push_unique(outcomes: &mut Vec<CommentOutcome>, outcome: CommentOutcome) {
    if !outcomes.contains(&outcome) {
        outcomes.push(outcome);
    }
}

fn normalize_comment_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

pub fn infer_addressed_by_follow_up_comments(
    comments: &[Comment],
    follow_up_diffs: &[UnifiedDiff],
) -> HashSet<String> {
    let mut changed_old_lines_by_path: HashMap<String, HashSet<usize>> = HashMap::new();

    for diff in follow_up_diffs {
        let changed_old_lines = changed_old_lines_by_path
            .entry(normalize_comment_path(diff.file_path.as_path()))
            .or_default();

        for hunk in &diff.hunks {
            for line in &hunk.changes {
                if line.change_type != ChangeType::Context {
                    if let Some(old_line_no) = line.old_line_no {
                        changed_old_lines.insert(old_line_no);
                    }
                }
            }
        }
    }

    comments
        .iter()
        .filter(|comment| comment.status == CommentStatus::Open)
        .filter_map(|comment| {
            let path = normalize_comment_path(comment.file_path.as_path());
            changed_old_lines_by_path
                .get(&path)
                .filter(|changed_lines| changed_lines.contains(&comment.line_number))
                .map(|_| comment.id.clone())
        })
        .collect()
}

pub fn infer_follow_up_comment_resolution_outcomes(
    previous_comments: &[Comment],
    current_comments: &[Comment],
    follow_up_diffs: &[UnifiedDiff],
) -> FollowUpCommentResolutionOutcomes {
    let addressed_comment_ids =
        infer_addressed_by_follow_up_comments(previous_comments, follow_up_diffs);
    let current_comment_ids = current_comments
        .iter()
        .map(|comment| comment.id.as_str())
        .collect::<HashSet<_>>();

    let not_addressed_comment_ids = previous_comments
        .iter()
        .filter(|comment| comment.status == CommentStatus::Open)
        .filter(|comment| !addressed_comment_ids.contains(&comment.id))
        .filter(|comment| current_comment_ids.contains(comment.id.as_str()))
        .map(|comment| comment.id.clone())
        .collect();

    FollowUpCommentResolutionOutcomes {
        addressed_comment_ids,
        not_addressed_comment_ids,
    }
}

pub fn derive_comment_outcomes(
    comment: &Comment,
    context: CommentOutcomeContext,
) -> Vec<CommentOutcome> {
    let mut outcomes = Vec::new();

    match comment
        .feedback
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("accept") => push_unique(&mut outcomes, CommentOutcome::Accepted),
        Some("reject") => push_unique(&mut outcomes, CommentOutcome::Rejected),
        _ => {}
    }

    if comment.status == CommentStatus::Resolved || context.addressed_by_follow_up {
        push_unique(&mut outcomes, CommentOutcome::Addressed);
    }

    if context.auto_fixed {
        push_unique(&mut outcomes, CommentOutcome::Addressed);
        push_unique(&mut outcomes, CommentOutcome::AutoFixed);
    }

    if comment.status == CommentStatus::Open && context.stale_review {
        push_unique(&mut outcomes, CommentOutcome::Stale);
    }

    if outcomes.is_empty() && comment.status == CommentStatus::Open {
        push_unique(&mut outcomes, CommentOutcome::New);
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::core::{
        comment::{Category, Comment, CommentOutcomeContext, FixEffort, Severity},
        DiffParser,
    };

    use super::*;

    fn make_comment() -> Comment {
        Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn derives_new_for_open_comments_without_other_signals() {
        assert_eq!(
            derive_comment_outcomes(&make_comment(), CommentOutcomeContext::default()),
            vec![CommentOutcome::New]
        );
    }

    #[test]
    fn derives_feedback_and_addressed_outcomes_independently() {
        let mut comment = make_comment();
        comment.feedback = Some("accept".to_string());
        comment.status = CommentStatus::Resolved;

        assert_eq!(
            derive_comment_outcomes(&comment, CommentOutcomeContext::default()),
            vec![CommentOutcome::Accepted, CommentOutcome::Addressed]
        );
    }

    #[test]
    fn derives_rejected_without_marking_new() {
        let mut comment = make_comment();
        comment.feedback = Some("reject".to_string());

        assert_eq!(
            derive_comment_outcomes(&comment, CommentOutcomeContext::default()),
            vec![CommentOutcome::Rejected]
        );
    }

    #[test]
    fn derives_stale_for_open_comments_in_stale_reviews() {
        assert_eq!(
            derive_comment_outcomes(
                &make_comment(),
                CommentOutcomeContext {
                    stale_review: true,
                    ..CommentOutcomeContext::default()
                }
            ),
            vec![CommentOutcome::Stale]
        );
    }

    #[test]
    fn derives_addressed_for_open_comments_touched_by_follow_up_commits() {
        assert_eq!(
            derive_comment_outcomes(
                &make_comment(),
                CommentOutcomeContext {
                    addressed_by_follow_up: true,
                    ..CommentOutcomeContext::default()
                }
            ),
            vec![CommentOutcome::Addressed]
        );
    }

    #[test]
    fn dismissed_comments_keep_lifecycle_without_derived_outcomes() {
        let mut comment = make_comment();
        comment.status = CommentStatus::Dismissed;

        assert!(derive_comment_outcomes(&comment, CommentOutcomeContext::default()).is_empty());
    }

    #[test]
    fn infer_follow_up_marks_open_comments_as_addressed_when_old_line_changes() {
        let diffs = DiffParser::parse_text_diff(
            "first\nsecond\nthird\n",
            "first\nupdated second\nthird\n",
            PathBuf::from("src/lib.rs"),
        )
        .unwrap();

        let mut comment = make_comment();
        comment.line_number = 2;

        assert_eq!(
            infer_addressed_by_follow_up_comments(&[comment], &[diffs])
                .into_iter()
                .collect::<HashSet<_>>(),
            HashSet::from(["comment-1".to_string()])
        );
    }

    #[test]
    fn infer_follow_up_ignores_context_only_line_matches() {
        let diffs = DiffParser::parse_text_diff(
            "first\nsecond\nthird\n",
            "first\ninserted\nsecond\nthird\n",
            PathBuf::from("src/lib.rs"),
        )
        .unwrap();

        let mut comment = make_comment();
        comment.line_number = 2;

        assert!(infer_addressed_by_follow_up_comments(&[comment], &[diffs]).is_empty());
    }

    #[test]
    fn infer_follow_up_resolution_outcomes_splits_addressed_and_persistent_findings() {
        let diffs = DiffParser::parse_text_diff(
            "first\nsecond\nthird\n",
            "first\nupdated second\nthird\n",
            PathBuf::from("src/lib.rs"),
        )
        .unwrap();

        let mut addressed = make_comment();
        addressed.id = "comment-addressed".to_string();
        addressed.line_number = 2;

        let mut persistent = make_comment();
        persistent.id = "comment-persistent".to_string();
        persistent.line_number = 3;

        let current_comments = vec![persistent.clone()];
        let outcomes = infer_follow_up_comment_resolution_outcomes(
            &[addressed, persistent],
            &current_comments,
            &[diffs],
        );

        assert_eq!(
            outcomes.addressed_comment_ids,
            HashSet::from(["comment-addressed".to_string()])
        );
        assert_eq!(
            outcomes.not_addressed_comment_ids,
            HashSet::from(["comment-persistent".to_string()])
        );
    }
}
