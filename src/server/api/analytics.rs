use super::*;

use crate::server::state::ReviewEvent;
use crate::server::storage::EventStats;
use std::collections::{HashMap, HashSet};

const ANALYTICS_RECOMPUTE_PAGE_SIZE: i64 = 200;

#[derive(Default)]
struct AnalyticsRecomputeOutcome {
    reviews_scanned: usize,
    reviews_updated: usize,
    events_updated: usize,
    warnings: Vec<String>,
}

/// Returns all wide events, filtered and sorted newest-first.
/// Uses the storage backend (PostgreSQL or JSON) for querying.
pub(crate) async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsParams>,
) -> Json<Vec<ReviewEvent>> {
    let filters = params.into_filters();
    match state.storage.list_events(&filters).await {
        Ok(events) => Json(events),
        Err(e) => {
            warn!("Failed to list events from storage: {}", e);
            Json(Vec::new())
        }
    }
}

/// Returns aggregated event statistics (server-side analytics).
pub(crate) async fn get_event_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListEventsParams>,
) -> Json<EventStats> {
    let filters = params.into_filters();
    match state.storage.get_event_stats(&filters).await {
        Ok(stats) => Json(stats),
        Err(e) => {
            warn!("Failed to get event stats from storage: {}", e);
            Json(EventStats::default())
        }
    }
}

pub(crate) async fn get_analytics_trends(
    State(state): State<Arc<AppState>>,
) -> Json<AnalyticsTrendsResponse> {
    let config = state.config.read().await.clone();
    let mut warnings = Vec::new();
    let eval_trend = load_optional_json_artifact::<crate::core::eval_benchmarks::QualityTrend>(
        &config.eval_trend_path,
        "eval trend",
        &mut warnings,
    );
    let feedback_eval_trend = load_optional_json_artifact::<FeedbackEvalTrendResponse>(
        &config.feedback_eval_trend_path,
        "feedback-eval trend",
        &mut warnings,
    );

    Json(AnalyticsTrendsResponse {
        eval_trend_path: config.eval_trend_path.display().to_string(),
        feedback_eval_trend_path: config.feedback_eval_trend_path.display().to_string(),
        eval_trend,
        feedback_eval_trend,
        warnings,
    })
}

pub(crate) async fn get_analytics_learned_rules(
    State(state): State<Arc<AppState>>,
) -> Json<LearnedRulesResponse> {
    let config = state.config.read().await.clone();
    let mut warnings = Vec::new();
    let Some(convention_store_path) = resolve_convention_store_path(&config) else {
        warnings.push("Failed to resolve convention store path".to_string());
        return Json(LearnedRulesResponse {
            convention_store_path: None,
            boost: Vec::new(),
            suppress: Vec::new(),
            warnings,
        });
    };

    let convention_store = load_optional_json_artifact::<ConventionStore>(
        &convention_store_path,
        "convention store",
        &mut warnings,
    );

    Json(LearnedRulesResponse {
        convention_store_path: Some(convention_store_path.display().to_string()),
        boost: summarize_learned_rule_patterns(convention_store.boost_patterns()),
        suppress: summarize_learned_rule_patterns(convention_store.suppression_patterns()),
        warnings,
    })
}

pub(crate) async fn get_analytics_attention_gaps(
    State(state): State<Arc<AppState>>,
) -> Json<AttentionGapsResponse> {
    let config = state.config.read().await.clone();
    let mut warnings = Vec::new();
    let feedback_eval_trend = load_optional_json_artifact::<FeedbackEvalTrendResponse>(
        &config.feedback_eval_trend_path,
        "feedback-eval trend",
        &mut warnings,
    );

    Json(AttentionGapsResponse {
        feedback_eval_trend_path: config.feedback_eval_trend_path.display().to_string(),
        latest: latest_attention_gap_snapshot(&feedback_eval_trend),
        warnings,
    })
}

