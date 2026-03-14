use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::state::{ReviewEvent, ReviewSession, ReviewStatus};
use super::storage::{
    EventFilters, EventStats, ModelStats, RepoStats, SourceStats, StorageBackend,
};

/// JSON file-based storage backend (original behavior).
/// Reviews are kept in-memory and periodically flushed to a JSON file.
pub struct JsonStorageBackend {
    reviews: RwLock<HashMap<String, ReviewSession>>,
    storage_path: PathBuf,
}

impl JsonStorageBackend {
    pub fn new(storage_path: &std::path::Path) -> Self {
        let reviews = Self::load_from_disk(storage_path);
        Self {
            reviews: RwLock::new(reviews),
            storage_path: storage_path.to_path_buf(),
        }
    }

    fn load_from_disk(path: &std::path::Path) -> HashMap<String, ReviewSession> {
        if !path.exists() {
            return HashMap::new();
        }
        match std::fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<HashMap<String, ReviewSession>>(&data) {
                Ok(loaded) => {
                    info!("Loaded {} reviews from disk", loaded.len());
                    loaded
                }
                Err(e) => {
                    warn!("Failed to parse reviews.json: {}", e);
                    HashMap::new()
                }
            },
            Err(e) => {
                warn!("Failed to read reviews.json: {}", e);
                HashMap::new()
            }
        }
    }

    async fn flush_to_disk(&self) {
        let json = {
            let reviews = self.reviews.read().await;
            let stripped: HashMap<String, ReviewSession> = reviews
                .iter()
                .map(|(k, v)| {
                    let mut session = v.clone();
                    session.diff_content = None;
                    (k.clone(), session)
                })
                .collect();
            match serde_json::to_string_pretty(&stripped) {
                Ok(j) => j,
                Err(e) => {
                    error!("Failed to serialize reviews: {}", e);
                    return;
                }
            }
        };

        if let Some(parent) = self.storage_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                error!("Failed to create storage directory: {}", e);
                return;
            }
        }
        let tmp_path = self.storage_path.with_extension("json.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
            error!("Failed to write reviews temp file: {}", e);
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &self.storage_path).await {
            error!("Failed to rename reviews file: {}", e);
            let _ = tokio::fs::remove_file(&tmp_path).await;
        }
    }

    fn refresh_summary(session: &mut ReviewSession) {
        if session.summary.is_some() || !session.comments.is_empty() {
            let previous_summary = session.summary.clone();
            session.summary = Some(crate::core::CommentSynthesizer::inherit_review_state(
                crate::core::CommentSynthesizer::generate_summary(&session.comments),
                previous_summary.as_ref(),
            ));
        }
    }
}

#[async_trait]
impl StorageBackend for JsonStorageBackend {
    async fn save_review(&self, session: &ReviewSession) -> anyhow::Result<()> {
        {
            let mut reviews = self.reviews.write().await;
            reviews.insert(session.id.clone(), session.clone());
        }
        self.flush_to_disk().await;
        Ok(())
    }

    async fn get_review(&self, id: &str) -> anyhow::Result<Option<ReviewSession>> {
        let reviews = self.reviews.read().await;
        Ok(reviews.get(id).cloned().map(|mut session| {
            Self::refresh_summary(&mut session);
            session
        }))
    }

