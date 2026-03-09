//! GitHub App integration: OAuth device flow, webhooks, and Check Runs.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;
use tracing::{info, warn};

use super::state::{
    build_progress_callback, count_diff_files, count_reviewed_files, current_timestamp,
    emit_wide_event, AppState, ReviewEventBuilder, ReviewSession, ReviewStatus,
};

// ── OAuth Device Flow ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// POST /api/gh/auth/device — start an OAuth device flow.
#[tracing::instrument(name = "github.device_flow_start", skip(state))]
pub async fn start_device_flow(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DeviceFlowResponse>, (StatusCode, String)> {
    let config = state.config.read().await;
    let client_id = config
        .github_client_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub App not configured. Set github_client_id in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    let resp = state
        .http_client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str()), ("scope", "repo")])
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("GitHub request failed: {}", e),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("GitHub returned {}: {}", status, body),
        ));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to parse response: {}", e),
        )
    })?;

    Ok(Json(DeviceFlowResponse {
        device_code: body["device_code"].as_str().unwrap_or("").to_string(),
        user_code: body["user_code"].as_str().unwrap_or("").to_string(),
        verification_uri: body["verification_uri"]
            .as_str()
            .unwrap_or("https://github.com/login/device")
            .to_string(),
        expires_in: body["expires_in"].as_u64().unwrap_or(900),
        interval: body["interval"].as_u64().unwrap_or(5),
    }))
}

#[derive(Deserialize)]
pub struct PollDeviceFlowRequest {
    pub device_code: String,
}

#[derive(Serialize)]
pub struct PollDeviceFlowResponse {
    pub authenticated: bool,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub error: Option<String>,
}

/// POST /api/gh/auth/poll — poll for device flow completion.
#[tracing::instrument(name = "github.device_flow_poll", skip(state, request))]
pub async fn poll_device_flow(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PollDeviceFlowRequest>,
) -> Result<Json<PollDeviceFlowResponse>, (StatusCode, String)> {
    let config = state.config.read().await;
    let client_id = config
        .github_client_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub App not configured.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    let resp = state
        .http_client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("device_code", request.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("GitHub request failed: {}", e),
            )
        })?;

    let body: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to parse response: {}", e),
        )
    })?;

    // Check for errors (authorization_pending, slow_down, expired_token, etc.)
    if let Some(error) = body.get("error").and_then(|v| v.as_str()) {
        return Ok(Json(PollDeviceFlowResponse {
            authenticated: false,
            username: None,
            avatar_url: None,
            error: Some(error.to_string()),
        }));
    }

    // Got an access token
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "No access_token in response".to_string(),
            )
        })?
        .to_string();

    // Fetch user info with the new token
    let user_resp = state
        .http_client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DiffScope")
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to fetch user: {}", e),
            )
        })?;

    let user: serde_json::Value = user_resp.json().await.unwrap_or_default();

    let username = user["login"].as_str().map(|s| s.to_string());
    let avatar_url = user["avatar_url"].as_str().map(|s| s.to_string());

    // Store the token in config
    {
        let mut config = state.config.write().await;
        config.github_token = Some(access_token);
    }
    AppState::save_config_async(&state);

    info!(username = ?username, "GitHub OAuth device flow completed");

    Ok(Json(PollDeviceFlowResponse {
        authenticated: true,
        username,
        avatar_url,
        error: None,
    }))
}

/// DELETE /api/gh/auth — disconnect GitHub (clear token).
pub async fn disconnect_github(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    {
        let mut config = state.config.write().await;
        config.github_token = None;
    }
    AppState::save_config_async(&state);
    info!("GitHub disconnected");
    Json(serde_json::json!({ "ok": true }))
}

// ── Webhooks ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WebhookStatusResponse {
    pub configured: bool,
    pub url: String,
}

/// GET /api/gh/webhook/status — return webhook configuration status.
pub async fn get_webhook_status(State(state): State<Arc<AppState>>) -> Json<WebhookStatusResponse> {
    let config = state.config.read().await;
    let configured = config
        .github_webhook_secret
        .as_ref()
        .is_some_and(|s| !s.is_empty());
    Json(WebhookStatusResponse {
        configured,
        url: "/api/webhooks/github".to_string(),
    })
}

