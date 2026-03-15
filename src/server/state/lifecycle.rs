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
        let retention = { state.config.read().await.retention.clone() };
        let max_age_secs = retention.review_max_age_days.saturating_mul(86_400);

        if let Err(err) =
            Self::prune_reviews_with_limits(state, max_age_secs, retention.review_max_count).await
        {
            warn!("Failed to apply review retention policy: {}", err);
        }
    }

    pub async fn prune_reviews_with_limits(
        state: &Arc<AppState>,
        max_age_secs: i64,
        max_count: usize,
    ) -> anyhow::Result<usize> {
        let memory_pruned = prune_reviews_in_memory(state, max_age_secs, max_count).await;
        if memory_pruned > 0 {
            Self::save_reviews_async(state);
        }

        let storage_pruned = state.storage.prune(max_age_secs, max_count).await?;
        Ok(memory_pruned.max(storage_pruned))
    }
}

async fn prune_reviews_in_memory(
    state: &Arc<AppState>,
    max_age_secs: i64,
    max_count: usize,
) -> usize {
    let mut reviews = state.reviews.write().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let expired: Vec<String> = reviews
        .iter()
        .filter(|(_, r)| now - r.started_at > max_age_secs)
        .map(|(id, _)| id.clone())
        .collect();
    let mut removed = expired.len();
    for id in &expired {
        reviews.remove(id);
    }

    if reviews.len() > max_count {
        let mut completed: Vec<(String, i64)> = reviews
            .iter()
            .filter(|(_, r)| r.status == ReviewStatus::Complete || r.status == ReviewStatus::Failed)
            .map(|(id, r)| (id.clone(), r.started_at))
            .collect();
        completed.sort_by_key(|(_, ts)| *ts);

        let to_remove = reviews.len() - max_count;
        for (id, _) in completed.into_iter().take(to_remove) {
            reviews.remove(&id);
            removed += 1;
        }
    }

    removed
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
