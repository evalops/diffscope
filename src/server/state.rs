use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::core::comment::{Comment, ReviewSummary};

use super::storage::StorageBackend;

/// Per-file review metric for the wide event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetricEvent {
    pub file_path: String,
    pub latency_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub comment_count: usize,
}

/// Serializable hotspot detail for the wide event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotDetail {
    pub file_path: String,
    pub risk_score: f32,
    pub reasons: Vec<String>,
}

/// Serializable agent tool call event for the wide event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCallEvent {
    pub iteration: usize,
    pub tool_name: String,
    pub duration_ms: u64,
}

/// A "wide event" capturing the full lifecycle of a single review operation.
/// Emitted once at completion as a single structured log entry and stored
/// alongside the review session for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewEvent {
    // --- identity ---
    pub review_id: String,
    pub event_type: String, // "review.completed" | "review.failed" | "review.timeout"

    // --- request ---
    pub diff_source: String,
    pub title: Option<String>,
    pub model: String,
    pub provider: Option<String>,
    pub base_url: Option<String>,

    // --- timing (ms) ---
    pub duration_ms: u64,
    pub diff_fetch_ms: Option<u64>,
    pub llm_total_ms: Option<u64>,

    // --- diff stats ---
    pub diff_bytes: usize,
    pub diff_files_total: usize,
    pub diff_files_reviewed: usize,
    pub diff_files_skipped: usize,

    // --- results ---
    pub comments_total: usize,
    pub comments_by_severity: HashMap<String, usize>,
    pub comments_by_category: HashMap<String, usize>,
    pub overall_score: Option<f32>,

    // --- ensemble / multi-pass ---
    pub hotspots_detected: usize,
    pub high_risk_files: usize,

    // --- token usage ---
    pub tokens_prompt: Option<usize>,
    pub tokens_completion: Option<usize>,
    pub tokens_total: Option<usize>,

    // --- cost (server-side estimate for stats / log pipelines) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_estimate_usd: Option<f64>,

    // --- per-file breakdown ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_metrics: Option<Vec<FileMetricEvent>>,

    // --- hotspot details ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_details: Option<Vec<HotspotDetail>>,

    // --- convention learning ---
    pub convention_suppressed: Option<usize>,

    // --- specialized pass breakdown ---
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub comments_by_pass: HashMap<String, usize>,

    // --- agent review ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_iterations: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_tool_calls: Option<Vec<AgentToolCallEvent>>,

    // --- GitHub integration ---
    pub github_posted: bool,
    pub github_repo: Option<String>,
    pub github_pr: Option<u32>,

    // --- errors ---
    pub error: Option<String>,

    // --- timestamp ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Maximum number of reviews to keep in memory. Oldest completed reviews are pruned.
const MAX_REVIEWS: usize = 200;

/// Reviews older than this (in seconds) are pruned regardless of status.
const MAX_REVIEW_AGE_SECS: i64 = 86_400; // 24 hours

/// Maximum allowed diff size in bytes (50 MB).
pub const MAX_DIFF_SIZE: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSession {
    pub id: String,
    pub status: ReviewStatus,
    pub diff_source: String,
    #[serde(default)]
    pub github_head_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_post_results_requested: Option<bool>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub comments: Vec<Comment>,
    pub summary: Option<ReviewSummary>,
    pub files_reviewed: usize,
    pub error: Option<String>,
    /// AI-generated PR summary markdown (when smart_review_summary is enabled).
    #[serde(default)]
    pub pr_summary_text: Option<String>,
    #[serde(default)]
    pub diff_content: Option<String>,
    #[serde(default)]
    pub event: Option<ReviewEvent>,
    #[serde(default)]
    pub progress: Option<ReviewProgress>,
}

/// Live progress tracking for a running review.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewProgress {
    pub current_file: Option<String>,
    pub files_total: usize,
    pub files_completed: usize,
    pub files_skipped: usize,
    pub elapsed_ms: u64,
    pub estimated_remaining_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReviewStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

