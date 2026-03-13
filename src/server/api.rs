use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::state::{
    build_progress_callback, count_diff_files, count_reviewed_files, current_timestamp,
    emit_wide_event, AppState, FileMetricEvent, HotspotDetail, ReviewEventBuilder, ReviewListItem,
    ReviewSession, ReviewStatus, MAX_DIFF_SIZE,
};
use crate::core::comment::CommentSynthesizer;
use crate::core::convention_learner::ConventionStore;
use tracing::{info, warn};

// === Request/Response types ===

#[derive(Deserialize)]
pub struct StartReviewRequest {
    pub diff_source: String,
    pub base_branch: Option<String>,
    /// Raw diff content (used when diff_source is "raw", e.g. from a GitHub PR)
    pub diff_content: Option<String>,
    /// Optional title for the review (e.g. "owner/repo#123: PR title")
    pub title: Option<String>,
    // --- per-review overrides ---
    pub model: Option<String>,
    pub strictness: Option<u8>,
    pub review_profile: Option<String>,
}

/// Per-review config overrides from the start request.
#[derive(Clone, Default)]
struct ReviewOverrides {
    model: Option<String>,
    strictness: Option<u8>,
    review_profile: Option<String>,
}

#[derive(Serialize)]
pub struct StartReviewResponse {
    pub id: String,
    pub status: ReviewStatus,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub repo_path: String,
    pub branch: Option<String>,
    pub model: String,
    pub adapter: Option<String>,
    pub base_url: Option<String>,
    pub active_reviews: usize,
}

#[derive(Deserialize)]
pub struct FeedbackRequest {
    pub comment_id: String,
    pub action: String,
}

#[derive(Serialize)]
pub struct FeedbackResponse {
    pub ok: bool,
}

#[derive(Deserialize)]
pub struct ListReviewsParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
}

#[derive(Deserialize)]
pub struct ListEventsParams {
    pub source: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub github_repo: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

use super::state::ReviewEvent;
use super::storage::{EventFilters, EventStats};

impl ListEventsParams {
    fn into_filters(self) -> EventFilters {
        EventFilters {
            source: self.source,
            model: self.model,
            status: self.status,
            time_from: self.time_from.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|t| t.with_timezone(&chrono::Utc))
            }),
            time_to: self.time_to.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|t| t.with_timezone(&chrono::Utc))
            }),
            github_repo: self.github_repo,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

/// Returns all wide events, filtered and sorted newest-first.
/// Uses the storage backend (PostgreSQL or JSON) for querying.
pub async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsParams>,
) -> Json<Vec<ReviewEvent>> {
    let filters = params.into_filters();
    match state.storage.list_events(&filters).await {
        Ok(events) => Json(events),
        Err(e) => {
            warn!("Failed to list events from storage: {}", e);
            Json(Vec::new())
        }
    }
}

/// Returns aggregated event statistics (server-side analytics).
pub async fn get_event_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsParams>,
) -> Json<EventStats> {
    let filters = params.into_filters();
    match state.storage.get_event_stats(&filters).await {
        Ok(stats) => Json(stats),
        Err(e) => {
            warn!("Failed to get event stats from storage: {}", e);
            Json(EventStats::default())
        }
    }
}

// === Handlers ===

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let config = state.config.read().await;
    let reviews = state.reviews.read().await;

    let branch = git2::Repository::discover(&state.repo_path)
        .ok()
        .and_then(|repo| {
            repo.head()
                .ok()
                .and_then(|h| h.shorthand().map(|s| s.to_string()))
        });

    Json(StatusResponse {
        repo_path: state.repo_path.display().to_string(),
        branch,
        model: config.model.clone(),
        adapter: config.adapter.clone(),
        base_url: config.base_url.clone(),
        active_reviews: reviews
            .values()
            .filter(|r| r.status == ReviewStatus::Running)
            .count(),
    })
}

pub async fn get_agent_tools() -> Json<Vec<crate::core::agent_tools::AgentToolInfo>> {
    Json(crate::core::agent_tools::list_all_tool_info())
}

#[tracing::instrument(name = "api.start_review", skip(state, request), fields(diff_source = %request.diff_source))]
pub async fn start_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartReviewRequest>,
) -> Result<Json<StartReviewResponse>, (StatusCode, String)> {
    // Validate diff_source
    let diff_source = match request.diff_source.as_str() {
        "head" | "staged" | "branch" | "raw" => request.diff_source.clone(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid diff_source: must be head, staged, branch, or raw".to_string(),
            ))
        }
    };

    // "raw" requires diff_content
    if diff_source == "raw"
        && request
            .diff_content
            .as_ref()
            .is_none_or(|c| c.trim().is_empty())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "diff_content is required when diff_source is 'raw'".to_string(),
        ));
    }

    // Reject oversized diffs
    if let Some(ref content) = request.diff_content {
        if content.len() > MAX_DIFF_SIZE {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "Diff content exceeds maximum size of {} MB",
                    MAX_DIFF_SIZE / (1024 * 1024)
                ),
            ));
        }
    }

    info!(diff_source = %diff_source, title = ?request.title, "Starting review");

    // Validate branch name if provided
    if let Some(ref branch) = request.base_branch {
        if branch.is_empty()
            || branch.len() > 200
            || !branch
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        {
            return Err((StatusCode::BAD_REQUEST, "Invalid branch name".to_string()));
        }
    }

    let id = Uuid::new_v4().to_string();

    let display_source = if diff_source == "raw" {
        request.title.clone().unwrap_or_else(|| "raw".to_string())
    } else {
        diff_source.clone()
    };

    let session = ReviewSession {
        id: id.clone(),
        status: ReviewStatus::Pending,
        diff_source: display_source,
        started_at: current_timestamp(),
        completed_at: None,
        comments: Vec::new(),
        summary: None,
        files_reviewed: 0,
        error: None,
        pr_summary_text: None,
        diff_content: None,
        event: None,
        progress: None,
    };

    state.reviews.write().await.insert(id.clone(), session);

    let state_clone = state.clone();
    let review_id = id.clone();
    let base_branch = request.base_branch.clone();
    let raw_diff = request.diff_content.clone();
    let overrides = ReviewOverrides {
        model: request.model.clone(),
        strictness: request.strictness,
        review_profile: request.review_profile.clone(),
    };

    tokio::spawn(async move {
        run_review_task(
            state_clone,
            review_id,
            diff_source,
            base_branch,
            raw_diff,
            overrides,
        )
        .await;
    });

    Ok(Json(StartReviewResponse {
        id,
        status: ReviewStatus::Pending,
    }))
}

