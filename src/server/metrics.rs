use axum::extract::State;
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::Arc;

use super::state::{AppState, ReviewStatus, MAX_CONCURRENT_REVIEWS};

#[derive(Clone, Copy)]
struct LongRunningJobMetrics {
    job_type: &'static str,
    queue_depth: u64,
    worker_capacity: u64,
    workers_active: u64,
    workers_available: u64,
}

impl LongRunningJobMetrics {
    const fn new(
        job_type: &'static str,
        queue_depth: u64,
        worker_capacity: u64,
        workers_active: u64,
        workers_available: u64,
    ) -> Self {
        Self {
            job_type,
            queue_depth,
            worker_capacity,
            workers_active,
            workers_available,
        }
    }

    fn worker_saturation_ratio(self) -> f64 {
        if self.worker_capacity == 0 {
            0.0
        } else {
            self.workers_active as f64 / self.worker_capacity as f64
        }
    }
}

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
    let mut total_tokens: u64 = 0;
    let mut total_prompt_tokens: u64 = 0;
    let mut total_completion_tokens: u64 = 0;
    let mut severity_counts: HashMap<String, u64> = HashMap::new();
    let mut category_counts: HashMap<String, u64> = HashMap::new();

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

        for comment in &session.comments {
            *severity_counts
                .entry(comment.severity.to_string())
                .or_default() += 1;
            *category_counts
                .entry(comment.category.to_string())
                .or_default() += 1;
        }

        if let Some(event) = &session.event {
            if event.duration_ms > 0 {
                total_duration_ms += event.duration_ms;
                completed_with_duration += 1;
            }
            total_diff_bytes += event.diff_bytes as u64;
            if event.github_posted {
                github_posted += 1;
            }
            if let Some(t) = event.tokens_total {
                total_tokens += t as u64;
            }
            if let Some(t) = event.tokens_prompt {
                total_prompt_tokens += t as u64;
            }
            if let Some(t) = event.tokens_completion {
                total_completion_tokens += t as u64;
            }
        }
    }

    drop(reviews);

    let review_worker_capacity = MAX_CONCURRENT_REVIEWS as u64;
    let review_workers_available = state
        .review_semaphore
        .available_permits()
        .min(MAX_CONCURRENT_REVIEWS) as u64;
    let review_workers_active = review_worker_capacity.saturating_sub(review_workers_available);
    let long_running_job_metrics = [
        LongRunningJobMetrics::new(
            "review",
            pending,
            review_worker_capacity,
            review_workers_active,
            review_workers_available,
        ),
        LongRunningJobMetrics::new("eval", 0, 0, 0, 0),
    ];

    let mut buf = String::with_capacity(4096);

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
    write_metric(
        &mut buf,
        "diffscope_tokens_total",
        "Total LLM tokens consumed",
        "counter",
        total_tokens,
    );
    write_metric(
        &mut buf,
        "diffscope_tokens_prompt_total",
        "Total LLM prompt tokens consumed",
        "counter",
        total_prompt_tokens,
    );
    write_metric(
        &mut buf,
        "diffscope_tokens_completion_total",
        "Total LLM completion tokens consumed",
        "counter",
        total_completion_tokens,
    );

    write_metric_header(
        &mut buf,
        "diffscope_job_queue_depth",
        "Current queue depth for long-running jobs by job type",
        "gauge",
    );
    for metric in &long_running_job_metrics {
        write_labeled_metric(
            &mut buf,
            "diffscope_job_queue_depth",
            &[("job_type", metric.job_type)],
            metric.queue_depth,
        );
    }

    write_metric_header(
        &mut buf,
        "diffscope_job_worker_capacity",
        "Worker capacity for long-running jobs by job type",
        "gauge",
    );
    for metric in &long_running_job_metrics {
        write_labeled_metric(
            &mut buf,
            "diffscope_job_worker_capacity",
            &[("job_type", metric.job_type)],
            metric.worker_capacity,
        );
    }

    write_metric_header(
        &mut buf,
        "diffscope_job_workers_active",
        "Active workers handling long-running jobs by job type",
        "gauge",
    );
    for metric in &long_running_job_metrics {
        write_labeled_metric(
            &mut buf,
            "diffscope_job_workers_active",
            &[("job_type", metric.job_type)],
            metric.workers_active,
        );
    }

    write_metric_header(
        &mut buf,
        "diffscope_job_workers_available",
        "Available workers for long-running jobs by job type",
        "gauge",
    );
    for metric in &long_running_job_metrics {
        write_labeled_metric(
            &mut buf,
            "diffscope_job_workers_available",
            &[("job_type", metric.job_type)],
            metric.workers_available,
        );
    }

    write_metric_header(
        &mut buf,
        "diffscope_job_worker_saturation_ratio",
        "Worker saturation ratio for long-running jobs by job type",
        "gauge",
    );
    for metric in &long_running_job_metrics {
        write_labeled_metric(
            &mut buf,
            "diffscope_job_worker_saturation_ratio",
            &[("job_type", metric.job_type)],
            metric.worker_saturation_ratio(),
        );
    }

    // Per-severity comment counts
    let _ = writeln!(
        buf,
        "# HELP diffscope_comments_by_severity Comments by severity level"
    );
    let _ = writeln!(buf, "# TYPE diffscope_comments_by_severity counter");
    for severity in &["Error", "Warning", "Info", "Suggestion"] {
        let count = severity_counts.get(*severity).copied().unwrap_or(0);
        let _ = writeln!(
            buf,
            "diffscope_comments_by_severity{{severity=\"{severity}\"}} {count}"
        );
    }

    // Per-category comment counts
    let _ = writeln!(
        buf,
        "# HELP diffscope_comments_by_category Comments by category"
    );
    let _ = writeln!(buf, "# TYPE diffscope_comments_by_category counter");
    for category in &[
        "Bug",
        "Security",
        "Performance",
        "Style",
        "Documentation",
        "BestPractice",
        "Maintainability",
        "Testing",
        "Architecture",
    ] {
        let count = category_counts.get(*category).copied().unwrap_or(0);
        let _ = writeln!(
            buf,
            "diffscope_comments_by_category{{category=\"{category}\"}} {count}"
        );
    }

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        buf,
    )
}

