use super::*;

impl AppState {
    pub async fn mark_running(state: &Arc<AppState>, review_id: &str) {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(review_id) {
            session.status = ReviewStatus::Running;
        }
    }

    /// Update a review session on successful completion.
    pub async fn complete_review(
        state: &Arc<AppState>,
        review_id: &str,
        comments: Vec<Comment>,
        summary: ReviewSummary,
        files_reviewed: usize,
        event: ReviewEvent,
    ) {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(review_id) {
            session.status = ReviewStatus::Complete;
            session.comments = comments;
            session.summary = Some(summary);
            session.files_reviewed = files_reviewed;
            session.completed_at = Some(current_timestamp());
            session.event = Some(event);
            session.progress = None;
        }
    }

    /// Update a review session on failure.
    pub async fn fail_review(
        state: &Arc<AppState>,
        review_id: &str,
        error: String,
        event: Option<ReviewEvent>,
    ) {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(review_id) {
            session.status = ReviewStatus::Failed;
            session.error = Some(error);
            session.completed_at = Some(current_timestamp());
            session.event = event;
            session.progress = None;
        }
    }

    /// Prune old and excess reviews.
    pub async fn prune_old_reviews(state: &Arc<AppState>) {
        let mut reviews = state.reviews.write().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Phase 1: Remove reviews older than MAX_REVIEW_AGE_SECS (including zombie Running/Pending)
        let expired: Vec<String> = reviews
            .iter()
            .filter(|(_, r)| now - r.started_at > MAX_REVIEW_AGE_SECS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired {
            reviews.remove(id);
        }

        // Phase 2: If still over limit, prune oldest completed/failed
        if reviews.len() > MAX_REVIEWS {
            let mut completed: Vec<(String, i64)> = reviews
                .iter()
                .filter(|(_, r)| {
                    r.status == ReviewStatus::Complete || r.status == ReviewStatus::Failed
                })
                .map(|(id, r)| (id.clone(), r.started_at))
                .collect();
            completed.sort_by_key(|(_, ts)| *ts);

            let to_remove = reviews.len() - MAX_REVIEWS;
            for (id, _) in completed.into_iter().take(to_remove) {
                reviews.remove(&id);
            }
        }
    }
}

pub fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Count the number of files in a unified diff string.
pub fn count_diff_files(diff_content: &str) -> usize {
    diff_content.matches("\ndiff --git ").count()
        + if diff_content.starts_with("diff --git ") {
            1
        } else {
            0
        }
}