pub(crate) async fn get_analytics_rejected_patterns(
    State(state): State<Arc<AppState>>,
) -> Json<RejectedPatternsResponse> {
    let config = state.config.read().await.clone();
    let mut warnings = Vec::new();
    let feedback_store = load_optional_json_artifact::<crate::review::FeedbackStore>(
        &config.feedback_path,
        "feedback store",
        &mut warnings,
    );
    let (by_category, by_rule, by_file_pattern) = summarize_rejected_patterns(&feedback_store);

    Json(RejectedPatternsResponse {
        feedback_path: config.feedback_path.display().to_string(),
        by_category,
        by_rule,
        by_file_pattern,
        warnings,
    })
}

pub(crate) async fn start_analytics_recompute(
    State(state): State<Arc<AppState>>,
) -> Json<crate::server::state::AnalyticsRecomputeJobStatus> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let job = crate::server::state::AnalyticsRecomputeJobStatus {
        job_id: job_id.clone(),
        status: crate::server::state::AnalyticsRecomputeJobState::Running,
        started_at: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    state
        .analytics_recompute_jobs
        .write()
        .await
        .insert(job_id.clone(), job.clone());

    let state = Arc::clone(&state);
    tokio::spawn(async move {
        run_analytics_recompute_job(state, job_id).await;
    });

    Json(job)
}

pub(crate) async fn get_analytics_recompute_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<crate::server::state::AnalyticsRecomputeJobStatus>, (StatusCode, String)> {
    let jobs = state.analytics_recompute_jobs.read().await;
    let Some(job) = jobs.get(&job_id).cloned() else {
        return Err((
            StatusCode::NOT_FOUND,
            "Analytics recompute job not found.".to_string(),
        ));
    };

    Ok(Json(job))
}

async fn run_analytics_recompute_job(state: Arc<AppState>, job_id: String) {
    let result = recompute_analytics_artifacts(&state).await;
    let finished_at = chrono::Utc::now().to_rfc3339();

    let mut jobs = state.analytics_recompute_jobs.write().await;
    let Some(job) = jobs.get_mut(&job_id) else {
        return;
    };

    job.finished_at = Some(finished_at);
    match result {
        Ok(outcome) => {
            job.status = crate::server::state::AnalyticsRecomputeJobState::Completed;
            job.reviews_scanned = outcome.reviews_scanned;
            job.reviews_updated = outcome.reviews_updated;
            job.events_updated = outcome.events_updated;
            job.warnings = outcome.warnings;
            job.error = None;
        }
        Err(error) => {
            job.status = crate::server::state::AnalyticsRecomputeJobState::Failed;
            job.error = Some(error.to_string());
        }
    }
}

async fn recompute_analytics_artifacts(
    state: &Arc<AppState>,
) -> anyhow::Result<AnalyticsRecomputeOutcome> {
    let mut outcome = AnalyticsRecomputeOutcome::default();
    let sessions = load_all_review_sessions(state).await?;

    for mut session in sessions {
        outcome.reviews_scanned += 1;
        let mut session_changed = refresh_review_summary(&mut session);

        let comments = session.comments.clone();
        let summary = session.summary.clone();
        let diff_source = session.diff_source.clone();
        let event_changed = if let Some(event) = session.event.as_mut() {
            refresh_review_event(&comments, summary.as_ref(), &diff_source, event)
        } else {
            false
        };

        if session_changed {
            outcome.reviews_updated += 1;
        }
        if event_changed {
            outcome.events_updated += 1;
            session_changed = true;
        }

        if !session_changed {
            continue;
        }

        {
            let mut reviews = state.reviews.write().await;
            reviews.insert(session.id.clone(), session.clone());
        }

        state.storage.save_review(&session).await?;
        if let Some(event) = session.event.as_ref() {
            state.storage.save_event(event).await?;
        }
    }

    Ok(outcome)
}

