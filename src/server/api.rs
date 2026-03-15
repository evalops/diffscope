use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

use super::pr_readiness::{
    apply_dynamic_review_state, build_pr_readiness_snapshot, build_repo_blocker_rollups,
    get_pr_readiness_snapshot, latest_pr_review_session, latest_review_head_by_source,
    load_review_inventory, parse_pr_diff_source, pr_diff_source, PrReadinessSnapshot,
};
use super::state::{
    build_progress_callback, count_diff_files, count_reviewed_files, current_timestamp,
    emit_wide_event, AppState, FileMetricEvent, HotspotDetail, ReviewEventBuilder, ReviewListItem,
    ReviewSession, ReviewStatus, MAX_DIFF_SIZE,
};
use crate::core::comment::{CommentStatus, CommentSynthesizer, MergeReadiness};
use crate::core::convention_learner::ConventionStore;
use tracing::{info, warn};

#[path = "api/admin.rs"]
mod admin;
#[path = "api/analytics.rs"]
mod analytics;
#[path = "api/gh.rs"]
mod gh;
#[path = "api/reviews.rs"]
mod reviews;
#[path = "api/types.rs"]
mod types;

pub(super) use admin::*;
pub(super) use analytics::*;
pub(super) use gh::*;
pub(super) use reviews::*;
pub(super) use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    // === ListEventsParams::into_filters tests ===

    #[test]
    fn test_into_filters_all_none() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: None,
            time_to: None,
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, None);
        assert_eq!(filters.model, None);
        assert_eq!(filters.status, None);
        assert!(filters.time_from.is_none());
        assert!(filters.time_to.is_none());
        assert_eq!(filters.github_repo, None);
        assert_eq!(filters.limit, None);
        assert_eq!(filters.offset, None);
    }

    #[test]
    fn test_into_filters_with_source_and_model() {
        let params = ListEventsParams {
            source: Some("staged".to_string()),
            model: Some("claude-opus-4-6".to_string()),
            status: None,
            time_from: None,
            time_to: None,
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, Some("staged".to_string()));
        assert_eq!(filters.model, Some("claude-opus-4-6".to_string()));
    }

    #[test]
    fn test_into_filters_with_status_and_github_repo() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: Some("completed".to_string()),
            time_from: None,
            time_to: None,
            github_repo: Some("owner/repo".to_string()),
            limit: Some(50),
            offset: Some(10),
        };
        let filters = params.into_filters();
        assert_eq!(filters.status, Some("completed".to_string()));
        assert_eq!(filters.github_repo, Some("owner/repo".to_string()));
        assert_eq!(filters.limit, Some(50));
        assert_eq!(filters.offset, Some(10));
    }

    #[test]
    fn test_into_filters_valid_rfc3339_time() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: Some("2024-01-01T00:00:00Z".to_string()),
            time_to: Some("2024-12-31T23:59:59Z".to_string()),
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert!(filters.time_from.is_some());
        assert!(filters.time_to.is_some());
        assert_eq!(
            filters.time_from.unwrap().to_rfc3339(),
            "2024-01-01T00:00:00+00:00"
        );
        assert_eq!(
            filters.time_to.unwrap().to_rfc3339(),
            "2024-12-31T23:59:59+00:00"
        );
    }

    #[test]
    fn test_into_filters_invalid_time_becomes_none() {
        let params = ListEventsParams {
            source: None,
            model: None,
            status: None,
            time_from: Some("not-a-date".to_string()),
            time_to: Some("also-invalid".to_string()),
            github_repo: None,
            limit: None,
            offset: None,
        };
        let filters = params.into_filters();
        assert!(filters.time_from.is_none());
        assert!(filters.time_to.is_none());
    }

    #[test]
    fn test_into_filters_all_fields_populated() {
        let params = ListEventsParams {
            source: Some("head".to_string()),
            model: Some("gpt-4".to_string()),
            status: Some("failed".to_string()),
            time_from: Some("2025-06-01T12:00:00Z".to_string()),
            time_to: Some("2025-06-30T12:00:00Z".to_string()),
            github_repo: Some("evalops/diffscope".to_string()),
            limit: Some(100),
            offset: Some(25),
        };
        let filters = params.into_filters();
        assert_eq!(filters.source, Some("head".to_string()));
        assert_eq!(filters.model, Some("gpt-4".to_string()));
        assert_eq!(filters.status, Some("failed".to_string()));
        assert!(filters.time_from.is_some());
        assert!(filters.time_to.is_some());
        assert_eq!(filters.github_repo, Some("evalops/diffscope".to_string()));
        assert_eq!(filters.limit, Some(100));
        assert_eq!(filters.offset, Some(25));
    }

    // === ReviewOverrides Default impl tests ===

    #[test]
    fn test_review_overrides_default() {
        let overrides = ReviewOverrides::default();
        assert_eq!(overrides.model, None);
        assert_eq!(overrides.strictness, None);
        assert_eq!(overrides.review_profile, None);
    }

    #[test]
    fn test_review_overrides_clone() {
        let overrides = ReviewOverrides {
            model: Some("test-model".to_string()),
            strictness: Some(2),
            review_profile: Some("security".to_string()),
        };
        let cloned = overrides.clone();
        assert_eq!(cloned.model, Some("test-model".to_string()));
        assert_eq!(cloned.strictness, Some(2));
        assert_eq!(cloned.review_profile, Some("security".to_string()));
    }

    // === mask_config_secrets tests ===

    #[test]
    fn test_mask_config_secrets_masks_api_key() {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "api_key".to_string(),
            serde_json::json!("sk-secret-key-123"),
        );
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
    }

    #[test]
    fn test_mask_config_secrets_masks_all_secret_fields() {
        let mut obj = serde_json::Map::new();
        obj.insert("api_key".to_string(), serde_json::json!("secret1"));
        obj.insert("github_token".to_string(), serde_json::json!("secret2"));
        obj.insert(
            "github_client_secret".to_string(),
            serde_json::json!("secret3"),
        );
        obj.insert(
            "github_private_key".to_string(),
            serde_json::json!("secret4"),
        );
        obj.insert(
            "github_webhook_secret".to_string(),
            serde_json::json!("secret5"),
        );
        obj.insert("vault_token".to_string(), serde_json::json!("secret6"));
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(obj.get("github_token").unwrap(), &serde_json::json!("***"));
        assert_eq!(
            obj.get("github_client_secret").unwrap(),
            &serde_json::json!("***")
        );
        assert_eq!(
            obj.get("github_private_key").unwrap(),
            &serde_json::json!("***")
        );
        assert_eq!(
            obj.get("github_webhook_secret").unwrap(),
            &serde_json::json!("***")
        );
        assert_eq!(obj.get("vault_token").unwrap(), &serde_json::json!("***"));
    }

    #[test]
    fn test_mask_config_secrets_does_not_mask_null_secrets() {
        let mut obj = serde_json::Map::new();
        obj.insert("api_key".to_string(), serde_json::Value::Null);
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_config_secrets(&mut obj);
        // Null values are not strings, so they should not be masked
        assert_eq!(obj.get("api_key").unwrap(), &serde_json::Value::Null);
    }

    #[test]
    fn test_mask_config_secrets_no_secret_fields_present() {
        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        obj.insert(
            "base_url".to_string(),
            serde_json::json!("http://localhost"),
        );
        mask_config_secrets(&mut obj);
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
        assert_eq!(
            obj.get("base_url").unwrap(),
            &serde_json::json!("http://localhost")
        );
    }

    // === mask_provider_api_keys tests ===

    #[test]
    fn test_mask_provider_api_keys_masks_nested_keys() {
        let mut obj = serde_json::Map::new();
        let mut providers = serde_json::Map::new();
        let mut openai = serde_json::Map::new();
        openai.insert("api_key".to_string(), serde_json::json!("sk-openai-key"));
        openai.insert(
            "base_url".to_string(),
            serde_json::json!("https://api.openai.com"),
        );
        providers.insert("openai".to_string(), serde_json::Value::Object(openai));
        obj.insert(
            "providers".to_string(),
            serde_json::Value::Object(providers),
        );

        mask_provider_api_keys(&mut obj);

        let providers = obj.get("providers").unwrap().as_object().unwrap();
        let openai = providers.get("openai").unwrap().as_object().unwrap();
        assert_eq!(openai.get("api_key").unwrap(), &serde_json::json!("***"));
        assert_eq!(
            openai.get("base_url").unwrap(),
            &serde_json::json!("https://api.openai.com")
        );
    }

    #[test]
    fn test_mask_provider_api_keys_no_providers_field() {
        let mut obj = serde_json::Map::new();
        obj.insert("model".to_string(), serde_json::json!("gpt-4"));
        mask_provider_api_keys(&mut obj);
        // Should not panic; nothing to mask
        assert_eq!(obj.get("model").unwrap(), &serde_json::json!("gpt-4"));
    }

    #[test]
    fn test_mask_provider_api_keys_multiple_providers() {
        let mut obj = serde_json::Map::new();
        let mut providers = serde_json::Map::new();

        let mut anthropic = serde_json::Map::new();
        anthropic.insert("api_key".to_string(), serde_json::json!("ant-key"));
        providers.insert(
            "anthropic".to_string(),
            serde_json::Value::Object(anthropic),
        );

        let mut ollama = serde_json::Map::new();
        ollama.insert(
            "base_url".to_string(),
            serde_json::json!("http://localhost:11434"),
        );
        // No api_key for ollama
        providers.insert("ollama".to_string(), serde_json::Value::Object(ollama));

        obj.insert(
            "providers".to_string(),
            serde_json::Value::Object(providers),
        );
        mask_provider_api_keys(&mut obj);

        let providers = obj.get("providers").unwrap().as_object().unwrap();
        let anthropic = providers.get("anthropic").unwrap().as_object().unwrap();
        assert_eq!(anthropic.get("api_key").unwrap(), &serde_json::json!("***"));

        let ollama = providers.get("ollama").unwrap().as_object().unwrap();
        assert!(ollama.get("api_key").is_none());
    }

    // === is_valid_repo_name tests ===

    #[test]
    fn test_is_valid_repo_name_standard() {
        assert!(is_valid_repo_name("owner/repo"));
    }

    #[test]
    fn test_is_valid_repo_name_with_hyphens_and_dots() {
        assert!(is_valid_repo_name("my-org/my-repo.rs"));
    }

    #[test]
    fn test_is_valid_repo_name_with_underscores() {
        assert!(is_valid_repo_name("my_org/my_repo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_empty() {
        assert!(!is_valid_repo_name(""));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_no_slash() {
        assert!(!is_valid_repo_name("justrepo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_spaces() {
        assert!(!is_valid_repo_name("owner/my repo"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_multiple_slashes() {
        assert!(!is_valid_repo_name("a/b/c"));
    }

    #[test]
    fn test_is_valid_repo_name_rejects_special_chars() {
        assert!(!is_valid_repo_name("owner/repo@v1"));
    }

    // === urlencoded tests ===

    #[test]
    fn test_urlencoded_alphanumeric() {
        assert_eq!(urlencoded("hello123"), "hello123");
    }

    #[test]
    fn test_urlencoded_spaces_become_plus() {
        assert_eq!(urlencoded("hello world"), "hello+world");
    }

    #[test]
    fn test_urlencoded_special_chars_percent_encoded() {
        assert_eq!(urlencoded("a=b&c"), "a%3Db%26c");
    }

    #[test]
    fn test_urlencoded_unreserved_chars_pass_through() {
        assert_eq!(urlencoded("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn test_urlencoded_empty_string() {
        assert_eq!(urlencoded(""), "");
    }

    #[test]
    fn test_urlencoded_slash() {
        assert_eq!(urlencoded("owner/repo"), "owner%2Frepo");
    }

    // === Request/Response serialization tests ===

    #[test]
    fn test_start_review_request_deserialize_minimal() {
        let json = r#"{"diff_source": "head"}"#;
        let req: StartReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.diff_source, "head");
        assert_eq!(req.base_branch, None);
        assert_eq!(req.diff_content, None);
        assert_eq!(req.title, None);
        assert_eq!(req.model, None);
        assert_eq!(req.strictness, None);
        assert_eq!(req.review_profile, None);
    }

    #[test]
    fn test_start_review_request_deserialize_full() {
        let json = r#"{
            "diff_source": "raw",
            "base_branch": "main",
            "diff_content": "diff --git a/file.rs",
            "title": "Test PR",
            "model": "claude-opus-4-6",
            "strictness": 3,
            "review_profile": "security"
        }"#;
        let req: StartReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.diff_source, "raw");
        assert_eq!(req.base_branch, Some("main".to_string()));
        assert_eq!(req.diff_content, Some("diff --git a/file.rs".to_string()));
        assert_eq!(req.title, Some("Test PR".to_string()));
        assert_eq!(req.model, Some("claude-opus-4-6".to_string()));
        assert_eq!(req.strictness, Some(3));
        assert_eq!(req.review_profile, Some("security".to_string()));
    }

    #[test]
    fn test_start_review_response_serialize() {
        let resp = StartReviewResponse {
            id: "abc-123".to_string(),
            status: ReviewStatus::Pending,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "abc-123");
        assert_eq!(json["status"], "Pending");
    }

    #[test]
    fn test_feedback_request_deserialize() {
        let json = r#"{"comment_id": "cmt-1", "action": "accept"}"#;
        let req: FeedbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.comment_id, "cmt-1");
        assert_eq!(req.action, "accept");
    }

    #[test]
    fn test_feedback_response_serialize() {
        let resp = FeedbackResponse { ok: true };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
    }

    #[test]
    fn test_status_response_serialize() {
        let resp = StatusResponse {
            repo_path: "/path/to/repo".to_string(),
            branch: Some("main".to_string()),
            model: "gpt-4".to_string(),
            adapter: Some("openai".to_string()),
            base_url: None,
            active_reviews: 2,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["repo_path"], "/path/to/repo");
        assert_eq!(json["branch"], "main");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["adapter"], "openai");
        assert!(json["base_url"].is_null());
        assert_eq!(json["active_reviews"], 2);
    }

    #[test]
    fn test_list_reviews_params_deserialize_empty() {
        let json = r#"{}"#;
        let params: ListReviewsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.page, None);
        assert_eq!(params.per_page, None);
    }

    #[test]
    fn test_list_reviews_params_deserialize_with_values() {
        let json = r#"{"page": 3, "per_page": 50}"#;
        let params: ListReviewsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.page, Some(3));
        assert_eq!(params.per_page, Some(50));
    }

    #[test]
    fn test_list_events_params_deserialize() {
        let json = r#"{"source": "head", "limit": 100}"#;
        let params: ListEventsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.source, Some("head".to_string()));
        assert_eq!(params.limit, Some(100));
        assert_eq!(params.model, None);
    }

    #[test]
    fn test_test_provider_request_deserialize() {
        let json = r#"{"provider": "anthropic", "api_key": "sk-ant-123"}"#;
        let req: TestProviderRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.provider, "anthropic");
        assert_eq!(req.api_key, Some("sk-ant-123".to_string()));
        assert_eq!(req.base_url, None);
    }

    #[test]
    fn test_test_provider_response_serialize() {
        let resp = TestProviderResponse {
            ok: true,
            message: "Connected".to_string(),
            models: vec!["model-a".to_string(), "model-b".to_string()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["message"], "Connected");
        assert_eq!(json["models"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_gh_status_response_serialize() {
        let resp = GhStatusResponse {
            authenticated: true,
            username: Some("testuser".to_string()),
            avatar_url: Some("https://example.com/avatar.png".to_string()),
            scopes: vec!["repo".to_string(), "read:org".to_string()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["authenticated"], true);
        assert_eq!(json["username"], "testuser");
        assert_eq!(json["scopes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_gh_repo_serialize() {
        let repo = GhRepo {
            full_name: "owner/repo".to_string(),
            description: Some("A test repo".to_string()),
            language: Some("Rust".to_string()),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            open_prs: 5,
            open_blockers: Some(3),
            blocking_prs: Some(2),
            default_branch: "main".to_string(),
            stargazers_count: 42,
            private: false,
        };
        let json = serde_json::to_value(&repo).unwrap();
        assert_eq!(json["full_name"], "owner/repo");
        assert_eq!(json["language"], "Rust");
        assert_eq!(json["open_prs"], 5);
        assert_eq!(json["open_blockers"], 3);
        assert_eq!(json["blocking_prs"], 2);
        assert_eq!(json["stargazers_count"], 42);
        assert_eq!(json["private"], false);
    }

    #[test]
    fn test_gh_pull_request_serialize() {
        let pr = GhPullRequest {
            number: 42,
            title: "Fix bug".to_string(),
            author: "dev".to_string(),
            state: "open".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-02T00:00:00Z".to_string(),
            additions: 10,
            deletions: 5,
            changed_files: 3,
            head_branch: "fix-branch".to_string(),
            base_branch: "main".to_string(),
            labels: vec!["bugfix".to_string()],
            draft: false,
            open_blockers: Some(2),
            merge_readiness: Some(MergeReadiness::NeedsAttention),
        };
        let json = serde_json::to_value(&pr).unwrap();
        assert_eq!(json["number"], 42);
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["author"], "dev");
        assert_eq!(json["draft"], false);
        assert_eq!(json["open_blockers"], 2);
        assert_eq!(json["merge_readiness"], "NeedsAttention");
        assert_eq!(json["labels"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_pr_readiness_snapshot_serialize() {
        let snapshot = PrReadinessSnapshot {
            repo: "owner/repo".to_string(),
            pr_number: 42,
            diff_source: "pr:owner/repo#42".to_string(),
            current_head_sha: Some("abc123".to_string()),
            latest_review: None,
            timeline: Vec::new(),
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["repo"], "owner/repo");
        assert_eq!(json["pr_number"], 42);
        assert_eq!(json["current_head_sha"], "abc123");
    }

    #[test]
    fn test_start_pr_review_request_deserialize() {
        let json = r#"{"repo": "owner/repo", "pr_number": 42, "post_results": true}"#;
        let req: StartPrReviewRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.repo, "owner/repo");
        assert_eq!(req.pr_number, 42);
        assert!(req.post_results);
    }

    #[test]
    fn test_apply_comment_lifecycle_transition_sets_and_clears_resolved_at() {
        let mut comment = make_search_comment("lifecycle", CommentStatus::Open);

        assert!(apply_comment_lifecycle_transition(
            &mut comment,
            CommentStatus::Resolved,
            123,
        ));
        assert_eq!(comment.status, CommentStatus::Resolved);
        assert_eq!(comment.resolved_at, Some(123));

        assert!(apply_comment_lifecycle_transition(
            &mut comment,
            CommentStatus::Open,
            456,
        ));
        assert_eq!(comment.status, CommentStatus::Open);
        assert_eq!(comment.resolved_at, None);
    }

    #[test]
    fn test_apply_comment_lifecycle_transition_is_noop_when_status_matches() {
        let mut comment = make_search_comment("lifecycle", CommentStatus::Resolved);
        comment.resolved_at = Some(123);

        assert!(!apply_comment_lifecycle_transition(
            &mut comment,
            CommentStatus::Resolved,
            456,
        ));
        assert_eq!(comment.resolved_at, Some(123));
    }

    fn make_search_comment(id: &str, status: CommentStatus) -> crate::core::Comment {
        crate::core::Comment {
            id: id.to_string(),
            file_path: std::path::PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: format!("comment {id}"),
            rule_id: None,
            severity: crate::core::comment::Severity::Warning,
            category: crate::core::comment::Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: crate::core::comment::FixEffort::Low,
            feedback: None,
            status,
            resolved_at: None,
        }
    }

    fn make_grouped_comment(
        id: &str,
        file_path: &str,
        severity: crate::core::comment::Severity,
        status: CommentStatus,
    ) -> crate::core::Comment {
        let mut comment = make_search_comment(id, status);
        comment.file_path = std::path::PathBuf::from(file_path);
        comment.severity = severity;
        comment
    }

    #[test]
    fn test_comment_search_filter_parses_aliases() {
        assert_eq!(
            CommentSearchFilter::from_api_value(None),
            Some(CommentSearchFilter::All)
        );
        assert_eq!(
            CommentSearchFilter::from_api_value(Some("open")),
            Some(CommentSearchFilter::Unresolved)
        );
        assert_eq!(
            CommentSearchFilter::from_api_value(Some("unresolved")),
            Some(CommentSearchFilter::Unresolved)
        );
        assert_eq!(
            CommentSearchFilter::from_api_value(Some("resolved")),
            Some(CommentSearchFilter::Resolved)
        );
        assert_eq!(
            CommentSearchFilter::from_api_value(Some("dismissed")),
            Some(CommentSearchFilter::Dismissed)
        );
        assert_eq!(CommentSearchFilter::from_api_value(Some("wat")), None);
    }

    #[test]
    fn test_filter_comments_by_search_filter_matches_status() {
        let comments = vec![
            make_search_comment("open", CommentStatus::Open),
            make_search_comment("resolved", CommentStatus::Resolved),
            make_search_comment("dismissed", CommentStatus::Dismissed),
        ];

        assert_eq!(
            filter_comments_by_search_filter(&comments, CommentSearchFilter::All)
                .into_iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>(),
            vec!["open", "resolved", "dismissed"]
        );
        assert_eq!(
            filter_comments_by_search_filter(&comments, CommentSearchFilter::Unresolved)
                .into_iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>(),
            vec!["open"]
        );
        assert_eq!(
            filter_comments_by_search_filter(&comments, CommentSearchFilter::Resolved)
                .into_iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>(),
            vec!["resolved"]
        );
        assert_eq!(
            filter_comments_by_search_filter(&comments, CommentSearchFilter::Dismissed)
                .into_iter()
                .map(|comment| comment.id)
                .collect::<Vec<_>>(),
            vec!["dismissed"]
        );
    }

    #[test]
    fn test_findings_group_by_parser_defaults_to_severity() {
        assert_eq!(
            FindingsGroupBy::from_api_value(None),
            Some(FindingsGroupBy::Severity)
        );
        assert_eq!(
            FindingsGroupBy::from_api_value(Some("severity")),
            Some(FindingsGroupBy::Severity)
        );
        assert_eq!(
            FindingsGroupBy::from_api_value(Some("file")),
            Some(FindingsGroupBy::File)
        );
        assert_eq!(
            FindingsGroupBy::from_api_value(Some("lifecycle")),
            Some(FindingsGroupBy::Lifecycle)
        );
        assert_eq!(
            FindingsGroupBy::from_api_value(Some("status")),
            Some(FindingsGroupBy::Lifecycle)
        );
        assert_eq!(FindingsGroupBy::from_api_value(Some("wat")), None);
    }

    #[test]
    fn test_group_pr_findings_by_severity_orders_buckets() {
        let findings = vec![
            make_grouped_comment(
                "warning",
                "src/b.rs",
                crate::core::comment::Severity::Warning,
                CommentStatus::Open,
            ),
            make_grouped_comment(
                "error",
                "src/a.rs",
                crate::core::comment::Severity::Error,
                CommentStatus::Resolved,
            ),
            make_grouped_comment(
                "info",
                "src/c.rs",
                crate::core::comment::Severity::Info,
                CommentStatus::Dismissed,
            ),
        ];

        let groups = group_pr_findings(&findings, FindingsGroupBy::Severity);

        assert_eq!(
            groups
                .iter()
                .map(|group| group.value.as_str())
                .collect::<Vec<_>>(),
            vec!["Error", "Warning", "Info"]
        );
        assert_eq!(
            groups.iter().map(|group| group.count).collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
    }

    #[test]
    fn test_group_pr_findings_by_file_orders_paths() {
        let findings = vec![
            make_grouped_comment(
                "b",
                "src/z.rs",
                crate::core::comment::Severity::Warning,
                CommentStatus::Open,
            ),
            make_grouped_comment(
                "a",
                "src/a.rs",
                crate::core::comment::Severity::Error,
                CommentStatus::Open,
            ),
            make_grouped_comment(
                "a-2",
                "src/a.rs",
                crate::core::comment::Severity::Info,
                CommentStatus::Resolved,
            ),
        ];

        let groups = group_pr_findings(&findings, FindingsGroupBy::File);

        assert_eq!(
            groups
                .iter()
                .map(|group| group.value.as_str())
                .collect::<Vec<_>>(),
            vec!["src/a.rs", "src/z.rs"]
        );
        assert_eq!(
            groups.iter().map(|group| group.count).collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn test_group_pr_findings_by_lifecycle_orders_statuses() {
        let findings = vec![
            make_grouped_comment(
                "resolved",
                "src/a.rs",
                crate::core::comment::Severity::Warning,
                CommentStatus::Resolved,
            ),
            make_grouped_comment(
                "open",
                "src/b.rs",
                crate::core::comment::Severity::Error,
                CommentStatus::Open,
            ),
            make_grouped_comment(
                "dismissed",
                "src/c.rs",
                crate::core::comment::Severity::Info,
                CommentStatus::Dismissed,
            ),
        ];

        let groups = group_pr_findings(&findings, FindingsGroupBy::Lifecycle);

        assert_eq!(
            groups
                .iter()
                .map(|group| group.value.as_str())
                .collect::<Vec<_>>(),
            vec!["Open", "Resolved", "Dismissed"]
        );
        assert_eq!(
            groups.iter().map(|group| group.count).collect::<Vec<_>>(),
            vec![1, 1, 1]
        );
    }

    fn make_pr_review_session(
        diff_source: &str,
        requested_post_results: Option<bool>,
        github_posted: bool,
    ) -> ReviewSession {
        ReviewSession {
            id: "review-123".to_string(),
            status: ReviewStatus::Complete,
            diff_source: diff_source.to_string(),
            github_head_sha: Some("abc123".to_string()),
            github_post_results_requested: requested_post_results,
            started_at: 10,
            completed_at: Some(20),
            comments: Vec::new(),
            summary: None,
            files_reviewed: 0,
            error: None,
            pr_summary_text: None,
            diff_content: None,
            event: Some(
                ReviewEventBuilder::new("review-123", "review.completed", diff_source, "gpt-4")
                    .github_posted(github_posted)
                    .build(),
            ),
            progress: None,
        }
    }

    #[test]
    fn test_build_rerun_pr_review_request_reuses_saved_policy() {
        let session = make_pr_review_session("pr:owner/repo#42", Some(true), false);

        let request = build_rerun_pr_review_request(&session, None).expect("rerun request");

        assert_eq!(request.repo, "owner/repo");
        assert_eq!(request.pr_number, 42);
        assert!(request.post_results);
    }

    #[test]
    fn test_build_rerun_pr_review_request_prefers_override() {
        let session = make_pr_review_session("pr:owner/repo#42", Some(true), false);

        let request = build_rerun_pr_review_request(&session, Some(false)).expect("rerun request");

        assert!(!request.post_results);
    }

    #[test]
    fn test_build_rerun_pr_review_request_falls_back_to_legacy_event_signal() {
        let session = make_pr_review_session("pr:owner/repo#42", None, true);

        let request = build_rerun_pr_review_request(&session, None).expect("rerun request");

        assert!(request.post_results);
    }

    #[test]
    fn test_build_rerun_pr_review_request_rejects_non_pr_reviews() {
        let session = make_pr_review_session("head", Some(true), false);

        let err = build_rerun_pr_review_request(&session, None).expect_err("non-pr review");

        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1, "Review is not tied to a GitHub PR.");
    }

    #[test]
    fn test_summarize_learned_rule_patterns_orders_by_total_observations() {
        let mut store = ConventionStore::new();

        for _ in 0..3 {
            store.record_feedback(
                "Prefer iterator adapters over manual loops",
                "BestPractice",
                true,
                Some("*.rs"),
                "2024-01-01T00:00:00Z",
            );
        }
        for _ in 0..4 {
            store.record_feedback(
                "Use the builder pattern for complex config",
                "BestPractice",
                true,
                Some("*.rs"),
                "2024-01-02T00:00:00Z",
            );
        }

        let summaries = summarize_learned_rule_patterns(store.boost_patterns());

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].total_observations, 4);
        assert!(summaries[0].pattern_text.contains("builder pattern"));
        assert_eq!(
            summaries[1].pattern_text,
            "prefer iterator adapters over manual loops"
        );
        assert!(summaries[0].confidence >= summaries[1].confidence);
    }

    #[test]
    fn test_latest_attention_gap_snapshot_skips_newer_empty_entries() {
        let trend = FeedbackEvalTrendResponse {
            entries: vec![
                FeedbackEvalTrendEntryResponse {
                    timestamp: "2024-01-01T00:00:00Z".to_string(),
                    eval_label: Some("weekly-batch".to_string()),
                    eval_model: Some("frontier".to_string()),
                    eval_provider: Some("anthropic".to_string()),
                    attention_by_category: vec![FeedbackEvalTrendGapResponse {
                        name: "Security".to_string(),
                        feedback_total: 8,
                        high_confidence_total: 3,
                        high_confidence_acceptance_rate: 0.2,
                        eval_score: Some(0.6),
                        gap: Some(-0.4),
                    }],
                    attention_by_rule: vec![FeedbackEvalTrendGapResponse {
                        name: "sec.sql.injection".to_string(),
                        feedback_total: 5,
                        high_confidence_total: 2,
                        high_confidence_acceptance_rate: 0.1,
                        eval_score: Some(0.5),
                        gap: Some(-0.4),
                    }],
                    ..Default::default()
                },
                FeedbackEvalTrendEntryResponse {
                    timestamp: "2024-01-02T00:00:00Z".to_string(),
                    ..Default::default()
                },
            ],
        };

        let snapshot = latest_attention_gap_snapshot(&trend);

        assert_eq!(snapshot.timestamp, "2024-01-01T00:00:00Z");
        assert_eq!(snapshot.eval_label.as_deref(), Some("weekly-batch"));
        assert_eq!(snapshot.by_category.len(), 1);
        assert_eq!(snapshot.by_category[0].name, "Security");
        assert_eq!(snapshot.by_rule.len(), 1);
        assert_eq!(snapshot.by_rule[0].name, "sec.sql.injection");
    }

    #[test]
    fn test_summarize_rejected_patterns_orders_by_rejected_count() {
        let mut store = crate::review::FeedbackStore::default();

        for _ in 0..2 {
            store.record_feedback("Style", Some("*.rs"), false);
        }
        for _ in 0..4 {
            store.record_feedback("Bug", Some("*.ts"), false);
        }
        for _ in 0..3 {
            store.record_rule_feedback_patterns("style.rule", &["*.rs"], false);
        }
        for _ in 0..1 {
            store.record_rule_feedback_patterns("bug.rule", &["*.ts"], false);
        }

        let (by_category, by_rule, by_file_pattern) = summarize_rejected_patterns(&store);

        assert_eq!(
            by_category
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Bug", "Style"]
        );
        assert_eq!(by_category[0].rejected, 4);
        assert_eq!(
            by_rule
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            vec!["style.rule", "bug.rule"]
        );
        assert_eq!(by_rule[0].rejected, 3);
        assert_eq!(
            by_file_pattern
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            vec!["*.ts", "*.rs"]
        );
        assert_eq!(by_file_pattern[0].rejected, 4);
    }
}
