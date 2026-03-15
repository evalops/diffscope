use super::*;

pub struct ReviewEventBuilder {
    event: ReviewEvent,
}

impl ReviewEventBuilder {
    pub fn new(review_id: &str, event_type: &str, diff_source: &str, model: &str) -> Self {
        Self {
            event: ReviewEvent {
                review_id: review_id.to_string(),
                event_type: event_type.to_string(),
                diff_source: diff_source.to_string(),
                title: None,
                model: model.to_string(),
                provider: None,
                base_url: None,
                duration_ms: 0,
                diff_fetch_ms: None,
                llm_total_ms: None,
                diff_bytes: 0,
                diff_files_total: 0,
                diff_files_reviewed: 0,
                diff_files_skipped: 0,
                comments_total: 0,
                comments_by_severity: HashMap::new(),
                comments_by_category: HashMap::new(),
                overall_score: None,
                hotspots_detected: 0,
                high_risk_files: 0,
                tokens_prompt: None,
                tokens_completion: None,
                tokens_total: None,
                cost_estimate_usd: None,
                file_metrics: None,
                hotspot_details: None,
                convention_suppressed: None,
                comments_by_pass: HashMap::new(),
                agent_iterations: None,
                agent_tool_calls: None,
                github_posted: false,
                github_repo: None,
                github_pr: None,
                error: None,
                created_at: None,
            },
        }
    }

    #[allow(dead_code)]
    pub fn title(mut self, title: &str) -> Self {
        self.event.title = Some(title.to_string());
        self
    }

    pub fn provider(mut self, provider: Option<&str>) -> Self {
        self.event.provider = provider.map(str::to_string);
        self
    }

    pub fn base_url(mut self, base_url: Option<&str>) -> Self {
        self.event.base_url = base_url.map(str::to_string);
        self
    }

    pub fn duration_ms(mut self, ms: u64) -> Self {
        self.event.duration_ms = ms;
        self
    }

    pub fn diff_fetch_ms(mut self, ms: u64) -> Self {
        self.event.diff_fetch_ms = Some(ms);
        self
    }

    pub fn llm_total_ms(mut self, ms: u64) -> Self {
        self.event.llm_total_ms = Some(ms);
        self
    }

    pub fn diff_stats(
        mut self,
        bytes: usize,
        files_total: usize,
        files_reviewed: usize,
        files_skipped: usize,
    ) -> Self {
        self.event.diff_bytes = bytes;
        self.event.diff_files_total = files_total;
        self.event.diff_files_reviewed = files_reviewed;
        self.event.diff_files_skipped = files_skipped;
        self
    }

    pub fn comments(mut self, comments: &[Comment], summary: Option<&ReviewSummary>) -> Self {
        let mut by_severity: HashMap<String, usize> = HashMap::new();
        let mut by_category: HashMap<String, usize> = HashMap::new();
        for c in comments {
            *by_severity.entry(c.severity.to_string()).or_default() += 1;
            *by_category.entry(c.category.to_string()).or_default() += 1;
        }
        self.event.comments_total = comments.len();
        self.event.comments_by_severity = by_severity;
        self.event.comments_by_category = by_category;
        self.event.overall_score = summary.map(|s| s.overall_score);
        self
    }

    pub fn error(mut self, err: &str) -> Self {
        self.event.error = Some(err.to_string());
        self
    }

    pub fn github(mut self, repo: &str, pr: u32) -> Self {
        self.event.github_repo = Some(repo.to_string());
        self.event.github_pr = Some(pr);
        self
    }

    pub fn github_posted(mut self, posted: bool) -> Self {
        self.event.github_posted = posted;
        self
    }

    pub fn tokens(mut self, prompt: usize, completion: usize, total: usize) -> Self {
        self.event.tokens_prompt = Some(prompt);
        self.event.tokens_completion = Some(completion);
        self.event.tokens_total = Some(total);
        self.event.cost_estimate_usd = Some(crate::server::cost::estimate_cost_usd(
            &self.event.model,
            total,
        ));
        self
    }

    pub fn file_metrics(mut self, metrics: Vec<FileMetricEvent>) -> Self {
        if metrics.is_empty() {
            self.event.file_metrics = None;
        } else {
            self.event.file_metrics = Some(metrics);
        }
        self
    }

    pub fn hotspot_details(mut self, details: Vec<HotspotDetail>) -> Self {
        self.event.hotspots_detected = details.len();
        self.event.high_risk_files = details.iter().filter(|h| h.risk_score > 0.6).count();
        if details.is_empty() {
            self.event.hotspot_details = None;
        } else {
            self.event.hotspot_details = Some(details);
        }
        self
    }

    pub fn convention_suppressed(mut self, count: usize) -> Self {
        if count > 0 {
            self.event.convention_suppressed = Some(count);
        }
        self
    }

    pub fn comments_by_pass(mut self, by_pass: HashMap<String, usize>) -> Self {
        self.event.comments_by_pass = by_pass;
        self
    }

    pub fn agent_activity(mut self, activity: Option<&crate::review::AgentActivity>) -> Self {
        if let Some(a) = activity {
            self.event.agent_iterations = Some(a.total_iterations);
            self.event.agent_tool_calls = Some(
                a.tool_calls
                    .iter()
                    .map(|tc| AgentToolCallEvent {
                        iteration: tc.iteration,
                        tool_name: tc.tool_name.clone(),
                        duration_ms: tc.duration_ms,
                    })
                    .collect(),
            );
        }
        self
    }

    pub fn build(mut self) -> ReviewEvent {
        self.event.created_at = Some(chrono::Utc::now());
        self.event
    }
}

/// Emit a review wide event via structured tracing.
/// Also logs one full JSON line per event (target "review.event.json") for log pipelines / OTEL.
pub fn emit_wide_event(event: &ReviewEvent) {
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
        tokens_total = ?event.tokens_total,
        convention_suppressed = ?event.convention_suppressed,
        hotspots_detected = event.hotspots_detected,
        high_risk_files = event.high_risk_files,
        github_posted = event.github_posted,
        error = ?event.error,
        "review.event"
    );
    // One JSON line per event for log pipelines / OTEL: include @timestamp and event.name for filtering.
    let timestamp = event
        .created_at
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let payload = serde_json::json!({
        "@timestamp": timestamp,
        "event": { "name": "review.event", "kind": "event" },
        "review": event
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        info!(target: "review.event.json", "{}", json);
    }
}