async fn load_all_review_sessions(state: &Arc<AppState>) -> anyhow::Result<Vec<ReviewSession>> {
    let mut sessions: Vec<ReviewSession> = {
        let reviews = state.reviews.read().await;
        reviews.values().cloned().collect()
    };
    let mut seen_ids: HashSet<String> = sessions.iter().map(|session| session.id.clone()).collect();
    let mut offset = 0;

    loop {
        let page = state
            .storage
            .list_reviews(ANALYTICS_RECOMPUTE_PAGE_SIZE, offset)
            .await?;
        if page.is_empty() {
            break;
        }

        let page_len = page.len();
        for session in page {
            if seen_ids.insert(session.id.clone()) {
                sessions.push(session);
            }
        }

        if page_len < ANALYTICS_RECOMPUTE_PAGE_SIZE as usize {
            break;
        }
        offset += ANALYTICS_RECOMPUTE_PAGE_SIZE;
    }

    Ok(sessions)
}

fn refresh_review_summary(session: &mut ReviewSession) -> bool {
    if session.summary.is_none() && session.comments.is_empty() {
        return false;
    }

    let refreshed = crate::core::CommentSynthesizer::inherit_review_state(
        crate::core::CommentSynthesizer::generate_summary(&session.comments),
        session.summary.as_ref(),
    );
    if review_summary_matches(session.summary.as_ref(), &refreshed) {
        return false;
    }
    session.summary = Some(refreshed);
    true
}

fn review_summary_matches(
    current: Option<&crate::core::comment::ReviewSummary>,
    refreshed: &crate::core::comment::ReviewSummary,
) -> bool {
    current.is_some_and(|current| {
        serde_json::to_value(current).ok() == serde_json::to_value(refreshed).ok()
    })
}

fn refresh_review_event(
    comments: &[crate::core::Comment],
    summary: Option<&crate::core::comment::ReviewSummary>,
    diff_source: &str,
    event: &mut ReviewEvent,
) -> bool {
    let mut changed = false;

    let comments_total = comments.len();
    if event.comments_total != comments_total {
        event.comments_total = comments_total;
        changed = true;
    }

    let mut comments_by_severity = HashMap::new();
    let mut comments_by_category = HashMap::new();
    for comment in comments {
        *comments_by_severity
            .entry(comment.severity.to_string())
            .or_insert(0usize) += 1;
        *comments_by_category
            .entry(comment.category.to_string())
            .or_insert(0usize) += 1;
    }
    if event.comments_by_severity != comments_by_severity {
        event.comments_by_severity = comments_by_severity;
        changed = true;
    }
    if event.comments_by_category != comments_by_category {
        event.comments_by_category = comments_by_category;
        changed = true;
    }

    let overall_score = summary.map(|summary| summary.overall_score);
    if event.overall_score != overall_score {
        event.overall_score = overall_score;
        changed = true;
    }

    if let Some((repo, pr_number)) = parse_pr_diff_source(diff_source) {
        if event.github_repo.as_deref() != Some(repo.as_str()) {
            event.github_repo = Some(repo);
            changed = true;
        }
        if event.github_pr != Some(pr_number) {
            event.github_pr = Some(pr_number);
            changed = true;
        }
    }

    let cost_breakdowns =
        crate::server::cost::aggregate_cost_breakdowns(event.cost_breakdowns.clone());
    if event.cost_breakdowns != cost_breakdowns {
        event.cost_breakdowns = cost_breakdowns;
        changed = true;
    }

    if !event.cost_breakdowns.is_empty() {
        let prompt_tokens = event
            .cost_breakdowns
            .iter()
            .map(|row| row.prompt_tokens)
            .sum::<usize>();
        let completion_tokens = event
            .cost_breakdowns
            .iter()
            .map(|row| row.completion_tokens)
            .sum::<usize>();
        let total_tokens = event
            .cost_breakdowns
            .iter()
            .map(|row| row.total_tokens)
            .sum::<usize>();
        let total_cost = event
            .cost_breakdowns
            .iter()
            .map(|row| row.cost_estimate_usd)
            .sum::<f64>();

        if event.tokens_prompt != Some(prompt_tokens) {
            event.tokens_prompt = Some(prompt_tokens);
            changed = true;
        }
        if event.tokens_completion != Some(completion_tokens) {
            event.tokens_completion = Some(completion_tokens);
            changed = true;
        }
        if event.tokens_total != Some(total_tokens) {
            event.tokens_total = Some(total_tokens);
            changed = true;
        }
        if event.cost_estimate_usd != Some(total_cost) {
            event.cost_estimate_usd = Some(total_cost);
            changed = true;
        }
    }

    changed
}