    async fn list_reviews(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ReviewSession>> {
        let reviews = self.reviews.read().await;
        let mut list: Vec<&ReviewSession> = reviews.values().collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        let offset = offset.max(0) as usize;
        let limit = limit.max(0) as usize;
        Ok(list
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .map(|mut session| {
                Self::refresh_summary(&mut session);
                session
            })
            .collect())
    }

    async fn delete_review(&self, id: &str) -> anyhow::Result<()> {
        let mut reviews = self.reviews.write().await;
        reviews.remove(id);
        drop(reviews);
        self.flush_to_disk().await;
        Ok(())
    }

    async fn save_event(&self, _event: &ReviewEvent) -> anyhow::Result<()> {
        // Events are stored inline in ReviewSession.event for JSON backend
        // No separate operation needed - save_review handles it
        Ok(())
    }

    async fn list_events(&self, filters: &EventFilters) -> anyhow::Result<Vec<ReviewEvent>> {
        let reviews = self.reviews.read().await;
        let mut events: Vec<ReviewEvent> = reviews
            .values()
            .filter_map(|s| s.event.clone())
            .filter(|e| {
                let source_ok = filters
                    .source
                    .as_ref()
                    .is_none_or(|f| e.diff_source.eq_ignore_ascii_case(f));
                let model_ok = filters
                    .model
                    .as_ref()
                    .is_none_or(|f| e.model.eq_ignore_ascii_case(f));
                let status_ok = filters
                    .status
                    .as_ref()
                    .is_none_or(|f| e.event_type.eq_ignore_ascii_case(&format!("review.{f}")));
                // Time filters: if a time bound is specified, events without a
                // timestamp are excluded (they cannot satisfy the constraint).
                let time_from_ok = filters
                    .time_from
                    .as_ref()
                    .is_none_or(|from| e.created_at.is_some_and(|t| t >= *from));
                let time_to_ok = filters
                    .time_to
                    .as_ref()
                    .is_none_or(|to| e.created_at.is_some_and(|t| t <= *to));
                let repo_ok = filters
                    .github_repo
                    .as_ref()
                    .is_none_or(|f| e.github_repo.as_deref().is_some_and(|r| r == f.as_str()));
                source_ok && model_ok && status_ok && time_from_ok && time_to_ok && repo_ok
            })
            .collect();

        // Sort by created_at (newest first), falling back to review_id
        events.sort_by(|a, b| {
            let a_time = a.created_at.unwrap_or_default();
            let b_time = b.created_at.unwrap_or_default();
            b_time.cmp(&a_time).then(b.review_id.cmp(&a.review_id))
        });

        // Apply limit/offset
        let offset = filters.offset.unwrap_or(0).max(0) as usize;
        let limit = filters.limit.unwrap_or(500).max(0) as usize;
        events = events.into_iter().skip(offset).take(limit).collect();

        Ok(events)
    }

    async fn get_event_stats(&self, filters: &EventFilters) -> anyhow::Result<EventStats> {
        // Stats must cover ALL matching events, not a paginated subset
        let mut stats_filters = filters.clone();
        stats_filters.limit = Some(i64::MAX);
        stats_filters.offset = Some(0);
        let events = self.list_events(&stats_filters).await?;

        let total = events.len() as i64;
        let completed = events
            .iter()
            .filter(|e| e.event_type == "review.completed")
            .count() as i64;
        let failed = events
            .iter()
            .filter(|e| e.event_type == "review.failed")
            .count() as i64;
        let error_rate = if total > 0 {
            failed as f64 / total as f64
        } else {
            0.0
        };

        let total_tokens: i64 = events.iter().filter_map(|e| e.tokens_total).sum::<usize>() as i64;
        let avg_duration_ms = if total > 0 {
            events.iter().map(|e| e.duration_ms).sum::<u64>() as f64 / total as f64
        } else {
            0.0
        };

        let scores: Vec<f32> = events.iter().filter_map(|e| e.overall_score).collect();
        let avg_score = if scores.is_empty() {
            None
        } else {
            Some(scores.iter().sum::<f32>() as f64 / scores.len() as f64)
        };

        // Latency percentiles
        let mut durations: Vec<u64> = events.iter().map(|e| e.duration_ms).collect();
        durations.sort();
        let percentile = |p: f64| -> i64 {
            if durations.is_empty() {
                return 0;
            }
            let idx = ((p / 100.0) * (durations.len() as f64 - 1.0)).round() as usize;
            durations[idx.min(durations.len() - 1)] as i64
        };

        // By model
        let mut model_map: HashMap<String, (i64, f64, i64, Vec<f32>)> = HashMap::new();
        for e in &events {
            let entry = model_map.entry(e.model.clone()).or_default();
            entry.0 += 1;
            entry.1 += e.duration_ms as f64;
            entry.2 += e.tokens_total.unwrap_or(0) as i64;
            if let Some(s) = e.overall_score {
                entry.3.push(s);
            }
        }
        let by_model: Vec<ModelStats> = model_map
            .into_iter()
            .map(|(model, (count, dur, tok, scores))| {
                let avg_s = if scores.is_empty() {
                    None
                } else {
                    Some(scores.iter().sum::<f32>() as f64 / scores.len() as f64)
                };
                ModelStats {
                    model,
                    count,
                    avg_duration_ms: dur / count as f64,
                    total_tokens: tok,
                    avg_score: avg_s,
                }
            })
            .collect();

        // By source
        let mut source_map: HashMap<String, i64> = HashMap::new();
        for e in &events {
            *source_map.entry(e.diff_source.clone()).or_default() += 1;
        }
        let by_source: Vec<SourceStats> = source_map
            .into_iter()
            .map(|(source, count)| SourceStats { source, count })
            .collect();

        // By repo
        let mut repo_map: HashMap<String, (i64, Vec<f32>)> = HashMap::new();
        for e in &events {
            if let Some(ref repo) = e.github_repo {
                let entry = repo_map.entry(repo.clone()).or_default();
                entry.0 += 1;
                if let Some(s) = e.overall_score {
                    entry.1.push(s);
                }
            }
        }
        let by_repo: Vec<RepoStats> = repo_map
            .into_iter()
            .map(|(repo, (count, scores))| {
                let avg_s = if scores.is_empty() {
                    None
                } else {
                    Some(scores.iter().sum::<f32>() as f64 / scores.len() as f64)
                };
                RepoStats {
                    repo,
                    count,
                    avg_score: avg_s,
                }
            })
            .collect();

        // Severity totals
        let mut severity_totals: HashMap<String, i64> = HashMap::new();
        for e in &events {
            for (k, v) in &e.comments_by_severity {
                *severity_totals.entry(k.clone()).or_default() += *v as i64;
            }
        }

        // Category totals
        let mut category_totals: HashMap<String, i64> = HashMap::new();
        for e in &events {
            for (k, v) in &e.comments_by_category {
                *category_totals.entry(k.clone()).or_default() += *v as i64;
            }
        }

        // Daily counts (group by created_at date)
        let mut daily_map: HashMap<String, (i64, i64)> = HashMap::new();
        for e in &events {
            if let Some(created) = e.created_at {
                let date_str = created.format("%Y-%m-%d").to_string();
                let entry = daily_map.entry(date_str).or_insert((0, 0));
                if e.event_type == "review.completed" {
                    entry.0 += 1;
                } else if e.event_type == "review.failed" {
                    entry.1 += 1;
                }
            }
        }
        let mut daily_counts: Vec<super::storage::DailyCount> = daily_map
            .into_iter()
            .map(|(date, (completed, failed))| super::storage::DailyCount {
                date,
                completed,
                failed,
            })
            .collect();
        daily_counts.sort_by(|a, b| a.date.cmp(&b.date));

        let total_cost_estimate: f64 = events.iter().filter_map(|e| e.cost_estimate_usd).sum();

        Ok(EventStats {
            total_reviews: total,
            completed_count: completed,
            failed_count: failed,
            total_tokens,
            avg_duration_ms,
            avg_score,
            error_rate,
            p50_latency_ms: percentile(50.0),
            p95_latency_ms: percentile(95.0),
            p99_latency_ms: percentile(99.0),
            by_model,
            by_source,
            by_repo,
            severity_totals,
            category_totals,
            daily_counts,
            total_cost_estimate,
        })
    }

    async fn update_comment_feedback(
        &self,
        review_id: &str,
        comment_id: &str,
        feedback: &str,
    ) -> anyhow::Result<()> {
        let mut reviews = self.reviews.write().await;
        if let Some(session) = reviews.get_mut(review_id) {
            if let Some(comment) = session.comments.iter_mut().find(|c| c.id == comment_id) {
                comment.feedback = Some(feedback.to_string());
            }
        }
        drop(reviews);
        self.flush_to_disk().await;
        Ok(())
    }

    async fn prune(&self, max_age_secs: i64, max_count: usize) -> anyhow::Result<usize> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.prune_at(max_age_secs, max_count, now).await
    }
}

