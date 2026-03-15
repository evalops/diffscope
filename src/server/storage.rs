use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::state::{ReviewEvent, ReviewSession};

/// Filters for querying events
#[derive(Debug, Default, Clone, Deserialize)]
pub struct EventFilters {
    pub source: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub time_from: Option<chrono::DateTime<chrono::Utc>>,
    pub time_to: Option<chrono::DateTime<chrono::Utc>>,
    pub github_repo: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Aggregated statistics returned by the stats endpoint
#[derive(Debug, Default, Clone, Serialize)]
pub struct EventStats {
    pub total_reviews: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub total_tokens: i64,
    pub avg_duration_ms: f64,
    pub avg_score: Option<f64>,
    pub error_rate: f64,
    pub p50_latency_ms: i64,
    pub p95_latency_ms: i64,
    pub p99_latency_ms: i64,
    pub by_model: Vec<ModelStats>,
    pub by_source: Vec<SourceStats>,
    pub by_repo: Vec<RepoStats>,
    pub severity_totals: HashMap<String, i64>,
    pub category_totals: HashMap<String, i64>,
    pub daily_counts: Vec<DailyCount>,
    pub total_cost_estimate: f64,
    pub cost_breakdowns: Vec<crate::server::cost::CostBreakdownRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStats {
    pub model: String,
    pub count: i64,
    pub avg_duration_ms: f64,
    pub total_tokens: i64,
    pub avg_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceStats {
    pub source: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoStats {
    pub repo: String,
    pub count: i64,
    pub avg_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyCount {
    pub date: String,
    pub completed: i64,
    pub failed: i64,
}

/// Abstract storage backend - implemented by JSON file and PostgreSQL
#[async_trait]
pub trait StorageBackend: Send + Sync {
    // Reviews
    async fn save_review(&self, session: &ReviewSession) -> anyhow::Result<()>;
    async fn get_review(&self, id: &str) -> anyhow::Result<Option<ReviewSession>>;
    async fn list_reviews(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ReviewSession>>;
    async fn delete_review(&self, id: &str) -> anyhow::Result<()>;

    // Events
    async fn save_event(&self, event: &ReviewEvent) -> anyhow::Result<()>;
    async fn list_events(&self, filters: &EventFilters) -> anyhow::Result<Vec<ReviewEvent>>;
    async fn get_event_stats(&self, filters: &EventFilters) -> anyhow::Result<EventStats>;

    // Feedback
    async fn update_comment_feedback(
        &self,
        review_id: &str,
        comment_id: &str,
        feedback: &str,
    ) -> anyhow::Result<()>;

    // Lifecycle
    async fn prune(&self, max_age_secs: i64, max_count: usize) -> anyhow::Result<usize>;
}