pub(crate) fn load_optional_json_artifact<T>(
    path: &std::path::Path,
    label: &str,
    warnings: &mut Vec<String>,
) -> T
where
    T: DeserializeOwned + Default,
{
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<T>(&content) {
            Ok(value) => value,
            Err(err) => {
                warnings.push(format!(
                    "Failed to parse {} at {}: {}",
                    label,
                    path.display(),
                    err
                ));
                T::default()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => T::default(),
        Err(err) => {
            warnings.push(format!(
                "Failed to read {} at {}: {}",
                label,
                path.display(),
                err
            ));
            T::default()
        }
    }
}

pub(crate) fn resolve_convention_store_path(
    config: &crate::config::Config,
) -> Option<std::path::PathBuf> {
    config
        .convention_store_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            dirs::data_local_dir().map(|dir| dir.join("diffscope").join("conventions.json"))
        })
}

pub(crate) fn summarize_learned_rule_patterns(
    patterns: Vec<&crate::core::convention_learner::ConventionPattern>,
) -> Vec<LearnedRuleResponse> {
    let mut summaries = patterns
        .into_iter()
        .map(|pattern| LearnedRuleResponse {
            pattern_text: pattern.pattern_text.clone(),
            category: pattern.category.clone(),
            accepted_count: pattern.accepted_count,
            rejected_count: pattern.rejected_count,
            total_observations: pattern.total_observations(),
            acceptance_rate: pattern.acceptance_rate(),
            confidence: pattern.confidence(),
            file_patterns: pattern.file_patterns.clone(),
            first_seen: pattern.first_seen.clone(),
            last_seen: pattern.last_seen.clone(),
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .total_observations
            .cmp(&left.total_observations)
            .then_with(|| right.confidence.total_cmp(&left.confidence))
            .then_with(|| left.pattern_text.cmp(&right.pattern_text))
    });
    summaries
}

pub(crate) fn latest_attention_gap_snapshot(
    trend: &FeedbackEvalTrendResponse,
) -> AttentionGapSnapshotResponse {
    trend
        .entries
        .iter()
        .rev()
        .find(|entry| {
            !entry.attention_by_category.is_empty() || !entry.attention_by_rule.is_empty()
        })
        .map(|entry| AttentionGapSnapshotResponse {
            timestamp: entry.timestamp.clone(),
            eval_label: entry.eval_label.clone(),
            eval_model: entry.eval_model.clone(),
            eval_provider: entry.eval_provider.clone(),
            by_category: entry.attention_by_category.clone(),
            by_rule: entry.attention_by_rule.clone(),
        })
        .unwrap_or_default()
}