#[tracing::instrument(name = "review_task", skip(state, raw_diff, overrides), fields(review_id = %review_id, diff_source = %diff_source))]
async fn run_review_task(
    state: Arc<AppState>,
    review_id: String,
    diff_source: String,
    base_branch: Option<String>,
    raw_diff: Option<String>,
    overrides: ReviewOverrides,
) {
    // Acquire semaphore permit to limit concurrent reviews
    let _permit = match state.review_semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            AppState::fail_review(
                &state,
                &review_id,
                "Review semaphore closed".to_string(),
                None,
            )
            .await;
            return;
        }
    };

    let task_start = std::time::Instant::now();
    AppState::mark_running(&state, &review_id).await;

    let mut config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();

    // Apply per-review overrides
    if let Some(m) = overrides.model {
        config.model = m;
    }
    if let Some(s) = overrides.strictness {
        config.strictness = s.clamp(1, 3);
    }
    if let Some(p) = overrides.review_profile {
        config.review_profile = Some(p);
    }

    let model = config.model.clone();
    let provider = config.adapter.clone();
    let base_url = config.base_url.clone();

    // Get the diff content based on source
    let diff_fetch_start = std::time::Instant::now();
    let diff_result = if diff_source == "raw" {
        Ok(raw_diff.unwrap_or_default())
    } else {
        match diff_source.as_str() {
            "staged" => get_diff_from_git(&repo_path, "staged", None),
            "branch" => {
                let base = base_branch.as_deref().unwrap_or("main");
                get_diff_from_git(&repo_path, "branch", Some(base))
            }
            _ => get_diff_from_git(&repo_path, "head", None),
        }
    };
    let diff_fetch_ms = diff_fetch_start.elapsed().as_millis() as u64;

    let diff_content = match diff_result {
        Ok(diff) => diff,
        Err(e) => {
            let err_msg = format!("Failed to get diff: {}", e);
            let event = ReviewEventBuilder::new(&review_id, "review.failed", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .diff_fetch_ms(diff_fetch_ms)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
            AppState::save_reviews_async(&state);
            return;
        }
    };

    let diff_bytes = diff_content.len();
    let diff_files_total = count_diff_files(&diff_content);

    // Store diff content for the frontend viewer
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.diff_content = Some(diff_content.clone());
        }
    }

    if diff_content.trim().is_empty() {
        let event = ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
            .provider(provider.as_deref())
            .base_url(base_url.as_deref())
            .duration_ms(task_start.elapsed().as_millis() as u64)
            .diff_fetch_ms(diff_fetch_ms)
            .build();
        emit_wide_event(&event);
        AppState::complete_review(
            &state,
            &review_id,
            Vec::new(),
            CommentSynthesizer::generate_summary(&[]),
            0,
            event,
        )
        .await;
        AppState::save_reviews_async(&state);
        return;
    }

    let on_progress = Some(build_progress_callback(&state, &review_id, task_start));

    // Save config for post-review PR summary generation (config is moved into the review call)
    let summary_config = if config.smart_review_summary {
        Some(config.clone())
    } else {
        None
    };

    // Run the review with a 5-minute timeout
    let llm_start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::review::review_diff_content_raw_with_progress(
            &diff_content,
            config,
            &repo_path,
            on_progress,
        ),
    )
    .await;
    let llm_ms = llm_start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(review_result)) => {
            let comments = review_result.comments;
            let summary = CommentSynthesizer::generate_summary(&comments);
            let files_reviewed = count_reviewed_files(&comments);
            let file_metric_events: Vec<FileMetricEvent> = review_result
                .file_metrics
                .iter()
                .map(|m| FileMetricEvent {
                    file_path: m.file_path.display().to_string(),
                    latency_ms: m.latency_ms,
                    prompt_tokens: m.prompt_tokens,
                    completion_tokens: m.completion_tokens,
                    total_tokens: m.total_tokens,
                    comment_count: m.comment_count,
                })
                .collect();
            let event =
                ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
                    .provider(provider.as_deref())
                    .base_url(base_url.as_deref())
                    .duration_ms(task_start.elapsed().as_millis() as u64)
                    .diff_fetch_ms(diff_fetch_ms)
                    .llm_total_ms(llm_ms)
                    .diff_stats(
                        diff_bytes,
                        diff_files_total,
                        files_reviewed,
                        diff_files_total.saturating_sub(files_reviewed),
                    )
                    .comments(&comments, Some(&summary))
                    .tokens(
                        review_result.total_prompt_tokens,
                        review_result.total_completion_tokens,
                        review_result.total_tokens,
                    )
                    .file_metrics(file_metric_events)
                    .hotspot_details(
                        review_result
                            .hotspots
                            .iter()
                            .map(|h| HotspotDetail {
                                file_path: h.file_path.display().to_string(),
                                risk_score: h.risk_score,
                                reasons: h.reasons.clone(),
                            })
                            .collect(),
                    )
                    .convention_suppressed(review_result.convention_suppressed_count)
                    .comments_by_pass(review_result.comments_by_pass)
                    .agent_activity(review_result.agent_activity.as_ref())
                    .build();
            emit_wide_event(&event);
            AppState::complete_review(&state, &review_id, comments, summary, files_reviewed, event)
                .await;

            // Generate AI-powered PR summary if enabled
            if let Some(ref cfg) = summary_config {
                generate_and_store_pr_summary(&state, &review_id, &diff_content, cfg).await;
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {}", e);
            let event = ReviewEventBuilder::new(&review_id, "review.failed", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .diff_fetch_ms(diff_fetch_ms)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
        Err(_) => {
            let err_msg = "Review timed out after 5 minutes".to_string();
            let event = ReviewEventBuilder::new(&review_id, "review.timeout", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .diff_fetch_ms(diff_fetch_ms)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;

    // Persist to storage backend (PostgreSQL or JSON)
    {
        let reviews = state.reviews.read().await;
        if let Some(session) = reviews.get(&review_id) {
            if let Err(e) = state.storage.save_review(session).await {
                warn!(review_id = %review_id, "Failed to persist review to storage: {}", e);
            }
            if let Some(ref event) = session.event {
                if let Err(e) = state.storage.save_event(event).await {
                    warn!(review_id = %review_id, "Failed to persist event to storage: {}", e);
                }
            }
        }
    }
}

fn get_diff_from_git(
    repo_path: &std::path::Path,
    source: &str,
    base: Option<&str>,
) -> anyhow::Result<String> {
    use std::process::Command;

    let output = match source {
        "staged" => Command::new("git")
            .args(["diff", "--cached"])
            .current_dir(repo_path)
            .output()?,
        "branch" => {
            let base_branch = base.unwrap_or("main");
            Command::new("git")
                .args(["diff", &format!("{}...HEAD", base_branch)])
                .current_dir(repo_path)
                .output()?
        }
        _ => Command::new("git")
            .args(["diff", "HEAD~1"])
            .current_dir(repo_path)
            .output()?,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn get_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ReviewSession>, StatusCode> {
    // Check in-memory first (active reviews with progress tracking)
    {
        let reviews = state.reviews.read().await;
        if let Some(session) = reviews.get(&id) {
            return Ok(Json(session.clone()));
        }
    }
    // Fall back to storage backend (historical reviews in PostgreSQL)
    match state.storage.get_review(&id).await {
        Ok(Some(session)) => Ok(Json(session)),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn list_reviews(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListReviewsParams>,
) -> Json<Vec<ReviewListItem>> {
    let page = params.page.unwrap_or(1).clamp(1, 10_000);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);

    // Check in-memory first
    let mut list: Vec<ReviewListItem> = {
        let reviews = state.reviews.read().await;
        reviews.values().map(ReviewListItem::from_session).collect()
    };

    // Fall back to storage backend for historical reviews not in memory
    let limit = (page * per_page + per_page) as i64; // fetch enough to cover requested page
    if let Ok(stored) = state.storage.list_reviews(limit, 0).await {
        let in_memory_ids: std::collections::HashSet<String> =
            list.iter().map(|r| r.id.clone()).collect();
        for session in &stored {
            if !in_memory_ids.contains(&session.id) {
                list.push(ReviewListItem::from_session(session));
            }
        }
    }

    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    let start = (page - 1).saturating_mul(per_page);
    let list = if start < list.len() {
        let end = list.len().min(start.saturating_add(per_page));
        list[start..end].to_vec()
    } else {
        Vec::new()
    };

    Json(list)
}

pub async fn delete_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Remove from in-memory state
    {
        let mut reviews = state.reviews.write().await;
        reviews.remove(&id);
    }
    // Remove from storage backend
    if let Err(e) = state.storage.delete_review(&id).await {
        warn!("Failed to delete review {} from storage: {}", id, e);
    }
    Ok(Json(serde_json::json!({ "ok": true, "deleted": id })))
}

#[derive(Deserialize)]
pub struct PruneParams {
    pub max_age_days: Option<i64>,
    pub max_count: Option<usize>,
}

pub async fn prune_reviews(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PruneParams>,
) -> Json<serde_json::Value> {
    let max_age_secs = params.max_age_days.unwrap_or(30).max(1) * 86400;
    let max_count = params.max_count.unwrap_or(1000).max(1);

    // Prune in-memory state first
    AppState::prune_old_reviews(&state).await;

    match state.storage.prune(max_age_secs, max_count).await {
        Ok(pruned) => {
            info!("Pruned {} old reviews", pruned);
            Json(serde_json::json!({ "ok": true, "pruned": pruned }))
        }
        Err(e) => {
            warn!("Prune failed: {}", e);
            Json(serde_json::json!({ "ok": false, "error": e.to_string() }))
        }
    }
}

pub async fn submit_feedback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, StatusCode> {
    // Validate action
    if request.action != "accept" && request.action != "reject" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let comment_id_for_storage = request.comment_id.clone();

    let mut reviews = state.reviews.write().await;
    let session = reviews.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    let comment = session
        .comments
        .iter_mut()
        .find(|c| c.id == request.comment_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    // Capture comment data for convention store before mutating
    let semantic_comment = comment.clone();
    let comment_content = comment.content.clone();
    let comment_category = comment.category.to_string();
    let file_patterns = crate::review::derive_file_patterns(&comment.file_path);
    let primary_file_pattern = file_patterns.first().map(String::as_str);
    let is_accepted = request.action == "accept";

    comment.feedback = Some(request.action);
    drop(reviews);

    AppState::save_reviews_async(&state);

    // Persist feedback to storage backend
    let _ = state
        .storage
        .update_comment_feedback(
            &id,
            &comment_id_for_storage,
            if is_accepted { "accept" } else { "reject" },
        )
        .await;

    // Record enhanced feedback pattern stats
    {
        let config = state.config.read().await.clone();
        let mut feedback_store = crate::review::load_feedback_store(&config);
        feedback_store.record_feedback_patterns(&comment_category, &file_patterns, is_accepted);
        let _ = crate::review::save_feedback_store(&config.feedback_path, &feedback_store);
        let _ = crate::review::record_semantic_feedback_examples(
            &config,
            std::slice::from_ref(&semantic_comment),
            is_accepted,
        )
        .await;
    }

    // Record in convention store for learned patterns
    let config = state.config.read().await;
    let convention_path = config
        .convention_store_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|d| d.join("diffscope").join("conventions.json")));
    drop(config);

    if let Some(ref cpath) = convention_path {
        let json = std::fs::read_to_string(cpath).ok();
        let mut cstore = json
            .as_deref()
            .and_then(|j| ConventionStore::from_json(j).ok())
            .unwrap_or_default();
        let now = chrono::Utc::now().to_rfc3339();
        cstore.record_feedback(
            &comment_content,
            &comment_category,
            is_accepted,
            primary_file_pattern,
            &now,
        );
        if let Ok(out_json) = cstore.to_json() {
            if let Some(parent) = cpath.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(cpath, out_json);
        }
    }

    Ok(Json(FeedbackResponse { ok: true }))
}

pub async fn get_doctor(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let config = state.config.read().await.clone();

    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".to_string());

    let mut result = serde_json::json!({
        "config": {
            "model": config.model,
            "adapter": config.adapter,
            "base_url": base_url,
            "api_key_set": config.api_key.is_some(),
            "context_window": config.context_window,
        },
        "endpoint_reachable": false,
        "endpoint_type": null,
        "models": [],
        "recommended_model": null,
    });

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Json(result),
    };

    // Check Ollama
    let ollama_url = format!("{}/api/tags", base_url);
    if let Ok(resp) = client.get(&ollama_url).send().await {
        if resp.status().is_success() {
            result["endpoint_reachable"] = serde_json::json!(true);
            result["endpoint_type"] = serde_json::json!("ollama");
            if let Ok(body) = resp.text().await {
                if let Ok(models) =
                    crate::core::offline::OfflineModelManager::parse_model_list(&body)
                {
                    let model_names: Vec<serde_json::Value> = models
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "name": m.name,
                                "size_mb": m.size_mb,
                                "quantization": m.quantization,
                                "family": m.family,
                                "parameter_size": m.parameter_size,
                            })
                        })
                        .collect();
                    result["models"] = serde_json::json!(model_names);

                    let mut manager = crate::core::offline::OfflineModelManager::new(&base_url);
                    manager.set_models(models);
                    if let Some(rec) = manager.recommend_review_model() {
                        result["recommended_model"] = serde_json::json!(rec.name);
                    }
                }
            }
        }
    }

    // Check OpenAI-compatible
    if !result["endpoint_reachable"].as_bool().unwrap_or(false) {
        let openai_url = format!("{}/v1/models", base_url);
        if let Ok(resp) = client.get(&openai_url).send().await {
            if resp.status().is_success() {
                result["endpoint_reachable"] = serde_json::json!(true);
                result["endpoint_type"] = serde_json::json!("openai-compatible");
            }
        }
    }

    Json(result)
}