/// POST /api/webhooks/github — receive GitHub webhook events.
#[tracing::instrument(name = "github.webhook", skip(state, headers, body), fields(event_type = tracing::field::Empty))]
pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config = state.config.read().await;

    // Verify signature if webhook secret is configured
    if let Some(ref secret) = config.github_webhook_secret {
        if !secret.is_empty() {
            let signature = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    (
                        StatusCode::UNAUTHORIZED,
                        "Missing webhook signature".to_string(),
                    )
                })?;

            verify_webhook_signature(secret, &body, signature)
                .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
        }
    }

    let event_type = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::Span::current().record("event_type", event_type);

    let token = config.github_token.clone();
    let github_app_id = config.github_app_id;
    let private_key = config.github_private_key.clone();
    drop(config);

    info!(event = %event_type, "Received GitHub webhook");

    match event_type {
        "pull_request" => {
            let payload: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)))?;

            let action = payload["action"].as_str().unwrap_or("");

            // Auto-review on opened or synchronized (new push)
            if matches!(action, "opened" | "synchronize") {
                let repo = payload["repository"]["full_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let pr_number = payload["pull_request"]["number"].as_u64().unwrap_or(0) as u32;
                let head_sha = payload["pull_request"]["head"]["sha"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let pr_title = payload["pull_request"]["title"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                if repo.is_empty() || pr_number == 0 {
                    return Err((StatusCode::BAD_REQUEST, "Invalid PR payload".to_string()));
                }

                info!(repo = %repo, pr = pr_number, action = %action, "Auto-reviewing PR");

                // Determine token to use: installation token (if app) or user token
                let auth_token =
                    if let (Some(app_id), Some(ref pkey)) = (github_app_id, &private_key) {
                        // Get installation token for this repo
                        let installation_id =
                            payload["installation"]["id"].as_u64().ok_or_else(|| {
                                (
                                    StatusCode::BAD_REQUEST,
                                    "No installation ID in webhook payload".to_string(),
                                )
                            })?;
                        get_installation_token(&state.http_client, app_id, pkey, installation_id)
                            .await
                            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
                    } else {
                        token.ok_or_else(|| {
                            (
                                StatusCode::BAD_REQUEST,
                                "No GitHub token configured".to_string(),
                            )
                        })?
                    };

                // Fetch diff and start review
                let diff_url =
                    format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_number,);
                let diff_resp = state
                    .http_client
                    .get(&diff_url)
                    .header("Authorization", format!("Bearer {}", auth_token))
                    .header("Accept", "application/vnd.github.v3.diff")
                    .header("User-Agent", "DiffScope")
                    .send()
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::BAD_GATEWAY,
                            format!("Failed to fetch diff: {}", e),
                        )
                    })?;

                if !diff_resp.status().is_success() {
                    let status = diff_resp.status();
                    let body = diff_resp.text().await.unwrap_or_default();
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        format!("GitHub returned {}: {}", status, body),
                    ));
                }

                let diff_content = diff_resp.text().await.unwrap_or_default();

                let review_id = uuid::Uuid::new_v4().to_string();
                let diff_source = format!("pr:{}#{}", repo, pr_number);

                let session = ReviewSession {
                    id: review_id.clone(),
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

                state
                    .reviews
                    .write()
                    .await
                    .insert(review_id.clone(), session);

                // Spawn review task with check run creation
                let state_clone = state.clone();
                let review_id_clone = review_id.clone();
                tokio::spawn(async move {
                    run_webhook_review(
                        state_clone,
                        WebhookReviewParams {
                            review_id: review_id_clone,
                            diff_content,
                            repo,
                            pr_number,
                            head_sha,
                            pr_title,
                            auth_token,
                        },
                    )
                    .await;
                });

                return Ok(Json(serde_json::json!({
                    "ok": true,
                    "action": "review_started",
                    "review_id": review_id,
                })));
            }
        }
        "ping" => {
            info!("GitHub webhook ping received");
            return Ok(Json(serde_json::json!({ "ok": true, "action": "pong" })));
        }
        _ => {}
    }

    Ok(Json(serde_json::json!({ "ok": true, "action": "ignored" })))
}

