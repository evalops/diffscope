pub mod api;
pub mod github;
pub mod state;
pub mod storage;
pub mod storage_json;
pub mod storage_pg;

pub mod metrics;

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::config::Config;
use tracing::{info, warn};

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct WebAssets;

async fn serve_embedded(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Don't serve SPA for unmatched /api/ routes
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    // Try exact path first, then fall back to index.html (SPA routing)
    let (file, serve_path) = if path.is_empty() {
        (WebAssets::get("index.html"), "index.html")
    } else {
        (WebAssets::get(path), path)
    };

    match file {
        Some(content) => {
            let mime = mime_guess::from_path(serve_path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => {
            // SPA fallback: serve index.html for non-file routes
            match WebAssets::get("index.html") {
                Some(content) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html")],
                    content.data.into_owned(),
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "Frontend not built").into_response(),
            }
        }
    }
}

pub async fn start_server(config: Config, host: &str, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(state::AppState::new(config).await?);

    let origin_strings = [
        format!("http://localhost:{}", port),
        format!("http://127.0.0.1:{}", port),
        "http://localhost:5173".to_string(),
    ];
    let allowed_origins: Vec<axum::http::HeaderValue> = origin_strings
        .iter()
        .filter_map(|s| match s.parse() {
            Ok(v) => Some(v),
            Err(e) => {
                warn!("Failed to parse CORS origin '{}': {}", s, e);
                None
            }
        })
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let api_routes = Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
        )
        .route("/status", get(api::get_status))
        .route("/review", post(api::start_review))
        .route("/reviews", get(api::list_reviews))
        .route("/events", get(api::list_events))
        .route("/events/stats", get(api::get_event_stats))
        .route("/review/{id}", get(api::get_review))
        .route("/review/{id}", delete(api::delete_review))
        .route("/review/{id}/feedback", post(api::submit_feedback))
        .route("/reviews/prune", post(api::prune_reviews))
        .route("/doctor", get(api::get_doctor))
        .route("/config", get(api::get_config))
        .route("/config", put(api::update_config))
        .route("/providers/test", post(api::test_provider))
        .route("/gh/status", get(api::get_gh_status))
        .route("/gh/repos", get(api::get_gh_repos))
        .route("/gh/prs", get(api::get_gh_prs))
        .route("/gh/review", post(api::start_pr_review))
        .route("/agent/tools", get(api::get_agent_tools))
        .route("/gh/auth/device", post(github::start_device_flow))
        .route("/gh/auth/poll", post(github::poll_device_flow))
        .route("/gh/auth", delete(github::disconnect_github))
        .route("/gh/webhook/status", get(github::get_webhook_status))
        .route("/webhooks/github", post(github::handle_webhook))
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/metrics", get(metrics::get_metrics))
        .with_state(state.clone())
        .fallback(serve_embedded)
        .layer(cors);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    info!("DiffScope server running at http://{}", addr);
    info!("Press Ctrl+C to stop");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
