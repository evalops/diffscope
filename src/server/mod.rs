pub mod api;
pub mod state;

use axum::{
    Router,
    routing::{get, post, put},
    response::{IntoResponse, Response},
    http::{StatusCode, header},
};
use tower_http::cors::CorsLayer;
use rust_embed::Embed;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::config::Config;

#[derive(Embed)]
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
    let state = Arc::new(state::AppState::new(config)?);

    let allowed_origins: Vec<axum::http::HeaderValue> = [
        format!("http://localhost:{}", port),
        format!("http://127.0.0.1:{}", port),
        "http://localhost:5173".to_string(),
    ]
    .iter()
    .filter_map(|s| s.parse().ok())
    .collect();

    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let api_routes = Router::new()
        .route("/status", get(api::get_status))
        .route("/review", post(api::start_review))
        .route("/reviews", get(api::list_reviews))
        .route("/review/{id}", get(api::get_review))
        .route("/review/{id}/feedback", post(api::submit_feedback))
        .route("/doctor", get(api::get_doctor))
        .route("/config", get(api::get_config))
        .route("/config", put(api::update_config))
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api", api_routes)
        .fallback(serve_embedded)
        .layer(cors);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    eprintln!("DiffScope server running at http://{}", addr);
    eprintln!("Press Ctrl+C to stop");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
