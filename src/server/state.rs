use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

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

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub repo_path: PathBuf,
    pub reviews: Arc<RwLock<HashMap<String, ReviewSession>>>,
    pub storage_path: PathBuf,
    pub config_path: PathBuf,
    /// Shared HTTP client for GitHub API and provider tests (connection pooling).
    pub http_client: reqwest::Client,
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
        };

        Ok(state)
    }

    /// Serialize reviews under the read lock, then write to disk after releasing it.
    /// The serialization happens atomically under the lock so the snapshot is consistent.
    pub fn save_reviews_async(state: &Arc<AppState>) {
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
        });
    }

    /// Persist config to disk asynchronously.
    pub fn save_config_async(state: &Arc<AppState>) {
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
        });
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
