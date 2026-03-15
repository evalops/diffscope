use super::state::AppState;
use crate::config::DEFAULT_SERVER_RATE_LIMIT_PER_MINUTE;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json, Router,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::info;
use uuid::Uuid;

const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const HEADER_RATE_LIMIT_LIMIT: &str = "x-diffscope-ratelimit-limit";
const HEADER_RATE_LIMIT_REMAINING: &str = "x-diffscope-ratelimit-remaining";

#[derive(Debug, Serialize)]
struct ApiAuditEvent {
    request_id: String,
    method: String,
    path: String,
    status: u16,
    outcome: &'static str,
    subject: Option<String>,
    auth_source: Option<&'static str>,
    rate_limit_per_minute: Option<u32>,
    rate_limit_remaining: Option<u32>,
    retry_after_secs: Option<u64>,
}

pub(crate) fn protected_api_routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/review", axum::routing::post(super::api::start_review))
        .route(
            "/review/{id}",
            axum::routing::delete(super::api::delete_review),
        )
        .route(
            "/review/{id}/feedback",
            axum::routing::post(super::api::submit_feedback),
        )
        .route(
            "/review/{id}/lifecycle",
            axum::routing::post(super::api::update_comment_lifecycle),
        )
        .route(
            "/reviews/prune",
            axum::routing::post(super::api::prune_reviews),
        )
        .route("/config", axum::routing::put(super::api::update_config))
        .route(
            "/providers/test",
            axum::routing::post(super::api::test_provider),
        )
        .route(
            "/gh/pr-fix-loop",
            axum::routing::post(super::api::run_gh_pr_fix_loop),
        )
        .route(
            "/gh/review",
            axum::routing::post(super::api::start_pr_review),
        )
        .route(
            "/gh/review/rerun",
            axum::routing::post(super::api::rerun_pr_review),
        )
        .route(
            "/gh/auth/device",
            axum::routing::post(super::github::start_device_flow),
        )
        .route(
            "/gh/auth/poll",
            axum::routing::post(super::github::poll_device_flow),
        )
        .route(
            "/gh/auth",
            axum::routing::delete(super::github::disconnect_github),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_api_auth,
        ))
}

pub(crate) async fn require_api_auth(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let (configured_api_key, rate_limit_per_minute) = {
        let config = state.config.read().await;
        (
            config.server_security.api_key.clone(),
            config
                .server_security
                .rate_limit_per_minute
                .unwrap_or(DEFAULT_SERVER_RATE_LIMIT_PER_MINUTE),
        )
    };

    let Some(configured_api_key) = configured_api_key else {
        return next.run(request).await;
    };

    let (presented_api_key, auth_source) = extract_api_key(request.headers());
    let Some(presented_api_key) = presented_api_key else {
        emit_api_audit_event(&ApiAuditEvent {
            request_id,
            method,
            path,
            status: StatusCode::UNAUTHORIZED.as_u16(),
            outcome: "unauthorized",
            subject: None,
            auth_source,
            rate_limit_per_minute: Some(rate_limit_per_minute),
            rate_limit_remaining: None,
            retry_after_secs: None,
        });
        return unauthorized_response();
    };

    if !constant_time_eq(&configured_api_key, presented_api_key) {
        emit_api_audit_event(&ApiAuditEvent {
            request_id,
            method,
            path,
            status: StatusCode::UNAUTHORIZED.as_u16(),
            outcome: "unauthorized",
            subject: None,
            auth_source,
            rate_limit_per_minute: Some(rate_limit_per_minute),
            rate_limit_remaining: None,
            retry_after_secs: None,
        });
        return unauthorized_response();
    }

    let subject = api_key_subject(&configured_api_key);
    if rate_limit_per_minute > 0 {
        match take_rate_limit_slot(&state, &subject, rate_limit_per_minute).await {
            Ok(remaining) => {
                let mut response = next.run(request).await;
                insert_rate_limit_headers(response.headers_mut(), rate_limit_per_minute, remaining);
                emit_api_audit_event(&ApiAuditEvent {
                    request_id,
                    method,
                    path,
                    status: response.status().as_u16(),
                    outcome: "authorized",
                    subject: Some(subject),
                    auth_source,
                    rate_limit_per_minute: Some(rate_limit_per_minute),
                    rate_limit_remaining: Some(remaining),
                    retry_after_secs: None,
                });
                return response;
            }
            Err(retry_after_secs) => {
                emit_api_audit_event(&ApiAuditEvent {
                    request_id,
                    method,
                    path,
                    status: StatusCode::TOO_MANY_REQUESTS.as_u16(),
                    outcome: "rate_limited",
                    subject: Some(subject),
                    auth_source,
                    rate_limit_per_minute: Some(rate_limit_per_minute),
                    rate_limit_remaining: Some(0),
                    retry_after_secs: Some(retry_after_secs),
                });
                return rate_limited_response(rate_limit_per_minute, retry_after_secs);
            }
        }
    }

    let response = next.run(request).await;
    emit_api_audit_event(&ApiAuditEvent {
        request_id,
        method,
        path,
        status: response.status().as_u16(),
        outcome: "authorized",
        subject: Some(subject),
        auth_source,
        rate_limit_per_minute: None,
        rate_limit_remaining: None,
        retry_after_secs: None,
    });
    response
}

