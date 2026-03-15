use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const PRODUCTION_REPLAY_PACK_NAME: &str = "production-replay";
const PRODUCTION_REPLAY_PACK_VERSION: &str = "1";

pub fn default_production_replay_pack_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("diffscope")
        .join("eval")
        .join("production_replay")
        .join("replay.json")
}

pub async fn record_review_feedback_fixture(
    session: &crate::server::state::ReviewSession,
) -> Result<Option<PathBuf>> {
    let pack_path = default_production_replay_pack_path();
    record_review_feedback_fixture_at(session, &pack_path).await
}

async fn record_review_feedback_fixture_at(
    session: &crate::server::state::ReviewSession,
    pack_path: &Path,
) -> Result<Option<PathBuf>> {
    let Some(fixture) = build_replay_fixture(session) else {
        return Ok(None);
    };

    if let Some(parent) = pack_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut pack = load_existing_pack(pack_path).await?;
    if let Some(existing) = pack
        .fixtures
        .iter_mut()
        .find(|candidate| candidate.name == fixture.name)
    {
        *existing = fixture;
    } else {
        pack.fixtures.push(fixture);
    }
    normalize_pack_metadata(&mut pack);
    tokio::fs::write(pack_path, serde_json::to_string_pretty(&pack)?).await?;

    Ok(Some(pack_path.to_path_buf()))
}

async fn load_existing_pack(
    path: &Path,
) -> Result<crate::core::eval_benchmarks::CommunityFixturePack> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => crate::core::eval_benchmarks::CommunityFixturePack::from_json(&content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(empty_pack()),
        Err(err) => Err(err.into()),
    }
}

fn build_replay_fixture(
    session: &crate::server::state::ReviewSession,
) -> Option<crate::core::eval_benchmarks::BenchmarkFixture> {
    let diff_content = session.diff_content.as_ref()?.trim().to_string();
    if diff_content.is_empty() {
        return None;
    }

    let expected_findings = session
        .comments
        .iter()
        .enumerate()
        .filter_map(|(index, comment)| build_expected_finding(index, comment))
        .collect::<Vec<_>>();
    let negative_findings = session
        .comments
        .iter()
        .enumerate()
        .filter_map(|(index, comment)| build_negative_finding(index, comment))
        .collect::<Vec<_>>();

    if expected_findings.is_empty() && negative_findings.is_empty() {
        return None;
    }

    Some(crate::core::eval_benchmarks::BenchmarkFixture {
        name: anonymized_fixture_name(&session.id),
        category: dominant_category(session),
        language: infer_language(session),
        difficulty: crate::core::eval_benchmarks::Difficulty::Hard,
        diff_content,
        repo_path: None,
        expected_findings,
        negative_findings,
        min_total: None,
        max_total: None,
        description: Some(
            "Anonymized production replay fixture captured from accepted and rejected review outcomes."
                .to_string(),
        ),
        source: Some("production-replay".to_string()),
    })
}

fn build_expected_finding(
    index: usize,
    comment: &crate::core::Comment,
) -> Option<crate::core::eval_benchmarks::ExpectedFinding> {
    if comment.feedback.as_deref() != Some("accept")
        || crate::review::is_vague_review_comment(comment)
    {
        return None;
    }

    let terms = replay_terms(comment);
    Some(crate::core::eval_benchmarks::ExpectedFinding {
        description: format!("accepted finding {}", index + 1),
        severity: Some(comment.severity.to_string()),
        category: Some(comment.category.to_string()),
        file_pattern: crate::review::derive_file_patterns(&comment.file_path)
            .into_iter()
            .next(),
        line_hint: Some(comment.line_number),
        contains: None,
        contains_any: terms,
        tags_any: Vec::new(),
        confidence_at_least: None,
        confidence_at_most: None,
        fix_effort: None,
        rule_id: comment.rule_id.clone(),
        rule_id_aliases: Vec::new(),
    })
}

fn build_negative_finding(
    index: usize,
    comment: &crate::core::Comment,
) -> Option<crate::core::eval_benchmarks::NegativeFinding> {
    if comment.feedback.as_deref() != Some("reject")
        || crate::review::is_vague_review_comment(comment)
    {
        return None;
    }

    let mut terms = replay_terms(comment);
    let contains = terms.first().cloned();
    if contains.is_some() {
        terms.remove(0);
    }

    Some(crate::core::eval_benchmarks::NegativeFinding {
        description: format!("rejected finding {}", index + 1),
        file_pattern: crate::review::derive_file_patterns(&comment.file_path)
            .into_iter()
            .next(),
        contains,
        contains_any: terms,
    })
}

