use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::core::{comment::ReviewSummary, CommentSynthesizer};

use super::state::{AppState, ReviewSession, ReviewStatus};

const REVIEW_STATE_SCAN_LIMIT: i64 = 1000;

#[derive(Debug, Clone)]
pub(crate) struct LatestGitHubHead {
    started_at: i64,
    head_sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReadinessSnapshot {
    pub repo: String,
    pub pr_number: u32,
    pub diff_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review: Option<PrReadinessReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReadinessReview {
    pub id: String,
    pub status: ReviewStatus,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<ReviewSummary>,
    pub files_reviewed: usize,
    pub comment_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoBlockerRollup {
    pub open_blockers: usize,
    pub blocking_prs: usize,
}

impl PrReadinessReview {
    fn from_session(session: &ReviewSession) -> Self {
        Self {
            id: session.id.clone(),
            status: session.status.clone(),
            started_at: session.started_at,
            completed_at: session.completed_at,
            reviewed_head_sha: session.github_head_sha.clone(),
            summary: session.summary.clone(),
            files_reviewed: session.files_reviewed,
            comment_count: session.comments.len(),
            error: session.error.clone(),
        }
    }
}

pub(crate) fn pr_diff_source(repo: &str, pr_number: u32) -> String {
    format!("pr:{repo}#{pr_number}")
}

pub(crate) fn parse_pr_diff_source(diff_source: &str) -> Option<(String, u32)> {
    let rest = diff_source.strip_prefix("pr:")?;
    let (repo, pr_number) = rest.rsplit_once('#')?;
    Some((repo.to_string(), pr_number.parse().ok()?))
}

pub(crate) async fn load_review_inventory(state: &Arc<AppState>) -> Vec<ReviewSession> {
    let mut sessions: Vec<ReviewSession> = {
        let reviews = state.reviews.read().await;
        reviews.values().cloned().collect()
    };

    if let Ok(stored) = state.storage.list_reviews(REVIEW_STATE_SCAN_LIMIT, 0).await {
        let in_memory_ids: HashSet<String> =
            sessions.iter().map(|session| session.id.clone()).collect();
        for session in stored {
            if !in_memory_ids.contains(&session.id) {
                sessions.push(session);
            }
        }
    }

    sessions
}

pub(crate) fn latest_review_head_by_source(
    reviews: &[ReviewSession],
) -> HashMap<String, LatestGitHubHead> {
    let mut latest: HashMap<String, LatestGitHubHead> = HashMap::new();
    for review in reviews {
        let Some(head_sha) = review.github_head_sha.as_ref() else {
            continue;
        };
        if !review.diff_source.starts_with("pr:") {
            continue;
        }
        let candidate = LatestGitHubHead {
            started_at: review.started_at,
            head_sha: head_sha.clone(),
        };
        match latest.get(&review.diff_source) {
            Some(current) if current.started_at >= review.started_at => {}
            _ => {
                latest.insert(review.diff_source.clone(), candidate);
            }
        }
    }
    latest
}

pub(crate) fn apply_dynamic_review_state(
    mut session: ReviewSession,
    latest_by_source: &HashMap<String, LatestGitHubHead>,
    current_head_sha: Option<&str>,
) -> ReviewSession {
    let latest_known_head_stale = session
        .github_head_sha
        .as_ref()
        .zip(latest_by_source.get(&session.diff_source))
        .is_some_and(|(current_head, latest)| latest.head_sha != *current_head);
    let current_head_stale = session
        .github_head_sha
        .as_deref()
        .zip(current_head_sha)
        .is_some_and(|(reviewed_head, current_head)| reviewed_head != current_head);
    let stale_review = latest_known_head_stale || current_head_stale;

    if let Some(summary) = session.summary.take() {
        session.summary = Some(CommentSynthesizer::apply_runtime_review_state(
            summary,
            stale_review,
        ));
    }

    session
}

fn latest_summarized_reviews_by_source(
    reviews: &[ReviewSession],
) -> HashMap<String, ReviewSession> {
    let mut latest: HashMap<String, ReviewSession> = HashMap::new();
    for review in reviews {
        if review.summary.is_none() || !review.diff_source.starts_with("pr:") {
            continue;
        }
        match latest.get(&review.diff_source) {
            Some(current) if current.started_at >= review.started_at => {}
            _ => {
                latest.insert(review.diff_source.clone(), review.clone());
            }
        }
    }
    latest
}

pub(crate) fn latest_pr_review_session(
    reviews: &[ReviewSession],
    repo: &str,
    pr_number: u32,
) -> Option<ReviewSession> {
    let diff_source = pr_diff_source(repo, pr_number);

    reviews
        .iter()
        .filter(|session| session.diff_source == diff_source && session.summary.is_some())
        .max_by_key(|session| (session.started_at, session.completed_at.unwrap_or_default()))
        .cloned()
}

pub(crate) fn build_repo_blocker_rollups(
    reviews: &[ReviewSession],
) -> HashMap<String, RepoBlockerRollup> {
    let mut rollups = HashMap::new();
    for review in latest_summarized_reviews_by_source(reviews).into_values() {
        let Some(summary) = review.summary.as_ref() else {
            continue;
        };
        let Some((repo, _)) = parse_pr_diff_source(&review.diff_source) else {
            continue;
        };

        let rollup = rollups
            .entry(repo)
            .or_insert_with(RepoBlockerRollup::default);
        rollup.open_blockers += summary.open_blockers;
        if summary.open_blockers > 0 {
            rollup.blocking_prs += 1;
        }
    }
    rollups
}

pub(crate) fn build_pr_readiness_snapshot(
    reviews: &[ReviewSession],
    repo: &str,
    pr_number: u32,
    current_head_sha: Option<&str>,
) -> PrReadinessSnapshot {
    let diff_source = pr_diff_source(repo, pr_number);
    let latest_by_source = latest_review_head_by_source(reviews);
    let latest_review = latest_pr_review_session(reviews, repo, pr_number)
        .map(|session| apply_dynamic_review_state(session, &latest_by_source, current_head_sha))
        .map(|session| PrReadinessReview::from_session(&session));

    PrReadinessSnapshot {
        repo: repo.to_string(),
        pr_number,
        diff_source,
        current_head_sha: current_head_sha.map(str::to_string),
        latest_review,
    }
}

pub(crate) async fn get_pr_readiness_snapshot(
    state: &Arc<AppState>,
    repo: &str,
    pr_number: u32,
    current_head_sha: Option<&str>,
) -> PrReadinessSnapshot {
    let inventory = load_review_inventory(state).await;
    build_pr_readiness_snapshot(&inventory, repo, pr_number, current_head_sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};
    use crate::core::CommentSynthesizer;
    use std::path::PathBuf;

    fn make_comment(id: &str, severity: Severity, status: CommentStatus) -> crate::core::Comment {
        crate::core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "test".to_string(),
            rule_id: None,
            severity,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
            status,
        }
    }

    fn make_pr_review_session(
        id: &str,
        started_at: i64,
        head_sha: &str,
        comments: Vec<crate::core::Comment>,
    ) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some(head_sha.to_string()),
            started_at,
            completed_at: Some(started_at + 1),
            summary: Some(CommentSynthesizer::generate_summary(&comments)),
            files_reviewed: 1,
            comments,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[test]
    fn stale_detection_ignores_same_head_reruns() {
        let older = make_pr_review_session("r1", 10, "sha-a", Vec::new());
        let newer_same_head = make_pr_review_session("r2", 20, "sha-a", Vec::new());
        let latest_by_source = latest_review_head_by_source(&[older.clone(), newer_same_head]);
        let updated = apply_dynamic_review_state(older, &latest_by_source, None);
        let summary = updated.summary.expect("summary");
        assert_eq!(
            summary.merge_readiness,
            crate::core::comment::MergeReadiness::Ready
        );
        assert!(summary.readiness_reasons.is_empty());
    }

    #[test]
    fn stale_detection_requires_newer_head_sha() {
        let older = make_pr_review_session("r1", 10, "sha-a", Vec::new());
        let newer_head = make_pr_review_session("r2", 20, "sha-b", Vec::new());
        let latest_by_source = latest_review_head_by_source(&[older.clone(), newer_head]);
        let updated = apply_dynamic_review_state(older, &latest_by_source, None);
        let summary = updated.summary.expect("summary");
        assert_eq!(
            summary.merge_readiness,
            crate::core::comment::MergeReadiness::NeedsReReview
        );
        assert_eq!(
            summary.readiness_reasons,
            vec!["new commits landed after this review".to_string()]
        );
    }

    #[test]
    fn current_head_marks_latest_review_stale_without_newer_review() {
        let review = make_pr_review_session("r1", 10, "sha-a", Vec::new());
        let snapshot = build_pr_readiness_snapshot(&[review], "owner/repo", 42, Some("sha-b"));
        let summary = snapshot
            .latest_review
            .expect("latest review")
            .summary
            .expect("summary");
        assert_eq!(
            summary.merge_readiness,
            crate::core::comment::MergeReadiness::NeedsReReview
        );
        assert_eq!(
            summary.readiness_reasons,
            vec!["new commits landed after this review".to_string()]
        );
    }

    #[test]
    fn readiness_snapshot_uses_latest_completed_review() {
        let older_complete = make_pr_review_session(
            "r1",
            10,
            "sha-a",
            vec![make_comment("c1", Severity::Warning, CommentStatus::Open)],
        );
        let newer_failed = ReviewSession {
            id: "r2".to_string(),
            status: ReviewStatus::Failed,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-b".to_string()),
            started_at: 20,
            completed_at: Some(21),
            summary: None,
            files_reviewed: 0,
            comments: Vec::new(),
            error: Some("boom".to_string()),
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };

        let snapshot = build_pr_readiness_snapshot(
            &[older_complete, newer_failed],
            "owner/repo",
            42,
            Some("sha-b"),
        );

        let latest_review = snapshot.latest_review.expect("latest review");
        assert_eq!(latest_review.id, "r1");
        assert_eq!(latest_review.status, ReviewStatus::Complete);
        assert_eq!(latest_review.comment_count, 1);
        assert_eq!(
            latest_review.summary.expect("summary").merge_readiness,
            crate::core::comment::MergeReadiness::NeedsReReview
        );
    }

    #[test]
    fn latest_pr_review_session_ignores_newer_failed_reviews() {
        let older_complete = make_pr_review_session(
            "r1",
            10,
            "sha-a",
            vec![make_comment("c1", Severity::Warning, CommentStatus::Open)],
        );
        let newer_failed = ReviewSession {
            id: "r2".to_string(),
            status: ReviewStatus::Failed,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-b".to_string()),
            started_at: 20,
            completed_at: Some(21),
            summary: None,
            files_reviewed: 0,
            comments: Vec::new(),
            error: Some("boom".to_string()),
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };

        let latest_review =
            latest_pr_review_session(&[older_complete, newer_failed], "owner/repo", 42)
                .expect("latest completed review");

        assert_eq!(latest_review.id, "r1");
        assert_eq!(latest_review.status, ReviewStatus::Complete);
        assert_eq!(latest_review.comments.len(), 1);
    }

    #[test]
    fn repo_blocker_rollups_use_latest_review_per_pr() {
        let older_pr = make_pr_review_session(
            "r1",
            10,
            "sha-a",
            vec![make_comment("c1", Severity::Warning, CommentStatus::Open)],
        );
        let newer_same_pr = ReviewSession {
            id: "r2".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-b".to_string()),
            started_at: 20,
            completed_at: Some(21),
            summary: Some(CommentSynthesizer::generate_summary(&[])),
            files_reviewed: 1,
            comments: Vec::new(),
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };
        let other_pr = ReviewSession {
            id: "r3".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#43".to_string(),
            github_head_sha: Some("sha-c".to_string()),
            started_at: 15,
            completed_at: Some(16),
            summary: Some(CommentSynthesizer::generate_summary(&vec![make_comment(
                "c2",
                Severity::Error,
                CommentStatus::Open,
            )])),
            files_reviewed: 1,
            comments: vec![make_comment("c2", Severity::Error, CommentStatus::Open)],
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };
        let other_repo = ReviewSession {
            id: "r4".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:other/repo#7".to_string(),
            github_head_sha: Some("sha-d".to_string()),
            started_at: 12,
            completed_at: Some(13),
            summary: Some(CommentSynthesizer::generate_summary(&vec![make_comment(
                "c3",
                Severity::Warning,
                CommentStatus::Open,
            )])),
            files_reviewed: 1,
            comments: vec![make_comment("c3", Severity::Warning, CommentStatus::Open)],
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };

        let rollups = build_repo_blocker_rollups(&[older_pr, newer_same_pr, other_pr, other_repo]);

        assert_eq!(
            rollups.get("owner/repo"),
            Some(&RepoBlockerRollup {
                open_blockers: 1,
                blocking_prs: 1,
            })
        );
        assert_eq!(
            rollups.get("other/repo"),
            Some(&RepoBlockerRollup {
                open_blockers: 1,
                blocking_prs: 1,
            })
        );
    }
}
