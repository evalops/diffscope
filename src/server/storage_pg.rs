use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::info;

use super::state::{ReviewEvent, ReviewSession, ReviewStatus};
use super::storage::{
    DailyCount, EventFilters, EventStats, ModelStats, RepoStats, SourceStats, StorageBackend,
};
use crate::core::comment::ReviewSummary;
use crate::core::comment::{Category, CodeSuggestion, Comment, CommentStatus, FixEffort, Severity};

/// PostgreSQL storage backend implementation.
pub struct PgStorageBackend {
    pool: PgPool,
}

impl PgStorageBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Run embedded migrations.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Database migrations applied successfully");
        Ok(())
    }

    /// Check if the reviews table is empty (for initial migration from JSON).
    pub async fn is_empty(&self) -> anyhow::Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reviews")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0 == 0)
    }
}

fn parse_status(s: &str) -> ReviewStatus {
    match s {
        "Running" => ReviewStatus::Running,
        "Complete" => ReviewStatus::Complete,
        "Failed" => ReviewStatus::Failed,
        _ => ReviewStatus::Pending,
    }
}

fn parse_severity(s: &str) -> Severity {
    match s {
        "Error" => Severity::Error,
        "Warning" => Severity::Warning,
        "Info" => Severity::Info,
        _ => Severity::Suggestion,
    }
}

fn parse_category(s: &str) -> Category {
    match s {
        "Bug" => Category::Bug,
        "Security" => Category::Security,
        "Performance" => Category::Performance,
        "Style" => Category::Style,
        "Documentation" => Category::Documentation,
        "BestPractice" => Category::BestPractice,
        "Maintainability" => Category::Maintainability,
        "Testing" => Category::Testing,
        _ => Category::Architecture,
    }
}

fn parse_fix_effort(s: &str) -> FixEffort {
    match s {
        "Medium" => FixEffort::Medium,
        "High" => FixEffort::High,
        _ => FixEffort::Low,
    }
}

fn parse_comment_status(s: &str) -> CommentStatus {
    match s {
        "Resolved" => CommentStatus::Resolved,
        "Dismissed" => CommentStatus::Dismissed,
        _ => CommentStatus::Open,
    }
}

