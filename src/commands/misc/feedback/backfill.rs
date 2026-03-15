use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::core;
use crate::core::comment::CommentStatus;
use crate::review;
use crate::server::state::{ReviewSession, ReviewStatus};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct FeedbackBackfillSummary {
    pub(super) reviews_seen: usize,
    pub(super) comments_seen: usize,
    pub(super) accepted: usize,
    pub(super) rejected: usize,
    pub(super) dismissed: usize,
    pub(super) not_addressed: usize,
}

pub(super) async fn backfill_feedback_store(
    input_path: &Path,
    feedback_path: &Path,
) -> Result<FeedbackBackfillSummary> {
    let sessions = crate::commands::load_review_sessions_input(input_path).await?;
    let (store, summary) = build_feedback_store_from_reviews(sessions);
    review::save_feedback_store(feedback_path, &store)?;
    Ok(summary)
}

fn build_feedback_store_from_reviews(
    mut sessions: Vec<ReviewSession>,
) -> (review::FeedbackStore, FeedbackBackfillSummary) {
    normalize_review_comment_ids(&mut sessions);
    sort_review_sessions(&mut sessions);

    let mut store = review::FeedbackStore::default();
    let mut summary = FeedbackBackfillSummary {
        reviews_seen: sessions.len(),
        comments_seen: sessions.iter().map(|session| session.comments.len()).sum(),
        ..FeedbackBackfillSummary::default()
    };

    for session in &sessions {
        let timestamp = feedback_event_timestamp(session);
        for comment in &session.comments {
            if let Some(accepted) = normalize_feedback_label(comment.feedback.as_deref()) {
                if review::apply_comment_feedback_signal_at(
                    &mut store, comment, accepted, timestamp,
                ) {
                    if accepted {
                        summary.accepted += 1;
                    } else {
                        summary.rejected += 1;
                    }
                }
            }

            if comment.status == CommentStatus::Dismissed
                && review::apply_comment_dismissal_signal(&mut store, comment)
            {
                summary.dismissed += 1;
            }
        }
    }

    summary.not_addressed = backfill_not_addressed_outcomes(&sessions, &mut store);

    (store, summary)
}

fn backfill_not_addressed_outcomes(
    sessions: &[ReviewSession],
    store: &mut review::FeedbackStore,
) -> usize {
    let mut sessions_by_pr: HashMap<String, Vec<&ReviewSession>> = HashMap::new();
    for session in sessions
        .iter()
        .filter(|session| session.status == ReviewStatus::Complete)
    {
        let Some(pr_key) = session_pr_key(session) else {
            continue;
        };
        sessions_by_pr.entry(pr_key).or_default().push(session);
    }

    let mut not_addressed = 0usize;
    for pr_sessions in sessions_by_pr.values_mut() {
        pr_sessions.sort_by(|left, right| compare_review_sessions(left, right));

        for window in pr_sessions.windows(2) {
            let previous = window[0];
            let current = window[1];

            let Some(previous_head_sha) = previous.github_head_sha.as_deref() else {
                continue;
            };
            let Some(current_head_sha) = current.github_head_sha.as_deref() else {
                continue;
            };
            if previous_head_sha == current_head_sha {
                continue;
            }

            let current_comment_ids = current
                .comments
                .iter()
                .map(|comment| comment.id.as_str())
                .collect::<HashSet<_>>();

            for comment in previous
                .comments
                .iter()
                .filter(|comment| comment.status == CommentStatus::Open)
            {
                if current_comment_ids.contains(comment.id.as_str())
                    && review::apply_comment_resolution_outcome_signal_at(
                        store,
                        comment,
                        review::CommentResolutionOutcome::NotAddressed,
                        feedback_event_timestamp(current),
                    )
                {
                    not_addressed += 1;
                }
            }
        }
    }

    not_addressed
}

fn session_pr_key(session: &ReviewSession) -> Option<String> {
    crate::server::pr_readiness::parse_pr_diff_source(&session.diff_source)
        .map(|(repo, pr_number)| format!("{repo}#{pr_number}"))
}

