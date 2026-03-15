use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::core::comment::{Comment, ReviewSummary};

use super::storage::StorageBackend;

#[path = "state/events.rs"]
mod events;
#[path = "state/github.rs"]
mod github;
#[path = "state/lifecycle.rs"]
mod lifecycle;
#[path = "state/persistence.rs"]
mod persistence;
#[path = "state/progress.rs"]
mod progress;
#[path = "state/types.rs"]
mod types;

pub(crate) use events::*;
pub(crate) use lifecycle::*;
pub(crate) use progress::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    #[test]
    fn test_current_timestamp_is_positive() {
        let ts = current_timestamp();
        assert!(ts > 0);
    }

    #[test]
    fn test_count_diff_files_empty() {
        assert_eq!(count_diff_files(""), 0);
        assert_eq!(count_diff_files("some random text"), 0);
    }

    #[test]
    fn test_count_diff_files_single() {
        let diff = "diff --git a/foo.rs b/foo.rs\n+hello\n";
        assert_eq!(count_diff_files(diff), 1);
    }

    #[test]
    fn test_count_diff_files_multiple() {
        let diff = "diff --git a/a.rs b/a.rs\n+a\n\ndiff --git a/b.rs b/b.rs\n+b\n";
        assert_eq!(count_diff_files(diff), 2);
    }

    #[test]
    fn test_emit_wide_event_payload_has_otel_shape() {
        let event = ReviewEventBuilder::new("r-otel", "review.completed", "head", "gpt-4o")
            .duration_ms(100)
            .build();
        let timestamp = event
            .created_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let payload = serde_json::json!({
            "@timestamp": timestamp,
            "event": { "name": "review.event", "kind": "event" },
            "review": event
        });
        let json = serde_json::to_string(&payload).unwrap();
        assert!(
            json.contains("@timestamp"),
            "OTEL payload must include @timestamp"
        );
        assert!(
            json.contains("\"name\":\"review.event\""),
            "OTEL payload must include event.name for filtering"
        );
        assert!(
            json.contains("\"review\""),
            "OTEL payload must include review object"
        );
        assert!(json.contains("r-otel"), "payload must contain review_id");
    }

    #[test]
    fn test_review_event_builder_minimal() {
        let event = ReviewEventBuilder::new("r1", "review.completed", "head", "gpt-4o").build();
        assert_eq!(event.review_id, "r1");
        assert_eq!(event.event_type, "review.completed");
        assert_eq!(event.diff_source, "head");
        assert_eq!(event.model, "gpt-4o");
        assert!(event.title.is_none());
        assert!(event.error.is_none());
        assert!(!event.github_posted);
        assert_eq!(event.comments_total, 0);
        assert!(event.tokens_prompt.is_none());
        assert!(event.tokens_completion.is_none());
        assert!(event.tokens_total.is_none());
        assert!(event.file_metrics.is_none());
        assert!(event.hotspot_details.is_none());
        assert!(event.convention_suppressed.is_none());
        assert!(event.comments_by_pass.is_empty());
    }

    #[test]
    fn test_review_event_builder_full() {
        let comments = vec![Comment {
            id: "c1".to_string(),
            file_path: PathBuf::from("a.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Error,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);

        let mut by_pass = HashMap::new();
        by_pass.insert("security".to_string(), 1);

        let event =
            ReviewEventBuilder::new("r2", "review.completed", "staged", "claude-sonnet-4.6")
                .title("Test PR")
                .provider(Some("anthropic"))
                .base_url(Some("https://api.anthropic.com"))
                .duration_ms(5000)
                .diff_fetch_ms(100)
                .llm_total_ms(4500)
                .diff_stats(1024, 3, 2, 1)
                .comments(&comments, Some(&summary))
                .tokens(200, 100, 300)
                .file_metrics(vec![FileMetricEvent {
                    file_path: "a.rs".to_string(),
                    latency_ms: 100,
                    prompt_tokens: 200,
                    completion_tokens: 100,
                    total_tokens: 300,
                    comment_count: 1,
                }])
                .hotspot_details(vec![HotspotDetail {
                    file_path: "a.rs".to_string(),
                    risk_score: 0.8,
                    reasons: vec!["complex".to_string()],
                }])
                .convention_suppressed(2)
                .comments_by_pass(by_pass)
                .github("owner/repo", 42)
                .github_posted(true)
                .build();

        assert_eq!(event.title.as_deref(), Some("Test PR"));
        assert_eq!(event.provider.as_deref(), Some("anthropic"));
        assert_eq!(event.duration_ms, 5000);
        assert_eq!(event.diff_fetch_ms, Some(100));
        assert_eq!(event.llm_total_ms, Some(4500));
        assert_eq!(event.diff_bytes, 1024);
        assert_eq!(event.diff_files_total, 3);
        assert_eq!(event.diff_files_reviewed, 2);
        assert_eq!(event.diff_files_skipped, 1);
        assert_eq!(event.comments_total, 1);
        assert!(event.comments_by_severity.contains_key("Error"));
        assert!(event.comments_by_category.contains_key("Bug"));
        assert!(event.overall_score.is_some());
        assert_eq!(event.tokens_prompt, Some(200));
        assert_eq!(event.tokens_completion, Some(100));
        assert_eq!(event.tokens_total, Some(300));
        assert!(event.file_metrics.is_some());
        assert_eq!(event.file_metrics.as_ref().unwrap().len(), 1);
        assert_eq!(event.hotspots_detected, 1);
        assert_eq!(event.high_risk_files, 1);
        assert!(event.hotspot_details.is_some());
        assert_eq!(event.convention_suppressed, Some(2));
        assert_eq!(event.comments_by_pass.get("security"), Some(&1));
        assert_eq!(event.github_repo.as_deref(), Some("owner/repo"));
        assert_eq!(event.github_pr, Some(42));
        assert!(event.github_posted);
    }

    #[test]
    fn test_review_event_builder_error() {
        let event = ReviewEventBuilder::new("r3", "review.failed", "head", "gpt-4o")
            .error("timeout")
            .build();
        assert_eq!(event.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_count_reviewed_files() {
        let comments = vec![
            Comment {
                id: "c1".to_string(),
                file_path: PathBuf::from("a.rs"),
                line_number: 1,
                content: "test".to_string(),
                rule_id: None,
                severity: Severity::Warning,
                category: Category::Style,
                suggestion: None,
                confidence: 0.5,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Low,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
            Comment {
                id: "c2".to_string(),
                file_path: PathBuf::from("b.rs"),
                line_number: 2,
                content: "test2".to_string(),
                rule_id: None,
                severity: Severity::Info,
                category: Category::Style,
                suggestion: None,
                confidence: 0.5,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Low,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
            Comment {
                id: "c3".to_string(),
                file_path: PathBuf::from("a.rs"),
                line_number: 5,
                content: "test3".to_string(),
                rule_id: None,
                severity: Severity::Warning,
                category: Category::Bug,
                suggestion: None,
                confidence: 0.8,
                code_suggestion: None,
                tags: vec![],
                fix_effort: FixEffort::Medium,
                feedback: None,
                status: crate::core::comment::CommentStatus::Open,
                resolved_at: None,
            },
        ];
        assert_eq!(count_reviewed_files(&comments), 2);
    }

    #[test]
    fn test_count_reviewed_files_empty() {
        assert_eq!(count_reviewed_files(&[]), 0);
    }

    #[test]
    fn test_save_reviews_returns_awaitable_handle() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path: storage_path.clone(),
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });
            let handle = AppState::save_reviews_async(&state);
            // The handle should be awaitable and complete successfully
            handle.await.unwrap();
            // File should exist on disk
            assert!(storage_path.exists());
        });
    }

    #[test]
    fn test_builder_tokens() {
        let event = ReviewEventBuilder::new("r-tok", "review.completed", "head", "gpt-4o")
            .tokens(100, 50, 150)
            .build();
        assert_eq!(event.tokens_prompt, Some(100));
        assert_eq!(event.tokens_completion, Some(50));
        assert_eq!(event.tokens_total, Some(150));
    }

    #[test]
    fn test_builder_hotspot_details() {
        let details = vec![
            HotspotDetail {
                file_path: "risky.rs".to_string(),
                risk_score: 0.9,
                reasons: vec!["high complexity".to_string()],
            },
            HotspotDetail {
                file_path: "safe.rs".to_string(),
                risk_score: 0.3,
                reasons: vec!["minor change".to_string()],
            },
        ];
        let event = ReviewEventBuilder::new("r-hot", "review.completed", "head", "gpt-4o")
            .hotspot_details(details)
            .build();
        assert_eq!(event.hotspots_detected, 2);
        assert_eq!(event.high_risk_files, 1); // only risky.rs has score > 0.6
        assert!(event.hotspot_details.is_some());
        assert_eq!(event.hotspot_details.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_builder_convention_suppressed() {
        let event = ReviewEventBuilder::new("r-conv", "review.completed", "head", "gpt-4o")
            .convention_suppressed(3)
            .build();
        assert_eq!(event.convention_suppressed, Some(3));

        let event_zero = ReviewEventBuilder::new("r-conv0", "review.completed", "head", "gpt-4o")
            .convention_suppressed(0)
            .build();
        assert!(event_zero.convention_suppressed.is_none());
    }

    #[test]
    fn test_review_semaphore_limits_concurrency() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let sem = Arc::new(tokio::sync::Semaphore::new(2));
            // Acquire 2 permits
            let _p1 = sem.clone().acquire_owned().await.unwrap();
            let _p2 = sem.clone().acquire_owned().await.unwrap();
            // Third should not be available immediately
            assert_eq!(sem.available_permits(), 0);
            // Drop one permit
            drop(_p1);
            assert_eq!(sem.available_permits(), 1);
        });
    }

    #[test]
    fn test_record_and_get_last_reviewed_sha() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path,
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });

            let pr_key = "owner/repo#42";

            // Initially no SHA recorded
            assert!(AppState::get_last_reviewed_sha(&state, pr_key)
                .await
                .is_none());

            // Record a SHA
            AppState::record_reviewed_sha(&state, pr_key, "abc123").await;
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, pr_key)
                    .await
                    .as_deref(),
                Some("abc123"),
            );

            // Update the SHA
            AppState::record_reviewed_sha(&state, pr_key, "def456").await;
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, pr_key)
                    .await
                    .as_deref(),
                Some("def456"),
            );

            // Different PR key is independent
            let other_key = "owner/repo#99";
            assert!(AppState::get_last_reviewed_sha(&state, other_key)
                .await
                .is_none());
        });
    }

    #[test]
    fn test_last_reviewed_shas_multiple_prs() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let storage_path = dir.path().join("reviews.json");
            let config_path = dir.path().join("config.json");
            let json_backend = crate::server::storage_json::JsonStorageBackend::new(&storage_path);
            let state = Arc::new(AppState {
                config: Arc::new(tokio::sync::RwLock::new(crate::config::Config::default())),
                repo_path: dir.path().to_path_buf(),
                reviews: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                storage: Arc::new(json_backend),
                http_client: reqwest::Client::new(),
                storage_path,
                config_path,
                review_semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
                last_reviewed_shas: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            });

            // Record SHAs for multiple PRs across different repos
            AppState::record_reviewed_sha(&state, "org/repo-a#1", "sha_a1").await;
            AppState::record_reviewed_sha(&state, "org/repo-a#2", "sha_a2").await;
            AppState::record_reviewed_sha(&state, "org/repo-b#1", "sha_b1").await;

            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-a#1")
                    .await
                    .as_deref(),
                Some("sha_a1"),
            );
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-a#2")
                    .await
                    .as_deref(),
                Some("sha_a2"),
            );
            assert_eq!(
                AppState::get_last_reviewed_sha(&state, "org/repo-b#1")
                    .await
                    .as_deref(),
                Some("sha_b1"),
            );
        });
    }

    // ── Agent activity builder tests ─────────────────────────────────────

    #[test]
    fn test_builder_agent_activity_none() {
        let event = ReviewEventBuilder::new("r-ag0", "review.completed", "head", "gpt-4o")
            .agent_activity(None)
            .build();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_builder_agent_activity_with_data() {
        let activity = crate::review::AgentActivity {
            total_iterations: 3,
            tool_calls: vec![
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "read_file".to_string(),
                    duration_ms: 15,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "search_codebase".to_string(),
                    duration_ms: 42,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 1,
                    tool_name: "read_file".to_string(),
                    duration_ms: 8,
                },
            ],
        };

        let event = ReviewEventBuilder::new("r-ag1", "review.completed", "head", "claude-opus-4-6")
            .agent_activity(Some(&activity))
            .build();

        assert_eq!(event.agent_iterations, Some(3));
        let calls = event.agent_tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].iteration, 0);
        assert_eq!(calls[0].tool_name, "read_file");
        assert_eq!(calls[0].duration_ms, 15);
        assert_eq!(calls[1].iteration, 0);
        assert_eq!(calls[1].tool_name, "search_codebase");
        assert_eq!(calls[1].duration_ms, 42);
        assert_eq!(calls[2].iteration, 1);
        assert_eq!(calls[2].tool_name, "read_file");
        assert_eq!(calls[2].duration_ms, 8);
    }

    #[test]
    fn test_builder_agent_activity_empty_tool_calls() {
        let activity = crate::review::AgentActivity {
            total_iterations: 1,
            tool_calls: vec![],
        };

        let event = ReviewEventBuilder::new("r-ag2", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();

        assert_eq!(event.agent_iterations, Some(1));
        assert!(event.agent_tool_calls.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_builder_agent_activity_default_none() {
        // Without calling .agent_activity(), fields should be None
        let event = ReviewEventBuilder::new("r-ag3", "review.completed", "head", "gpt-4o").build();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_agent_fields_serialize_skip_when_none() {
        let event = ReviewEventBuilder::new("r-ag4", "review.completed", "head", "gpt-4o").build();
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            !json.contains("agent_iterations"),
            "agent_iterations should be skipped when None"
        );
        assert!(
            !json.contains("agent_tool_calls"),
            "agent_tool_calls should be skipped when None"
        );
    }

    #[test]
    fn test_agent_fields_serialize_when_present() {
        let activity = crate::review::AgentActivity {
            total_iterations: 2,
            tool_calls: vec![crate::core::agent_loop::AgentToolCallLog {
                iteration: 0,
                tool_name: "read_file".to_string(),
                duration_ms: 10,
            }],
        };

        let event = ReviewEventBuilder::new("r-ag5", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"agent_iterations\":2"));
        assert!(json.contains("\"agent_tool_calls\""));
        assert!(json.contains("\"tool_name\":\"read_file\""));
        assert!(json.contains("\"duration_ms\":10"));
    }

    #[test]
    fn test_agent_fields_deserialize_round_trip() {
        let activity = crate::review::AgentActivity {
            total_iterations: 5,
            tool_calls: vec![
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 0,
                    tool_name: "search_codebase".to_string(),
                    duration_ms: 100,
                },
                crate::core::agent_loop::AgentToolCallLog {
                    iteration: 2,
                    tool_name: "get_file_history".to_string(),
                    duration_ms: 250,
                },
            ],
        };

        let event = ReviewEventBuilder::new("r-ag6", "review.completed", "head", "gpt-4o")
            .agent_activity(Some(&activity))
            .build();

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ReviewEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.agent_iterations, Some(5));
        let calls = deserialized.agent_tool_calls.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool_name, "search_codebase");
        assert_eq!(calls[0].duration_ms, 100);
        assert_eq!(calls[1].tool_name, "get_file_history");
        assert_eq!(calls[1].iteration, 2);
    }

    #[test]
    fn test_agent_fields_deserialize_missing_fields() {
        // JSON without agent fields should deserialize with None values
        let json = r#"{
            "review_id": "r-test",
            "event_type": "review.completed",
            "diff_source": "head",
            "model": "gpt-4o",
            "duration_ms": 100,
            "diff_bytes": 500,
            "diff_files_total": 3,
            "diff_files_reviewed": 2,
            "diff_files_skipped": 1,
            "comments_total": 0,
            "comments_by_severity": {},
            "comments_by_category": {},
            "hotspots_detected": 0,
            "high_risk_files": 0,
            "github_posted": false
        }"#;
        let event: ReviewEvent = serde_json::from_str(json).unwrap();
        assert!(event.agent_iterations.is_none());
        assert!(event.agent_tool_calls.is_none());
    }

    #[test]
    fn test_agent_tool_call_event_serde() {
        let tc = AgentToolCallEvent {
            iteration: 2,
            tool_name: "lookup_symbol".to_string(),
            duration_ms: 77,
        };
        let json = serde_json::to_string(&tc).unwrap();
        let deserialized: AgentToolCallEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.iteration, 2);
        assert_eq!(deserialized.tool_name, "lookup_symbol");
        assert_eq!(deserialized.duration_ms, 77);
    }

    #[test]
    fn test_builder_agent_activity_chained_with_other_fields() {
        let activity = crate::review::AgentActivity {
            total_iterations: 4,
            tool_calls: vec![crate::core::agent_loop::AgentToolCallLog {
                iteration: 0,
                tool_name: "read_file".to_string(),
                duration_ms: 5,
            }],
        };

        let comments = vec![Comment {
            id: "c1".to_string(),
            file_path: PathBuf::from("a.rs"),
            line_number: 1,
            content: "test".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Low,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }];
        let summary = crate::core::CommentSynthesizer::generate_summary(&comments);

        let event = ReviewEventBuilder::new("r-ag7", "review.completed", "head", "claude-opus-4-6")
            .provider(Some("anthropic"))
            .duration_ms(5000)
            .comments(&comments, Some(&summary))
            .tokens(200, 100, 300)
            .agent_activity(Some(&activity))
            .build();

        // Verify agent fields
        assert_eq!(event.agent_iterations, Some(4));
        assert_eq!(event.agent_tool_calls.as_ref().unwrap().len(), 1);
        // Verify other fields are unaffected
        assert_eq!(event.provider.as_deref(), Some("anthropic"));
        assert_eq!(event.duration_ms, 5000);
        assert_eq!(event.comments_total, 1);
        assert_eq!(event.tokens_total, Some(300));
    }
}