fn extract_api_key(headers: &HeaderMap) -> (Option<&str>, Option<&'static str>) {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            return (Some(token.trim()), Some("authorization_bearer"));
        }
    }

    if let Some(value) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    {
        return (Some(value.trim()), Some("x_api_key"));
    }

    (None, None)
}

fn constant_time_eq(expected: &str, actual: &str) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

fn api_key_subject(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    format!(
        "server_api_key:{}",
        digest
            .iter()
            .take(6)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

async fn take_rate_limit_slot(state: &AppState, subject: &str, limit: u32) -> Result<u32, u64> {
    let now = Instant::now();
    let mut buckets = state.api_rate_limits.lock().await;
    let bucket = buckets.entry(subject.to_string()).or_insert((now, 0u32));

    if now.duration_since(bucket.0) >= RATE_LIMIT_WINDOW {
        *bucket = (now, 0);
    }

    if bucket.1 >= limit {
        let elapsed = now.duration_since(bucket.0);
        let retry_after_secs = RATE_LIMIT_WINDOW
            .as_secs()
            .saturating_sub(elapsed.as_secs())
            .max(1);
        return Err(retry_after_secs);
    }

    bucket.1 += 1;
    Ok(limit.saturating_sub(bucket.1))
}

fn insert_rate_limit_headers(headers: &mut HeaderMap, limit: u32, remaining: u32) {
    if let Ok(value) = HeaderValue::from_str(&limit.to_string()) {
        headers.insert(HEADER_RATE_LIMIT_LIMIT, value);
    }
    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
        headers.insert(HEADER_RATE_LIMIT_REMAINING, value);
    }
}

fn unauthorized_response() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "unauthorized" })),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"diffscope\""),
    );
    response
}

fn rate_limited_response(limit: u32, retry_after_secs: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({ "error": "rate_limited" })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    insert_rate_limit_headers(response.headers_mut(), limit, 0);
    response
}

fn emit_api_audit_event(event: &ApiAuditEvent) {
    info!(
        request_id = %event.request_id,
        method = %event.method,
        path = %event.path,
        status = event.status,
        outcome = event.outcome,
        subject = ?event.subject,
        auth_source = ?event.auth_source,
        rate_limit_per_minute = ?event.rate_limit_per_minute,
        rate_limit_remaining = ?event.rate_limit_remaining,
        retry_after_secs = ?event.retry_after_secs,
        "api.audit"
    );
    let payload = serde_json::json!({
        "@timestamp": chrono::Utc::now().to_rfc3339(),
        "event": { "name": "api.audit", "kind": "event" },
        "api": event,
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        info!(target: "api.audit.json", "{}", json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ServerSecurityConfig};
    use crate::server::storage_json::JsonStorageBackend;
    use axum::{
        body::{to_bytes, Body},
        routing::post,
    };
    use std::collections::HashMap;
    use tokio::sync::{Mutex, RwLock};
    use tower::ServiceExt;

    fn test_state(config: Config) -> Arc<AppState> {
        let unique_root = std::env::temp_dir().join(format!("diffscope-auth-{}", Uuid::new_v4()));
        Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            repo_path: unique_root.clone(),
            reviews: Arc::new(RwLock::new(HashMap::new())),
            storage: Arc::new(JsonStorageBackend::new(&unique_root.join("reviews.json"))),
            storage_path: unique_root.join("reviews.json"),
            config_path: unique_root.join("config.json"),
            http_client: reqwest::Client::new(),
            review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
            last_reviewed_shas: Arc::new(RwLock::new(HashMap::new())),
            pr_verification_reuse_caches: Arc::new(RwLock::new(HashMap::new())),
            api_rate_limits: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn test_app(config: Config) -> Router {
        let state = test_state(config);
        Router::new()
            .route(
                "/protected",
                post(|| async { Json(serde_json::json!({ "ok": true })) }),
            )
            .route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_api_auth,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn auth_disabled_allows_protected_route() {
        let app = test_app(Config::default());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_api_key_is_rejected() {
        let mut config = Config {
            server_security: ServerSecurityConfig {
                api_key: Some("shared-key".to_string()),
                rate_limit_per_minute: Some(2),
            },
            ..Config::default()
        };
        config.normalize();
        let app = test_app(config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/protected")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("unauthorized"));
    }

    #[tokio::test]
    async fn bearer_api_key_is_authorized_and_sets_rate_limit_headers() {
        let mut config = Config {
            server_security: ServerSecurityConfig {
                api_key: Some("shared-key".to_string()),
                rate_limit_per_minute: Some(2),
            },
            ..Config::default()
        };
        config.normalize();
        let app = test_app(config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/protected")
                    .header(header::AUTHORIZATION, "Bearer shared-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(HEADER_RATE_LIMIT_LIMIT).unwrap(),
            "2"
        );
        assert_eq!(
            response.headers().get(HEADER_RATE_LIMIT_REMAINING).unwrap(),
            "1"
        );
    }

    #[tokio::test]
    async fn protected_routes_are_rate_limited() {
        let mut config = Config {
            server_security: ServerSecurityConfig {
                api_key: Some("shared-key".to_string()),
                rate_limit_per_minute: Some(1),
            },
            ..Config::default()
        };
        config.normalize();
        let app = test_app(config);

        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/protected")
                    .header("x-api-key", "shared-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/protected")
                    .header("x-api-key", "shared-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(second.headers().get(header::RETRY_AFTER).unwrap(), "60");
    }
}
