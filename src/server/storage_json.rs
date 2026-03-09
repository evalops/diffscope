use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;

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
                    eprintln!("Loaded {} reviews from disk", loaded.len());
                    loaded
                }
                Err(e) => {
                    eprintln!("Failed to parse reviews.json: {}", e);
                    HashMap::new()
                }
            },
            Err(e) => {
                eprintln!("Failed to read reviews.json: {}", e);
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
                    eprintln!("Failed to serialize reviews: {}", e);
                    return;
                }
            }
        };

        if let Some(parent) = self.storage_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                eprintln!("Failed to create storage directory: {}", e);
                return;
            }
        }
        let tmp_path = self.storage_path.with_extension("json.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
            eprintln!("Failed to write reviews temp file: {}", e);
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &self.storage_path).await {
            eprintln!("Failed to rename reviews file: {}", e);
            let _ = tokio::fs::remove_file(&tmp_path).await;
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
        Ok(reviews.get(id).cloned())
    }

    async fn list_reviews(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ReviewSession>> {
        let reviews = self.reviews.read().await;
        let mut list: Vec<&ReviewSession> = reviews.values().collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        let offset = offset as usize;
        let limit = limit as usize;
        Ok(list.into_iter().skip(offset).take(limit).cloned().collect())
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
                    .is_none_or(|f| e.event_type.eq_ignore_ascii_case(&format!("review.{}", f)));
                // Time filters (best-effort for JSON backend using created_at if available)
                let time_from_ok = filters
                    .time_from
                    .as_ref()
                    .is_none_or(|from| e.created_at.is_none_or(|t| t >= *from));
                let time_to_ok = filters
                    .time_to
                    .as_ref()
                    .is_none_or(|to| e.created_at.is_none_or(|t| t <= *to));
                source_ok && model_ok && status_ok && time_from_ok && time_to_ok
            })
            .collect();

        // Sort by created_at (newest first), falling back to review_id
        events.sort_by(|a, b| {
            let a_time = a.created_at.unwrap_or_default();
            let b_time = b.created_at.unwrap_or_default();
            b_time.cmp(&a_time).then(b.review_id.cmp(&a.review_id))
        });

        // Apply limit/offset
        let offset = filters.offset.unwrap_or(0) as usize;
        let limit = filters.limit.unwrap_or(500) as usize;
        events = events.into_iter().skip(offset).take(limit).collect();

        Ok(events)
    }

    async fn get_event_stats(&self, filters: &EventFilters) -> anyhow::Result<EventStats> {
        let events = self.list_events(filters).await?;

        let total = events.len() as i64;
        let completed = events
            .iter()
            .filter(|e| e.event_type == "review.completed")
            .count() as i64;
        let failed = total - completed;
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
            daily_counts: Vec::new(), // No date grouping for JSON backend
            total_cost_estimate: 0.0,
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
        let mut reviews = self.reviews.write().await;
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

        Ok(removed)
    }
}