pub async fn get_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    let mut value = serde_json::to_value(&*config).unwrap_or_default();
    if let Some(obj) = value.as_object_mut() {
        mask_config_secrets(obj);
    }
    Json(value)
}

pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut config = state.config.write().await;

    let mut current = serde_json::to_value(&*config).unwrap_or_default();
    if let (Some(current_obj), Some(updates_obj)) = (current.as_object_mut(), updates.as_object()) {
        for (key, value) in updates_obj {
            // Skip masked secret fields (don't overwrite with "***")
            if value.as_str() == Some("***") {
                continue;
            }
            current_obj.insert(key.clone(), value.clone());
        }
    }

    let new_config: crate::config::Config = serde_json::from_value(current)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid config: {}", e)))?;

    *config = new_config;
    config.normalize();

    // Build response while still holding the write lock
    let mut result = serde_json::to_value(&*config).unwrap_or_default();
    if let Some(obj) = result.as_object_mut() {
        mask_config_secrets(obj);
    }

    drop(config);

    // Persist config to disk
    AppState::save_config_async(&state);

    Ok(Json(result))
}

/// Mask all secret fields in a config object for safe serialization.
fn mask_config_secrets(obj: &mut serde_json::Map<String, serde_json::Value>) {
    for key in &[
        "api_key",
        "github_token",
        "github_client_secret",
        "github_private_key",
        "github_webhook_secret",
    ] {
        if obj.get(*key).and_then(|v| v.as_str()).is_some() {
            obj.insert(key.to_string(), serde_json::json!("***"));
        }
    }
    mask_provider_api_keys(obj);
}

/// Mask api_key fields inside the providers map for safe serialization.
fn mask_provider_api_keys(obj: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(serde_json::Value::Object(providers)) = obj.get_mut("providers") {
        for (_name, provider_val) in providers.iter_mut() {
            if let serde_json::Value::Object(provider) = provider_val {
                if provider.get("api_key").and_then(|v| v.as_str()).is_some() {
                    provider.insert("api_key".to_string(), serde_json::json!("***"));
                }
            }
        }
    }
}

// === Provider test types and handler ===

#[derive(Deserialize)]
pub struct TestProviderRequest {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Serialize)]
pub struct TestProviderResponse {
    pub ok: bool,
    pub message: String,
    pub models: Vec<String>,
}