#[async_trait]
impl StorageBackend for PgStorageBackend {
    async fn save_review(&self, session: &ReviewSession) -> anyhow::Result<()> {
        let status_str = format!("{:?}", session.status);
        let started_at = chrono::DateTime::from_timestamp(session.started_at, 0);
        let completed_at = session
            .completed_at
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0));
        let summary_json = session
            .summary
            .as_ref()
            .map(|s| serde_json::to_value(s).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO reviews (id, status, diff_source, started_at, completed_at, files_reviewed, error, pr_summary_text, summary_json)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                status = EXCLUDED.status,
                completed_at = EXCLUDED.completed_at,
                files_reviewed = EXCLUDED.files_reviewed,
                error = EXCLUDED.error,
                pr_summary_text = EXCLUDED.pr_summary_text,
                summary_json = EXCLUDED.summary_json,
                updated_at = NOW()
            "#,
        )
        .bind(&session.id)
        .bind(&status_str)
        .bind(&session.diff_source)
        .bind(started_at)
        .bind(completed_at)
        .bind(session.files_reviewed as i32)
        .bind(&session.error)
        .bind(&session.pr_summary_text)
        .bind(&summary_json)
        .execute(&self.pool)
        .await?;

        // Upsert comments
        if !session.comments.is_empty() {
            for c in &session.comments {
                let code_suggestion_json = c
                    .code_suggestion
                    .as_ref()
                    .map(|cs| serde_json::to_value(cs).unwrap_or_default());
                let tags: Vec<&str> = c.tags.iter().map(|t| t.as_str()).collect();

                sqlx::query(
                    r#"
                    INSERT INTO comments (id, review_id, file_path, line_number, content, rule_id, severity, category, suggestion, confidence, code_suggestion, tags, fix_effort, feedback, lifecycle_status)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                    ON CONFLICT (id) DO UPDATE SET
                        feedback = EXCLUDED.feedback,
                        lifecycle_status = EXCLUDED.lifecycle_status
                    "#,
                )
                .bind(&c.id)
                .bind(&session.id)
                .bind(c.file_path.to_string_lossy().to_string())
                .bind(c.line_number as i32)
                .bind(&c.content)
                .bind(&c.rule_id)
                .bind(c.severity.to_string())
                .bind(c.category.to_string())
                .bind(&c.suggestion)
                .bind(c.confidence)
                .bind(&code_suggestion_json)
                .bind(&tags)
                .bind(format!("{:?}", c.fix_effort))
                .bind(&c.feedback)
                .bind(c.status.to_string())
                .execute(&self.pool)
                .await?;
            }
        }

        Ok(())
    }

    async fn get_review(&self, id: &str) -> anyhow::Result<Option<ReviewSession>> {
        let row = sqlx::query_as::<_, (
            String, String, String, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>,
            i32, Option<String>, Option<String>, Option<serde_json::Value>,
        )>(
            "SELECT id, status, diff_source, started_at, completed_at, files_reviewed, error, pr_summary_text, summary_json FROM reviews WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else { return Ok(None) };

        let comments = self.load_comments(id).await?;
        let event = self.load_event(id).await?;
        let summary = Some(crate::core::CommentSynthesizer::generate_summary(&comments));

        Ok(Some(ReviewSession {
            id: row.0,
            status: parse_status(&row.1),
            diff_source: row.2,
            started_at: row.3.timestamp(),
            completed_at: row.4.map(|t| t.timestamp()),
            comments,
            summary,
            files_reviewed: row.5 as usize,
            error: row.6,
            pr_summary_text: row.7,
            diff_content: None,
            event,
            progress: None,
        }))
    }

    async fn list_reviews(&self, limit: i64, offset: i64) -> anyhow::Result<Vec<ReviewSession>> {
        let rows = sqlx::query_as::<_, (
            String, String, String, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>,
            i32, Option<String>, Option<String>, Option<serde_json::Value>,
        )>(
            "SELECT id, status, diff_source, started_at, completed_at, files_reviewed, error, pr_summary_text, summary_json FROM reviews ORDER BY started_at DESC LIMIT $1 OFFSET $2"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::with_capacity(rows.len());
        for row in rows {
            let summary: Option<ReviewSummary> = row.8.and_then(|v| serde_json::from_value(v).ok());
            sessions.push(ReviewSession {
                id: row.0,
                status: parse_status(&row.1),
                diff_source: row.2,
                started_at: row.3.timestamp(),
                completed_at: row.4.map(|t| t.timestamp()),
                comments: Vec::new(), // Don't load comments for list
                summary,
                files_reviewed: row.5 as usize,
                error: row.6,
                pr_summary_text: row.7,
                diff_content: None,
                event: None,
                progress: None,
            });
        }
        Ok(sessions)
    }

    async fn delete_review(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM reviews WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn save_event(&self, event: &ReviewEvent) -> anyhow::Result<()> {
        let comments_by_severity = serde_json::to_value(&event.comments_by_severity)?;
        let comments_by_category = serde_json::to_value(&event.comments_by_category)?;
        let file_metrics = event
            .file_metrics
            .as_ref()
            .map(|fm| serde_json::to_value(fm).unwrap_or_default());
        let hotspot_details = event
            .hotspot_details
            .as_ref()
            .map(|hd| serde_json::to_value(hd).unwrap_or_default());
        let comments_by_pass = serde_json::to_value(&event.comments_by_pass)?;

        sqlx::query(
            r#"
            INSERT INTO review_events (
                review_id, event_type, diff_source, title, model, provider, base_url,
                duration_ms, diff_fetch_ms, llm_total_ms,
                diff_bytes, diff_files_total, diff_files_reviewed, diff_files_skipped,
                comments_total, comments_by_severity, comments_by_category, overall_score,
                hotspots_detected, high_risk_files,
                tokens_prompt, tokens_completion, tokens_total,
                file_metrics, hotspot_details, convention_suppressed, comments_by_pass,
                github_posted, github_repo, github_pr, error
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10,
                $11, $12, $13, $14,
                $15, $16, $17, $18,
                $19, $20,
                $21, $22, $23,
                $24, $25, $26, $27,
                $28, $29, $30, $31
            )
            ON CONFLICT (review_id) DO UPDATE SET
                event_type = EXCLUDED.event_type,
                duration_ms = EXCLUDED.duration_ms,
                comments_total = EXCLUDED.comments_total,
                comments_by_severity = EXCLUDED.comments_by_severity,
                comments_by_category = EXCLUDED.comments_by_category,
                overall_score = EXCLUDED.overall_score,
                tokens_total = EXCLUDED.tokens_total,
                github_posted = EXCLUDED.github_posted,
                error = EXCLUDED.error
            "#,
        )
        .bind(&event.review_id)
        .bind(&event.event_type)
        .bind(&event.diff_source)
        .bind(&event.title)
        .bind(&event.model)
        .bind(&event.provider)
        .bind(&event.base_url)
        .bind(event.duration_ms as i64)
        .bind(event.diff_fetch_ms.map(|v| v as i64))
        .bind(event.llm_total_ms.map(|v| v as i64))
        .bind(event.diff_bytes as i32)
        .bind(event.diff_files_total as i32)
        .bind(event.diff_files_reviewed as i32)
        .bind(event.diff_files_skipped as i32)
        .bind(event.comments_total as i32)
        .bind(&comments_by_severity)
        .bind(&comments_by_category)
        .bind(event.overall_score)
        .bind(event.hotspots_detected as i32)
        .bind(event.high_risk_files as i32)
        .bind(event.tokens_prompt.map(|v| v as i32))
        .bind(event.tokens_completion.map(|v| v as i32))
        .bind(event.tokens_total.map(|v| v as i32))
        .bind(&file_metrics)
        .bind(&hotspot_details)
        .bind(event.convention_suppressed.map(|v| v as i32))
        .bind(&comments_by_pass)
        .bind(event.github_posted)
        .bind(&event.github_repo)
        .bind(event.github_pr.map(|v| v as i32))
        .bind(&event.error)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list_events(&self, filters: &EventFilters) -> anyhow::Result<Vec<ReviewEvent>> {
        // Build dynamic query
        let mut conditions = Vec::new();
        let mut param_idx = 0u32;

        if filters.source.is_some() {
            param_idx += 1;
            conditions.push(format!("LOWER(diff_source) = LOWER(${param_idx})"));
        }
        if filters.model.is_some() {
            param_idx += 1;
            conditions.push(format!("LOWER(model) = LOWER(${param_idx})"));
        }
        if filters.status.is_some() {
            param_idx += 1;
            conditions.push(format!(
                "LOWER(event_type) = LOWER('review.' || ${param_idx})"
            ));
        }
        if filters.time_from.is_some() {
            param_idx += 1;
            conditions.push(format!("created_at >= ${param_idx}"));
        }
        if filters.time_to.is_some() {
            param_idx += 1;
            conditions.push(format!("created_at <= ${param_idx}"));
        }
        if filters.github_repo.is_some() {
            param_idx += 1;
            conditions.push(format!("github_repo = ${param_idx}"));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let limit = filters.limit.unwrap_or(500);
        let offset = filters.offset.unwrap_or(0);

        let query_str = format!(
            "SELECT review_id, event_type, diff_source, title, model, provider, base_url, \
             duration_ms, diff_fetch_ms, llm_total_ms, \
             diff_bytes, diff_files_total, diff_files_reviewed, diff_files_skipped, \
             comments_total, comments_by_severity, comments_by_category, overall_score, \
             hotspots_detected, high_risk_files, \
             tokens_prompt, tokens_completion, tokens_total, \
             file_metrics, hotspot_details, convention_suppressed, comments_by_pass, \
             github_posted, github_repo, github_pr, error, created_at \
             FROM review_events {where_clause} ORDER BY created_at DESC LIMIT {limit} OFFSET {offset}"
        );

        let mut query = sqlx::query_as::<_, EventRow>(&query_str);

        // Bind params in order
        if let Some(ref source) = filters.source {
            query = query.bind(source);
        }
        if let Some(ref model) = filters.model {
            query = query.bind(model);
        }
        if let Some(ref status) = filters.status {
            query = query.bind(status);
        }
        if let Some(ref time_from) = filters.time_from {
            query = query.bind(time_from);
        }
        if let Some(ref time_to) = filters.time_to {
            query = query.bind(time_to);
        }
        if let Some(ref github_repo) = filters.github_repo {
            query = query.bind(github_repo);
        }

        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into_event()).collect())
    }

    async fn get_event_stats(&self, filters: &EventFilters) -> anyhow::Result<EventStats> {
        let where_clause = self.build_time_where(filters);

        // Aggregate stats
        let agg = sqlx::query_as::<_, (i64, i64, i64, i64, f64, Option<f64>)>(&format!(
            "SELECT \
                 COUNT(*), \
                 COUNT(*) FILTER (WHERE event_type = 'review.completed'), \
                 COUNT(*) FILTER (WHERE event_type = 'review.failed'), \
                 COALESCE(SUM(COALESCE(tokens_total, 0)), 0), \
                 COALESCE(AVG(duration_ms)::FLOAT8, 0), \
                 (AVG(overall_score) FILTER (WHERE overall_score IS NOT NULL))::FLOAT8 \
                 FROM review_events {where_clause}"
        ))
        .fetch_one(&self.pool)
        .await?;

        let total = agg.0;
        let completed = agg.1;
        let failed = agg.2;
        let error_rate = if total > 0 {
            failed as f64 / total as f64
        } else {
            0.0
        };

        // Latency percentiles
        let latency = sqlx::query_as::<_, (i64, i64, i64)>(&format!(
            "SELECT \
                 COALESCE((PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY duration_ms))::BIGINT, 0), \
                 COALESCE((PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY duration_ms))::BIGINT, 0), \
                 COALESCE((PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY duration_ms))::BIGINT, 0) \
                 FROM review_events {where_clause}"
        ))
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0));

        // By model
        let by_model = sqlx::query_as::<_, (String, i64, f64, i64, Option<f64>)>(
            &format!(
                "SELECT model, COUNT(*), AVG(duration_ms)::FLOAT8, COALESCE(SUM(COALESCE(tokens_total, 0)), 0), \
                 (AVG(overall_score) FILTER (WHERE overall_score IS NOT NULL))::FLOAT8 \
                 FROM review_events {where_clause} GROUP BY model ORDER BY COUNT(*) DESC"
            )
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| ModelStats { model: r.0, count: r.1, avg_duration_ms: r.2, total_tokens: r.3, avg_score: r.4 })
        .collect();

        // By source
        let by_source = sqlx::query_as::<_, (String, i64)>(
            &format!(
                "SELECT diff_source, COUNT(*) FROM review_events {where_clause} GROUP BY diff_source ORDER BY COUNT(*) DESC"
            )
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| SourceStats { source: r.0, count: r.1 })
        .collect();

        // By repo
        let repo_where = if where_clause.is_empty() {
            "WHERE github_repo IS NOT NULL".to_string()
        } else {
            format!("{where_clause} AND github_repo IS NOT NULL")
        };
        let by_repo = sqlx::query_as::<_, (String, i64, Option<f64>)>(
            &format!(
                "SELECT github_repo, COUNT(*), (AVG(overall_score) FILTER (WHERE overall_score IS NOT NULL))::FLOAT8 \
                 FROM review_events {repo_where} GROUP BY github_repo ORDER BY COUNT(*) DESC"
            )
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| RepoStats { repo: r.0, count: r.1, avg_score: r.2 })
        .collect();

        // Severity totals - aggregate from JSONB
        let severity_rows = sqlx::query_as::<_, (String, i64)>(&format!(
            "SELECT key, SUM(value::int)::BIGINT FROM review_events, \
                 jsonb_each_text(comments_by_severity) {where_clause} GROUP BY key"
        ))
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        let severity_totals: HashMap<String, i64> = severity_rows.into_iter().collect();

        // Category totals
        let category_rows = sqlx::query_as::<_, (String, i64)>(&format!(
            "SELECT key, SUM(value::int)::BIGINT FROM review_events, \
                 jsonb_each_text(comments_by_category) {where_clause} GROUP BY key"
        ))
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        let category_totals: HashMap<String, i64> = category_rows.into_iter().collect();

        // Daily counts
        let daily_counts = sqlx::query_as::<_, (chrono::NaiveDate, i64, i64)>(
            &format!(
                "SELECT created_at::date, \
                 COUNT(*) FILTER (WHERE event_type = 'review.completed'), \
                 COUNT(*) FILTER (WHERE event_type = 'review.failed') \
                 FROM review_events {where_clause} GROUP BY created_at::date ORDER BY created_at::date"
            )
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| DailyCount { date: r.0.to_string(), completed: r.1, failed: r.2 })
        .collect();

        Ok(EventStats {
            total_reviews: total,
            completed_count: completed,
            failed_count: failed,
            total_tokens: agg.3,
            avg_duration_ms: agg.4,
            avg_score: agg.5,
            error_rate,
            p50_latency_ms: latency.0,
            p95_latency_ms: latency.1,
            p99_latency_ms: latency.2,
            by_model,
            by_source,
            by_repo,
            severity_totals,
            category_totals,
            daily_counts,
            total_cost_estimate: 0.0, // Computed client-side using model pricing
        })
    }

    async fn update_comment_feedback(
        &self,
        _review_id: &str,
        comment_id: &str,
        feedback: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE comments SET feedback = $1 WHERE id = $2")
            .bind(feedback)
            .bind(comment_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn prune(&self, max_age_secs: i64, _max_count: usize) -> anyhow::Result<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(max_age_secs);
        // Only prune Pending/Running reviews that are stale (completed reviews are kept forever in PG)
        let result = sqlx::query(
            "DELETE FROM reviews WHERE status IN ('Pending', 'Running') AND started_at < $1",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }
}

impl PgStorageBackend {
    async fn load_comments(&self, review_id: &str) -> anyhow::Result<Vec<Comment>> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                i32,
                String,
                Option<String>,
                String,
                String,
                Option<String>,
                f32,
                Option<serde_json::Value>,
                Vec<String>,
                String,
                Option<String>,
                String,
            ),
        >(
            "SELECT id, file_path, line_number, content, rule_id, severity, category, \
             suggestion, confidence, code_suggestion, tags, fix_effort, feedback, lifecycle_status \
             FROM comments WHERE review_id = $1 ORDER BY created_at",
        )
        .bind(review_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let code_suggestion: Option<CodeSuggestion> =
                    r.9.and_then(|v| serde_json::from_value(v).ok());
                Comment {
                    id: r.0,
                    file_path: std::path::PathBuf::from(r.1),
                    line_number: r.2 as usize,
                    content: r.3,
                    rule_id: r.4,
                    severity: parse_severity(&r.5),
                    category: parse_category(&r.6),
                    suggestion: r.7,
                    confidence: r.8,
                    code_suggestion,
                    tags: r.10,
                    fix_effort: parse_fix_effort(&r.11),
                    feedback: r.12,
                    status: parse_comment_status(&r.13),
                }
            })
            .collect())
    }

    async fn load_event(&self, review_id: &str) -> anyhow::Result<Option<ReviewEvent>> {
        let row = sqlx::query_as::<_, EventRow>(
            "SELECT review_id, event_type, diff_source, title, model, provider, base_url, \
             duration_ms, diff_fetch_ms, llm_total_ms, \
             diff_bytes, diff_files_total, diff_files_reviewed, diff_files_skipped, \
             comments_total, comments_by_severity, comments_by_category, overall_score, \
             hotspots_detected, high_risk_files, \
             tokens_prompt, tokens_completion, tokens_total, \
             file_metrics, hotspot_details, convention_suppressed, comments_by_pass, \
             github_posted, github_repo, github_pr, error, created_at \
             FROM review_events WHERE review_id = $1",
        )
        .bind(review_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into_event()))
    }

    fn build_time_where(&self, filters: &EventFilters) -> String {
        let mut conditions = Vec::new();
        if let Some(ref from) = filters.time_from {
            conditions.push(format!("created_at >= '{}'", from.to_rfc3339()));
        }
        if let Some(ref to) = filters.time_to {
            conditions.push(format!("created_at <= '{}'", to.to_rfc3339()));
        }
        if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        }
    }
}

