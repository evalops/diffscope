use super::*;

use crate::server::state::ReviewEvent;
use crate::server::storage::EventStats;

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
