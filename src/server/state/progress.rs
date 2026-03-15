use super::*;

pub fn build_progress_callback(
    state: &Arc<AppState>,
    review_id: &str,
    task_start: std::time::Instant,
) -> crate::review::ProgressCallback {
    let ps = state.clone();
    let pid = review_id.to_string();
    Arc::new(move |update: crate::review::ProgressUpdate| {
        let state = ps.clone();
        let id = pid.clone();
        let start = task_start;
        tokio::spawn(async move {
            let elapsed = start.elapsed().as_millis() as u64;
            let avg = if update.files_completed > 0 {
                elapsed / update.files_completed as u64
            } else {
                0
            };
            let remaining = update
                .files_total
                .saturating_sub(update.files_completed)
                .saturating_sub(update.files_skipped);
            let est = if update.files_completed > 0 {
                Some(remaining as u64 * avg)
            } else {
                None
            };
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&id) {
                session.progress = Some(ReviewProgress {
                    current_file: Some(update.current_file),
                    files_total: update.files_total,
                    files_completed: update.files_completed,
                    files_skipped: update.files_skipped,
                    elapsed_ms: elapsed,
                    estimated_remaining_ms: est,
                });
                session.comments = update.comments_so_far.clone();
                session.files_reviewed = update.files_completed;
                if !session.comments.is_empty() {
                    session.summary = Some(crate::core::CommentSynthesizer::generate_summary(
                        &session.comments,
                    ));
                }
            }
        });
    })
}

/// Count unique files in a set of review comments.
pub fn count_reviewed_files(comments: &[Comment]) -> usize {
    let mut files = std::collections::HashSet::new();
    for c in comments {
        files.insert(c.file_path.clone());
    }
    files.len()
}
