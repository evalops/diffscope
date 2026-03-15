use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FORENSICS_CONTRACT_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForensicsBundleKind {
    Review,
    Eval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForensicsBundleFile {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForensicsBundleManifest {
    pub contract_version: u8,
    pub kind: ForensicsBundleKind,
    pub trigger: String,
    pub identifier: String,
    pub created_at: String,
    pub root_path: String,
    #[serde(default)]
    pub files: Vec<ForensicsBundleFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewForensicsAgentActivity {
    #[serde(default)]
    pub total_iterations: usize,
    #[serde(default)]
    pub tool_calls: Vec<crate::core::agent_loop::AgentToolCallLog>,
}

impl From<&crate::review::AgentActivity> for ReviewForensicsAgentActivity {
    fn from(activity: &crate::review::AgentActivity) -> Self {
        Self {
            total_iterations: activity.total_iterations,
            tool_calls: activity.tool_calls.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewForensicsRuntime {
    #[serde(default)]
    pub diff_source: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<crate::review::verification::VerificationReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_activity: Option<ReviewForensicsAgentActivity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dag_traces: Vec<crate::core::dag::DagExecutionTrace>,
}

#[derive(Debug, Clone)]
pub struct ReviewForensicsBundleInput {
    pub review_id: String,
    pub trigger: String,
    pub session: crate::server::state::ReviewSession,
    pub runtime: ReviewForensicsRuntime,
}

#[derive(Debug, Clone)]
pub struct EvalForensicsBundleInput {
    pub trigger: String,
    pub report: crate::commands::EvalReport,
    pub artifact_dir: Option<PathBuf>,
}

pub fn default_forensics_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("diffscope")
        .join("forensics")
}

pub fn review_bundle_root(review_id: &str) -> PathBuf {
    default_forensics_root()
        .join("reviews")
        .join(sanitize_path_segment(review_id))
}

pub async fn load_review_forensics_manifest(review_id: &str) -> Result<ForensicsBundleManifest> {
    load_manifest(&review_bundle_root(review_id).join("manifest.json")).await
}

pub async fn write_review_forensics_bundle(
    config: &crate::config::Config,
    input: ReviewForensicsBundleInput,
) -> Result<ForensicsBundleManifest> {
    let root = review_bundle_root(&input.review_id);
    write_review_forensics_bundle_at(config, &root, input).await
}

pub async fn write_eval_forensics_bundle(
    config: &crate::config::Config,
    input: EvalForensicsBundleInput,
) -> Result<ForensicsBundleManifest> {
    let identifier = eval_bundle_identifier(&input.report);
    let root = input
        .artifact_dir
        .as_ref()
        .map(|artifact_dir| artifact_dir.join("forensics").join(&identifier))
        .unwrap_or_else(|| default_forensics_root().join("eval").join(&identifier));
    write_eval_forensics_bundle_at(config, &root, input).await
}

async fn write_review_forensics_bundle_at(
    config: &crate::config::Config,
    root: &Path,
    input: ReviewForensicsBundleInput,
) -> Result<ForensicsBundleManifest> {
    tokio::fs::create_dir_all(root).await?;

    let mut files = Vec::new();
    files.push(write_json_file(root, "session.json", &input.session).await?);
    if let Some(event) = input.session.event.as_ref() {
        files.push(write_json_file(root, "event.json", event).await?);
    }
    files.push(write_json_file(root, "runtime.json", &input.runtime).await?);
    let masked_config = masked_config_snapshot(config)?;
    files.push(write_json_file(root, "config.json", &masked_config).await?);

    let manifest = ForensicsBundleManifest {
        contract_version: FORENSICS_CONTRACT_VERSION,
        kind: ForensicsBundleKind::Review,
        trigger: input.trigger,
        identifier: input.review_id,
        created_at: chrono::Utc::now().to_rfc3339(),
        root_path: root.display().to_string(),
        files,
    };
    write_manifest(root, &manifest).await
}

async fn write_eval_forensics_bundle_at(
    config: &crate::config::Config,
    root: &Path,
    input: EvalForensicsBundleInput,
) -> Result<ForensicsBundleManifest> {
    tokio::fs::create_dir_all(root).await?;

    let failed_artifact_paths = input
        .report
        .results
        .iter()
        .filter_map(|result| result.artifact_path.clone())
        .collect::<Vec<_>>();

    let mut files = Vec::new();
    files.push(write_json_file(root, "report.json", &input.report).await?);
    files.push(write_json_file(root, "failed-artifacts.json", &failed_artifact_paths).await?);
    let masked_config = masked_config_snapshot(config)?;
    files.push(write_json_file(root, "config.json", &masked_config).await?);

    let manifest = ForensicsBundleManifest {
        contract_version: FORENSICS_CONTRACT_VERSION,
        kind: ForensicsBundleKind::Eval,
        trigger: input.trigger,
        identifier: eval_bundle_identifier(&input.report),
        created_at: chrono::Utc::now().to_rfc3339(),
        root_path: root.display().to_string(),
        files,
    };
    write_manifest(root, &manifest).await
}

async fn write_manifest(
    root: &Path,
    manifest: &ForensicsBundleManifest,
) -> Result<ForensicsBundleManifest> {
    write_json_file(root, "manifest.json", manifest).await?;
    Ok(manifest.clone())
}

async fn load_manifest(path: &Path) -> Result<ForensicsBundleManifest> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read forensics manifest {}", path.display()))?;
    Ok(serde_json::from_str(&content)?)
}

async fn write_json_file<T>(root: &Path, file_name: &str, value: &T) -> Result<ForensicsBundleFile>
where
    T: Serialize,
{
    let path = root.join(file_name);
    let content = serde_json::to_string_pretty(value)?;
    tokio::fs::write(&path, content)
        .await
        .with_context(|| format!("failed to write forensics artifact {}", path.display()))?;
    Ok(ForensicsBundleFile {
        name: file_name.to_string(),
        path: path.display().to_string(),
    })
}

fn eval_bundle_identifier(report: &crate::commands::EvalReport) -> String {
    let label = report
        .run
        .label
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(report.run.model.as_str());
    format!(
        "{}-{}",
        sanitize_path_segment(label),
        sanitize_path_segment(&report.run.started_at)
    )
}

fn sanitize_path_segment(value: &str) -> String {
    let mut sanitized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    let sanitized = sanitized
        .trim_matches('-')
        .chars()
        .take(120)
        .collect::<String>();
    if sanitized.is_empty() {
        "bundle".to_string()
    } else {
        sanitized
    }
}

fn masked_config_snapshot(config: &crate::config::Config) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(config)?;
    if let Some(obj) = value.as_object_mut() {
        mask_secret_fields(obj);
    }
    Ok(value)
}

fn mask_secret_fields(obj: &mut serde_json::Map<String, serde_json::Value>) {
    for key in [
        "api_key",
        "github_token",
        "github_client_secret",
        "github_private_key",
        "github_webhook_secret",
        "jira_api_token",
        "linear_api_key",
        "automation_webhook_secret",
        "server_api_key",
        "vault_token",
    ] {
        if obj.get(key).and_then(|value| value.as_str()).is_some() {
            obj.insert(key.to_string(), serde_json::json!("***"));
        }
    }
    if let Some(serde_json::Value::Object(providers)) = obj.get_mut("providers") {
        for provider in providers.values_mut() {
            if let serde_json::Value::Object(provider_obj) = provider {
                if provider_obj
                    .get("api_key")
                    .and_then(|value| value.as_str())
                    .is_some()
                {
                    provider_obj.insert("api_key".to_string(), serde_json::json!("***"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::EvalReport;
    use crate::config::Config;
    use crate::core::comment::{CommentStatus, FixEffort, Severity};
    use crate::server::state::{ReviewSession, ReviewStatus};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn sample_comment() -> crate::core::Comment {
        crate::core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 7,
            content: "Guard user input".to_string(),
            rule_id: Some("sec.input.validation".to_string()),
            severity: Severity::Warning,
            category: crate::core::comment::Category::Security,
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

    fn sample_review_session() -> ReviewSession {
        ReviewSession {
            id: "review-1".to_string(),
            status: ReviewStatus::Failed,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-1".to_string()),
            github_post_results_requested: Some(false),
            started_at: 1,
            completed_at: Some(2),
            comments: vec![sample_comment()],
            summary: None,
            files_reviewed: 0,
            error: Some("boom".to_string()),
            pr_summary_text: None,
            diff_content: Some("diff --git a/src/lib.rs b/src/lib.rs".to_string()),
            event: None,
            progress: None,
        }
    }

    #[tokio::test]
    async fn writes_review_bundle_and_masks_config_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            api_key: Some("top-secret".to_string()),
            providers: HashMap::from([(
                "openrouter".to_string(),
                crate::config::ProviderConfig {
                    api_key: Some("provider-secret".to_string()),
                    base_url: Some("https://openrouter.ai/api/v1".to_string()),
                    enabled: true,
                },
            )]),
            ..Config::default()
        };
        let manifest = write_review_forensics_bundle_at(
            &config,
            dir.path(),
            ReviewForensicsBundleInput {
                review_id: "review-1".to_string(),
                trigger: "review_failed".to_string(),
                session: sample_review_session(),
                runtime: ReviewForensicsRuntime {
                    diff_source: "pr:owner/repo#42".to_string(),
                    model: "claude-opus-4-6".to_string(),
                    warnings: vec!["request failed".to_string()],
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();

        assert_eq!(manifest.kind, ForensicsBundleKind::Review);
        let config_path = dir.path().join("config.json");
        let config_snapshot = tokio::fs::read_to_string(config_path).await.unwrap();
        assert!(config_snapshot.contains("\"api_key\": \"***\""));
        assert!(config_snapshot.contains("\"openrouter\""));
        assert!(!config_snapshot.contains("provider-secret"));
        assert!(!config_snapshot.contains("top-secret"));
    }

    #[tokio::test]
    async fn writes_eval_bundle_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let report: EvalReport = serde_json::from_value(serde_json::json!({
            "run": {
                "started_at": "2026-03-15T12:00:00Z",
                "fixtures_root": "eval/fixtures",
                "fixtures_discovered": 1,
                "fixtures_selected": 1,
                "label": "production-replay",
                "model": "claude-opus-4-6",
                "review_mode": "single-pass"
            },
            "fixtures_total": 1,
            "fixtures_passed": 0,
            "fixtures_failed": 1,
            "warnings": ["fixture degraded"],
            "rule_metrics": [],
            "suite_results": [],
            "benchmark_by_category": {},
            "benchmark_by_language": {},
            "benchmark_by_difficulty": {},
            "suite_comparisons": [],
            "category_comparisons": [],
            "language_comparisons": [],
            "threshold_failures": [],
            "results": []
        }))
        .unwrap();

        let manifest = write_eval_forensics_bundle_at(
            &Config::default(),
            dir.path(),
            EvalForensicsBundleInput {
                trigger: "eval_degraded".to_string(),
                report,
                artifact_dir: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(manifest.kind, ForensicsBundleKind::Eval);
        assert!(dir.path().join("manifest.json").exists());
        assert!(dir.path().join("report.json").exists());
    }
}