fn verify_webhook_signature(secret: &str, body: &str, signature: &str) -> Result<(), String> {
    let sig_hex = signature
        .strip_prefix("sha256=")
        .ok_or_else(|| "Invalid signature format".to_string())?;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC init failed: {}", e))?;
    mac.update(body.as_bytes());

    let expected = hex::encode(mac.finalize().into_bytes());

    // Constant-time comparison
    if expected.len() != sig_hex.len() {
        return Err("Signature mismatch".to_string());
    }
    let matches = expected
        .bytes()
        .zip(sig_hex.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    if matches != 0 {
        return Err("Signature mismatch".to_string());
    }
    Ok(())
}

/// Hex-encode bytes (avoids adding hex crate).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

// ── GitHub App Installation Tokens ─────────────────────────────────────

/// Create a JWT for GitHub App authentication.
fn create_app_jwt(app_id: u64, private_key_pem: &str) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    #[derive(serde::Serialize)]
    struct Claims {
        iat: u64,
        exp: u64,
        iss: String,
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Time error: {}", e))?
        .as_secs();

    let claims = Claims {
        iat: now.saturating_sub(60), // 60s clock skew tolerance
        exp: now + 600,              // 10 minute expiry (max allowed)
        iss: app_id.to_string(),
    };

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| format!("Invalid private key: {}", e))?;

    encode(&Header::new(Algorithm::RS256), &claims, &key)
        .map_err(|e| format!("JWT encoding failed: {}", e))
}

/// Get an installation access token for a specific installation.
async fn get_installation_token(
    client: &reqwest::Client,
    app_id: u64,
    private_key_pem: &str,
    installation_id: u64,
) -> Result<String, String> {
    let jwt = create_app_jwt(app_id, private_key_pem)?;

    let url = format!(
        "https://api.github.com/app/installations/{}/access_tokens",
        installation_id,
    );

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DiffScope")
        .send()
        .await
        .map_err(|e| format!("Installation token request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub returned {}: {}", status, body));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    body["token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No token in installation response".to_string())
}

// ── Check Runs ─────────────────────────────────────────────────────────

/// Create a check run on a commit with DiffScope review results.
async fn create_check_run(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    head_sha: &str,
    title: &str,
    comments: &[crate::core::Comment],
    summary: &crate::core::comment::ReviewSummary,
) -> Result<(), String> {
    // Build annotations from comments (max 50 per API call)
    let annotations: Vec<serde_json::Value> = comments
        .iter()
        .take(50)
        .map(|c| {
            let path = c.file_path.display().to_string();
            let path = path.trim_start_matches('/');
            let path = if path.starts_with("a/") || path.starts_with("b/") {
                &path[2..]
            } else {
                path
            };

            let level = match c.severity {
                crate::core::comment::Severity::Error => "failure",
                crate::core::comment::Severity::Warning => "warning",
                _ => "notice",
            };

            serde_json::json!({
                "path": path,
                "start_line": c.line_number,
                "end_line": c.line_number,
                "annotation_level": level,
                "title": format!("{}: {}", c.severity, c.category),
                "message": c.content,
            })
        })
        .collect();

    // Determine conclusion
    let has_errors = comments
        .iter()
        .any(|c| matches!(c.severity, crate::core::comment::Severity::Error));
    let conclusion = if has_errors { "failure" } else { "success" };

    let summary_text = format!(
        "**Score:** {:.1}/10 | **Findings:** {} | **Files:** {}\n\n{}",
        summary.overall_score,
        summary.total_comments,
        summary.files_reviewed,
        if summary.recommendations.is_empty() {
            String::new()
        } else {
            format!(
                "**Recommendations:**\n{}",
                summary
                    .recommendations
                    .iter()
                    .map(|r| format!("- {}", r))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        },
    );

    let check_run = serde_json::json!({
        "name": "DiffScope Review",
        "head_sha": head_sha,
        "status": "completed",
        "conclusion": conclusion,
        "output": {
            "title": title,
            "summary": summary_text,
            "annotations": annotations,
        },
    });

    let url = format!("https://api.github.com/repos/{}/check-runs", repo);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DiffScope")
        .json(&check_run)
        .send()
        .await
        .map_err(|e| format!("Check run request failed: {}", e))?;

    if resp.status().is_success() {
        info!(repo = %repo, sha = %head_sha, conclusion = %conclusion, "Created check run");
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        warn!(repo = %repo, status = %status, "Failed to create check run: {}", body);
        Err(format!("GitHub returned {}: {}", status, body))
    }
}

// ── Post PR Summary Comment ─────────────────────────────────────────────

/// Post an AI-generated PR summary as a standalone issue comment on the PR.
async fn post_pr_summary_comment(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    pr_number: u32,
    summary_markdown: &str,
) -> Result<(), String> {
    let url = format!(
        "https://api.github.com/repos/{}/issues/{}/comments",
        repo, pr_number,
    );

    let body = serde_json::json!({
        "body": summary_markdown,
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DiffScope")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Failed to post PR summary comment: {}", e))?;

    if resp.status().is_success() {
        info!(repo = %repo, pr = pr_number, "Posted PR summary comment");
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("GitHub returned {}: {}", status, body))
    }
}

// ── Webhook-triggered review task ──────────────────────────────────────