/// Internal row type for mapping review_events query results.
#[derive(sqlx::FromRow)]
struct EventRow {
    review_id: String,
    event_type: String,
    diff_source: String,
    title: Option<String>,
    model: String,
    provider: Option<String>,
    base_url: Option<String>,
    duration_ms: i64,
    diff_fetch_ms: Option<i64>,
    llm_total_ms: Option<i64>,
    diff_bytes: i32,
    diff_files_total: i32,
    diff_files_reviewed: i32,
    diff_files_skipped: i32,
    comments_total: i32,
    comments_by_severity: serde_json::Value,
    comments_by_category: serde_json::Value,
    overall_score: Option<f32>,
    hotspots_detected: i32,
    high_risk_files: i32,
    tokens_prompt: Option<i32>,
    tokens_completion: Option<i32>,
    tokens_total: Option<i32>,
    file_metrics: Option<serde_json::Value>,
    hotspot_details: Option<serde_json::Value>,
    convention_suppressed: Option<i32>,
    comments_by_pass: serde_json::Value,
    github_posted: bool,
    github_repo: Option<String>,
    github_pr: Option<i32>,
    error: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl EventRow {
    fn into_event(self) -> ReviewEvent {
        let comments_by_severity: HashMap<String, usize> =
            serde_json::from_value(self.comments_by_severity).unwrap_or_default();
        let comments_by_category: HashMap<String, usize> =
            serde_json::from_value(self.comments_by_category).unwrap_or_default();
        let comments_by_pass: HashMap<String, usize> =
            serde_json::from_value(self.comments_by_pass).unwrap_or_default();

        ReviewEvent {
            review_id: self.review_id,
            event_type: self.event_type,
            diff_source: self.diff_source,
            title: self.title,
            model: self.model,
            provider: self.provider,
            base_url: self.base_url,
            duration_ms: self.duration_ms as u64,
            diff_fetch_ms: self.diff_fetch_ms.map(|v| v as u64),
            llm_total_ms: self.llm_total_ms.map(|v| v as u64),
            diff_bytes: self.diff_bytes as usize,
            diff_files_total: self.diff_files_total as usize,
            diff_files_reviewed: self.diff_files_reviewed as usize,
            diff_files_skipped: self.diff_files_skipped as usize,
            comments_total: self.comments_total as usize,
            comments_by_severity,
            comments_by_category,
            overall_score: self.overall_score,
            hotspots_detected: self.hotspots_detected as usize,
            high_risk_files: self.high_risk_files as usize,
            tokens_prompt: self.tokens_prompt.map(|v| v as usize),
            tokens_completion: self.tokens_completion.map(|v| v as usize),
            tokens_total: self.tokens_total.map(|v| v as usize),
            file_metrics: self
                .file_metrics
                .and_then(|v| serde_json::from_value(v).ok()),
            hotspot_details: self
                .hotspot_details
                .and_then(|v| serde_json::from_value(v).ok()),
            convention_suppressed: self.convention_suppressed.map(|v| v as usize),
            comments_by_pass,
            agent_iterations: None,
            agent_tool_calls: None,
            github_posted: self.github_posted,
            github_repo: self.github_repo,
            github_pr: self.github_pr.map(|v| v as u32),
            error: self.error,
            created_at: Some(self.created_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use crate::server::state::ReviewStatus;

    // === parse_status tests ===

    #[test]
    fn test_parse_status_running() {
        assert_eq!(parse_status("Running"), ReviewStatus::Running);
    }

    #[test]
    fn test_parse_status_complete() {
        assert_eq!(parse_status("Complete"), ReviewStatus::Complete);
    }

    #[test]
    fn test_parse_status_failed() {
        assert_eq!(parse_status("Failed"), ReviewStatus::Failed);
    }

    #[test]
    fn test_parse_status_pending() {
        assert_eq!(parse_status("Pending"), ReviewStatus::Pending);
    }

    #[test]
    fn test_parse_status_unknown_defaults_to_pending() {
        assert_eq!(parse_status("unknown"), ReviewStatus::Pending);
    }

    #[test]
    fn test_parse_status_empty_defaults_to_pending() {
        assert_eq!(parse_status(""), ReviewStatus::Pending);
    }

    #[test]
    fn test_parse_status_case_sensitive() {
        // Lowercase variants should not match; they fall through to default
        assert_eq!(parse_status("running"), ReviewStatus::Pending);
        assert_eq!(parse_status("complete"), ReviewStatus::Pending);
        assert_eq!(parse_status("failed"), ReviewStatus::Pending);
    }

    // === parse_severity tests ===

    #[test]
    fn test_parse_severity_error() {
        assert_eq!(parse_severity("Error"), Severity::Error);
    }

    #[test]
    fn test_parse_severity_warning() {
        assert_eq!(parse_severity("Warning"), Severity::Warning);
    }

    #[test]
    fn test_parse_severity_info() {
        assert_eq!(parse_severity("Info"), Severity::Info);
    }

    #[test]
    fn test_parse_severity_suggestion() {
        assert_eq!(parse_severity("Suggestion"), Severity::Suggestion);
    }

    #[test]
    fn test_parse_severity_unknown_defaults_to_suggestion() {
        assert_eq!(parse_severity("unknown"), Severity::Suggestion);
    }

    #[test]
    fn test_parse_severity_empty_defaults_to_suggestion() {
        assert_eq!(parse_severity(""), Severity::Suggestion);
    }

    #[test]
    fn test_parse_severity_case_sensitive() {
        assert_eq!(parse_severity("error"), Severity::Suggestion);
        assert_eq!(parse_severity("WARNING"), Severity::Suggestion);
    }

    // === parse_category tests ===

    #[test]
    fn test_parse_category_bug() {
        assert_eq!(parse_category("Bug"), Category::Bug);
    }

    #[test]
    fn test_parse_category_security() {
        assert_eq!(parse_category("Security"), Category::Security);
    }

    #[test]
    fn test_parse_category_performance() {
        assert_eq!(parse_category("Performance"), Category::Performance);
    }

    #[test]
    fn test_parse_category_style() {
        assert_eq!(parse_category("Style"), Category::Style);
    }

    #[test]
    fn test_parse_category_documentation() {
        assert_eq!(parse_category("Documentation"), Category::Documentation);
    }

    #[test]
    fn test_parse_category_best_practice() {
        assert_eq!(parse_category("BestPractice"), Category::BestPractice);
    }

    #[test]
    fn test_parse_category_maintainability() {
        assert_eq!(parse_category("Maintainability"), Category::Maintainability);
    }

    #[test]
    fn test_parse_category_testing() {
        assert_eq!(parse_category("Testing"), Category::Testing);
    }

    #[test]
    fn test_parse_category_architecture() {
        assert_eq!(parse_category("Architecture"), Category::Architecture);
    }

    #[test]
    fn test_parse_category_unknown_defaults_to_architecture() {
        assert_eq!(parse_category("unknown"), Category::Architecture);
    }

    #[test]
    fn test_parse_category_empty_defaults_to_architecture() {
        assert_eq!(parse_category(""), Category::Architecture);
    }

    #[test]
    fn test_parse_category_case_sensitive() {
        assert_eq!(parse_category("bug"), Category::Architecture);
        assert_eq!(parse_category("SECURITY"), Category::Architecture);
    }

    // === parse_fix_effort tests ===

    #[test]
    fn test_parse_fix_effort_low() {
        assert_eq!(parse_fix_effort("Low"), FixEffort::Low);
    }

    #[test]
    fn test_parse_fix_effort_medium() {
        assert_eq!(parse_fix_effort("Medium"), FixEffort::Medium);
    }

    #[test]
    fn test_parse_fix_effort_high() {
        assert_eq!(parse_fix_effort("High"), FixEffort::High);
    }

    #[test]
    fn test_parse_fix_effort_unknown_defaults_to_low() {
        assert_eq!(parse_fix_effort("unknown"), FixEffort::Low);
    }

    #[test]
    fn test_parse_fix_effort_empty_defaults_to_low() {
        assert_eq!(parse_fix_effort(""), FixEffort::Low);
    }

    #[test]
    fn test_parse_fix_effort_case_sensitive() {
        assert_eq!(parse_fix_effort("medium"), FixEffort::Low);
        assert_eq!(parse_fix_effort("HIGH"), FixEffort::Low);
    }
}
