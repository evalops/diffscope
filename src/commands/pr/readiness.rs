use anyhow::Result;
use std::sync::Arc;

use crate::config;
use crate::output::OutputFormat;
use crate::server::pr_readiness::{get_pr_readiness_snapshot, PrReadinessSnapshot};
use crate::server::state::AppState;

use super::gh::{fetch_pr_metadata, resolve_pr_number};

struct PrReadinessTarget {
    repo: String,
    pr_number: u32,
    current_head_sha: Option<String>,
}

pub(super) async fn run_pr_readiness_flow(
    number: Option<u32>,
    repo: Option<&str>,
    config: config::Config,
    format: OutputFormat,
) -> Result<()> {
    let target = resolve_pr_readiness_target(number, repo)?;
    let state = Arc::new(AppState::new(config).await?);
    let snapshot = get_pr_readiness_snapshot(
        &state,
        &target.repo,
        target.pr_number,
        target.current_head_sha.as_deref(),
    )
    .await;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&snapshot)?),
        OutputFormat::Markdown | OutputFormat::Patch => {
            println!("{}", format_pr_readiness_markdown(&snapshot))
        }
    }

    Ok(())
}

fn resolve_pr_readiness_target(
    number: Option<u32>,
    repo: Option<&str>,
) -> Result<PrReadinessTarget> {
    match (number, repo) {
        (Some(pr_number), Some(repo)) => {
            let current_head_sha = fetch_pr_metadata(&pr_number.to_string(), Some(repo))
                .ok()
                .map(|metadata| metadata.head_ref_oid);
            Ok(PrReadinessTarget {
                repo: repo.to_string(),
                pr_number,
                current_head_sha,
            })
        }
        _ => {
            let pr_number = resolve_pr_number(number, repo)?;
            let metadata = fetch_pr_metadata(&pr_number, repo)?;
            Ok(PrReadinessTarget {
                repo: repo
                    .map(str::to_string)
                    .unwrap_or(metadata.base_repository.name_with_owner),
                pr_number: metadata.number,
                current_head_sha: Some(metadata.head_ref_oid),
            })
        }
    }
}

fn format_pr_readiness_markdown(snapshot: &PrReadinessSnapshot) -> String {
    let mut output = String::new();
    output.push_str("# PR Readiness\n\n");
    output.push_str(&format!(
        "- PR: `{}#{}`\n",
        snapshot.repo, snapshot.pr_number
    ));
    if let Some(current_head_sha) = snapshot.current_head_sha.as_deref() {
        output.push_str(&format!(
            "- Current head: `{}`\n",
            short_sha(current_head_sha)
        ));
    }

    match &snapshot.latest_review {
        Some(review) => {
            output.push_str(&format!(
                "- Latest DiffScope review: `{}` ({:?})\n",
                review.id, review.status
            ));
            if let Some(reviewed_head_sha) = review.reviewed_head_sha.as_deref() {
                output.push_str(&format!(
                    "- Reviewed head: `{}`\n",
                    short_sha(reviewed_head_sha)
                ));
            }
            if let Some(summary) = review.summary.as_ref() {
                output.push_str(&format!("- Merge readiness: {}\n", summary.merge_readiness));
                output.push_str(&format!("- Open blockers: {}\n", summary.open_blockers));
                output.push_str(&format!(
                    "- Lifecycle: {} open · {} resolved · {} dismissed\n",
                    summary.open_comments, summary.resolved_comments, summary.dismissed_comments
                ));
                output.push_str(&format!(
                    "- Completeness: {} acknowledged · {} fixed · {} stale\n",
                    summary.completeness.acknowledged_findings,
                    summary.completeness.fixed_findings,
                    summary.completeness.stale_findings
                ));
                output.push_str(&format!("- Verification: {}\n", summary.verification.state));
                if !summary.readiness_reasons.is_empty() {
                    output.push_str("- Readiness reasons:\n");
                    for reason in &summary.readiness_reasons {
                        output.push_str(&format!("  - {}\n", reason));
                    }
                }
            } else {
                output.push_str("- State: readiness summary is not available yet\n");
            }
        }
        None => {
            output.push_str("- Latest DiffScope review: none\n");
            output.push_str("- State: no stored PR readiness summary found\n");
        }
    }

    output
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{MergeReadiness, ReviewVerificationState};

    #[test]
    fn markdown_output_includes_summary_fields() {
        let mut summary = crate::core::CommentSynthesizer::generate_summary(&[]);
        summary.merge_readiness = MergeReadiness::NeedsAttention;
        summary.open_blockers = 2;
        summary.open_comments = 3;
        summary.resolved_comments = 1;
        summary.dismissed_comments = 1;
        summary.completeness.total_findings = 5;
        summary.completeness.acknowledged_findings = 2;
        summary.completeness.fixed_findings = 1;
        summary.completeness.stale_findings = 3;
        summary.verification.state = ReviewVerificationState::Inconclusive;
        summary.readiness_reasons = vec!["new commits landed after this review".to_string()];
        let snapshot = PrReadinessSnapshot {
            repo: "owner/repo".to_string(),
            pr_number: 42,
            diff_source: "pr:owner/repo#42".to_string(),
            current_head_sha: Some("0123456789abcdef".to_string()),
            latest_review: Some(crate::server::pr_readiness::PrReadinessReview {
                id: "review-1".to_string(),
                status: crate::server::state::ReviewStatus::Complete,
                started_at: 10,
                completed_at: Some(11),
                reviewed_head_sha: Some("fedcba9876543210".to_string()),
                summary: Some(summary),
                files_reviewed: 2,
                comment_count: 4,
                error: None,
            }),
        };

        let output = format_pr_readiness_markdown(&snapshot);
        assert!(output.contains("# PR Readiness"));
        assert!(output.contains("PR: `owner/repo#42`"));
        assert!(output.contains("Current head: `0123456789ab`"));
        assert!(output.contains("Merge readiness: Needs attention"));
        assert!(output.contains("Open blockers: 2"));
        assert!(output.contains("Completeness: 2 acknowledged · 1 fixed · 3 stale"));
        assert!(output.contains("new commits landed after this review"));
    }

    #[test]
    fn markdown_output_handles_missing_reviews() {
        let snapshot = PrReadinessSnapshot {
            repo: "owner/repo".to_string(),
            pr_number: 42,
            diff_source: "pr:owner/repo#42".to_string(),
            current_head_sha: None,
            latest_review: None,
        };

        let output = format_pr_readiness_markdown(&snapshot);
        assert!(output.contains("Latest DiffScope review: none"));
        assert!(output.contains("no stored PR readiness summary found"));
    }
}
