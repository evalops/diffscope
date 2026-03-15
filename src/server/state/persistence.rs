use super::*;

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
            let pg = crate::server::storage_pg::PgStorageBackend::new(pool);
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
            Arc::new(crate::server::storage_json::JsonStorageBackend::new(
                &storage_path,
            ))
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
            pr_verification_reuse_caches: Arc::new(RwLock::new(HashMap::new())),
            api_rate_limits: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
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
}