pub async fn test_provider(Json(request): Json<TestProviderRequest>) -> Json<TestProviderResponse> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Json(TestProviderResponse {
                ok: false,
                message: format!("Failed to create HTTP client: {}", e),
                models: Vec::new(),
            });
        }
    };

    let provider = request.provider.to_lowercase();

    match provider.as_str() {
        "ollama" => {
            let base_url = request
                .base_url
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut models = Vec::new();
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let Some(model_list) = body.get("models").and_then(|m| m.as_array()) {
                            for m in model_list {
                                if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                                    models.push(name.to_string());
                                }
                            }
                        }
                    }
                    Json(TestProviderResponse {
                        ok: true,
                        message: format!("Connected to Ollama. Found {} models.", models.len()),
                        models,
                    })
                }
                Ok(resp) => Json(TestProviderResponse {
                    ok: false,
                    message: format!("Ollama returned status {}", resp.status()),
                    models: Vec::new(),
                }),
                Err(e) => Json(TestProviderResponse {
                    ok: false,
                    message: format!("Failed to connect to Ollama at {}: {}", url, e),
                    models: Vec::new(),
                }),
            }
        }
        "openai" | "openrouter" => {
            let default_base = if provider == "openrouter" {
                "https://openrouter.ai/api"
            } else {
                "https://api.openai.com"
            };
            let base_url = request.base_url.unwrap_or_else(|| default_base.to_string());
            let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
            let api_key = request.api_key.unwrap_or_default();
            if api_key.is_empty() {
                return Json(TestProviderResponse {
                    ok: false,
                    message: "API key is required".to_string(),
                    models: Vec::new(),
                });
            }
            let req = client
                .get(&url)
                .header("Authorization", format!("Bearer {}", api_key));
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut models = Vec::new();
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                            for m in data {
                                if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                                    models.push(id.to_string());
                                }
                            }
                        }
                    }
                    Json(TestProviderResponse {
                        ok: true,
                        message: format!(
                            "Connected to {}. Found {} models.",
                            provider,
                            models.len()
                        ),
                        models,
                    })
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    let msg = if status.as_u16() == 401 {
                        "Authentication failed. Check your API key.".to_string()
                    } else {
                        format!("{} returned status {}: {}", provider, status, body)
                    };
                    Json(TestProviderResponse {
                        ok: false,
                        message: msg,
                        models: Vec::new(),
                    })
                }
                Err(e) => Json(TestProviderResponse {
                    ok: false,
                    message: format!("Failed to connect to {}: {}", provider, e),
                    models: Vec::new(),
                }),
            }
        }
        "anthropic" => {
            let base_url = request
                .base_url
                .unwrap_or_else(|| "https://api.anthropic.com".to_string());
            let api_key = request.api_key.unwrap_or_default();
            if api_key.is_empty() {
                return Json(TestProviderResponse {
                    ok: false,
                    message: "API key is required".to_string(),
                    models: Vec::new(),
                });
            }
            let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": "claude-haiku-4-5-20251001",
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}]
            });
            let req = client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body);
            match req.send().await {
                Ok(resp) if resp.status().is_success() => Json(TestProviderResponse {
                    ok: true,
                    message: "Connected to Anthropic API.".to_string(),
                    models: vec![
                        "claude-sonnet-4-6".to_string(),
                        "claude-opus-4-6".to_string(),
                        "claude-haiku-4-5-20251001".to_string(),
                    ],
                }),
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    let msg = if status.as_u16() == 401 {
                        "Authentication failed. Check your API key.".to_string()
                    } else {
                        format!("Anthropic returned status {}: {}", status, body_text)
                    };
                    Json(TestProviderResponse {
                        ok: false,
                        message: msg,
                        models: Vec::new(),
                    })
                }
                Err(e) => Json(TestProviderResponse {
                    ok: false,
                    message: format!("Failed to connect to Anthropic: {}", e),
                    models: Vec::new(),
                }),
            }
        }
        _ => Json(TestProviderResponse {
            ok: false,
            message: format!("Unknown provider: {}", request.provider),
            models: Vec::new(),
        }),
    }
}

// === GitHub API helpers (use shared HTTP client for connection pooling) ===

fn log_rate_limit(resp: &reqwest::Response) {
    if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
        if let Ok(remaining_str) = remaining.to_str() {
            if let Ok(n) = remaining_str.parse::<u32>() {
                if n < 10 {
                    warn!(remaining = n, "GitHub API rate limit low");
                }
            }
        }
    }
}

async fn github_api_get(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<reqwest::Response, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {}", e))?;

    log_rate_limit(&resp);

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        // Check for rate limit
        if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
            if remaining.to_str().unwrap_or("1") == "0" {
                return Err("GitHub API rate limit exceeded. Wait and retry.".to_string());
            }
        }
    }

    Ok(resp)
}

async fn github_api_post(
    client: &reqwest::Client,
    token: &str,
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, String> {
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {}", e))?;

    log_rate_limit(&resp);
    Ok(resp)
}

/// GET a GitHub API URL, returning the raw diff text (used for PR diffs).
async fn github_api_get_diff(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3.diff")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {}", e))?;

    log_rate_limit(&resp);

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API returned {}: {}", status, body));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read diff response: {}", e))?;

    // Enforce diff size limit
    if text.len() > MAX_DIFF_SIZE {
        return Err(format!(
            "PR diff is {} MB, exceeds limit of {} MB",
            text.len() / (1024 * 1024),
            MAX_DIFF_SIZE / (1024 * 1024),
        ));
    }

    Ok(text)
}

// === GitHub integration types and handlers ===

#[derive(Serialize)]
pub struct GhStatusResponse {
    pub authenticated: bool,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub scopes: Vec<String>,
}

pub async fn get_gh_status(State(state): State<Arc<AppState>>) -> Json<GhStatusResponse> {
    let config = state.config.read().await;
    let token = match config.github.token.as_deref() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };
    drop(config);

    let resp = match github_api_get(&state.http_client, &token, "https://api.github.com/user").await
    {
        Ok(r) => r,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };

    // Extract scopes from response header
    let scopes: Vec<String> = resp
        .headers()
        .get("x-oauth-scopes")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|scope| scope.trim().to_string())
                .filter(|scope| !scope.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if !resp.status().is_success() {
        return Json(GhStatusResponse {
            authenticated: false,
            username: None,
            avatar_url: None,
            scopes: Vec::new(),
        });
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };

    let username = body
        .get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let avatar_url = body
        .get("avatar_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Json(GhStatusResponse {
        authenticated: true,
        username,
        avatar_url,
        scopes,
    })
}

#[derive(Deserialize)]
pub struct GhReposParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
    pub search: Option<String>,
}

#[derive(Serialize)]
pub struct GhRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub updated_at: String,
    pub open_prs: usize,
    pub default_branch: String,
    pub stargazers_count: u32,
    pub private: bool,
}

pub async fn get_gh_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GhReposParams>,
) -> Result<Json<Vec<GhRepo>>, (StatusCode, String)> {
    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let page = params.page.unwrap_or(1).clamp(1, 10_000);

    if let Some(ref search) = params.search {
        // First, get the authenticated user's login for the search query
        let username = get_github_username(&state.http_client, &token)
            .await
            .unwrap_or_default();
        let user_qualifier = if username.is_empty() {
            String::new()
        } else {
            format!("+user:{}", urlencoded(&username))
        };
        let url = format!(
            "https://api.github.com/search/repositories?q={}{}&per_page={}",
            urlencoded(search),
            user_qualifier,
            per_page,
        );

        let resp = github_api_get(&state.http_client, &token, &url)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("GitHub API returned {}: {}", status, body),
            ));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse response: {}", e),
            )
        })?;

        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let repos: Vec<GhRepo> = items
            .into_iter()
            .map(|item| GhRepo {
                full_name: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                language: item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                open_prs: 0,
                default_branch: item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string(),
                stargazers_count: item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                private: item
                    .get("private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .collect();

        Ok(Json(repos))
    } else {
        let url = format!(
            "https://api.github.com/user/repos?sort=updated&per_page={}&page={}",
            per_page, page,
        );

        let resp = github_api_get(&state.http_client, &token, &url)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("GitHub API returned {}: {}", status, body),
            ));
        }

        let items: Vec<serde_json::Value> = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse response: {}", e),
            )
        })?;

        let repos: Vec<GhRepo> = items
            .into_iter()
            .map(|item| GhRepo {
                full_name: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                language: item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                open_prs: 0,
                default_branch: item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string(),
                stargazers_count: item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                private: item
                    .get("private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .collect();

        Ok(Json(repos))
    }
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(byte >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(byte & 0x0F) as usize]));
            }
        }
    }
    out
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

