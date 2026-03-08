use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::state::{AppState, ReviewEvent, ReviewSession, ReviewStatus, MAX_DIFF_SIZE};
use crate::core::comment::CommentSynthesizer;
use std::collections::HashMap;
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

pub async fn start_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartReviewRequest>,
) -> Result<Json<StartReviewResponse>, (StatusCode, String)> {
    // Validate diff_source
    let diff_source = match request.diff_source.as_str() {
        "head" | "staged" | "branch" | "raw" => request.diff_source.clone(),
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid diff_source: must be head, staged, branch, or raw".to_string())),
    };

    // "raw" requires diff_content
    if diff_source == "raw" && request.diff_content.as_ref().map_or(true, |c| c.trim().is_empty()) {
        return Err((StatusCode::BAD_REQUEST, "diff_content is required when diff_source is 'raw'".to_string()));
    }

    // Reject oversized diffs
    if let Some(ref content) = request.diff_content {
        if content.len() > MAX_DIFF_SIZE {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, format!(
                "Diff content exceeds maximum size of {} MB",
                MAX_DIFF_SIZE / (1024 * 1024)
            )));
        }
    }

    info!(diff_source = %diff_source, title = ?request.title, "Starting review");

    // Validate branch name if provided
    if let Some(ref branch) = request.base_branch {
        if branch.is_empty() || branch.len() > 200
            || !branch.chars().all(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
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
        diff_content: None,
        event: None,
    };

    state.reviews.write().await.insert(id.clone(), session);

    let state_clone = state.clone();
    let review_id = id.clone();
    let base_branch = request.base_branch.clone();
    let raw_diff = request.diff_content.clone();

    tokio::spawn(async move {
        run_review_task(state_clone, review_id, diff_source, base_branch, raw_diff).await;
    });

    Ok(Json(StartReviewResponse {
        id,
        status: ReviewStatus::Pending,
    }))
}

fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn run_review_task(
    state: Arc<AppState>,
    review_id: String,
    diff_source: String,
    base_branch: Option<String>,
    raw_diff: Option<String>,
) {
    let task_start = std::time::Instant::now();

    // Update status to Running
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Running;
        }
    }

    let config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();

    // --- wide event seed ---
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
            let event = build_review_event(
                &review_id, "review.failed", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                Some(diff_fetch_ms), None, 0, 0, 0, 0,
                &[], None, Some(&err_msg),
            );
            emit_wide_event(&event);
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(err_msg);
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
            AppState::save_reviews_async(&state);
            return;
        }
    };

    let diff_bytes = diff_content.len();
    let diff_files_total = diff_content.matches("\ndiff --git ").count()
        + if diff_content.starts_with("diff --git ") { 1 } else { 0 };

    // Store diff content for the frontend viewer
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.diff_content = Some(diff_content.clone());
        }
    }

    if diff_content.trim().is_empty() {
        let event = build_review_event(
            &review_id, "review.completed", &diff_source, None,
            &model, provider.as_deref(), base_url.as_deref(),
            task_start.elapsed().as_millis() as u64,
            Some(diff_fetch_ms), None, 0, 0, 0, 0,
            &[], None, None,
        );
        emit_wide_event(&event);
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Complete;
            session.comments = Vec::new();
            session.summary = Some(CommentSynthesizer::generate_summary(&[]));
            session.files_reviewed = 0;
            session.completed_at = Some(current_timestamp());
            session.event = Some(event);
        }
        AppState::save_reviews_async(&state);
        return;
    }

    // Run the review with a 5-minute timeout
    let llm_start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::review::review_diff_content_raw(&diff_content, config, &repo_path),
    )
    .await;
    let llm_ms = llm_start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(comments)) => {
            let summary = CommentSynthesizer::generate_summary(&comments);
            let files_reviewed = {
                let mut files = std::collections::HashSet::new();
                for c in &comments {
                    files.insert(c.file_path.clone());
                }
                files.len()
            };

            let event = build_review_event(
                &review_id, "review.completed", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                Some(diff_fetch_ms), Some(llm_ms),
                diff_bytes, diff_files_total, files_reviewed,
                diff_files_total.saturating_sub(files_reviewed),
                &comments, Some(&summary), None,
            );
            emit_wide_event(&event);

            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Complete;
                session.comments = comments;
                session.summary = Some(summary);
                session.files_reviewed = files_reviewed;
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {}", e);
            let event = build_review_event(
                &review_id, "review.failed", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                Some(diff_fetch_ms), Some(llm_ms),
                diff_bytes, diff_files_total, 0, 0,
                &[], None, Some(&err_msg),
            );
            emit_wide_event(&event);
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(err_msg);
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
        Err(_) => {
            let err_msg = "Review timed out after 5 minutes".to_string();
            let event = build_review_event(
                &review_id, "review.timeout", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                Some(diff_fetch_ms), Some(llm_ms),
                diff_bytes, diff_files_total, 0, 0,
                &[], None, Some(&err_msg),
            );
            emit_wide_event(&event);
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(err_msg);
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;
}

/// Build a wide event from review results.
#[allow(clippy::too_many_arguments)]
fn build_review_event(
    review_id: &str,
    event_type: &str,
    diff_source: &str,
    title: Option<&str>,
    model: &str,
    provider: Option<&str>,
    base_url: Option<&str>,
    duration_ms: u64,
    diff_fetch_ms: Option<u64>,
    llm_total_ms: Option<u64>,
    diff_bytes: usize,
    diff_files_total: usize,
    diff_files_reviewed: usize,
    diff_files_skipped: usize,
    comments: &[crate::core::Comment],
    summary: Option<&crate::core::comment::ReviewSummary>,
    error: Option<&str>,
) -> ReviewEvent {
    let mut by_severity: HashMap<String, usize> = HashMap::new();
    let mut by_category: HashMap<String, usize> = HashMap::new();
    for c in comments {
        *by_severity.entry(format!("{:?}", c.severity)).or_default() += 1;
        *by_category.entry(format!("{:?}", c.category)).or_default() += 1;
    }

    ReviewEvent {
        review_id: review_id.to_string(),
        event_type: event_type.to_string(),
        diff_source: diff_source.to_string(),
        title: title.map(str::to_string),
        model: model.to_string(),
        provider: provider.map(str::to_string),
        base_url: base_url.map(str::to_string),
        duration_ms,
        diff_fetch_ms,
        llm_total_ms,
        diff_bytes,
        diff_files_total,
        diff_files_reviewed,
        diff_files_skipped,
        comments_total: comments.len(),
        comments_by_severity: by_severity,
        comments_by_category: by_category,
        overall_score: summary.map(|s| s.overall_score),
        hotspots_detected: 0,
        high_risk_files: 0,
        github_posted: false,
        github_repo: None,
        github_pr: None,
        error: error.map(str::to_string),
    }
}

/// Emit a review wide event via structured tracing.
fn emit_wide_event(event: &ReviewEvent) {
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
    let reviews = state.reviews.read().await;
    reviews
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn list_reviews(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListReviewsParams>,
) -> Json<Vec<ReviewSession>> {
    let reviews = state.reviews.read().await;
    let mut list: Vec<ReviewSession> = reviews
        .values()
        .map(|r| {
            let mut r = r.clone();
            r.diff_content = None;
            r
        })
        .collect();
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    let page = params.page.unwrap_or(1).clamp(1, 10_000);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let start = (page - 1).saturating_mul(per_page);
    let list = if start < list.len() {
        let end = list.len().min(start.saturating_add(per_page));
        list[start..end].to_vec()
    } else {
        Vec::new()
    };

    Json(list)
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

    let mut reviews = state.reviews.write().await;
    let session = reviews.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    let comment = session
        .comments
        .iter_mut()
        .find(|c| c.id == request.comment_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    comment.feedback = Some(request.action);
    drop(reviews);

    AppState::save_reviews_async(&state);

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

                    let mut manager =
                        crate::core::offline::OfflineModelManager::new(&base_url);
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
        if obj.contains_key("api_key") {
            obj.insert("api_key".to_string(), serde_json::json!("***"));
        }
        if obj.contains_key("github_token") {
            obj.insert("github_token".to_string(), serde_json::json!("***"));
        }
        mask_provider_api_keys(obj);
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
            if key == "api_key" && value.as_str() == Some("***") {
                continue;
            }
            if key == "github_token" && value.as_str() == Some("***") {
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
        if obj.contains_key("api_key") {
            obj.insert("api_key".to_string(), serde_json::json!("***"));
        }
        if obj.contains_key("github_token") {
            obj.insert("github_token".to_string(), serde_json::json!("***"));
        }
        mask_provider_api_keys(obj);
    }

    drop(config);

    // Persist config to disk
    AppState::save_config_async(&state);

    Ok(Json(result))
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

pub async fn test_provider(
    Json(request): Json<TestProviderRequest>,
) -> Json<TestProviderResponse> {
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
    pub scopes: Vec<String>,
}

pub async fn get_gh_status(
    State(state): State<Arc<AppState>>,
) -> Json<GhStatusResponse> {
    let config = state.config.read().await;
    let token = match config.github_token.as_deref() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                scopes: Vec::new(),
            });
        }
    };
    drop(config);

    let resp = match github_api_get(&state.http_client, &token, "https://api.github.com/user").await {
        Ok(r) => r,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
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
            scopes: Vec::new(),
        });
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                scopes: Vec::new(),
            });
        }
    };

    let username = body
        .get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Json(GhStatusResponse {
        authenticated: true,
        username,
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
}

pub async fn get_gh_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GhReposParams>,
) -> Result<Json<Vec<GhRepo>>, (StatusCode, String)> {
    let config = state.config.read().await;
    let token = config
        .github_token
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
        let username = get_github_username(&state.http_client, &token).await.unwrap_or_default();
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

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to parse response: {}", e)))?;

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

        let items: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to parse response: {}", e)))?;

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
        .github_token
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

    let items: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to parse response: {}", e)))?;

    let prs: Vec<GhPullRequest> = items
        .into_iter()
        .filter(|item| {
            // If the user asked for "merged", only include PRs that have merged_at set
            if pr_state == "merged" {
                item.get("merged_at")
                    .map(|v| !v.is_null())
                    .unwrap_or(false)
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

            let state_str = if item
                .get("merged_at")
                .map(|v| !v.is_null())
                .unwrap_or(false)
            {
                "merged".to_string()
            } else {
                item.get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };

            GhPullRequest {
                number: item
                    .get("number")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
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
                draft: item
                    .get("draft")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
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
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid PR number.".to_string(),
        ));
    }

    let config = state.config.read().await;
    let token = config
        .github_token
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
        diff_content: Some(diff_content.clone()),
        event: None,
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
    let task_start = std::time::Instant::now();
    let diff_source = format!("pr:{}#{}", repo, pr_number);

    // Update status to Running
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Running;
        }
    }

    let config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();
    let github_token = config.github_token.clone();
    let model = config.model.clone();
    let provider = config.adapter.clone();
    let base_url = config.base_url.clone();

    let diff_bytes = diff_content.len();
    let diff_files_total = diff_content.matches("\ndiff --git ").count()
        + if diff_content.starts_with("diff --git ") { 1 } else { 0 };

    if diff_content.trim().is_empty() {
        let mut event = build_review_event(
            &review_id, "review.completed", &diff_source, None,
            &model, provider.as_deref(), base_url.as_deref(),
            task_start.elapsed().as_millis() as u64,
            None, None, 0, 0, 0, 0, &[], None, None,
        );
        event.github_repo = Some(repo.clone());
        event.github_pr = Some(pr_number);
        emit_wide_event(&event);
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Complete;
            session.comments = Vec::new();
            session.summary = Some(CommentSynthesizer::generate_summary(&[]));
            session.files_reviewed = 0;
            session.completed_at = Some(current_timestamp());
            session.event = Some(event);
        }
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
        Ok(Ok(comments)) => {
            let summary = CommentSynthesizer::generate_summary(&comments);
            let files_reviewed = {
                let mut files = std::collections::HashSet::new();
                for c in &comments {
                    files.insert(c.file_path.clone());
                }
                files.len()
            };

            // Post results to GitHub if requested
            let mut github_posted = false;
            if post_results && !comments.is_empty() {
                if let Some(ref token) = github_token {
                    github_posted = post_pr_review_comments(
                        &state.http_client, token, &repo, pr_number, &comments,
                    )
                    .await
                    .is_ok();
                }
            }

            let mut event = build_review_event(
                &review_id, "review.completed", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                None, Some(llm_ms),
                diff_bytes, diff_files_total, files_reviewed,
                diff_files_total.saturating_sub(files_reviewed),
                &comments, Some(&summary), None,
            );
            event.github_posted = github_posted;
            event.github_repo = Some(repo.clone());
            event.github_pr = Some(pr_number);
            emit_wide_event(&event);

            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Complete;
                session.comments = comments;
                session.summary = Some(summary);
                session.files_reviewed = files_reviewed;
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {}", e);
            let mut event = build_review_event(
                &review_id, "review.failed", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                None, Some(llm_ms),
                diff_bytes, diff_files_total, 0, 0,
                &[], None, Some(&err_msg),
            );
            event.github_repo = Some(repo.clone());
            event.github_pr = Some(pr_number);
            emit_wide_event(&event);
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(err_msg);
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
        Err(_) => {
            let err_msg = "Review timed out after 5 minutes".to_string();
            let mut event = build_review_event(
                &review_id, "review.timeout", &diff_source, None,
                &model, provider.as_deref(), base_url.as_deref(),
                task_start.elapsed().as_millis() as u64,
                None, Some(llm_ms),
                diff_bytes, diff_files_total, 0, 0,
                &[], None, Some(&err_msg),
            );
            event.github_repo = Some(repo.clone());
            event.github_pr = Some(pr_number);
            emit_wide_event(&event);
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(err_msg);
                session.completed_at = Some(current_timestamp());
                session.event = Some(event);
            }
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;
}

async fn post_pr_review_comments(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    pr_number: u32,
    comments: &[crate::core::Comment],
) -> Result<(), String> {
    let mut body = String::from("## DiffScope Review\n\n");
    for c in comments {
        body.push_str(&format!(
            "**{:?}** `{}` (line {}): {}\n\n",
            c.severity,
            c.file_path.display(),
            c.line_number,
            c.content
        ));
        if let Some(ref suggestion) = c.suggestion {
            body.push_str(&format!("  > Suggestion: {}\n\n", suggestion));
        }
    }

    let review_body = serde_json::json!({
        "body": body,
        "event": "COMMENT",
    });

    let url = format!(
        "https://api.github.com/repos/{}/pulls/{}/reviews",
        repo, pr_number,
    );

    let resp = github_api_post(client, token, &url, &review_body).await?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("GitHub API returned {}: {}", status, body))
    }
}
