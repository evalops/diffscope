use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

use crate::config::Config;
use crate::core::comment::{Comment, ReviewSummary};

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
}

impl AppState {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let repo_path = std::env::current_dir()?;
        let storage_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diffscope")
            .join("reviews.json");

        let state = Self {
            config: Arc::new(RwLock::new(config)),
            repo_path,
            reviews: Arc::new(RwLock::new(HashMap::new())),
            storage_path,
        };

        state.load_reviews();

        Ok(state)
    }

    pub fn save_reviews(&self) {
        let reviews = self.reviews.blocking_read();
        // Strip diff_content when persisting
        let stripped: HashMap<String, ReviewSession> = reviews
            .iter()
            .map(|(k, v)| {
                let mut session = v.clone();
                session.diff_content = None;
                (k.clone(), session)
            })
            .collect();

        if let Some(parent) = self.storage_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&stripped) {
            let _ = std::fs::write(&self.storage_path, json);
        }
    }

    pub fn save_reviews_async(state: &Arc<AppState>) {
        let state = state.clone();
        tokio::spawn(async move {
            let reviews = state.reviews.read().await;
            // Strip diff_content when persisting
            let stripped: HashMap<String, ReviewSession> = reviews
                .iter()
                .map(|(k, v)| {
                    let mut session = v.clone();
                    session.diff_content = None;
                    (k.clone(), session)
                })
                .collect();
            drop(reviews);

            if let Some(parent) = state.storage_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    eprintln!("Failed to create storage directory: {}", e);
                    return;
                }
            }
            match serde_json::to_string_pretty(&stripped) {
                Ok(json) => {
                    if let Err(e) = tokio::fs::write(&state.storage_path, json).await {
                        eprintln!("Failed to persist reviews: {}", e);
                    }
                }
                Err(e) => eprintln!("Failed to serialize reviews: {}", e),
            }
        });
    }

    pub fn load_reviews(&self) {
        if self.storage_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&self.storage_path) {
                if let Ok(loaded) = serde_json::from_str::<HashMap<String, ReviewSession>>(&data) {
                    let mut reviews = self.reviews.blocking_write();
                    *reviews = loaded;
                }
            }
        }
    }
}