/// Fetch the authenticated user's login from GitHub API.
async fn get_github_username(client: &reqwest::Client, token: &str) -> Result<String, String> {
    let resp = github_api_get(client, token, "https://api.github.com/user").await?;
    if !resp.status().is_success() {
        return Err("Failed to get user info".to_string());
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse user response: {}", e))?;
    body.get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No login field in user response".to_string())
}

// === GitHub PRs ===

#[derive(Deserialize)]
pub struct GhPrsParams {
    pub repo: String,
    pub state: Option<String>,
}

#[derive(Serialize)]
pub struct GhPullRequest {
    pub number: u32,
    pub title: String,
    pub author: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub head_branch: String,
    pub base_branch: String,
    pub labels: Vec<String>,
    pub draft: bool,
}

/// Regex for validating repo names: owner/repo
fn is_valid_repo_name(repo: &str) -> bool {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^[a-zA-Z0-9._-]+/[a-zA-Z0-9._-]+$").unwrap()
    });
    RE.is_match(repo)
}

pub async fn get_gh_prs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GhPrsParams>,
) -> Result<Json<Vec<GhPullRequest>>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    let pr_state = params.state.as_deref().unwrap_or("open");
    if !matches!(pr_state, "open" | "closed" | "all" | "merged") {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid state. Must be open, closed, merged, or all.".to_string(),
        ));
    }

    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    // GitHub API uses "open" or "closed" for state; "all" is also valid.
    // "merged" is not a valid state filter in the API; use "closed" and filter by merged_at.
    let api_state = match pr_state {
        "merged" => "closed",
        other => other,
    };

    let url = format!(
        "https://api.github.com/repos/{}/pulls?state={}&per_page=30&sort=updated&direction=desc",
        params.repo, api_state,
    );

    let resp = github_api_get(&state.http_client, &token, &url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("GitHub API returned {}: {}", status, body),
        ));
    }

    let items: Vec<serde_json::Value> = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to parse response: {}", e),
        )
    })?;

    let prs: Vec<GhPullRequest> = items
        .into_iter()
        .filter(|item| {
            // If the user asked for "merged", only include PRs that have merged_at set
            if pr_state == "merged" {
                item.get("merged_at").map(|v| !v.is_null()).unwrap_or(false)
            } else {
                true
            }
        })
        .map(|item| {
            let labels = item
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| {
                            l.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();

            let state_str = if item.get("merged_at").map(|v| !v.is_null()).unwrap_or(false) {
                "merged".to_string()
            } else {
                item.get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };

            GhPullRequest {
                number: item.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                title: item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                author: item
                    .get("user")
                    .and_then(|v| v.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                state: state_str,
                created_at: item
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                additions: 0,
                deletions: 0,
                changed_files: 0,
                head_branch: item
                    .get("head")
                    .and_then(|v| v.get("ref"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                base_branch: item
                    .get("base")
                    .and_then(|v| v.get("ref"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                labels,
                draft: item.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
            }
        })
        .collect();

    Ok(Json(prs))
}

// === GitHub PR Review ===

#[derive(Deserialize)]
pub struct StartPrReviewRequest {
    pub repo: String,
    pub pr_number: u32,
    pub post_results: bool,
}

#[tracing::instrument(name = "api.start_pr_review", skip(state, request), fields(repo = %request.repo, pr_number = request.pr_number))]
pub async fn start_pr_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartPrReviewRequest>,
) -> Result<Json<StartReviewResponse>, (StatusCode, String)> {
    info!(repo = %request.repo, pr = request.pr_number, post_results = request.post_results, "Starting PR review");

    if !is_valid_repo_name(&request.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if request.pr_number == 0 || request.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    // Fetch the diff via GitHub API
    let diff_url = format!(
        "https://api.github.com/repos/{}/pulls/{}",
        request.repo, request.pr_number,
    );
    let diff_content = github_api_get_diff(&state.http_client, &token, &diff_url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    let id = Uuid::new_v4().to_string();
    let diff_source = format!("pr:{}#{}", request.repo, request.pr_number);

    let session = ReviewSession {
        id: id.clone(),
        status: ReviewStatus::Pending,
        diff_source: diff_source.clone(),
        started_at: current_timestamp(),
        completed_at: None,
        comments: Vec::new(),
        summary: None,
        files_reviewed: 0,
        error: None,
        pr_summary_text: None,
        diff_content: Some(diff_content.clone()),
        event: None,
        progress: None,
    };

    state.reviews.write().await.insert(id.clone(), session);

    let state_clone = state.clone();
    let review_id = id.clone();
    let repo = request.repo.clone();
    let pr_number = request.pr_number;
    let post_results = request.post_results;

    tokio::spawn(async move {
        run_pr_review_task(
            state_clone,
            review_id,
            diff_content,
            repo,
            pr_number,
            post_results,
        )
        .await;
    });

    Ok(Json(StartReviewResponse {
        id,
        status: ReviewStatus::Pending,
    }))
}

async fn run_pr_review_task(
    state: Arc<AppState>,
    review_id: String,
    diff_content: String,
    repo: String,
    pr_number: u32,
    post_results: bool,
) {
    let _permit = match state.review_semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            AppState::fail_review(
                &state,
                &review_id,
                "Review semaphore closed".to_string(),
                None,
            )
            .await;
            return;
        }
    };

    let task_start = std::time::Instant::now();
    let diff_source = format!("pr:{}#{}", repo, pr_number);
    AppState::mark_running(&state, &review_id).await;

    let config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();
    let github_token = config.github.token.clone();
    let model = config.model.clone();
    let provider = config.adapter.clone();
    let base_url = config.base_url.clone();
    let summary_config = if config.smart_review_summary {
        Some(config.clone())
    } else {
        None
    };

    let diff_bytes = diff_content.len();
    let diff_files_total = count_diff_files(&diff_content);

    if diff_content.trim().is_empty() {
        let event = ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
            .provider(provider.as_deref())
            .base_url(base_url.as_deref())
            .duration_ms(task_start.elapsed().as_millis() as u64)
            .github(&repo, pr_number)
            .build();
        emit_wide_event(&event);
        AppState::complete_review(
            &state,
            &review_id,
            Vec::new(),
            CommentSynthesizer::generate_summary(&[]),
            0,
            event,
        )
        .await;
        AppState::save_reviews_async(&state);
        return;
    }

    let llm_start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::review::review_diff_content_raw(&diff_content, config, &repo_path),
    )
    .await;
    let llm_ms = llm_start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(review_result)) => {
            let comments = review_result.comments;
            let summary = CommentSynthesizer::generate_summary(&comments);
            let files_reviewed = count_reviewed_files(&comments);

            let mut github_posted = false;
            if post_results && !comments.is_empty() {
                if let Some(ref token) = github_token {
                    github_posted = post_pr_review_comments(
                        &state.http_client,
                        token,
                        &repo,
                        pr_number,
                        &comments,
                        Some(&summary),
                    )
                    .await
                    .is_ok();
                }
            }

            let file_metric_events: Vec<FileMetricEvent> = review_result
                .file_metrics
                .iter()
                .map(|m| FileMetricEvent {
                    file_path: m.file_path.display().to_string(),
                    latency_ms: m.latency_ms,
                    prompt_tokens: m.prompt_tokens,
                    completion_tokens: m.completion_tokens,
                    total_tokens: m.total_tokens,
                    comment_count: m.comment_count,
                })
                .collect();
            let event =
                ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
                    .provider(provider.as_deref())
                    .base_url(base_url.as_deref())
                    .duration_ms(task_start.elapsed().as_millis() as u64)
                    .llm_total_ms(llm_ms)
                    .diff_stats(
                        diff_bytes,
                        diff_files_total,
                        files_reviewed,
                        diff_files_total.saturating_sub(files_reviewed),
                    )
                    .comments(&comments, Some(&summary))
                    .tokens(
                        review_result.total_prompt_tokens,
                        review_result.total_completion_tokens,
                        review_result.total_tokens,
                    )
                    .file_metrics(file_metric_events)
                    .hotspot_details(
                        review_result
                            .hotspots
                            .iter()
                            .map(|h| HotspotDetail {
                                file_path: h.file_path.display().to_string(),
                                risk_score: h.risk_score,
                                reasons: h.reasons.clone(),
                            })
                            .collect(),
                    )
                    .convention_suppressed(review_result.convention_suppressed_count)
                    .comments_by_pass(review_result.comments_by_pass)
                    .agent_activity(review_result.agent_activity.as_ref())
                    .github(&repo, pr_number)
                    .github_posted(github_posted)
                    .build();
            emit_wide_event(&event);
            AppState::complete_review(&state, &review_id, comments, summary, files_reviewed, event)
                .await;

            // Generate AI-powered PR summary if enabled
            if let Some(ref cfg) = summary_config {
                generate_and_store_pr_summary(&state, &review_id, &diff_content, cfg).await;
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {}", e);
            let event = ReviewEventBuilder::new(&review_id, "review.failed", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .github(&repo, pr_number)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
        Err(_) => {
            let err_msg = "Review timed out after 5 minutes".to_string();
            let event = ReviewEventBuilder::new(&review_id, "review.timeout", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .github(&repo, pr_number)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;

    // Persist to storage backend (PostgreSQL or JSON)
    {
        let reviews = state.reviews.read().await;
        if let Some(session) = reviews.get(&review_id) {
            if let Err(e) = state.storage.save_review(session).await {
                warn!(review_id = %review_id, "Failed to persist PR review to storage: {}", e);
            }
            if let Some(ref event) = session.event {
                if let Err(e) = state.storage.save_event(event).await {
                    warn!(review_id = %review_id, "Failed to persist PR event to storage: {}", e);
                }
            }
        }
    }
}

/// Generate an AI-powered PR summary and store it in the review session.
/// Called after a successful review when `smart_review_summary` is enabled.
///
/// GitIntegration contains a raw pointer and is not `Sync`, so git operations
/// are performed in a blocking task before the async LLM call.
pub(super) async fn generate_and_store_pr_summary(
    state: &Arc<AppState>,
    review_id: &str,
    diff_content: &str,
    config: &crate::config::Config,
) {
    use crate::core::{DiffParser, PRSummaryGenerator, SummaryOptions};

    let diffs = match DiffParser::parse_unified_diff(diff_content) {
        Ok(d) => d,
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (diff parse error): {}", e);
            return;
        }
    };

    // Extract recent commits in a blocking task (GitIntegration is not Sync)
    let repo_path = state.repo_path.clone();
    let commits = match tokio::task::spawn_blocking(move || {
        let git = crate::core::GitIntegration::new(&repo_path)?;
        git.get_recent_commits(10)
    })
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            warn!(review_id = %review_id, "PR summary skipped (git error): {}", e);
            return;
        }
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (blocking task failed): {}", e);
            return;
        }
    };

    // Use Fast model for PR summary generation (lightweight task)
    let fast_config = config.to_model_config_for_role(crate::config::ModelRole::Fast);
    let adapter = match crate::adapters::llm::create_adapter(&fast_config) {
        Ok(a) => a,
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (adapter error): {}", e);
            return;
        }
    };

    let options = SummaryOptions {
        include_diagram: false,
    };

    match PRSummaryGenerator::generate_summary_with_commits(
        &diffs,
        &commits,
        adapter.as_ref(),
        options,
    )
    .await
    {
        Ok(summary) => {
            let markdown = summary.to_markdown();
            info!(review_id = %review_id, "PR summary generated ({} chars)", markdown.len());
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(review_id) {
                session.pr_summary_text = Some(markdown);
            }
        }
        Err(e) => {
            warn!(review_id = %review_id, "PR summary generation failed: {}", e);
        }
    }
}

pub(super) async fn post_pr_review_comments(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    pr_number: u32,
    comments: &[crate::core::Comment],
    summary: Option<&crate::core::comment::ReviewSummary>,
) -> Result<(), String> {
    // Fetch PR head SHA (required for inline comments)
    let pr_url = format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_number,);
    let pr_resp = github_api_get(client, token, &pr_url).await?;
    if !pr_resp.status().is_success() {
        let status = pr_resp.status();
        let body = pr_resp.text().await.unwrap_or_default();
        return Err(format!("Failed to get PR info {}: {}", status, body));
    }
    let pr_data: serde_json::Value = pr_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse PR response: {}", e))?;
    let commit_id = pr_data
        .get("head")
        .and_then(|h| h.get("sha"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "No head SHA in PR response".to_string())?
        .to_string();

    // Build inline review comments
    let mut inline_comments = Vec::new();
    for c in comments {
        let severity_icon = match c.severity {
            crate::core::comment::Severity::Error => ":rotating_light:",
            crate::core::comment::Severity::Warning => ":warning:",
            crate::core::comment::Severity::Info => ":information_source:",
            crate::core::comment::Severity::Suggestion => ":bulb:",
        };

        let mut comment_body = format!("{} **{}** | {}", severity_icon, c.severity, c.category);
        if c.confidence > 0.0 {
            comment_body.push_str(&format!(
                " | confidence: {}%",
                (c.confidence * 100.0) as u32
            ));
        }
        comment_body.push_str(&format!("\n\n{}", c.content));

        if let Some(ref suggestion) = c.suggestion {
            comment_body.push_str(&format!("\n\n> **Suggestion:** {}", suggestion));
        }
        // Calculate the line span for multi-line suggestions.
        // GitHub requires `start_line` + `line` when a suggestion covers multiple lines.
        let suggestion_line_span: Option<usize> = c
            .code_suggestion
            .as_ref()
            .map(|cs| cs.original_code.lines().count().max(1));

        if let Some(ref cs) = c.code_suggestion {
            if !cs.explanation.is_empty() {
                comment_body.push_str(&format!("\n\n**Suggested fix:** {}", cs.explanation));
            } else {
                comment_body.push_str("\n\n**Suggested fix:**");
            }
            comment_body.push_str(&format!("\n```suggestion\n{}\n```", cs.suggested_code));
        }

        // Normalize file path (strip leading / or a/ b/ prefixes)
        let path = c.file_path.display().to_string();
        let path = path.trim_start_matches('/');
        let path = if path.starts_with("a/") || path.starts_with("b/") {
            &path[2..]
        } else {
            path
        };

        let mut comment_json = serde_json::json!({
            "path": path,
            "line": c.line_number,
            "side": "RIGHT",
            "body": comment_body,
        });

        // For multi-line suggestions, set start_line so GitHub knows the full
        // range of lines being replaced by the suggestion block.
        if let Some(span) = suggestion_line_span {
            if span > 1 {
                let end_line = c.line_number + span - 1;
                comment_json["start_line"] = serde_json::json!(c.line_number);
                comment_json["line"] = serde_json::json!(end_line);
                comment_json["start_side"] = serde_json::json!("RIGHT");
            }
        }

        inline_comments.push(comment_json);
    }

    // Build summary body
    let mut review_body_text = String::from("## DiffScope Review\n\n");
    if let Some(s) = summary {
        review_body_text.push_str(&format!(
            "**Score:** {}/10 | **Findings:** {}\n\n",
            s.overall_score, s.total_comments
        ));
        if !s.recommendations.is_empty() {
            review_body_text.push_str("**Recommendations:**\n");
            for rec in &s.recommendations {
                review_body_text.push_str(&format!("- {}\n", rec));
            }
            review_body_text.push('\n');
        }
    } else {
        review_body_text.push_str(&format!(
            "Found **{}** issue{}.\n\n",
            comments.len(),
            if comments.len() == 1 { "" } else { "s" }
        ));
    }
    review_body_text
        .push_str("_Automated review by [DiffScope](https://github.com/evalops/diffscope)_");

    // Determine event type based on severity
    let has_errors = comments
        .iter()
        .any(|c| matches!(c.severity, crate::core::comment::Severity::Error));
    let event = if has_errors {
        "REQUEST_CHANGES"
    } else {
        "COMMENT"
    };

    let review_payload = serde_json::json!({
        "commit_id": commit_id,
        "body": review_body_text,
        "event": event,
        "comments": inline_comments,
    });

    let url = format!(
        "https://api.github.com/repos/{}/pulls/{}/reviews",
        repo, pr_number,
    );

    let resp = github_api_post(client, token, &url, &review_payload).await?;

    if resp.status().is_success() {
        info!(repo = %repo, pr = pr_number, comments = comments.len(), "Posted inline review to GitHub");
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("GitHub API returned {}: {}", status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ListEventsParams::into_filters tests ===

    #[test]
    fn test_into_filters_all_none() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: None,
            time_to: None,
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, None);
        assert_eq!(filters.model, None);
        assert_eq!(filters.status, None);
        assert!(filters.time_from.is_none());
        assert!(filters.time_to.is_none());
        assert_eq!(filters.github_repo, None);
        assert_eq!(filters.limit, None);
        assert_eq!(filters.offset, None);
    }

    #[test]
    fn test_into_filters_with_source_and_model() {
        let params = ListEventsParams {
            source: Some("staged".to_string()),
            model: Some("claude-opus-4-6".to_string()),
            status: None,
            time_from: None,
            time_to: None,
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, Some("staged".to_string()));
        assert_eq!(filters.model, Some("claude-opus-4-6".to_string()));
    }

    #[test]
    fn test_into_filters_with_status_and_github_repo() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: Some("completed".to_string()),
            time_from: None,
            time_to: None,
            github_repo: Some("owner/repo".to_string()),
            limit: Some(50),
            offset: Some(10),
        };
        let filters = params.into_filters();
        assert_eq!(filters.status, Some("completed".to_string()));
        assert_eq!(filters.github_repo, Some("owner/repo".to_string()));
        assert_eq!(filters.limit, Some(50));
        assert_eq!(filters.offset, Some(10));
    }

    #[test]
    fn test_into_filters_valid_rfc3339_time() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: Some("2024-01-01T00:00:00Z".to_string()),
            time_to: Some("2024-12-31T23:59:59Z".to_string()),
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert!(filters.time_from.is_some());
        assert!(filters.time_to.is_some());
        assert_eq!(
            filters.time_from.unwrap().to_rfc3339(),
            "2024-01-01T00:00:00+00:00"
        );
        assert_eq!(
            filters.time_to.unwrap().to_rfc3339(),
            "2024-12-31T23:59:59+00:00"
        );
    }

    #[test]
    fn test_into_filters_invalid_time_becomes_none() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: Some("not-a-date".to_string()),
            time_to: Some("also-invalid".to_string()),
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert!(filters.time_from.is_none());
        assert!(filters.time_to.is_none());
    }

    #[test]
    fn test_into_filters_all_fields_populated() {
        let params = ListEventsParams {
            source: Some("head".to_string()),
            model: Some("gpt-4".to_string()),
            status: Some("failed".to_string()),
            time_from: Some("2025-06-01T12:00:00Z".to_string()),
            time_to: Some("2025-06-30T12:00:00Z".to_string()),
            github_repo: Some("evalops/diffscope".to_string()),
            limit: Some(100),
            offset: Some(25),
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, Some("head".to_string()));
        assert_eq!(filters.model, Some("gpt-4".to_string()));
        assert_eq!(filters.status, Some("failed".to_string()));
        assert!(filters.time_from.is_some());
        assert!(filters.time_to.is_some());
        assert_eq!(filters.github_repo, Some("evalops/diffscope".to_string()));
        assert_eq!(filters.limit, Some(100));
        assert_eq!(filters.offset, Some(25));
    }

    // === ReviewOverrides Default impl tests ===

    #[test]
    fn test_review_overrides_default() {
        let overrides = ReviewOverrides::default();
        assert_eq!(overrides.model, None);
        assert_eq!(overrides.strictness, None);
        assert_eq!(overrides.review_profile, None);
    }

    #[test]
    fn test_review_overrides_clone() {
        let overrides = ReviewOverrides {
            model: Some("test-model".to_string()),
            strictness: Some(2),
            review_profile: Some("security".to_string()),
        };
        let cloned = overrides.clone();
        assert_eq!(cloned.model, Some("test-model".to_string()));
        assert_eq!(cloned.strictness, Some(2));
        assert_eq!(cloned.review_profile, Some("security".to_string()));
    }

    // === mask_config_secrets tests ===

    #[test]
    fn test_mask_config_secrets_masks_api_key() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "api_key".to_string(),
            serde_json::json!("sk-secret-key-123"),
        );
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
    }

    #[test]
    fn test_mask_config_secrets_masks_all_secret_fields() {
        let mut obj = serde_json::Map::new();
        obj.insert("api_key".to_string(), serde_json::json!("secret1"));
        obj.insert("github_token".to_string(), serde_json::json!("secret2"));
        obj.insert(
            "github_client_secret".to_string(),
            serde_json::json!("secret3"),
        );
        obj.insert(
            "github_private_key".to_string(),
            serde_json::json!("secret4"),
        );
        obj.insert(
            "github_webhook_secret".to_string(),
            serde_json::json!("secret5"),
        );
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(obj.get("github_token").unwrap(), &serde_json::json!("***"));
        assert_eq!(
            obj.get("github_client_secret").unwrap(),
            &serde_json::json!("***")
        );
        assert_eq!(
            obj.get("github_private_key").unwrap(),
            &serde_json::json!("***")
        );
        assert_eq!(
            obj.get("github_webhook_secret").unwrap(),
            &serde_json::json!("***")
        );
    }

    #[test]
    fn test_mask_config_secrets_does_not_mask_null_secrets() {
        let mut obj = serde_json::Map::new();
        obj.insert("api_key".to_string(), serde_json::Value::Null);
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_config_secrets(&mut obj);
        // Null values are not strings, so they should not be masked
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::Value::Null);
    }

    #[test]
    fn test_mask_config_secrets_no_secret_fields_present() {
        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        obj.insert(
            "base_url".to_string(),
            serde_json::json!("http://localhost"),
        );
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
        assert_eq!(
            obj.get("base_url").unwrap(),
            &serde_json::json!("http://localhost")
        );
    }

    // === mask_provider_api_keys tests ===

    #[test]
    fn test_mask_provider_api_keys_masks_nested_keys() {
        let mut obj = serde_json::Map::new();
        let mut providers = serde_json::Map::new();
        let mut openai = serde_json::Map::new();
        openai.insert("api_key".to_string(), serde_json::json!("sk-openai-key"));
        openai.insert(
            "base_url".to_string(),
            serde_json::json!("https://api.openai.com"),
        );
        providers.insert("openai".to_string(), serde_json::Value::Object(openai));
        obj.insert(
            "providers".to_string(),
            serde_json::Value::Object(providers),
        );

        mask_provider_api_keys(&mut obj);

        let providers = obj.get("providers").unwrap().as_object().unwrap();
        let openai = providers.get("openai").unwrap().as_object().unwrap();
        assert_eq!(openai.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(
            openai.get("base_url").unwrap(),
            &serde_json::json!("https://api.openai.com")
        );
    }

    #[test]
    fn test_mask_provider_api_keys_no_providers_field() {
        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_provider_api_keys(&mut obj);
        // Should not panic; nothing to mask
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
    }

    #[test]
    fn test_mask_provider_api_keys_multiple_providers() {
        let mut obj = serde_json::Map::new();
        let mut providers = serde_json::Map::new();

        let mut anthropic = serde_json::Map::new();
        anthropic.insert("api_key".to_string(), serde_json::json!("ant-key"));
        providers.insert(
            "anthropic".to_string(),
            serde_json::Value::Object(anthropic),
        );

        let mut ollama = serde_json::Map::new();
        ollama.insert(
            "base_url".to_string(),
            serde_json::json!("http://localhost:11434"),
        );
        // No api_key for ollama
        providers.insert("ollama".to_string(), serde_json::Value::Object(ollama));

        obj.insert(
            "providers".to_string(),
            serde_json::Value::Object(providers),
        );
        mask_provider_api_keys(&mut obj);

        let providers = obj.get("providers").unwrap().as_object().unwrap();
        let anthropic = providers.get("anthropic").unwrap().as_object().unwrap();
        assert_eq!(anthropic.get("api_key").unwrap(), &serde_json::json!("***"));

        let ollama = providers.get("ollama").unwrap().as_object().unwrap();
        assert!(ollama.get("api_key").is_none());
    }

    // === is_valid_repo_name tests ===

    #[test]
    fn test_is_valid_repo_name_standard() {
        assert!(is_valid_repo_name("owner/repo"));
    }

    #[test]
    fn test_is_valid_repo_name_with_hyphens_and_dots() {
        assert!(is_valid_repo_name("my-org/my-repo.rs"));
    }

    #[test]
    fn test_is_valid_repo_name_with_underscores() {
        assert!(is_valid_repo_name("my_org/my_repo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_empty() {
        assert!(!is_valid_repo_name(""));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_no_slash() {
        assert!(!is_valid_repo_name("justrepo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_spaces() {
        assert!(!is_valid_repo_name("owner/my repo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_multiple_slashes() {
        assert!(!is_valid_repo_name("a/b/c"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_special_chars() {
        assert!(!is_valid_repo_name("owner/repo@v1"));
    }

    // === urlencoded tests ===

    #[test]
    fn test_urlencoded_alphanumeric() {
        assert_eq!(urlencoded("hello123"), "hello123");
    }

    #[test]
    fn test_urlencoded_spaces_become_plus() {
        assert_eq!(urlencoded("hello world"), "hello+world");
    }

    #[test]
    fn test_urlencoded_special_chars_percent_encoded() {
        assert_eq!(urlencoded("a=b&c"), "a%3Db%26c");
    }

    #[test]
    fn test_urlencoded_unreserved_chars_pass_through() {
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn test_urlencoded_empty_string() {
        assert_eq!(urlencoded(""), "");
    }

    #[test]
    fn test_urlencoded_slash() {
        assert_eq!(urlencoded("owner/repo"), "owner%2Frepo");
    }

    // === Request/Response serialization tests ===

    #[test]
    fn test_start_review_request_deserialize_minimal() {
        let json = r#"{"diff_source": "head"}"#;
        let req: StartReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.diff_source, "head");
        assert_eq!(req.base_branch, None);
        assert_eq!(req.diff_content, None);
        assert_eq!(req.title, None);
        assert_eq!(req.model, None);
        assert_eq!(req.strictness, None);
        assert_eq!(req.review_profile, None);
    }

    #[test]
    fn test_start_review_request_deserialize_full() {
        let json = r#"{
            "diff_source": "raw",
            "base_branch": "main",
            "diff_content": "diff --git a/file.rs",
            "title": "Test PR",
            "model": "claude-opus-4-6",
            "strictness": 3,
            "review_profile": "security"
        }"#;
        let req: StartReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.diff_source, "raw");
        assert_eq!(req.base_branch, Some("main".to_string()));
        assert_eq!(req.diff_content, Some("diff --git a/file.rs".to_string()));
        assert_eq!(req.title, Some("Test PR".to_string()));
        assert_eq!(req.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(req.strictness, Some(3));
        assert_eq!(req.review_profile, Some("security".to_string()));
    }

    #[test]
    fn test_start_review_response_serialize() {
        let resp = StartReviewResponse {
            id: "abc-123".to_string(),
            status: ReviewStatus::Pending,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "abc-123");
        assert_eq!(json["status"], "Pending");
    }

    #[test]
    fn test_feedback_request_deserialize() {
        let json = r#"{"comment_id": "cmt-1", "action": "accept"}"#;
        let req: FeedbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.comment_id, "cmt-1");
        assert_eq!(req.action, "accept");
    }

    #[test]
    fn test_feedback_response_serialize() {
        let resp = FeedbackResponse { ok: true };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
    }

    #[test]
    fn test_status_response_serialize() {
        let resp = StatusResponse {
            repo_path: "/path/to/repo".to_string(),
            branch: Some("main".to_string()),
            model: "gpt-4".to_string(),
            adapter: Some("openai".to_string()),
            base_url: None,
            active_reviews: 2,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["repo_path"], "/path/to/repo");
        assert_eq!(json["branch"], "main");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["adapter"], "openai");
        assert!(json["base_url"].is_null());
        assert_eq!(json["active_reviews"], 2);
    }

    #[test]
    fn test_list_reviews_params_deserialize_empty() {
        let json = r#"{}"#;
        let params: ListReviewsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.page, None);
        assert_eq!(params.per_page, None);
    }

    #[test]
    fn test_list_reviews_params_deserialize_with_values() {
        let json = r#"{"page": 3, "per_page": 50}"#;
        let params: ListReviewsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.page, Some(3));
        assert_eq!(params.per_page, Some(50));
    }

    #[test]
    fn test_list_events_params_deserialize() {
        let json = r#"{"source": "head", "limit": 100}"#;
        let params: ListEventsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.source, Some("head".to_string()));
        assert_eq!(params.limit, Some(100));
        assert_eq!(params.model, None);
    }

    #[test]
    fn test_test_provider_request_deserialize() {
        let json = r#"{"provider": "anthropic", "api_key": "sk-ant-123"}"#;
        let req: TestProviderRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.provider, "anthropic");
        assert_eq!(req.api_key, Some("sk-ant-123".to_string()));
        assert_eq!(req.base_url, None);
    }

    #[test]
    fn test_test_provider_response_serialize() {
        let resp = TestProviderResponse {
            ok: true,
            message: "Connected".to_string(),
            models: vec!["model-a".to_string(), "model-b".to_string()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["message"], "Connected");
        assert_eq!(json["models"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_gh_status_response_serialize() {
        let resp = GhStatusResponse {
            authenticated: true,
            username: Some("testuser".to_string()),
            avatar_url: Some("https://example.com/avatar.png".to_string()),
            scopes: vec!["repo".to_string(), "read:org".to_string()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["authenticated"], true);
        assert_eq!(json["username"], "testuser");
        assert_eq!(json["scopes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_gh_repo_serialize() {
        let repo = GhRepo {
            full_name: "owner/repo".to_string(),
            description: Some("A test repo".to_string()),
            language: Some("Rust".to_string()),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            open_prs: 5,
            default_branch: "main".to_string(),
            stargazers_count: 42,
            private: false,
        };
        let json = serde_json::to_value(&repo).unwrap();
        assert_eq!(json["full_name"], "owner/repo");
        assert_eq!(json["language"], "Rust");
        assert_eq!(json["open_prs"], 5);
        assert_eq!(json["stargazers_count"], 42);
        assert_eq!(json["private"], false);
    }

    #[test]
    fn test_gh_pull_request_serialize() {
        let pr = GhPullRequest {
            number: 42,
            title: "Fix bug".to_string(),
            author: "dev".to_string(),
            state: "open".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-02T00:00:00Z".to_string(),
            additions: 10,
            deletions: 5,
            changed_files: 3,
            head_branch: "fix-branch".to_string(),
            base_branch: "main".to_string(),
            labels: vec!["bugfix".to_string()],
            draft: false,
        };
        let json = serde_json::to_value(&pr).unwrap();
        assert_eq!(json["number"], 42);
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["author"], "dev");
        assert_eq!(json["draft"], false);
        assert_eq!(json["labels"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_start_pr_review_request_deserialize() {
        let json = r#"{"repo": "owner/repo", "pr_number": 42, "post_results": true}"#;
        let req: StartPrReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.repo, "owner/repo");
        assert_eq!(req.pr_number, 42);
        assert!(req.post_results);
    }
}