impl JsonStorageBackend {
    /// Prune with a fixed "now" timestamp. Used by the trait implementation and by tests to avoid race conditions.
    pub(crate) async fn prune_at(
        &self,
        max_age_secs: i64,
        max_count: usize,
        now_secs: i64,
    ) -> anyhow::Result<usize> {
        let removed = {
            let mut reviews = self.reviews.write().await;

            let expired: Vec<String> = reviews
                .iter()
                .filter(|(_, r)| now_secs - r.started_at > max_age_secs)
                .map(|(id, _)| id.clone())
                .collect();
            let mut removed = expired.len();
            for id in &expired {
                reviews.remove(id);
            }

            if reviews.len() > max_count {
                let mut completed: Vec<(String, i64)> = reviews
                    .iter()
                    .filter(|(_, r)| {
                        r.status == ReviewStatus::Complete || r.status == ReviewStatus::Failed
                    })
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
        }; // write lock dropped here

        if removed > 0 {
            self.flush_to_disk().await;
        }

        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, Comment, FixEffort, ReviewSummary, Severity};
    use crate::server::state::ReviewEventBuilder;
    use crate::server::storage::EventFilters;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Create a minimal ReviewSession for testing.
    fn make_session(id: &str, started_at: i64, status: ReviewStatus) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status,
            diff_source: "head".to_string(),
            github_head_sha: None,
            github_post_results_requested: None,
            started_at,
            completed_at: None,
            comments: vec![],
            summary: None,
            files_reviewed: 0,
            error: None,
            pr_summary_text: None,
            diff_content: Some("diff content".to_string()),
            event: None,
            progress: None,
        }
    }

    /// Create a ReviewSession with an attached ReviewEvent.
    fn make_session_with_event(
        id: &str,
        started_at: i64,
        status: ReviewStatus,
        event_type: &str,
        model: &str,
        diff_source: &str,
        duration_ms: u64,
    ) -> ReviewSession {
        let event = ReviewEventBuilder::new(id, event_type, diff_source, model)
            .duration_ms(duration_ms)
            .build();
        let mut session = make_session(id, started_at, status);
        session.diff_source = diff_source.to_string();
        session.event = Some(event);
        session
    }

    /// Create a Comment for testing.
    fn make_comment(id: &str, file: &str) -> Comment {
        Comment {
            id: id.to_string(),
            file_path: PathBuf::from(file),
            line_number: 1,
            content: "test comment".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    /// Helper to get the current Unix timestamp.
    fn now_ts() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    // ---------------------------------------------------------------
    // 1. save_review + get_review round-trip
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_save_and_get_review_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let session = make_session("r1", now_ts(), ReviewStatus::Complete);
        backend.save_review(&session).await.unwrap();

        let loaded = backend.get_review("r1").await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, "r1");
        assert_eq!(loaded.status, ReviewStatus::Complete);
        assert_eq!(loaded.diff_source, "head");
    }

    #[tokio::test]
    async fn test_get_review_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let result = backend.get_review("does-not-exist").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_save_review_with_comments_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut session = make_session("r-full", now_ts(), ReviewStatus::Complete);
        session.comments = vec![
            make_comment("c1", "src/main.rs"),
            make_comment("c2", "src/lib.rs"),
        ];
        session.comments[1].status = crate::core::comment::CommentStatus::Resolved;
        session.comments[1].resolved_at = Some(123);
        session.summary = Some(ReviewSummary {
            total_comments: 2,
            by_severity: HashMap::from([("Warning".to_string(), 2)]),
            by_category: HashMap::from([("Bug".to_string(), 2)]),
            critical_issues: 0,
            files_reviewed: 2,
            overall_score: 8.0,
            recommendations: vec!["Fix bugs".to_string()],
            open_comments: 2,
            open_by_severity: HashMap::from([("Warning".to_string(), 2)]),
            open_blocking_comments: 2,
            open_informational_comments: 0,
            resolved_comments: 0,
            dismissed_comments: 0,
            open_blockers: 2,
            completeness: crate::core::comment::ReviewCompletenessSummary {
                total_findings: 2,
                acknowledged_findings: 0,
                fixed_findings: 0,
                stale_findings: 0,
            },
            merge_readiness: crate::core::comment::MergeReadiness::NeedsAttention,
            verification: crate::core::comment::ReviewVerificationSummary::default(),
            readiness_reasons: Vec::new(),
        });

        backend.save_review(&session).await.unwrap();
        let loaded = backend.get_review("r-full").await.unwrap().unwrap();
        assert_eq!(loaded.comments.len(), 2);
        assert_eq!(loaded.comments[1].resolved_at, Some(123));
        assert!(loaded.summary.is_some());
        assert_eq!(loaded.summary.unwrap().overall_score, 8.0);
    }

    // ---------------------------------------------------------------
    // 2. list_reviews — ordering, limit, offset
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_list_reviews_ordering_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let base = now_ts();
        for i in 0..5 {
            let session = make_session(&format!("r{i}"), base + i as i64, ReviewStatus::Complete);
            backend.save_review(&session).await.unwrap();
        }

        let list = backend.list_reviews(10, 0).await.unwrap();
        assert_eq!(list.len(), 5);
        // Should be sorted newest first (descending started_at)
        assert_eq!(list[0].id, "r4");
        assert_eq!(list[4].id, "r0");
    }

    #[tokio::test]
    async fn test_list_reviews_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let base = now_ts();
        for i in 0..5 {
            let session = make_session(&format!("r{i}"), base + i as i64, ReviewStatus::Complete);
            backend.save_review(&session).await.unwrap();
        }

        let list = backend.list_reviews(3, 0).await.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].id, "r4");
        assert_eq!(list[2].id, "r2");
    }

    #[tokio::test]
    async fn test_list_reviews_with_offset() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let base = now_ts();
        for i in 0..5 {
            let session = make_session(&format!("r{i}"), base + i as i64, ReviewStatus::Complete);
            backend.save_review(&session).await.unwrap();
        }

        let list = backend.list_reviews(10, 2).await.unwrap();
        assert_eq!(list.len(), 3);
        // Skipped r4, r3 (offset=2), so first result is r2
        assert_eq!(list[0].id, "r2");
        assert_eq!(list[2].id, "r0");
    }

    #[tokio::test]
    async fn test_list_reviews_empty() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let list = backend.list_reviews(10, 0).await.unwrap();
        assert!(list.is_empty());
    }

    // ---------------------------------------------------------------
    // 3. delete_review
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_delete_review() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let session = make_session("r-del", now_ts(), ReviewStatus::Complete);
        backend.save_review(&session).await.unwrap();
        assert!(backend.get_review("r-del").await.unwrap().is_some());

        backend.delete_review("r-del").await.unwrap();
        assert!(backend.get_review("r-del").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_review_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        // Should not panic or error
        backend.delete_review("ghost").await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_does_not_affect_others() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let base = now_ts();
        backend
            .save_review(&make_session("keep", base, ReviewStatus::Complete))
            .await
            .unwrap();
        backend
            .save_review(&make_session("remove", base + 1, ReviewStatus::Complete))
            .await
            .unwrap();

        backend.delete_review("remove").await.unwrap();
        assert!(backend.get_review("keep").await.unwrap().is_some());
        assert!(backend.get_review("remove").await.unwrap().is_none());
    }

    // ---------------------------------------------------------------
    // 4. prune
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_prune_removes_old_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        // Old review: 2 hours ago
        backend
            .save_review(&make_session("old", now - 7200, ReviewStatus::Complete))
            .await
            .unwrap();
        // Recent review: just now
        backend
            .save_review(&make_session("new", now, ReviewStatus::Complete))
            .await
            .unwrap();

        // Prune anything older than 1 hour, keep at most 100
        let removed = backend.prune(3600, 100).await.unwrap();
        assert_eq!(removed, 1);
        assert!(backend.get_review("old").await.unwrap().is_none());
        assert!(backend.get_review("new").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_prune_enforces_max_count() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        // All recent, but exceed max_count of 2
        for i in 0..5 {
            backend
                .save_review(&make_session(
                    &format!("r{i}"),
                    now + i as i64,
                    ReviewStatus::Complete,
                ))
                .await
                .unwrap();
        }

        // max_age very large (so nothing expires by age), max_count = 2
        let removed = backend.prune(999999, 2).await.unwrap();
        assert_eq!(removed, 3);
        let remaining = backend.list_reviews(100, 0).await.unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[tokio::test]
    async fn test_prune_preserves_running_reviews_when_enforcing_count() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        // One running review (oldest)
        backend
            .save_review(&make_session("running", now, ReviewStatus::Running))
            .await
            .unwrap();
        // Two completed reviews (newer)
        backend
            .save_review(&make_session("done1", now + 1, ReviewStatus::Complete))
            .await
            .unwrap();
        backend
            .save_review(&make_session("done2", now + 2, ReviewStatus::Complete))
            .await
            .unwrap();

        // max_count=1 -- prune should remove completed, but Running is not
        // eligible for the count-based pass (only Complete/Failed are pruned)
        let removed = backend.prune(999999, 1).await.unwrap();
        // 2 completed reviews removed to bring total towards max_count
        assert_eq!(removed, 2);
        // Running review should still be present
        assert!(backend.get_review("running").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_prune_empty_storage() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let removed = backend.prune(3600, 100).await.unwrap();
        assert_eq!(removed, 0);
    }

    // ---------------------------------------------------------------
    // 5. save_event + list_events
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_list_events_returns_events_from_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        let s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            1000,
        );
        let s2 = make_session_with_event(
            "r2",
            now + 1,
            ReviewStatus::Failed,
            "review.failed",
            "claude-sonnet-4.6",
            "staged",
            2000,
        );
        backend.save_review(&s1).await.unwrap();
        backend.save_review(&s2).await.unwrap();

        let events = backend.list_events(&EventFilters::default()).await.unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_list_events_filter_by_source() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                1000,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r2",
                now + 1,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "staged",
                1000,
            ))
            .await
            .unwrap();

        let filters = EventFilters {
            source: Some("head".to_string()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].diff_source, "head");
    }

    #[tokio::test]
    async fn test_list_events_filter_by_model() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                1000,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r2",
                now + 1,
                ReviewStatus::Complete,
                "review.completed",
                "claude-sonnet-4.6",
                "head",
                2000,
            ))
            .await
            .unwrap();

        let filters = EventFilters {
            model: Some("claude-sonnet-4.6".to_string()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].model, "claude-sonnet-4.6");
    }

    #[tokio::test]
    async fn test_list_events_filter_by_status() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                1000,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r2",
                now + 1,
                ReviewStatus::Failed,
                "review.failed",
                "gpt-4o",
                "head",
                500,
            ))
            .await
            .unwrap();

        let filters = EventFilters {
            status: Some("failed".to_string()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "review.failed");
    }

    #[tokio::test]
    async fn test_list_events_with_limit_and_offset() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        for i in 0..5 {
            backend
                .save_review(&make_session_with_event(
                    &format!("r{i}"),
                    now + i as i64,
                    ReviewStatus::Complete,
                    "review.completed",
                    "gpt-4o",
                    "head",
                    (i as u64 + 1) * 100,
                ))
                .await
                .unwrap();
        }

        let filters = EventFilters {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_list_events_empty() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let events = backend.list_events(&EventFilters::default()).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_save_event_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        // save_event for JSON backend is a no-op (events are embedded in sessions)
        let event = ReviewEventBuilder::new("r1", "review.completed", "head", "gpt-4o").build();
        let result = backend.save_event(&event).await;
        assert!(result.is_ok());

        // No events should be listed because no session holds this event
        let events = backend.list_events(&EventFilters::default()).await.unwrap();
        assert!(events.is_empty());
    }

    // ---------------------------------------------------------------
    // 6. get_event_stats
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_get_event_stats_basic_aggregation() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();

        // Two completed reviews with different models
        let mut s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            1000,
        );
        if let Some(ref mut e) = s1.event {
            e.tokens_total = Some(500);
            e.overall_score = Some(8.5);
            e.comments_by_severity.insert("Warning".to_string(), 2);
        }

        let mut s2 = make_session_with_event(
            "r2",
            now + 1,
            ReviewStatus::Complete,
            "review.completed",
            "claude-sonnet-4.6",
            "staged",
            3000,
        );
        if let Some(ref mut e) = s2.event {
            e.tokens_total = Some(1000);
            e.overall_score = Some(7.0);
            e.comments_by_category.insert("Bug".to_string(), 3);
        }

        // One failed review
        let mut s3 = make_session_with_event(
            "r3",
            now + 2,
            ReviewStatus::Failed,
            "review.failed",
            "gpt-4o",
            "head",
            500,
        );
        if let Some(ref mut e) = s3.event {
            e.tokens_total = Some(100);
        }

        backend.save_review(&s1).await.unwrap();
        backend.save_review(&s2).await.unwrap();
        backend.save_review(&s3).await.unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.total_reviews, 3);
        assert_eq!(stats.completed_count, 2);
        assert_eq!(stats.failed_count, 1);
        assert_eq!(stats.total_tokens, 1600); // 500 + 1000 + 100
        assert!(stats.avg_duration_ms > 0.0);
        assert!(stats.avg_score.is_some());
        assert!(stats.error_rate > 0.0);

        // Verify by_source has both "head" and "staged"
        assert_eq!(stats.by_source.len(), 2);

        // Verify severity_totals and category_totals
        assert_eq!(*stats.severity_totals.get("Warning").unwrap_or(&0), 2);
        assert_eq!(*stats.category_totals.get("Bug").unwrap_or(&0), 3);
    }

    #[tokio::test]
    async fn test_get_event_stats_empty() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.total_reviews, 0);
        assert_eq!(stats.completed_count, 0);
        assert_eq!(stats.failed_count, 0);
        assert_eq!(stats.total_tokens, 0);
        assert_eq!(stats.avg_duration_ms, 0.0);
        assert!(stats.avg_score.is_none());
        assert_eq!(stats.error_rate, 0.0);
        assert_eq!(stats.p50_latency_ms, 0);
        assert_eq!(stats.p95_latency_ms, 0);
        assert_eq!(stats.p99_latency_ms, 0);
        assert!(stats.by_model.is_empty());
        assert!(stats.by_source.is_empty());
        assert!(stats.by_repo.is_empty());
        assert!(stats.daily_counts.is_empty());
        assert_eq!(stats.total_cost_estimate, 0.0);
    }

    /// Single event: percentiles and avg must equal that single value (catches percentile/division mutants).
    #[tokio::test]
    async fn test_get_event_stats_single_event_exact_values() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut s = make_session_with_event(
            "r1",
            now_ts(),
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            150,
        );
        s.event.as_mut().unwrap().overall_score = Some(7.0);
        backend.save_review(&s).await.unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.total_reviews, 1);
        assert_eq!(stats.avg_duration_ms, 150.0);
        assert_eq!(stats.p50_latency_ms, 150, "single value is p50/p95/p99");
        assert_eq!(stats.p95_latency_ms, 150);
        assert_eq!(stats.p99_latency_ms, 150);
        assert_eq!(stats.avg_score, Some(7.0));
    }

    /// By-repo avg_score: two events same repo (4.0 + 6.0) / 2 = 5.0.
    #[tokio::test]
    async fn test_get_event_stats_by_repo_avg_score_exact() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        for (i, &score) in [4.0_f32, 6.0_f32].iter().enumerate() {
            let mut s = make_session_with_event(
                &format!("r{}", i),
                now + i as i64,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                100,
            );
            if let Some(ref mut e) = s.event {
                e.github_repo = Some("org/same-repo".to_string());
                e.overall_score = Some(score);
            }
            backend.save_review(&s).await.unwrap();
        }

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.by_repo.len(), 1);
        assert_eq!(stats.by_repo[0].repo, "org/same-repo");
        assert_eq!(stats.by_repo[0].count, 2);
        assert_eq!(stats.by_repo[0].avg_score, Some(5.0), " (4+6)/2 ");
    }

    #[tokio::test]
    async fn test_get_event_stats_latency_percentiles() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        // Create 10 reviews with increasing durations
        for i in 0..10 {
            backend
                .save_review(&make_session_with_event(
                    &format!("r{i}"),
                    now + i as i64,
                    ReviewStatus::Complete,
                    "review.completed",
                    "gpt-4o",
                    "head",
                    (i as u64 + 1) * 100, // 100, 200, ..., 1000
                ))
                .await
                .unwrap();
        }

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert!(stats.p50_latency_ms > 0);
        assert!(stats.p95_latency_ms >= stats.p50_latency_ms);
        assert!(stats.p99_latency_ms >= stats.p95_latency_ms);
    }

    #[tokio::test]
    async fn test_get_event_stats_by_model() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                1000,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r2",
                now + 1,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                2000,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r3",
                now + 2,
                ReviewStatus::Complete,
                "review.completed",
                "claude-sonnet-4.6",
                "head",
                500,
            ))
            .await
            .unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.by_model.len(), 2);
        let gpt_stats = stats.by_model.iter().find(|m| m.model == "gpt-4o").unwrap();
        assert_eq!(gpt_stats.count, 2);
        assert_eq!(gpt_stats.avg_duration_ms, 1500.0); // (1000+2000)/2
    }

    #[tokio::test]
    async fn test_get_event_stats_by_repo() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        let mut s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            1000,
        );
        if let Some(ref mut e) = s1.event {
            e.github_repo = Some("owner/repo".to_string());
            e.overall_score = Some(9.0);
        }
        backend.save_review(&s1).await.unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.by_repo.len(), 1);
        assert_eq!(stats.by_repo[0].repo, "owner/repo");
        assert_eq!(stats.by_repo[0].count, 1);
        assert_eq!(stats.by_repo[0].avg_score, Some(9.0_f64));
    }

    #[tokio::test]
    async fn test_get_event_stats_daily_counts_and_total_cost() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let day1 = chrono::NaiveDate::from_ymd_opt(2025, 3, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_utc();
        let day2 = chrono::NaiveDate::from_ymd_opt(2025, 3, 2)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap()
            .and_utc();

        let mut s1 = make_session_with_event(
            "r1",
            0,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            100,
        );
        if let Some(ref mut e) = s1.event {
            e.created_at = Some(day1);
            e.cost_estimate_usd = Some(1.5);
        }
        let mut s2 = make_session_with_event(
            "r2",
            1,
            ReviewStatus::Failed,
            "review.failed",
            "gpt-4o",
            "head",
            200,
        );
        if let Some(ref mut e) = s2.event {
            e.created_at = Some(day1);
            e.cost_estimate_usd = Some(0.5);
        }
        let mut s3 = make_session_with_event(
            "r3",
            2,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "staged",
            300,
        );
        if let Some(ref mut e) = s3.event {
            e.created_at = Some(day2);
            e.cost_estimate_usd = Some(2.0);
        }

        backend.save_review(&s1).await.unwrap();
        backend.save_review(&s2).await.unwrap();
        backend.save_review(&s3).await.unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.daily_counts.len(), 2, "expected two distinct days");
        let day1_entry = stats
            .daily_counts
            .iter()
            .find(|d| d.date == "2025-03-01")
            .unwrap();
        assert_eq!(day1_entry.completed, 1);
        assert_eq!(day1_entry.failed, 1);
        let day2_entry = stats
            .daily_counts
            .iter()
            .find(|d| d.date == "2025-03-02")
            .unwrap();
        assert_eq!(day2_entry.completed, 1);
        assert_eq!(day2_entry.failed, 0);

        assert_eq!(stats.total_cost_estimate, 4.0, "1.5 + 0.5 + 2.0");
    }

    /// Exact aggregate values so mutations in get_event_stats (avg, percentile formula) are caught.
    #[tokio::test]
    async fn test_get_event_stats_exact_aggregates() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        for (i, &dur) in [100_u64, 200, 300].iter().enumerate() {
            let mut s = make_session_with_event(
                &format!("r{}", i),
                now + i as i64,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                dur,
            );
            if let Some(ref mut e) = s.event {
                e.overall_score = Some((i as f32 + 1.0) * 2.0); // 2.0, 4.0, 6.0 -> avg 4.0
            }
            backend.save_review(&s).await.unwrap();
        }

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();

        assert_eq!(stats.total_reviews, 3);
        assert_eq!(stats.avg_duration_ms, 200.0, " (100+200+300)/3 ");
        assert_eq!(
            stats.p50_latency_ms, 200,
            " percentile 50 of [100,200,300] "
        );
        assert_eq!(stats.p95_latency_ms, 300, " percentile 95 ");
        assert_eq!(stats.p99_latency_ms, 300, " percentile 99 ");
        assert_eq!(stats.avg_score, Some(4.0), " (2+4+6)/3 ");
        assert_eq!(stats.by_model.len(), 1);
        assert_eq!(stats.by_model[0].model, "gpt-4o");
        assert_eq!(stats.by_model[0].count, 3);
        assert_eq!(stats.by_model[0].avg_duration_ms, 200.0);
    }

    /// Percentile index formula: (p/100)*(len-1).round(). With 5 durations [10,20,30,40,50], p50 index=2 → 30, p95 index=4 → 50.
    #[tokio::test]
    async fn test_get_event_stats_percentile_index_formula() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();
        for (i, &dur) in [10_u64, 20, 30, 40, 50].iter().enumerate() {
            let s = make_session_with_event(
                &format!("r{}", i),
                now + i as i64,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                dur,
            );
            backend.save_review(&s).await.unwrap();
        }
        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();
        assert_eq!(stats.p50_latency_ms, 30, "p50 index (0.5*4).round()=2 → 30");
        assert_eq!(
            stats.p95_latency_ms, 50,
            "p95 index (0.95*4).round()=4 → 50"
        );
    }

    /// By-repo count: entry.0 += 1 per event. Two events same repo → count 2.
    #[tokio::test]
    async fn test_get_event_stats_by_repo_count_two_events() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();
        for i in 0..2 {
            let mut s = make_session_with_event(
                &format!("r{}", i),
                now + i as i64,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "head",
                100,
            );
            if let Some(ref mut e) = s.event {
                e.github_repo = Some("org/repo".to_string());
            }
            backend.save_review(&s).await.unwrap();
        }
        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();
        assert_eq!(stats.by_repo.len(), 1);
        assert_eq!(stats.by_repo[0].repo, "org/repo");
        assert_eq!(stats.by_repo[0].count, 2);
    }

    /// Prune max_count: exactly one over limit (4 reviews, max_count=3) → remove 1, keep 3.
    #[tokio::test]
    async fn test_prune_max_count_exactly_one_over() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let base = now_ts() - 10_000;
        for i in 0..4 {
            let s = make_session(&format!("r{}", i), base + i as i64, ReviewStatus::Complete);
            backend.save_review(&s).await.unwrap();
        }
        let removed = backend.prune_at(1_000_000, 3, base + 10).await.unwrap();
        assert_eq!(removed, 1, "4 - 3 = 1 removed");
        let list = backend.list_reviews(10, 0).await.unwrap();
        assert_eq!(list.len(), 3);
    }

    /// refresh_summary: get_review with comments but no summary produces synthesized summary.
    #[tokio::test]
    async fn test_get_review_refreshes_summary_when_comments_no_summary() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut session = make_session("ref-sum", now_ts(), ReviewStatus::Complete);
        session.comments = vec![
            make_comment("c1", "src/a.rs"),
            make_comment("c2", "src/b.rs"),
        ];
        session.summary = None;
        backend.save_review(&session).await.unwrap();

        let loaded = backend.get_review("ref-sum").await.unwrap().unwrap();
        assert!(
            loaded.summary.is_some(),
            "refresh_summary should synthesize summary when comments exist"
        );
        let sum = loaded.summary.unwrap();
        assert_eq!(sum.total_comments, 2);
    }

    /// refresh_summary: list_reviews returns sessions with refreshed summary when comments exist.
    #[tokio::test]
    async fn test_list_reviews_refreshes_summary_for_sessions_with_comments() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut with_comments = make_session("with-c", now_ts(), ReviewStatus::Complete);
        with_comments.comments = vec![make_comment("c1", "src/main.rs")];
        with_comments.summary = None;
        backend.save_review(&with_comments).await.unwrap();

        backend
            .save_review(&make_session("no-c", now_ts() - 1, ReviewStatus::Complete))
            .await
            .unwrap();

        let list = backend.list_reviews(10, 0).await.unwrap();
        let with_c = list.iter().find(|s| s.id == "with-c").unwrap();
        assert!(
            with_c.summary.is_some(),
            "list_reviews should refresh summary for session with comments"
        );
    }

    /// Prune age boundary: review with started_at exactly (now - max_age_secs) must NOT be expired (> not >=).
    /// Uses prune_at with a single timestamp to avoid race (Sentry feedback).
    #[tokio::test]
    async fn test_prune_age_boundary_not_expired() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        let max_age = 100_i64;
        let session = make_session("boundary", now - max_age, ReviewStatus::Complete);
        backend.save_review(&session).await.unwrap();

        let removed = backend.prune_at(max_age, 1000, now).await.unwrap();
        assert_eq!(
            removed, 0,
            "exactly at boundary (now - max_age) should not be pruned"
        );

        let list = backend.list_reviews(10, 0).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "boundary");
    }

    /// Prune max_count: keep newest max_count completed, remove oldest.
    #[tokio::test]
    async fn test_prune_max_count_removes_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let base = now_ts() - 10_000;
        for i in 0..5 {
            let s = make_session(&format!("r{}", i), base + i as i64, ReviewStatus::Complete);
            backend.save_review(&s).await.unwrap();
        }

        let removed = backend.prune(1_000_000, 3).await.unwrap();
        assert_eq!(removed, 2, "5 - 3 = 2 removed by count limit");

        let list = backend.list_reviews(10, 0).await.unwrap();
        assert_eq!(list.len(), 3);
        let ids: Vec<_> = list.iter().map(|s| s.id.as_str()).collect();
        let want = ["r2", "r3", "r4"];
        assert!(
            want.iter().all(|w| ids.contains(w)),
            "newest 3 (r2,r3,r4) kept; r0,r1 removed, got {:?}",
            ids
        );
    }

    /// time_to filter: event with created_at == time_to must be included (<=).
    #[tokio::test]
    async fn test_list_events_time_to_inclusive() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let t = chrono::Utc::now();
        let mut s = make_session_with_event(
            "r1",
            0,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "head",
            100,
        );
        s.event.as_mut().unwrap().created_at = Some(t);
        backend.save_review(&s).await.unwrap();

        let filters = EventFilters {
            time_from: Some(t - chrono::Duration::hours(1)),
            time_to: Some(t),
            ..EventFilters::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1, "event at exactly time_to must be included");
    }

    // ---------------------------------------------------------------
    // 7. is_empty (via internal state check)
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_empty_storage_has_no_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let list = backend.list_reviews(100, 0).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_non_empty_after_save() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        backend
            .save_review(&make_session("r1", now_ts(), ReviewStatus::Pending))
            .await
            .unwrap();
        let list = backend.list_reviews(100, 0).await.unwrap();
        assert!(!list.is_empty());
    }

    // ---------------------------------------------------------------
    // 8. Edge cases
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_duplicate_id_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut session = make_session("dup", now_ts(), ReviewStatus::Pending);
        backend.save_review(&session).await.unwrap();

        // Save again with same ID but different status
        session.status = ReviewStatus::Complete;
        session.files_reviewed = 5;
        backend.save_review(&session).await.unwrap();

        let loaded = backend.get_review("dup").await.unwrap().unwrap();
        assert_eq!(loaded.status, ReviewStatus::Complete);
        assert_eq!(loaded.files_reviewed, 5);

        // Should still be just one review
        let list = backend.list_reviews(100, 0).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_persistence_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reviews.json");

        // First instance: save a review
        {
            let backend = JsonStorageBackend::new(&path);
            backend
                .save_review(&make_session("persist", now_ts(), ReviewStatus::Complete))
                .await
                .unwrap();
        }

        // Second instance: load from the same file
        {
            let backend = JsonStorageBackend::new(&path);
            let loaded = backend.get_review("persist").await.unwrap();
            assert!(loaded.is_some());
            assert_eq!(loaded.unwrap().id, "persist");
        }
    }

    #[tokio::test]
    async fn test_diff_content_stripped_on_flush() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reviews.json");

        // Save a review that has diff_content set
        {
            let backend = JsonStorageBackend::new(&path);
            let mut session = make_session("strip", now_ts(), ReviewStatus::Complete);
            session.diff_content = Some("big diff content here".to_string());
            backend.save_review(&session).await.unwrap();

            // In-memory version still has diff_content
            let in_mem = backend.get_review("strip").await.unwrap().unwrap();
            assert!(in_mem.diff_content.is_some());
        }

        // Reload from disk: diff_content should be None (stripped during flush)
        {
            let backend = JsonStorageBackend::new(&path);
            let loaded = backend.get_review("strip").await.unwrap().unwrap();
            assert!(loaded.diff_content.is_none());
        }
    }

    #[tokio::test]
    async fn test_update_comment_feedback() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let mut session = make_session("fb", now_ts(), ReviewStatus::Complete);
        session.comments = vec![make_comment("c1", "src/main.rs")];
        backend.save_review(&session).await.unwrap();

        backend
            .update_comment_feedback("fb", "c1", "helpful")
            .await
            .unwrap();

        let loaded = backend.get_review("fb").await.unwrap().unwrap();
        assert_eq!(loaded.comments[0].feedback.as_deref(), Some("helpful"));
    }

    #[tokio::test]
    async fn test_update_comment_feedback_nonexistent_review() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        // Should not error, just no-op
        let result = backend
            .update_comment_feedback("ghost", "c1", "helpful")
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_comment_feedback_nonexistent_comment() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let session = make_session("fb2", now_ts(), ReviewStatus::Complete);
        backend.save_review(&session).await.unwrap();

        // Review exists but comment does not -- should not error
        let result = backend
            .update_comment_feedback("fb2", "nonexistent-comment", "helpful")
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_concurrent_writes() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            std::sync::Arc::new(JsonStorageBackend::new(&dir.path().join("reviews.json")));

        let base = now_ts();
        let mut handles = vec![];

        for i in 0..20 {
            let b = backend.clone();
            let handle = tokio::spawn(async move {
                let session = make_session(
                    &format!("conc-{i}"),
                    base + i as i64,
                    ReviewStatus::Complete,
                );
                b.save_review(&session).await.unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let list = backend.list_reviews(100, 0).await.unwrap();
        assert_eq!(list.len(), 20);
    }

    #[tokio::test]
    async fn test_load_from_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        // Point to a file that doesn't exist yet
        let backend = JsonStorageBackend::new(&dir.path().join("doesnt-exist.json"));

        let list = backend.list_reviews(100, 0).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_load_from_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reviews.json");

        // Write garbage to the file
        std::fs::write(&path, "{ not valid json !!!").unwrap();

        // Should not panic; falls back to empty map
        let backend = JsonStorageBackend::new(&path);
        let list = backend.list_reviews(100, 0).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_list_events_case_insensitive_filters() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        let now = now_ts();
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "GPT-4o",
                "HEAD",
                1000,
            ))
            .await
            .unwrap();

        // Filters use lowercase but source/model are uppercase -- should match
        let filters = EventFilters {
            source: Some("head".to_string()),
            model: Some("gpt-4o".to_string()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_sessions_without_events_excluded_from_list_events() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));

        // Session without an event
        backend
            .save_review(&make_session("no-event", now_ts(), ReviewStatus::Pending))
            .await
            .unwrap();

        let events = backend.list_events(&EventFilters::default()).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_error_rate_only_counts_explicit_failures() {
        // Regression: `failed = total - completed` used to misclassify non-failure events.
        // Events with type "review.started" or "review.timeout" are counted as failures.
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        // 1 completed, 1 failed, 1 timeout (not a failure per se)
        backend
            .save_review(&make_session_with_event(
                "r1",
                now,
                ReviewStatus::Complete,
                "review.completed",
                "gpt-4o",
                "github",
                100,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r2",
                now,
                ReviewStatus::Failed,
                "review.failed",
                "gpt-4o",
                "github",
                200,
            ))
            .await
            .unwrap();
        backend
            .save_review(&make_session_with_event(
                "r3",
                now,
                ReviewStatus::Failed,
                "review.timeout",
                "gpt-4o",
                "github",
                300,
            ))
            .await
            .unwrap();

        let stats = backend
            .get_event_stats(&EventFilters::default())
            .await
            .unwrap();
        // Only "review.failed" should count as a failure (1 out of 3).
        // "review.timeout" is a separate event type and should not inflate error_rate.
        let expected_error_rate = 1.0 / 3.0;
        assert!(
            (stats.error_rate - expected_error_rate).abs() < 0.01,
            "Error rate should be {:.2} (only explicit failures), got {:.2}",
            expected_error_rate,
            stats.error_rate
        );
    }

    #[tokio::test]
    async fn test_list_events_filter_by_github_repo() {
        // BUG: list_events doesn't check filters.github_repo
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        let mut s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "github",
            100,
        );
        s1.event.as_mut().unwrap().github_repo = Some("owner/repo-a".to_string());
        backend.save_review(&s1).await.unwrap();

        let mut s2 = make_session_with_event(
            "r2",
            now + 1,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "github",
            200,
        );
        s2.event.as_mut().unwrap().github_repo = Some("owner/repo-b".to_string());
        backend.save_review(&s2).await.unwrap();

        let filters = EventFilters {
            github_repo: Some("owner/repo-a".to_string()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(
            events.len(),
            1,
            "Should only return events for repo-a, got {}",
            events.len()
        );
        assert_eq!(events[0].github_repo.as_deref(), Some("owner/repo-a"));
    }

    // ── Bug: time filters include events with created_at = None ──────
    //
    // When a time_from or time_to filter is active, events whose
    // `created_at` is None should be *excluded* (they have no timestamp
    // to satisfy the constraint).  Previously `is_none_or` let them
    // through.

    #[tokio::test]
    async fn test_time_filter_excludes_events_without_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        // Event WITH a timestamp (via build())
        let s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "github",
            100,
        );
        backend.save_review(&s1).await.unwrap();

        // Event WITHOUT a timestamp (manually set created_at = None)
        let mut s2 = make_session_with_event(
            "r2",
            now + 1,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "github",
            200,
        );
        s2.event.as_mut().unwrap().created_at = None;
        backend.save_review(&s2).await.unwrap();

        // Filter with time_from = epoch (should match everything WITH a ts)
        let filters = EventFilters {
            time_from: Some(chrono::DateTime::from_timestamp(0, 0).unwrap()),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(
            events.len(),
            1,
            "Events with created_at = None should be excluded by time filters, got {}",
            events.len()
        );
        assert_eq!(events[0].review_id, "r1");
    }

    // ── Bug: negative limit/offset wraps to huge usize ───────────────
    //
    // Casting a negative i64 directly to usize wraps to a very large
    // number, causing list_reviews to skip/take billions of entries.

    #[tokio::test]
    async fn test_list_reviews_negative_offset_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        backend
            .save_review(&make_session("r1", now, ReviewStatus::Complete))
            .await
            .unwrap();

        // Negative offset and limit should not panic or return nonsense
        let result = backend.list_reviews(10, -1).await.unwrap();
        assert_eq!(result.len(), 1, "Negative offset should be clamped to 0");

        let result = backend.list_reviews(-1, 0).await.unwrap();
        assert!(
            result.is_empty(),
            "Negative limit should be clamped to 0, returning no results"
        );
    }

    #[tokio::test]
    async fn test_list_events_negative_offset_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        let s1 = make_session_with_event(
            "r1",
            now,
            ReviewStatus::Complete,
            "review.completed",
            "gpt-4o",
            "github",
            100,
        );
        backend.save_review(&s1).await.unwrap();

        let filters = EventFilters {
            offset: Some(-5),
            limit: Some(100),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert_eq!(events.len(), 1, "Negative offset should be clamped to 0");

        let filters = EventFilters {
            offset: Some(0),
            limit: Some(-10),
            ..Default::default()
        };
        let events = backend.list_events(&filters).await.unwrap();
        assert!(
            events.is_empty(),
            "Negative limit should be clamped to 0, returning no results"
        );
    }

    #[tokio::test]
    async fn test_prune_persists_to_disk() {
        // BUG: prune removes from memory but doesn't flush to disk
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reviews.json");
        let backend = JsonStorageBackend::new(&path);
        let now = now_ts();

        for i in 0..3 {
            backend
                .save_review(&make_session(
                    &format!("r{i}"),
                    now - 100000 + i as i64, // old enough to expire
                    ReviewStatus::Complete,
                ))
                .await
                .unwrap();
        }

        let removed = backend.prune(1000, 100).await.unwrap();
        assert_eq!(removed, 3);

        // Reload from disk — pruned reviews should be gone
        let backend2 = JsonStorageBackend::new(&path);
        let remaining = backend2.list_reviews(100, 0).await.unwrap();
        assert_eq!(
            remaining.len(),
            0,
            "Pruned reviews should not reappear after reload from disk, got {}",
            remaining.len()
        );
    }

    #[tokio::test]
    async fn test_get_event_stats_ignores_limit_offset() {
        // BUG: get_event_stats calls list_events which applies limit/offset,
        // so stats are computed on a truncated subset
        let dir = tempfile::tempdir().unwrap();
        let backend = JsonStorageBackend::new(&dir.path().join("reviews.json"));
        let now = now_ts();

        for i in 0..10 {
            backend
                .save_review(&make_session_with_event(
                    &format!("r{i}"),
                    now + i as i64,
                    ReviewStatus::Complete,
                    "review.completed",
                    "gpt-4o",
                    "head",
                    100,
                ))
                .await
                .unwrap();
        }

        let filters = EventFilters {
            limit: Some(3),
            ..Default::default()
        };
        let stats = backend.get_event_stats(&filters).await.unwrap();
        assert_eq!(
            stats.total_reviews, 10,
            "Stats should cover all 10 events regardless of limit, got {}",
            stats.total_reviews
        );
    }
}