fn normalize_review_comment_ids(sessions: &mut [ReviewSession]) {
    for session in sessions {
        for comment in &mut session.comments {
            if comment.id.trim().is_empty() {
                comment.id = core::comment::compute_comment_id(
                    &comment.file_path,
                    &comment.content,
                    &comment.category,
                );
            }
        }
    }
}

fn sort_review_sessions(sessions: &mut [ReviewSession]) {
    sessions.sort_by(compare_review_sessions);
}

fn compare_review_sessions(left: &ReviewSession, right: &ReviewSession) -> std::cmp::Ordering {
    left.started_at
        .cmp(&right.started_at)
        .then_with(|| left.completed_at.cmp(&right.completed_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn normalize_feedback_label(label: Option<&str>) -> Option<bool> {
    match label?.trim().to_ascii_lowercase().as_str() {
        "accept" | "accepted" => Some(true),
        "reject" | "rejected" => Some(false),
        _ => None,
    }
}

fn feedback_event_timestamp(session: &ReviewSession) -> i64 {
    session.completed_at.unwrap_or(session.started_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn sample_comment(content: &str, category: Category, file_path: &str) -> core::Comment {
        core::Comment {
            id: format!("id-{content}"),
            file_path: PathBuf::from(file_path),
            line_number: 12,
            content: content.to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    fn sample_review_session(
        id: &str,
        started_at: i64,
        diff_source: &str,
        head_sha: Option<&str>,
        comments: Vec<core::Comment>,
    ) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status: ReviewStatus::Complete,
            diff_source: diff_source.to_string(),
            github_head_sha: head_sha.map(str::to_string),
            github_post_results_requested: None,
            started_at,
            completed_at: Some(started_at + 1),
            comments,
            summary: None,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn build_feedback_store_backfills_explicit_feedback_and_dismissals() {
        let mut accepted = sample_comment(
            "Prevent SQL injection by parameterizing this query",
            Category::Security,
            "src/lib.rs",
        );
        accepted.feedback = Some("accept".to_string());
        accepted.rule_id = Some("SEC.SQL.INJECTION".to_string());

        let mut rejected = sample_comment(
            "This helper should use a different variable name",
            Category::Style,
            "web/src/App.tsx",
        );
        rejected.id.clear();
        rejected.feedback = Some("reject".to_string());
        rejected.rule_id = Some("STYLE.NAMING".to_string());

        let mut dismissed = sample_comment(
            "Prefer grouping these imports alphabetically",
            Category::BestPractice,
            "scripts/release.sh",
        );
        dismissed.status = CommentStatus::Dismissed;

        let rejected_id = core::comment::compute_comment_id(
            &rejected.file_path,
            &rejected.content,
            &rejected.category,
        );

        let (store, summary) = build_feedback_store_from_reviews(vec![sample_review_session(
            "review-1",
            10,
            "raw",
            None,
            vec![accepted, rejected, dismissed],
        )]);

        assert_eq!(
            summary,
            FeedbackBackfillSummary {
                reviews_seen: 1,
                comments_seen: 3,
                accepted: 1,
                rejected: 1,
                dismissed: 1,
                not_addressed: 0,
            }
        );
        assert!(store
            .accept
            .contains("id-Prevent SQL injection by parameterizing this query"));
        assert!(store.suppress.contains(&rejected_id));
        assert!(store
            .dismissed
            .contains("id-Prefer grouping these imports alphabetically"));
        assert_eq!(store.by_category["Security"].accepted, 1);
        assert_eq!(store.by_category["Style"].rejected, 1);
        assert_eq!(store.by_category["BestPractice"].dismissed, 1);
        assert_eq!(store.by_rule["sec.sql.injection"].accepted, 1);
        assert_eq!(store.by_rule["style.naming"].rejected, 1);
    }

    #[test]
    fn build_feedback_store_backfills_not_addressed_outcomes_from_repeated_pr_findings() {
        let persistent = sample_comment(
            "The request is still missing auth checks",
            Category::Security,
            "src/api.rs",
        );

        let previous = sample_review_session(
            "review-previous",
            10,
            "pr:octo/repo#42",
            Some("sha-old"),
            vec![persistent.clone()],
        );
        let current = sample_review_session(
            "review-current",
            20,
            "pr:octo/repo#42",
            Some("sha-new"),
            vec![persistent.clone()],
        );

        let (store, summary) = build_feedback_store_from_reviews(vec![current, previous]);

        assert_eq!(summary.not_addressed, 1);
        assert!(store.not_addressed.contains(&persistent.id));
        assert_eq!(store.by_category["Security"].not_addressed, 1);
        assert_eq!(store.by_file_pattern["src/**"].not_addressed, 1);
    }

    #[test]
    fn build_feedback_store_is_deterministic_across_review_maps_and_lists() {
        let mut feedback = sample_comment(
            "Add a regression test for this branch",
            Category::Testing,
            "tests/review.rs",
        );
        feedback.feedback = Some("accept".to_string());

        let persistent = sample_comment(
            "This branch still skips validation",
            Category::Bug,
            "src/review.rs",
        );

        let older = sample_review_session(
            "review-older",
            10,
            "pr:octo/repo#7",
            Some("sha-1"),
            vec![persistent.clone()],
        );
        let newer = sample_review_session(
            "review-newer",
            20,
            "pr:octo/repo#7",
            Some("sha-2"),
            vec![persistent, feedback],
        );

        let map_json = serde_json::to_string(&HashMap::from([
            (older.id.clone(), older.clone()),
            (newer.id.clone(), newer.clone()),
        ]))
        .unwrap();
        let list_json = serde_json::to_string(&vec![newer, older]).unwrap();

        let map_sessions = crate::commands::load_review_sessions_input_from_str(&map_json).unwrap();
        let list_sessions =
            crate::commands::load_review_sessions_input_from_str(&list_json).unwrap();

        let (map_store, map_summary) = build_feedback_store_from_reviews(map_sessions);
        let (list_store, list_summary) = build_feedback_store_from_reviews(list_sessions);

        assert_eq!(map_summary, list_summary);
        assert_eq!(
            serde_json::to_value(&map_store).unwrap(),
            serde_json::to_value(&list_store).unwrap()
        );
    }

    #[test]
    fn build_feedback_store_replays_rule_decay_in_chronological_order() {
        let half_life = 30 * 24 * 60 * 60;

        let stale_rejects = (0..32)
            .map(|index| {
                let mut comment = sample_comment(
                    &format!("Stale reject {index}"),
                    Category::Security,
                    "src/lib.rs",
                );
                comment.feedback = Some("reject".to_string());
                comment.rule_id = Some("SEC.SQL.INJECTION".to_string());
                comment.id = format!("stale-reject-{index}");
                comment
            })
            .collect::<Vec<_>>();
        let recent_accepts = (0..4)
            .map(|index| {
                let mut comment = sample_comment(
                    &format!("Recent accept {index}"),
                    Category::Security,
                    "src/lib.rs",
                );
                comment.feedback = Some("accept".to_string());
                comment.rule_id = Some("SEC.SQL.INJECTION".to_string());
                comment.id = format!("recent-accept-{index}");
                comment
            })
            .collect::<Vec<_>>();

        let (store, _) = build_feedback_store_from_reviews(vec![
            sample_review_session("stale", 1_000, "raw", None, stale_rejects),
            sample_review_session(
                "recent",
                1_000 + (4 * half_life),
                "raw",
                None,
                recent_accepts,
            ),
        ]);

        let stats = &store.by_rule["sec.sql.injection"];
        let recent_timestamp = 1_000 + (4 * half_life) + 1;
        assert!(stats.acceptance_rate() < 0.2);
        assert!(
            stats.decayed_acceptance_rate_at(recent_timestamp).unwrap() > 0.6,
            "expected recent accepts to outweigh stale rejects after replay"
        );
    }
}