fn write_metric_header(buf: &mut String, name: &str, help: &str, metric_type: &str) {
    let _ = writeln!(buf, "# HELP {name} {help}");
    let _ = writeln!(buf, "# TYPE {name} {metric_type}");
}

fn write_metric<T: Display>(buf: &mut String, name: &str, help: &str, metric_type: &str, value: T) {
    write_metric_header(buf, name, help, metric_type);
    let _ = writeln!(buf, "{name} {value}");
}

fn write_labeled_metric<T: Display>(
    buf: &mut String,
    name: &str,
    labels: &[(&str, &str)],
    value: T,
) {
    let mut label_buf = String::new();
    for (index, (key, raw_value)) in labels.iter().enumerate() {
        if index > 0 {
            label_buf.push(',');
        }
        let _ = write!(label_buf, "{key}=\"{raw_value}\"");
    }

    let _ = writeln!(buf, "{name}{{{label_buf}}} {value}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::ReviewSession;
    use axum::body::to_bytes;

    #[test]
    fn test_write_metric_format() {
        let mut buf = String::new();
        write_metric(&mut buf, "test_metric", "A test metric", "gauge", 42);
        assert!(buf.contains("# HELP test_metric A test metric\n"));
        assert!(buf.contains("# TYPE test_metric gauge\n"));
        assert!(buf.contains("test_metric 42\n"));
    }

    #[test]
    fn test_write_labeled_metric_format() {
        let mut buf = String::new();
        write_metric_header(&mut buf, "test_metric", "A labeled metric", "gauge");
        write_labeled_metric(&mut buf, "test_metric", &[("job_type", "review")], 0.4);

        assert!(buf.contains("# HELP test_metric A labeled metric\n"));
        assert!(buf.contains("# TYPE test_metric gauge\n"));
        assert!(buf.contains("test_metric{job_type=\"review\"} 0.4\n"));
    }

    fn make_session(id: &str, status: ReviewStatus) -> ReviewSession {
        ReviewSession {
            id: id.to_string(),
            status,
            diff_source: "head".to_string(),
            github_head_sha: None,
            github_post_results_requested: None,
            started_at: 1,
            completed_at: None,
            comments: Vec::new(),
            summary: None,
            files_reviewed: 0,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        }
    }

    #[tokio::test]
    async fn metrics_include_queue_depth_and_worker_saturation_gauges() {
        let storage_path = std::path::PathBuf::from("metrics-test-reviews.json");
        let config_path = std::path::PathBuf::from("metrics-test-config.json");
        let storage = crate::server::storage_json::JsonStorageBackend::new(&storage_path);

        let mut reviews = HashMap::new();
        reviews.insert(
            "running".to_string(),
            make_session("running", ReviewStatus::Running),
        );
        reviews.insert(
            "pending-1".to_string(),
            make_session("pending-1", ReviewStatus::Pending),
        );
        reviews.insert(
            "pending-2".to_string(),
            make_session("pending-2", ReviewStatus::Pending),
        );

        let state = Arc::new(AppState {
            config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
            repo_path: std::path::PathBuf::from("."),
            reviews: Arc::new(tokio::sync::RwLock::new(reviews)),
            storage: Arc::new(storage),
            storage_path,
            config_path,
            http_client: reqwest::Client::new(),
            review_semaphore: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REVIEWS)),
            last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pr_verification_reuse_caches: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            api_rate_limits: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        });
        let _permit = state
            .review_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap();

        let response = get_metrics(State(state)).await.into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(text.contains("diffscope_job_queue_depth{job_type=\"review\"} 2\n"));
        assert!(text.contains("diffscope_job_queue_depth{job_type=\"eval\"} 0\n"));
        assert!(text.contains("diffscope_job_worker_capacity{job_type=\"review\"} 5\n"));
        assert!(text.contains("diffscope_job_workers_active{job_type=\"review\"} 1\n"));
        assert!(text.contains("diffscope_job_workers_available{job_type=\"review\"} 4\n"));
        assert!(text.contains("diffscope_job_worker_saturation_ratio{job_type=\"review\"} 0.2\n"));
    }
}
