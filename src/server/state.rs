use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

use crate::config::Config;
use crate::core::comment::{Comment, ReviewSummary};

/// Maximum number of reviews to keep in memory. Oldest completed reviews are pruned.
const MAX_REVIEWS: usize = 200;

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
}

impl AppState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let repo_path = std::env::current_dir()?;
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diffscope");
        let storage_path = data_dir.join("reviews.json");
        let config_path = data_dir.join("config.json");

        let state = Self {
            config: Arc::new(RwLock::new(config)),
            repo_path,
            reviews: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
            config_path,
        };

        // Load persisted reviews (blocking is fine during startup, before server accepts)
        state.load_reviews();

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
            }
        });
    }

    fn load_reviews(&self) {
        if self.storage_path.exists() {
            match std::fs::read_to_string(&self.storage_path) {
                Ok(data) => match serde_json::from_str::<HashMap<String, ReviewSession>>(&data) {
                    Ok(loaded) => {
                        let mut reviews = self.reviews.blocking_write();
                        *reviews = loaded;
                        eprintln!("Loaded {} reviews from disk", reviews.len());
                    }
                    Err(e) => eprintln!("Failed to parse reviews.json: {}", e),
                },
                Err(e) => eprintln!("Failed to read reviews.json: {}", e),
            }
        }
    }

    /// Prune oldest completed reviews when over the limit.
    pub async fn prune_old_reviews(state: &Arc<AppState>) {
        let mut reviews = state.reviews.write().await;
        if reviews.len() <= MAX_REVIEWS {
            return;
        }
        // Collect completed review IDs sorted by started_at ascending (oldest first)
        let mut completed: Vec<(String, i64)> = reviews
            .iter()
            .filter(|(_, r)| r.status == ReviewStatus::Complete || r.status == ReviewStatus::Failed)
            .map(|(id, r)| (id.clone(), r.started_at))
            .collect();
        completed.sort_by_key(|(_, ts)| *ts);

        let to_remove = reviews.len() - MAX_REVIEWS;
        for (id, _) in completed.into_iter().take(to_remove) {
            reviews.remove(&id);
        }
    }
}