fn replay_terms(comment: &crate::core::Comment) -> Vec<String> {
    let mut terms = Vec::new();
    if let Some(rule_id) = comment
        .rule_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        terms.push(rule_id.trim().to_string());
    }

    let mut seen = terms.iter().cloned().collect::<HashSet<_>>();
    for chunk in comment
        .content
        .split(['\n', '.', ';', ':'])
        .map(str::trim)
        .filter(|chunk| chunk.len() >= 12)
    {
        let normalized = chunk.chars().take(160).collect::<String>();
        if seen.insert(normalized.clone()) {
            terms.push(normalized);
        }
        if terms.len() >= 4 {
            break;
        }
    }

    terms
}

fn dominant_category(session: &crate::server::state::ReviewSession) -> String {
    session
        .comments
        .iter()
        .find(|comment| matches!(comment.feedback.as_deref(), Some("accept") | Some("reject")))
        .map(|comment| comment.category.as_str().to_string())
        .unwrap_or_else(|| "production-replay".to_string())
}

fn infer_language(session: &crate::server::state::ReviewSession) -> String {
    session
        .comments
        .iter()
        .find_map(|comment| {
            comment
                .file_path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(language_from_extension)
        })
        .unwrap_or("unknown")
        .to_string()
}

fn language_from_extension(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "swift" => "swift",
        "scala" => "scala",
        "sh" | "bash" => "shell",
        _ => "unknown",
    }
}

fn anonymized_fixture_name(review_id: &str) -> String {
    let digest = Sha256::digest(review_id.as_bytes());
    format!("replay-{:x}", digest)
        .chars()
        .take(20)
        .collect::<String>()
}

fn empty_pack() -> crate::core::eval_benchmarks::CommunityFixturePack {
    crate::core::eval_benchmarks::CommunityFixturePack {
        name: PRODUCTION_REPLAY_PACK_NAME.to_string(),
        author: "diffscope".to_string(),
        version: PRODUCTION_REPLAY_PACK_VERSION.to_string(),
        description:
            "Anonymized production replay fixtures captured from accepted and rejected review outcomes."
                .to_string(),
        languages: Vec::new(),
        categories: Vec::new(),
        thresholds: None,
        metadata: std::collections::HashMap::from([(
            "source".to_string(),
            "production-replay".to_string(),
        )]),
        fixtures: Vec::new(),
    }
}

fn normalize_pack_metadata(pack: &mut crate::core::eval_benchmarks::CommunityFixturePack) {
    pack.languages = pack
        .fixtures
        .iter()
        .map(|fixture| fixture.language.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    pack.languages.sort();

    pack.categories = pack
        .fixtures
        .iter()
        .map(|fixture| fixture.category.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    pack.categories.sort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};

    fn sample_comment(id: &str, feedback: &str, line_number: usize) -> crate::core::Comment {
        crate::core::Comment {
            id: id.to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_number,
            content: "Missing tenant boundary check before privileged access.".to_string(),
            rule_id: Some("sec.tenant-boundary".to_string()),
            severity: Severity::Warning,
            category: Category::Security,
            suggestion: None,
            confidence: 0.92,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Low,
            feedback: Some(feedback.to_string()),
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    fn sample_session() -> crate::server::state::ReviewSession {
        crate::server::state::ReviewSession {
            id: "review-1".to_string(),
            status: crate::server::state::ReviewStatus::Complete,
            diff_source: "pr:owner/repo#42".to_string(),
            github_head_sha: Some("sha-1".to_string()),
            github_post_results_requested: Some(false),
            started_at: 1,
            completed_at: Some(2),
            comments: vec![
                sample_comment("accepted", "accept", 12),
                sample_comment("rejected", "reject", 22),
            ],
            summary: None,
            files_reviewed: 1,
            error: None,
            pr_summary_text: None,
            diff_content: Some("diff --git a/src/lib.rs b/src/lib.rs".to_string()),
            event: None,
            progress: None,
        }
    }

    #[tokio::test]
    async fn records_anonymized_replay_pack() {
        let dir = tempfile::tempdir().unwrap();
        let pack_path = dir.path().join("replay.json");

        let written = record_review_feedback_fixture_at(&sample_session(), &pack_path)
            .await
            .unwrap();

        assert_eq!(written.as_deref(), Some(pack_path.as_path()));
        let content = tokio::fs::read_to_string(&pack_path).await.unwrap();
        assert!(content.contains("production-replay"));
        assert!(content.contains("replay-"));
        assert!(!content.contains("owner/repo"));
        assert!(!content.contains("review-1"));
    }

    #[tokio::test]
    async fn skips_sessions_without_diff_content() {
        let dir = tempfile::tempdir().unwrap();
        let pack_path = dir.path().join("replay.json");
        let mut session = sample_session();
        session.diff_content = None;

        let written = record_review_feedback_fixture_at(&session, &pack_path)
            .await
            .unwrap();

        assert!(written.is_none());
        assert!(!pack_path.exists());
    }
}
