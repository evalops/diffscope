use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::state::{AppState, ReviewSession, ReviewStatus};
use crate::core::comment::CommentSynthesizer;

// === Request/Response types ===

#[derive(Deserialize)]
pub struct StartReviewRequest {
    pub diff_source: String,
    pub base_branch: Option<String>,
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
        "head" | "staged" | "branch" => request.diff_source.clone(),
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid diff_source: must be head, staged, or branch".to_string())),
    };

    // Validate branch name if provided
    if let Some(ref branch) = request.base_branch {
        if branch.is_empty() || branch.len() > 200
            || !branch.chars().all(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        {
            return Err((StatusCode::BAD_REQUEST, "Invalid branch name".to_string()));
        }
    }

    let id = Uuid::new_v4().to_string();

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
        diff_content: None,
    };

    state.reviews.write().await.insert(id.clone(), session);

    let state_clone = state.clone();
    let review_id = id.clone();
    let base_branch = request.base_branch.clone();

    tokio::spawn(async move {
        run_review_task(state_clone, review_id, diff_source, base_branch).await;
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
) {
    // Update status to Running
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Running;
        }
    }

    let config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();

    // Get the diff content based on source
    let diff_result = match diff_source.as_str() {
        "staged" => get_diff_from_git(&repo_path, "staged", None),
        "branch" => {
            let base = base_branch.as_deref().unwrap_or("main");
            get_diff_from_git(&repo_path, "branch", Some(base))
        }
        _ => get_diff_from_git(&repo_path, "head", None),
    };

    let diff_content = match diff_result {
        Ok(diff) => diff,
        Err(e) => {
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(format!("Failed to get diff: {}", e));
                session.completed_at = Some(current_timestamp());
            }
            AppState::save_reviews_async(&state);
            return;
        }
    };

    // Store diff content for the frontend viewer
    {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.diff_content = Some(diff_content.clone());
        }
    }

    if diff_content.trim().is_empty() {
        let mut reviews = state.reviews.write().await;
        if let Some(session) = reviews.get_mut(&review_id) {
            session.status = ReviewStatus::Complete;
            session.comments = Vec::new();
            session.summary = Some(CommentSynthesizer::generate_summary(&[]));
            session.files_reviewed = 0;
            session.completed_at = Some(current_timestamp());
        }
        AppState::save_reviews_async(&state);
        return;
    }

    // Run the review with a 5-minute timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::review::review_diff_content_raw(&diff_content, config, &repo_path),
    )
    .await;

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

            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Complete;
                session.comments = comments;
                session.summary = Some(summary);
                session.files_reviewed = files_reviewed;
                session.completed_at = Some(current_timestamp());
            }
        }
        Ok(Err(e)) => {
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some(format!("Review failed: {}", e));
                session.completed_at = Some(current_timestamp());
            }
        }
        Err(_) => {
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(&review_id) {
                session.status = ReviewStatus::Failed;
                session.error = Some("Review timed out after 5 minutes".to_string());
                session.completed_at = Some(current_timestamp());
            }
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;
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

    let page = params.page.unwrap_or(1).max(1).min(10_000);
    let per_page = params.per_page.unwrap_or(20).max(1).min(100);
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
    }

    drop(config);

    // Persist config to disk
    AppState::save_config_async(&state);

    Ok(Json(result))
}