/// Maximum number of concurrent reviews.
pub(crate) const MAX_CONCURRENT_REVIEWS: usize = 5;

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub repo_path: PathBuf,
    pub reviews: Arc<RwLock<HashMap<String, ReviewSession>>>,
    pub storage: Arc<dyn StorageBackend>,
    pub storage_path: PathBuf,
    pub config_path: PathBuf,
    /// Shared HTTP client for GitHub API and provider tests (connection pooling).
    pub http_client: reqwest::Client,
    /// Semaphore to limit concurrent review tasks.
    pub review_semaphore: Arc<tokio::sync::Semaphore>,
    /// Tracks the last reviewed head SHA per PR, keyed by "owner/repo#pr_number".
    /// Used for incremental (push-by-push) reviews.
    pub last_reviewed_shas: Arc<RwLock<HashMap<String, String>>>,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let repo_path = std::env::current_dir()?;
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diffscope");
        let storage_path = data_dir.join("reviews.json");
        let config_path = data_dir.join("config.json");

        // Initialize storage backend based on DATABASE_URL
        let storage: Arc<dyn StorageBackend> = if let Ok(db_url) = std::env::var("DATABASE_URL") {
            info!("Connecting to PostgreSQL...");
            let pool = sqlx::PgPool::connect(&db_url).await?;
            let pg = super::storage_pg::PgStorageBackend::new(pool);
            pg.migrate().await?;

            // Migrate existing JSON data if PG is empty
            if pg.is_empty().await? && storage_path.exists() {
                let json_reviews = Self::load_reviews_from_disk(&storage_path);
                if !json_reviews.is_empty() {
                    info!(
                        "Migrating {} reviews from JSON to PostgreSQL...",
                        json_reviews.len()
                    );
                    for session in json_reviews.values() {
                        if let Err(e) = pg.save_review(session).await {
                            tracing::warn!("Failed to migrate review {}: {}", session.id, e);
                        }
                        if let Some(ref event) = session.event {
                            let _ = pg.save_event(event).await;
                        }
                    }
                    info!("JSON to PostgreSQL migration complete");
                }
            }
            Arc::new(pg)
        } else {
            Arc::new(super::storage_json::JsonStorageBackend::new(&storage_path))
        };

        // Load persisted reviews for in-memory cache (active reviews / progress tracking)
        let reviews = Self::load_reviews_from_disk(&storage_path);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("diffscope")
            .pool_max_idle_per_host(5)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let state = Self {
            config: Arc::new(RwLock::new(config)),
            repo_path,
            reviews: Arc::new(RwLock::new(reviews)),
            storage,
            storage_path,
            config_path,
            http_client,
            review_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REVIEWS)),
            last_reviewed_shas: Arc::new(RwLock::new(HashMap::new())),
        };

        Ok(state)
    }

    /// Serialize reviews under the read lock, then write to disk after releasing it.
    /// The serialization happens atomically under the lock so the snapshot is consistent.
    pub fn save_reviews_async(state: &Arc<AppState>) -> tokio::task::JoinHandle<()> {
        let state = state.clone();
        tokio::spawn(async move {
            // Serialize under the lock to get a consistent snapshot
            let json = {
                let reviews = state.reviews.read().await;
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
                // read lock dropped here
            };

            if let Some(parent) = state.storage_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    error!("Failed to create storage directory: {}", e);
                    return;
                }
            }
            // Write to a temp file then rename for atomic writes
            let tmp_path = state.storage_path.with_extension("json.tmp");
            if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
                error!("Failed to write reviews temp file: {}", e);
                return;
            }
            if let Err(e) = tokio::fs::rename(&tmp_path, &state.storage_path).await {
                error!("Failed to rename reviews file: {}", e);
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        })
    }

    /// Persist config to disk asynchronously.
    pub fn save_config_async(state: &Arc<AppState>) -> tokio::task::JoinHandle<()> {
        let state = state.clone();
        tokio::spawn(async move {
            let json = {
                let config = state.config.read().await;
                match serde_json::to_string_pretty(&*config) {
                    Ok(j) => j,
                    Err(e) => {
                        error!("Failed to serialize config: {}", e);
                        return;
                    }
                }
            };

            if let Some(parent) = state.config_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    error!("Failed to create config directory: {}", e);
                    return;
                }
            }
            let tmp_path = state.config_path.with_extension("json.tmp");
            if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
                error!("Failed to write config: {}", e);
                return;
            }
            if let Err(e) = tokio::fs::rename(&tmp_path, &state.config_path).await {
                error!("Failed to rename config file: {}", e);
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        })
    }

    fn load_reviews_from_disk(path: &std::path::Path) -> HashMap<String, ReviewSession> {
        if !path.exists() {
            return HashMap::new();
        }
        match std::fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<HashMap<String, ReviewSession>>(&data) {
                Ok(mut loaded) => {
                    for session in loaded.values_mut() {
                        if session.summary.is_some() || !session.comments.is_empty() {
                            let previous_summary = session.summary.clone();
                            session.summary =
                                Some(crate::core::CommentSynthesizer::inherit_review_state(
                                    crate::core::CommentSynthesizer::generate_summary(
                                        &session.comments,
                                    ),
                                    previous_summary.as_ref(),
                                ));
                        }
                    }
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

    /// Update a review session to Running status.
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

    /// Record the head SHA after a successful review for a PR.
    /// The key is formatted as "owner/repo#pr_number".
    pub async fn record_reviewed_sha(state: &Arc<AppState>, pr_key: &str, head_sha: &str) {
        let mut shas = state.last_reviewed_shas.write().await;
        shas.insert(pr_key.to_string(), head_sha.to_string());
    }

    /// Look up the last reviewed head SHA for a PR.
    /// Returns `None` if this PR has never been reviewed.
    pub async fn get_last_reviewed_sha(state: &Arc<AppState>, pr_key: &str) -> Option<String> {
        let shas = state.last_reviewed_shas.read().await;
        shas.get(pr_key).cloned()
    }
}

/// Lightweight view of a review session for list endpoints (no comments/diff/event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewListItem {
    pub id: String,
    pub status: ReviewStatus,
    pub diff_source: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub comments: Vec<Comment>,
    pub summary: Option<ReviewSummary>,
    pub files_reviewed: usize,
    pub comment_count: usize,
    pub overall_score: Option<f32>,
    pub error: Option<String>,
    pub progress: Option<ReviewProgress>,
}

impl ReviewListItem {
    pub fn from_session(session: &ReviewSession) -> Self {
        Self {
            id: session.id.clone(),
            status: session.status.clone(),
            diff_source: session.diff_source.clone(),
            started_at: session.started_at,
            completed_at: session.completed_at,
            comments: session.comments.clone(),
            summary: session.summary.clone(),
            files_reviewed: session.files_reviewed,
            comment_count: session.comments.len(),
            overall_score: session.summary.as_ref().map(|s| s.overall_score),
            error: session.error.clone(),
            progress: session.progress.clone(),
        }
    }
}

/// Get current UNIX timestamp in seconds.
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

/// Builder for constructing `ReviewEvent` instances without 17 positional parameters.
pub struct ReviewEventBuilder {
    event: ReviewEvent,
}

impl ReviewEventBuilder {
    pub fn new(review_id: &str, event_type: &str, diff_source: &str, model: &str) -> Self {
        Self {
            event: ReviewEvent {
                review_id: review_id.to_string(),
                event_type: event_type.to_string(),
                diff_source: diff_source.to_string(),
                title: None,
                model: model.to_string(),
                provider: None,
                base_url: None,
                duration_ms: 0,
                diff_fetch_ms: None,
                llm_total_ms: None,
                diff_bytes: 0,
                diff_files_total: 0,
                diff_files_reviewed: 0,
                diff_files_skipped: 0,
                comments_total: 0,
                comments_by_severity: HashMap::new(),
                comments_by_category: HashMap::new(),
                overall_score: None,
                hotspots_detected: 0,
                high_risk_files: 0,
                tokens_prompt: None,
                tokens_completion: None,
                tokens_total: None,
                cost_estimate_usd: None,
                file_metrics: None,
                hotspot_details: None,
                convention_suppressed: None,
                comments_by_pass: HashMap::new(),
                agent_iterations: None,
                agent_tool_calls: None,
                github_posted: false,
                github_repo: None,
                github_pr: None,
                error: None,
                created_at: None,
            },
        }
    }

    #[allow(dead_code)]
    pub fn title(mut self, title: &str) -> Self {
        self.event.title = Some(title.to_string());
        self
    }

    pub fn provider(mut self, provider: Option<&str>) -> Self {
        self.event.provider = provider.map(str::to_string);
        self
    }

    pub fn base_url(mut self, base_url: Option<&str>) -> Self {
        self.event.base_url = base_url.map(str::to_string);
        self
    }

    pub fn duration_ms(mut self, ms: u64) -> Self {
        self.event.duration_ms = ms;
        self
    }

    pub fn diff_fetch_ms(mut self, ms: u64) -> Self {
        self.event.diff_fetch_ms = Some(ms);
        self
    }

    pub fn llm_total_ms(mut self, ms: u64) -> Self {
        self.event.llm_total_ms = Some(ms);
        self
    }

    pub fn diff_stats(
        mut self,
        bytes: usize,
        files_total: usize,
        files_reviewed: usize,
        files_skipped: usize,
    ) -> Self {
        self.event.diff_bytes = bytes;
        self.event.diff_files_total = files_total;
        self.event.diff_files_reviewed = files_reviewed;
        self.event.diff_files_skipped = files_skipped;
        self
    }

    pub fn comments(mut self, comments: &[Comment], summary: Option<&ReviewSummary>) -> Self {
        let mut by_severity: HashMap<String, usize> = HashMap::new();
        let mut by_category: HashMap<String, usize> = HashMap::new();
        for c in comments {
            *by_severity.entry(c.severity.to_string()).or_default() += 1;
            *by_category.entry(c.category.to_string()).or_default() += 1;
        }
        self.event.comments_total = comments.len();
        self.event.comments_by_severity = by_severity;
        self.event.comments_by_category = by_category;
        self.event.overall_score = summary.map(|s| s.overall_score);
        self
    }

    pub fn error(mut self, err: &str) -> Self {
        self.event.error = Some(err.to_string());
        self
    }

    pub fn github(mut self, repo: &str, pr: u32) -> Self {
        self.event.github_repo = Some(repo.to_string());
        self.event.github_pr = Some(pr);
        self
    }

    pub fn github_posted(mut self, posted: bool) -> Self {
        self.event.github_posted = posted;
        self
    }

    pub fn tokens(mut self, prompt: usize, completion: usize, total: usize) -> Self {
        self.event.tokens_prompt = Some(prompt);
        self.event.tokens_completion = Some(completion);
        self.event.tokens_total = Some(total);
        self.event.cost_estimate_usd =
            Some(super::cost::estimate_cost_usd(&self.event.model, total));
        self
    }

    pub fn file_metrics(mut self, metrics: Vec<FileMetricEvent>) -> Self {
        if metrics.is_empty() {
            self.event.file_metrics = None;
        } else {
            self.event.file_metrics = Some(metrics);
        }
        self
    }

    pub fn hotspot_details(mut self, details: Vec<HotspotDetail>) -> Self {
        self.event.hotspots_detected = details.len();
        self.event.high_risk_files = details.iter().filter(|h| h.risk_score > 0.6).count();
        if details.is_empty() {
            self.event.hotspot_details = None;
        } else {
            self.event.hotspot_details = Some(details);
        }
        self
    }

    pub fn convention_suppressed(mut self, count: usize) -> Self {
        if count > 0 {
            self.event.convention_suppressed = Some(count);
        }
        self
    }

    pub fn comments_by_pass(mut self, by_pass: HashMap<String, usize>) -> Self {
        self.event.comments_by_pass = by_pass;
        self
    }

    pub fn agent_activity(mut self, activity: Option<&crate::review::AgentActivity>) -> Self {
        if let Some(a) = activity {
            self.event.agent_iterations = Some(a.total_iterations);
            self.event.agent_tool_calls = Some(
                a.tool_calls
                    .iter()
                    .map(|tc| AgentToolCallEvent {
                        iteration: tc.iteration,
                        tool_name: tc.tool_name.clone(),
                        duration_ms: tc.duration_ms,
                    })
                    .collect(),
            );
        }
        self
    }

    pub fn build(mut self) -> ReviewEvent {
        self.event.created_at = Some(chrono::Utc::now());
        self.event
    }
}

/// Emit a review wide event via structured tracing.
/// Also logs one full JSON line per event (target "review.event.json") for log pipelines / OTEL.
pub fn emit_wide_event(event: &ReviewEvent) {
    info!(
        review_id = %event.review_id,
        event_type = %event.event_type,
        diff_source = %event.diff_source,
        model = %event.model,
        duration_ms = event.duration_ms,
        llm_total_ms = ?event.llm_total_ms,
        diff_bytes = event.diff_bytes,
        diff_files_total = event.diff_files_total,
        diff_files_reviewed = event.diff_files_reviewed,
        comments_total = event.comments_total,
        overall_score = ?event.overall_score,
        tokens_total = ?event.tokens_total,
        convention_suppressed = ?event.convention_suppressed,
        hotspots_detected = event.hotspots_detected,
        high_risk_files = event.high_risk_files,
        github_posted = event.github_posted,
        error = ?event.error,
        "review.event"
    );
    // One JSON line per event for log pipelines / OTEL: include @timestamp and event.name for filtering.
    let timestamp = event
        .created_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let payload = serde_json::json!({
        "@timestamp": timestamp,
        "event": { "name": "review.event", "kind": "event" },
        "review": event
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        info!(target: "review.event.json", "{}", json);
    }
}

/// Build a progress callback that updates the review session during review.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    #[test]
    fn test_current_timestamp_is_positive() {
        let ts = current_timestamp();
        assert!(ts > 0);
    }

    #[test]
    fn test_count_diff_files_empty() {
        assert_eq!(count_diff_files(""), 0);
        assert_eq!(count_diff_files("some random text"), 0);
    }

    #[test]
    fn test_count_diff_files_single() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+hello\n";
        assert_eq!(count_diff_files(diff), 1);
    }

    #[test]
    fn test_count_diff_files_multiple() {
        let diff = "diff --git a/a.rs b/a.rs\n+a\n\ndiff --git a/b.rs b/b.rs\n+b\n";
        assert_eq!(count_diff_files(diff), 2);
    }

    #[test]
    fn test_emit_wide_event_payload_has_otel_shape() {
        let event = ReviewEventBuilder::new("r-otel", "review.completed", "head", "gpt-4o")
            .duration_ms(100)
            .build();
        let timestamp = event
            .created_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let payload = serde_json::json!({
            "@timestamp": timestamp,
            "event": { "name": "review.event", "kind": "event" },
            "review": event
        });
        let json = serde_json::to_string(&payload).unwrap();
        assert!(
            json.contains("@timestamp"),
            "OTEL payload must include @timestamp"
        );
        assert!(
            json.contains("\"name\":\"review.event\""),
            "OTEL payload must include event.name for filtering"
        );
        assert!(
            json.contains("\"review\""),
            "OTEL payload must include review object"
        );
        assert!(json.contains("r-otel"), "payload must contain review_id");
    }

    #[test]
    fn test_review_event_builder_minimal() {
        let event = ReviewEventBuilder::new("r1", "review.completed", "head", "gpt-4o").build();
        assert_eq!(event.review_id, "r1");
        assert_eq!(event.event_type, "review.completed");
        assert_eq!(event.diff_source, "head");
        assert_eq!(event.model, "gpt-4o");
        assert!(event.title.is_none());
        assert!(event.error.is_none());
        assert!(!event.github_posted);
        assert_eq!(event.comments_total, 0);
        assert!(event.tokens_prompt.is_none());
        assert!(event.tokens_completion.is_none());
        assert!(event.tokens_total.is_none());
        assert!(event.file_metrics.is_none());
        assert!(event.hotspot_details.is_none());
        assert!(event.convention_suppressed.is_none());
        assert!(event.comments_by_pass.is_empty());
    }

    #[test]
    fn test_review_event_builder_full() {
        let comments = vec![Comment {
            id: "c1".to_string(),
            file_path: PathBuf::from("a.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Error,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);

        let mut by_pass = HashMap::new();
        by_pass.insert("security".to_string(), 1);

        let event =
            ReviewEventBuilder::new("r2", "review.completed", "staged", "claude-sonnet-4.6")
                .title("Test PR")
                .provider(Some("anthropic"))
                .base_url(Some("https://api.anthropic.com"))
                .duration_ms(5000)
                .diff_fetch_ms(100)
                .llm_total_ms(4500)
                .diff_stats(1024, 3, 2, 1)
                .comments(&comments, Some(&summary))
                .tokens(200, 100, 300)
                .file_metrics(vec![FileMetricEvent {
                    file_path: "a.rs".to_string(),
                    latency_ms: 100,
                    prompt_tokens: 200,
                    completion_tokens: 100,
                    total_tokens: 300,
                    comment_count: 1,
                }])
                .hotspot_details(vec![HotspotDetail {
                    file_path: "a.rs".to_string(),
                    risk_score: 0.8,
                    reasons: vec!["complex".to_string()],
                }])
                .convention_suppressed(2)
                .comments_by_pass(by_pass)
                .github("owner/repo", 42)
                .github_posted(true)
                .build();

        assert_eq!(event.title.as_deref(), Some("Test PR"));
        assert_eq!(event.provider.as_deref(), Some("anthropic"));
        assert_eq!(event.duration_ms, 5000);
        assert_eq!(event.diff_fetch_ms, Some(100));
        assert_eq!(event.llm_total_ms, Some(4500));
        assert_eq!(event.diff_bytes, 1024);
        assert_eq!(event.diff_files_total, 3);
        assert_eq!(event.diff_files_reviewed, 2);
        assert_eq!(event.diff_files_skipped, 1);
        assert_eq!(event.comments_total, 1);
        assert!(event.comments_by_severity.contains_key("Error"));
        assert!(event.comments_by_category.contains_key("Bug"));
        assert!(event.overall_score.is_some());
        assert_eq!(event.tokens_prompt, Some(200));
        assert_eq!(event.tokens_completion, Some(100));
        assert_eq!(event.tokens_total, Some(300));
        assert!(event.file_metrics.is_some());
        assert_eq!(event.file_metrics.as_ref().unwrap().len(), 1);
        assert_eq!(event.hotspots_detected, 1);
        assert_eq!(event.high_risk_files, 1);
        assert!(event.hotspot_details.is_some());
        assert_eq!(event.convention_suppressed, Some(2));
        assert_eq!(event.comments_by_pass.get("security"), Some(&1));
        assert_eq!(event.github_repo.as_deref(), Some("owner/repo"));
        assert_eq!(event.github_pr, Some(42));
        assert!(event.github_posted);
    }

    #[test]
    fn test_review_event_builder_error() {
        let event = ReviewEventBuilder::new("r3", "review.failed", "head", "gpt-4o")
            .error("timeout")
            .build();
        assert_eq!(event.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_count_reviewed_files() {
        let comments = vec![
            Comment {
                id: "c1".to_string(),
                file_path: PathBuf::from("a.rs"),
                line_number: 1,
                content: "test".to_string(),
                rule_id: None,
                severity: Severity::Warning,
                category: Category::Style,
                suggestion: None,
                confidence: 0.5,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Low,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
            Comment {
                id: "c2".to_string(),
                file_path: PathBuf::from("b.rs"),
                line_number: 2,
                content: "test2".to_string(),
                rule_id: None,
                severity: Severity::Info,
                category: Category::Style,
                suggestion: None,
                confidence: 0.5,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Low,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
            Comment {
                id: "c3".to_string(),
                file_path: PathBuf::from("a.rs"),
                line_number: 5,
                content: "test3".to_string(),
                rule_id: None,
                severity: Severity::Warning,
                category: Category::Bug,
                suggestion: None,
                confidence: 0.8,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Medium,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
        ];
        assert_eq!(count_reviewed_files(&comments), 2);
    }

    #[test]
    fn test_count_reviewed_files_empty() {
        assert_eq!(count_reviewed_files(&[]), 0);
    }

    #[test]
    fn test_save_reviews_returns_awaitable_handle() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path: storage_path.clone(),
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });
            let handle = AppState::save_reviews_async(&state);
            // The handle should be awaitable and complete successfully
            handle.await.unwrap();
            // File should exist on disk
            assert!(storage_path.exists());
        });
    }

    #[test]
    fn test_builder_tokens() {
        let event = ReviewEventBuilder::new("r-tok", "review.completed", "head", "gpt-4o")
            .tokens(100, 50, 150)
            .build();
        assert_eq!(event.tokens_prompt, Some(100));
        assert_eq!(event.tokens_completion, Some(50));
        assert_eq!(event.tokens_total, Some(150));
    }

    #[test]
    fn test_builder_hotspot_details() {
        let details = vec![
            HotspotDetail {
                file_path: "risky.rs".to_string(),
                risk_score: 0.9,
                reasons: vec!["high complexity".to_string()],
            },
            HotspotDetail {
                file_path: "safe.rs".to_string(),
                risk_score: 0.3,
                reasons: vec!["minor change".to_string()],
            },
        ];
        let event = ReviewEventBuilder::new("r-hot", "review.completed", "head", "gpt-4o")
            .hotspot_details(details)
            .build();
        assert_eq!(event.hotspots_detected, 2);
        assert_eq!(event.high_risk_files, 1); // only risky.rs has score > 0.6
        assert!(event.hotspot_details.is_some());
        assert_eq!(event.hotspot_details.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_builder_convention_suppressed() {
        let event = ReviewEventBuilder::new("r-conv", "review.completed", "head", "gpt-4o")
            .convention_suppressed(3)
            .build();
        assert_eq!(event.convention_suppressed, Some(3));

        let event_zero = ReviewEventBuilder::new("r-conv0", "review.completed", "head", "gpt-4o")
            .convention_suppressed(0)
            .build();
        assert!(event_zero.convention_suppressed.is_none());
    }

    #[test]
    fn test_review_semaphore_limits_concurrency() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let sem = Arc::new(tokio::sync::Semaphore::new(2));
            // Acquire 2 permits
            let _p1 = sem.clone().acquire_owned().await.unwrap();
            let _p2 = sem.clone().acquire_owned().await.unwrap();
            // Third should not be available immediately
            assert_eq!(sem.available_permits(), 0);
            // Drop one permit
            drop(_p1);
            assert_eq!(sem.available_permits(), 1);
        });
    }

    #[test]
    fn test_record_and_get_last_reviewed_sha() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path,
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });

            let pr_key = "owner/repo#42";

            // Initially no SHA recorded
            assert!(AppState::get_last_reviewed_sha(&state, pr_key)
                .await
                .is_none());

            // Record a SHA
            AppState::record_reviewed_sha(&state, pr_key, "abc123").await;
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, pr_key)
                    .await
                    .as_deref(),
                Some("abc123"),
            );

            // Update the SHA
            AppState::record_reviewed_sha(&state, pr_key, "def456").await;
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, pr_key)
                    .await
                    .as_deref(),
                Some("def456"),
            );

            // Different PR key is independent
            let other_key = "owner/repo#99";
            assert!(AppState::get_last_reviewed_sha(&state, other_key)
                .await
                .is_none());
        });
    }

    #[test]
    fn test_last_reviewed_shas_multiple_prs() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path,
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });

            // Record SHAs for multiple PRs across different repos
            AppState::record_reviewed_sha(&state, "org/repo-a#1", "sha_a1").await;
            AppState::record_reviewed_sha(&state, "org/repo-a#2", "sha_a2").await;
            AppState::record_reviewed_sha(&state, "org/repo-b#1", "sha_b1").await;

            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-a#1")
                    .await
                    .as_deref(),
                Some("sha_a1"),
            );
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-a#2")
                    .await
                    .as_deref(),
                Some("sha_a2"),
            );
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-b#1")
                    .await
                    .as_deref(),
                Some("sha_b1"),
            );
        });
    }

    // ── Agent activity builder tests ─────────────────────────────────────

    #[test]
    fn test_builder_agent_activity_none() {
        let event = ReviewEventBuilder::new("r-ag0", "review.completed", "head", "gpt-4o")
            .agent_activity(None)
            .build();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_builder_agent_activity_with_data() {
        let activity = crate::review::AgentActivity {
            total_iterations: 3,
            tool_calls: vec![
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "read_file".to_string(),
                    duration_ms: 15,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "search_codebase".to_string(),
                    duration_ms: 42,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 1,
                    tool_name: "read_file".to_string(),
                    duration_ms: 8,
                },
            ],
        };

        let event = ReviewEventBuilder::new("r-ag1", "review.completed", "head", "claude-opus-4-6")
            .agent_activity(Some(&activity))
            .build();

        assert_eq!(event.agent_iterations, Some(3));
        let calls = event.agent_tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].iteration, 0);
        assert_eq!(calls[0].tool_name, "read_file");
        assert_eq!(calls[0].duration_ms, 15);
        assert_eq!(calls[1].iteration, 0);
        assert_eq!(calls[1].tool_name, "search_codebase");
        assert_eq!(calls[1].duration_ms, 42);
        assert_eq!(calls[2].iteration, 1);
        assert_eq!(calls[2].tool_name, "read_file");
        assert_eq!(calls[2].duration_ms, 8);
    }

    #[test]
    fn test_builder_agent_activity_empty_tool_calls() {
        let activity = crate::review::AgentActivity {
            total_iterations: 1,
            tool_calls: vec![],
        };

        let event = ReviewEventBuilder::new("r-ag2", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();

        assert_eq!(event.agent_iterations, Some(1));
        assert!(event.agent_tool_calls.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_builder_agent_activity_default_none() {
        // Without calling .agent_activity(), fields should be None
        let event = ReviewEventBuilder::new("r-ag3", "review.completed", "head", "gpt-4o").build();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_agent_fields_serialize_skip_when_none() {
        let event = ReviewEventBuilder::new("r-ag4", "review.completed", "head", "gpt-4o").build();
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            !json.contains("agent_iterations"),
            "agent_iterations should be skipped when None"
        );
        assert!(
            !json.contains("agent_tool_calls"),
            "agent_tool_calls should be skipped when None"
        );
    }

    #[test]
    fn test_agent_fields_serialize_when_present() {
        let activity = crate::review::AgentActivity {
            total_iterations: 2,
            tool_calls: vec![crate::core::agent_loop::AgentToolCallLog {
                iteration: 0,
                tool_name: "read_file".to_string(),
                duration_ms: 10,
            }],
        };

        let event = ReviewEventBuilder::new("r-ag5", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"agent_iterations\":2"));
        assert!(json.contains("\"agent_tool_calls\""));
        assert!(json.contains("\"tool_name\":\"read_file\""));
        assert!(json.contains("\"duration_ms\":10"));
    }

    #[test]
    fn test_agent_fields_deserialize_round_trip() {
        let activity = crate::review::AgentActivity {
            total_iterations: 5,
            tool_calls: vec![
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "search_codebase".to_string(),
                    duration_ms: 100,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 2,
                    tool_name: "get_file_history".to_string(),
                    duration_ms: 250,
                },
            ],
        };

        let event = ReviewEventBuilder::new("r-ag6", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ReviewEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.agent_iterations, Some(5));
        let calls = deserialized.agent_tool_calls.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool_name, "search_codebase");
        assert_eq!(calls[0].duration_ms, 100);
        assert_eq!(calls[1].tool_name, "get_file_history");
        assert_eq!(calls[1].iteration, 2);
    }

    #[test]
    fn test_agent_fields_deserialize_missing_fields() {
        // JSON without agent fields should deserialize with None values
        let json = r#"{
            "review_id": "r-test",
            "event_type": "review.completed",
            "diff_source": "head",
            "model": "gpt-4o",
            "duration_ms": 100,
            "diff_bytes": 500,
            "diff_files_total": 3,
            "diff_files_reviewed": 2,
            "diff_files_skipped": 1,
            "comments_total": 0,
            "comments_by_severity": {},
            "comments_by_category": {},
            "hotspots_detected": 0,
            "high_risk_files": 0,
            "github_posted": false
        }"#;
        let event: ReviewEvent = serde_json::from_str(json).unwrap();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_agent_tool_call_event_serde() {
        let tc = AgentToolCallEvent {
            iteration: 2,
            tool_name: "lookup_symbol".to_string(),
            duration_ms: 77,
        };
        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: AgentToolCallEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.iteration, 2);
        assert_eq!(deserialized.tool_name, "lookup_symbol");
        assert_eq!(deserialized.duration_ms, 77);
    }

    #[test]
    fn test_builder_agent_activity_chained_with_other_fields() {
        let activity = crate::review::AgentActivity {
            total_iterations: 4,
            tool_calls: vec![crate::core::agent_loop::AgentToolCallLog {
                iteration: 0,
                tool_name: "read_file".to_string(),
                duration_ms: 5,
            }],
        };

        let comments = vec![Comment {
            id: "c1".to_string(),
            file_path: PathBuf::from("a.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);

        let event = ReviewEventBuilder::new("r-ag7", "review.completed", "head", "claude-opus-4-6")
            .provider(Some("anthropic"))
            .duration_ms(5000)
            .comments(&comments, Some(&summary))
            .tokens(200, 100, 300)
            .agent_activity(Some(&activity))
            .build();

        // Verify agent fields
        assert_eq!(event.agent_iterations, Some(4));
        assert_eq!(event.agent_tool_calls.as_ref().unwrap().len(), 1);
        // Verify other fields are unaffected
        assert_eq!(event.provider.as_deref(), Some("anthropic"));
        assert_eq!(event.duration_ms, 5000);
        assert_eq!(event.comments_total, 1);
        assert_eq!(event.tokens_total, Some(300));
    }
}