pub(crate) fn summarize_rejected_patterns(
    store: &crate::review::FeedbackStore,
) -> (
    Vec<RejectedPatternResponse>,
    Vec<RejectedPatternResponse>,
    Vec<RejectedPatternResponse>,
) {
    let by_category = sort_rejected_pattern_summaries(
        store
            .by_category
            .iter()
            .filter(|(_, stats)| stats.rejected > 0)
            .map(|(name, stats)| RejectedPatternResponse {
                name: name.clone(),
                accepted: stats.accepted,
                rejected: stats.rejected,
                total: stats.total(),
                acceptance_rate: stats.acceptance_rate(),
            })
            .collect(),
    );
    let by_rule = sort_rejected_pattern_summaries(
        store
            .by_rule
            .iter()
            .filter(|(_, stats)| stats.rejected > 0)
            .map(|(name, stats)| RejectedPatternResponse {
                name: name.clone(),
                accepted: stats.accepted,
                rejected: stats.rejected,
                total: stats.total(),
                acceptance_rate: stats.acceptance_rate(),
            })
            .collect(),
    );
    let by_file_pattern = sort_rejected_pattern_summaries(
        store
            .by_file_pattern
            .iter()
            .filter(|(_, stats)| stats.rejected > 0)
            .map(|(name, stats)| RejectedPatternResponse {
                name: name.clone(),
                accepted: stats.accepted,
                rejected: stats.rejected,
                total: stats.total(),
                acceptance_rate: stats.acceptance_rate(),
            })
            .collect(),
    );

    (by_category, by_rule, by_file_pattern)
}

pub(crate) fn sort_rejected_pattern_summaries(
    mut summaries: Vec<RejectedPatternResponse>,
) -> Vec<RejectedPatternResponse> {
    summaries.sort_by(|left, right| {
        right
            .rejected
            .cmp(&left.rejected)
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| left.name.cmp(&right.name))
    });
    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};
    use std::path::PathBuf;

    fn make_comment(severity: Severity, category: Category) -> crate::core::Comment {
        crate::core::Comment {
            id: format!("{}-{}", severity, category),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: "Check this path".to_string(),
            rule_id: None,
            severity,
            category,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn refresh_review_summary_rebuilds_scores_from_comments() {
        let mut stale_summary = crate::core::CommentSynthesizer::generate_summary(&[]);
        stale_summary.overall_score = 10.0;

        let mut session = ReviewSession {
            id: "review-1".to_string(),
            status: ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-1".to_string()),
            github_post_results_requested: Some(false),
            started_at: 1,
            completed_at: Some(2),
            comments: vec![make_comment(Severity::Warning, Category::Bug)],
            summary: Some(stale_summary),
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: None,
            progress: None,
        };

        assert!(refresh_review_summary(&mut session));
        assert_eq!(
            session
                .summary
                .as_ref()
                .map(|summary| summary.total_comments),
            Some(1)
        );
        assert!(session
            .summary
            .as_ref()
            .is_some_and(|summary| summary.overall_score < 10.0));
    }

    #[test]
    fn refresh_review_event_updates_aggregates_and_repo_metadata() {
        let comments = vec![
            make_comment(Severity::Error, Category::Security),
            make_comment(Severity::Warning, Category::Bug),
        ];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);
        let mut event = ReviewEventBuilder::new(
            "review-1",
            "review.completed",
            "pr:owner/repo#42",
            "claude-opus-4-6",
        )
        .cost_breakdowns(vec![crate::server::cost::CostBreakdownRow {
            workload: "review".to_string(),
            role: "primary".to_string(),
            provider: Some("anthropic".to_string()),
            model: "claude-opus-4-6".to_string(),
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cost_estimate_usd: 0.42,
        }])
        .build();
        event.comments_total = 0;
        event.github_repo = None;
        event.github_pr = None;

        assert!(refresh_review_event(
            &comments,
            Some(&summary),
            "pr:owner/repo#42",
            &mut event,
        ));
        assert_eq!(event.comments_total, 2);
        assert_eq!(event.comments_by_severity.get("Error"), Some(&1));
        assert_eq!(event.comments_by_category.get("Security"), Some(&1));
        assert_eq!(event.github_repo.as_deref(), Some("owner/repo"));
        assert_eq!(event.github_pr, Some(42));
        assert_eq!(event.tokens_total, Some(15));
        assert_eq!(event.cost_estimate_usd, Some(0.42));
    }
}