struct WebhookReviewParams {
    review_id: String,
    diff_content: String,
    repo: String,
    pr_number: u32,
    head_sha: String,
    pr_title: String,
    auth_token: String,
}

#[tracing::instrument(name = "github.webhook_review", skip(state, params), fields(review_id = %params.review_id, repo = %params.repo, pr_number = params.pr_number, diff_bytes = params.diff_content.len()))]
async fn run_webhook_review(state: Arc<AppState>, params: WebhookReviewParams) {
    let WebhookReviewParams {
        review_id,
        diff_content,
        repo,
        pr_number,
        head_sha,
        pr_title,
        auth_token,
    } = params;
    use crate::core::comment::CommentSynthesizer;

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

    let on_progress: Option<crate::review::ProgressCallback> =
        Some(build_progress_callback(&state, &review_id, task_start));

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
        Ok(Ok(comments)) => {
            let summary = CommentSynthesizer::generate_summary(&comments);
            let files_reviewed = count_reviewed_files(&comments);

            // Post inline review comments to PR
            let mut github_posted = false;
            if !comments.is_empty() {
                github_posted = super::api::post_pr_review_comments(
                    &state.http_client,
                    &auth_token,
                    &repo,
                    pr_number,
                    &comments,
                    Some(&summary),
                )
                .await
                .is_ok();
            }

            // Create Check Run
            let check_title = if pr_title.is_empty() {
                format!(
                    "DiffScope: {}/{}",
                    summary.total_comments, summary.overall_score
                )
            } else {
                format!("DiffScope: {} — {:.1}/10", pr_title, summary.overall_score)
            };
            let _ = create_check_run(
                &state.http_client,
                &auth_token,
                &repo,
                &head_sha,
                &check_title,
                &comments,
                &summary,
            )
            .await;

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
                    .github(&repo, pr_number)
                    .github_posted(github_posted)
                    .build();
            emit_wide_event(&event);
            AppState::complete_review(&state, &review_id, comments, summary, files_reviewed, event)
                .await;

            // Generate AI-powered PR summary and post it as a comment if enabled
            if let Some(ref cfg) = summary_config {
                super::api::generate_and_store_pr_summary(
                    &state,
                    &review_id,
                    &diff_content,
                    cfg,
                )
                .await;

                // Post the summary as a PR comment
                let pr_summary_text = {
                    let reviews = state.reviews.read().await;
                    reviews
                        .get(&review_id)
                        .and_then(|s| s.pr_summary_text.clone())
                };
                if let Some(summary_md) = pr_summary_text {
                    if let Err(e) = post_pr_summary_comment(
                        &state.http_client,
                        &auth_token,
                        &repo,
                        pr_number,
                        &summary_md,
                    )
                    .await
                    {
                        warn!(review_id = %review_id, "Failed to post PR summary comment: {}", e);
                    }
                }
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {}", e);
            warn!(review_id = %review_id, error = %err_msg, "Webhook review failed");
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
            warn!(review_id = %review_id, "Webhook review timed out");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex::encode([]), "");
        assert_eq!(hex::encode([0x00]), "00");
        assert_eq!(hex::encode([0xff]), "ff");
        assert_eq!(hex::encode([0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(
            hex::encode([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]),
            "0123456789abcdef"
        );
    }

    #[test]
    fn test_verify_webhook_signature_valid() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = "test-webhook-secret";
        let body = r#"{"action":"opened","pull_request":{"number":1}}"#;

        // Compute the expected signature
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        let signature = format!("sha256={}", expected);

        assert!(verify_webhook_signature(secret, body, &signature).is_ok());
    }

    #[test]
    fn test_verify_webhook_signature_invalid() {
        let secret = "test-secret";
        let body = "test body";
        let bad_sig = "sha256=0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_webhook_signature(secret, body, bad_sig).is_err());
    }

    #[test]
    fn test_verify_webhook_signature_missing_prefix() {
        let secret = "test-secret";
        let body = "test body";
        let no_prefix = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        assert!(verify_webhook_signature(secret, body, no_prefix).is_err());
    }

    #[test]
    fn test_verify_webhook_signature_wrong_length() {
        let secret = "test-secret";
        let body = "test body";
        let short_sig = "sha256=abcdef";
        assert!(verify_webhook_signature(secret, body, short_sig).is_err());
    }

    #[test]
    fn test_verify_webhook_signature_empty_body() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = "my-secret";
        let body = "";

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        let signature = format!("sha256={}", expected);

        assert!(verify_webhook_signature(secret, body, &signature).is_ok());
    }
}
