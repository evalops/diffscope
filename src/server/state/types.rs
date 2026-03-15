use super::*;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cost_breakdowns: Vec<crate::server::cost::CostBreakdownRow>,

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRecomputeJobState {
    #[default]
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalyticsRecomputeJobStatus {
    pub job_id: String,
    pub status: AnalyticsRecomputeJobState,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub reviews_scanned: usize,
    #[serde(default)]
    pub reviews_updated: usize,
    #[serde(default)]
    pub events_updated: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

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
    /// Reuses per-finding verifier decisions across PR reruns, keyed by "owner/repo#pr_number".
    pub pr_verification_reuse_caches:
        Arc<RwLock<HashMap<String, crate::review::verification::VerificationReuseCache>>>,
    /// Tracks background analytics recompute jobs.
    pub analytics_recompute_jobs: Arc<RwLock<HashMap<String, AnalyticsRecomputeJobStatus>>>,
    /// Tracks per-subject mutation counts for API rate limiting windows.
    pub api_rate_limits: Arc<tokio::sync::Mutex<HashMap<String, (std::time::Instant, u32)>>>,
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
