use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use tracing::info;

use crate::config::Config;
use crate::core::comment::{Comment, ReviewSummary};

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

    // --- GitHub integration ---
    pub github_posted: bool,
    pub github_repo: Option<String>,
    pub github_pr: Option<u32>,

    // --- errors ---
    pub error: Option<String>,
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
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub comments: Vec<Comment>,
    pub summary: Option<ReviewSummary>,
    pub files_reviewed: usize,
    pub error: Option<String>,
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
const MAX_CONCURRENT_REVIEWS: usize = 5;

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub repo_path: PathBuf,
    pub reviews: Arc<RwLock<HashMap<String, ReviewSession>>>,
    pub storage_path: PathBuf,
    pub config_path: PathBuf,
    /// Shared HTTP client for GitHub API and provider tests (connection pooling).
    pub http_client: reqwest::Client,
    /// Semaphore to limit concurrent review tasks.
    pub review_semaphore: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let repo_path = std::env::current_dir()?;
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diffscope");
        let storage_path = data_dir.join("reviews.json");
        let config_path = data_dir.join("config.json");

        // Load persisted reviews before creating the RwLock (avoids blocking_write inside runtime)
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
            storage_path,
            config_path,
            http_client,
            review_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REVIEWS)),
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
                        eprintln!("Failed to serialize reviews: {}", e);
                        return;
                    }
                }
                // read lock dropped here
            };

            if let Some(parent) = state.storage_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    eprintln!("Failed to create storage directory: {}", e);
                    return;
                }
            }
            // Write to a temp file then rename for atomic writes
            let tmp_path = state.storage_path.with_extension("json.tmp");
            if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
                eprintln!("Failed to write reviews temp file: {}", e);
                return;
            }
            if let Err(e) = tokio::fs::rename(&tmp_path, &state.storage_path).await {
                eprintln!("Failed to rename reviews file: {}", e);
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
                        eprintln!("Failed to serialize config: {}", e);
                        return;
                    }
                }
            };

            if let Some(parent) = state.config_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    eprintln!("Failed to create config directory: {}", e);
                    return;
                }
            }
            let tmp_path = state.config_path.with_extension("json.tmp");
            if let Err(e) = tokio::fs::write(&tmp_path, &json).await {
                eprintln!("Failed to write config: {}", e);
                return;
            }
            if let Err(e) = tokio::fs::rename(&tmp_path, &state.config_path).await {
                eprintln!("Failed to rename config file: {}", e);
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
}

/// Lightweight view of a review session for list endpoints (no comments/diff/event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewListItem {
    pub id: String,
    pub status: ReviewStatus,
    pub diff_source: String,
    pub started_at: i64,
    pub completed_at: Option<i64>,
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
        + if diff_content.starts_with("diff --git ") { 1 } else { 0 }
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
                github_posted: false,
                github_repo: None,
                github_pr: None,
                error: None,
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

    pub fn diff_stats(mut self, bytes: usize, files_total: usize, files_reviewed: usize, files_skipped: usize) -> Self {
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

    pub fn build(self) -> ReviewEvent {
        self.event
    }
}

/// Emit a review wide event via structured tracing.
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
        github_posted = event.github_posted,
        error = ?event.error,
        "review.event"
    );
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
                    session.summary =
                        Some(crate::core::CommentSynthesizer::generate_summary(&session.comments));
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
    use crate::core::comment::{Category, Severity, FixEffort};
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
    fn test_review_event_builder_minimal() {
        let event = ReviewEventBuilder::new("r1", "review.completed", "head", "gpt-4o")
            .build();
        assert_eq!(event.review_id, "r1");
        assert_eq!(event.event_type, "review.completed");
        assert_eq!(event.diff_source, "head");
        assert_eq!(event.model, "gpt-4o");
        assert!(event.title.is_none());
        assert!(event.error.is_none());
        assert!(!event.github_posted);
        assert_eq!(event.comments_total, 0);
    }

    #[test]
    fn test_review_event_builder_full() {
        let comments = vec![
            Comment {
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
            },
        ];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);

        let event = ReviewEventBuilder::new("r2", "review.completed", "staged", "claude-sonnet-4.6")
            .title("Test PR")
            .provider(Some("anthropic"))
            .base_url(Some("https://api.anthropic.com"))
            .duration_ms(5000)
            .diff_fetch_ms(100)
            .llm_total_ms(4500)
            .diff_stats(1024, 3, 2, 1)
            .comments(&comments, Some(&summary))
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
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                http_client: reqwest::Client::new(),
                storage_path: storage_path.clone(),
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
            });
            let handle = AppState::save_reviews_async(&state);
            // The handle should be awaitable and complete successfully
            handle.await.unwrap();
            // File should exist on disk
            assert!(storage_path.exists());
        });
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
}
