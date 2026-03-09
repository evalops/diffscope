use axum::extract::State;
use axum::response::IntoResponse;
use std::fmt::Write;
use std::sync::Arc;

use super::state::{AppState, ReviewStatus};

pub async fn get_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let reviews = state.reviews.read().await;

    let mut total: u64 = 0;
    let mut running: u64 = 0;
    let mut completed: u64 = 0;
    let mut failed: u64 = 0;
    let mut pending: u64 = 0;
    let mut total_comments: u64 = 0;
    let mut total_duration_ms: u64 = 0;
    let mut completed_with_duration: u64 = 0;
    let mut total_files_reviewed: u64 = 0;
    let mut total_diff_bytes: u64 = 0;
    let mut github_posted: u64 = 0;

    for session in reviews.values() {
        total += 1;
        match session.status {
            ReviewStatus::Running => running += 1,
            ReviewStatus::Complete => completed += 1,
            ReviewStatus::Failed => failed += 1,
            ReviewStatus::Pending => pending += 1,
        }
        total_comments += session.comments.len() as u64;
        total_files_reviewed += session.files_reviewed as u64;

        if let Some(event) = &session.event {
            if event.duration_ms > 0 {
                total_duration_ms += event.duration_ms;
                completed_with_duration += 1;
            }
            total_diff_bytes += event.diff_bytes as u64;
            if event.github_posted {
                github_posted += 1;
            }
        }
    }

    drop(reviews);

    let mut buf = String::with_capacity(2048);

    write_metric(
        &mut buf,
        "diffscope_reviews_total",
        "Total number of reviews",
        "counter",
        total,
    );
    write_metric(
        &mut buf,
        "diffscope_reviews_running",
        "Number of currently running reviews",
        "gauge",
        running,
    );
    write_metric(
        &mut buf,
        "diffscope_reviews_completed_total",
        "Total completed reviews",
        "counter",
        completed,
    );
    write_metric(
        &mut buf,
        "diffscope_reviews_failed_total",
        "Total failed reviews",
        "counter",
        failed,
    );
    write_metric(
        &mut buf,
        "diffscope_reviews_pending",
        "Number of pending reviews",
        "gauge",
        pending,
    );
    write_metric(
        &mut buf,
        "diffscope_comments_total",
        "Total comments generated across all reviews",
        "counter",
        total_comments,
    );
    write_metric(
        &mut buf,
        "diffscope_files_reviewed_total",
        "Total files reviewed across all reviews",
        "counter",
        total_files_reviewed,
    );
    write_metric(
        &mut buf,
        "diffscope_diff_bytes_total",
        "Total diff bytes processed",
        "counter",
        total_diff_bytes,
    );
    write_metric(
        &mut buf,
        "diffscope_review_duration_ms_total",
        "Total review duration in milliseconds",
        "counter",
        total_duration_ms,
    );
    write_metric(
        &mut buf,
        "diffscope_reviews_with_duration_total",
        "Number of completed reviews with duration data",
        "counter",
        completed_with_duration,
    );
    write_metric(
        &mut buf,
        "diffscope_github_reviews_posted_total",
        "Total reviews posted to GitHub",
        "counter",
        github_posted,
    );

    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        buf,
    )
}

fn write_metric(buf: &mut String, name: &str, help: &str, metric_type: &str, value: u64) {
    let _ = writeln!(buf, "# HELP {name} {help}");
    let _ = writeln!(buf, "# TYPE {name} {metric_type}");
    let _ = writeln!(buf, "{name} {value}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_metric_format() {
        let mut buf = String::new();
        write_metric(&mut buf, "test_metric", "A test metric", "gauge", 42);
        assert!(buf.contains("# HELP test_metric A test metric\n"));
        assert!(buf.contains("# TYPE test_metric gauge\n"));
        assert!(buf.contains("test_metric 42\n"));
    }
}
