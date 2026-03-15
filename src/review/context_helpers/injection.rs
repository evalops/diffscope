use anyhow::Result;
use std::collections::HashSet;

use crate::config;
use crate::core;

use super::pattern_repositories::PatternRepositoryMap;

pub async fn inject_custom_context(
    config: &config::Config,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    for entry in config.matching_custom_context(&diff.file_path) {
        if !entry.notes.is_empty() {
            context_chunks.push(
                core::LLMContextChunk::documentation(
                    diff.file_path.clone(),
                    format!("Custom context notes:\n{}", entry.notes.join("\n")),
                )
                .with_provenance(core::ContextProvenance::CustomContextNotes),
            );
        }

        if !entry.files.is_empty() {
            let mut extra_chunks = context_fetcher
                .fetch_additional_context(&entry.files)
                .await?;
            for chunk in &mut extra_chunks {
                chunk.provenance = Some(core::ContextProvenance::CustomContextNotes);
            }
            context_chunks.extend(extra_chunks);
        }
    }

    Ok(())
}

pub fn inject_linked_issue_context(
    config: &config::Config,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    for issue in &config.linked_issue_contexts {
        let mut lines = vec![format!(
            "Linked {} issue context: {}",
            match issue.provider {
                config::LinkedIssueProvider::Jira => "Jira",
                config::LinkedIssueProvider::Linear => "Linear",
            },
            issue.identifier
        )];

        if let Some(title) = issue
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Title: {title}"));
        }
        if let Some(status) = issue
            .status
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Status: {status}"));
        }
        if let Some(url) = issue
            .url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("URL: {url}"));
        }
        if !issue.summary.trim().is_empty() {
            lines.push("Acceptance criteria / ticket context:".to_string());
            lines.push(issue.summary.trim().to_string());
        }

        context_chunks.push(
            core::LLMContextChunk::documentation(diff.file_path.clone(), lines.join("\n"))
                .with_provenance(match issue.provider {
                    config::LinkedIssueProvider::Jira => {
                        core::ContextProvenance::jira_issue_context(issue.identifier.clone())
                    }
                    config::LinkedIssueProvider::Linear => {
                        core::ContextProvenance::linear_issue_context(issue.identifier.clone())
                    }
                }),
        );
    }
}

fn document_source_label(source: &str) -> &str {
    match source {
        "design-doc" => "design doc",
        "rfc" => "RFC",
        "runbook" => "runbook",
        _ => "document",
    }
}

pub fn inject_document_context(
    config: &config::Config,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) {
    for document in &config.document_contexts {
        let mut lines = vec![format!(
            "Linked {} context: {}",
            document_source_label(&document.source),
            document.title
        )];

        if !document.url.trim().is_empty() {
            lines.push(format!("URL: {}", document.url.trim()));
        }
        if !document.summary.trim().is_empty() {
            lines.push("Relevant design / runbook context:".to_string());
            lines.push(document.summary.trim().to_string());
        }

        context_chunks.push(
            core::LLMContextChunk::documentation(diff.file_path.clone(), lines.join("\n"))
                .with_provenance(core::ContextProvenance::document_context(
                    document.source.clone(),
                    document.title.clone(),
                )),
        );
    }
}

pub async fn inject_pattern_repository_context(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    context_fetcher: &core::ContextFetcher,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let mut sources_seen = HashSet::new();
    for repo in config.matching_pattern_repositories(&diff.file_path) {
        if !sources_seen.insert(repo.source.clone()) {
            continue;
        }

        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let mut chunks = context_fetcher
            .fetch_additional_context_from_base(
                base_path,
                &repo.include_patterns,
                repo.max_files,
                repo.max_lines,
            )
            .await?;

        if chunks.is_empty() {
            continue;
        }

        context_chunks.push(
            core::LLMContextChunk::documentation(
                diff.file_path.clone(),
                format!("Pattern repository context source: {}", repo.source),
            )
            .with_provenance(core::ContextProvenance::pattern_repository_source(
                repo.source.clone(),
            )),
        );

        for chunk in &mut chunks {
            chunk.content = format!("[Pattern repository: {}]\n{}", repo.source, chunk.content);
            chunk.provenance = Some(core::ContextProvenance::pattern_repository_context(
                repo.source.clone(),
            ));
        }
        context_chunks.extend(chunks);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn diff_for(path: &str) -> core::UnifiedDiff {
        core::UnifiedDiff {
            file_path: PathBuf::from(path),
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    #[tokio::test]
    async fn inject_custom_context_tags_repo_files_with_custom_context_provenance() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(
            dir.path().join("docs/review-notes.md"),
            "Prefer contract-safe status names.",
        )
        .unwrap();

        let mut config = config::Config::default();
        config.custom_context = vec![config::CustomContextConfig {
            scope: Some("src/**/*.rs".to_string()),
            notes: Vec::new(),
            files: vec!["docs/review-notes.md".to_string()],
        }];

        let fetcher = core::ContextFetcher::new(dir.path().to_path_buf());
        let mut context_chunks = Vec::new();

        inject_custom_context(
            &config,
            &fetcher,
            &diff_for("src/lib.rs"),
            &mut context_chunks,
        )
        .await
        .unwrap();

        assert_eq!(context_chunks.len(), 1);
        assert_eq!(
            context_chunks[0].provenance,
            Some(core::ContextProvenance::CustomContextNotes)
        );
        assert_eq!(
            context_chunks[0].file_path,
            PathBuf::from("docs/review-notes.md")
        );
    }

    #[test]
    fn inject_linked_issue_context_uses_provider_specific_provenance() {
        let mut config = config::Config::default();
        config.linked_issue_contexts = vec![
            config::LinkedIssueContext {
                provider: config::LinkedIssueProvider::Jira,
                identifier: "ENG-123".to_string(),
                title: Some("Keep API status enum aligned".to_string()),
                status: Some("In Progress".to_string()),
                url: Some("https://example.atlassian.net/browse/ENG-123".to_string()),
                summary: "The API contract must remain backwards compatible.".to_string(),
            },
            config::LinkedIssueContext {
                provider: config::LinkedIssueProvider::Linear,
                identifier: "OPS-9".to_string(),
                title: Some("Propagate webhook secret rename".to_string()),
                status: Some("Todo".to_string()),
                url: Some(
                    "https://linear.app/evalops/issue/OPS-9/rename-webhook-secret".to_string(),
                ),
                summary: "Deployment manifests should use the new secret name.".to_string(),
            },
        ];

        let mut context_chunks = Vec::new();
        inject_linked_issue_context(&config, &diff_for("src/lib.rs"), &mut context_chunks);

        assert_eq!(context_chunks.len(), 2);
        assert_eq!(
            context_chunks[0].provenance,
            Some(core::ContextProvenance::jira_issue_context("ENG-123"))
        );
        assert!(context_chunks[0]
            .content
            .contains("Keep API status enum aligned"));
        assert_eq!(
            context_chunks[1].provenance,
            Some(core::ContextProvenance::linear_issue_context("OPS-9"))
        );
        assert!(context_chunks[0]
            .content
            .contains("Acceptance criteria / ticket context:"));
        assert!(context_chunks[1]
            .content
            .contains("Deployment manifests should use the new secret name."));
    }

    #[test]
    fn inject_document_context_uses_document_provenance() {
        let mut config = config::Config::default();
        config.document_contexts = vec![config::DocumentContext {
            source: "design-doc".to_string(),
            title: "Checkout resiliency RFC".to_string(),
            url: "https://github.com/evalops/diffscope/blob/main/docs/rfcs/checkout.md".to_string(),
            summary: "Keep retries idempotent and preserve queue ordering.".to_string(),
        }];

        let mut context_chunks = Vec::new();
        inject_document_context(&config, &diff_for("src/lib.rs"), &mut context_chunks);

        assert_eq!(context_chunks.len(), 1);
        assert!(context_chunks[0]
            .content
            .contains("Linked design doc context: Checkout resiliency RFC"));
        assert_eq!(
            context_chunks[0].provenance,
            Some(core::ContextProvenance::document_context(
                "design-doc",
                "Checkout resiliency RFC"
            ))
        );
    }

    #[tokio::test]
    async fn inject_pattern_repository_context_tags_source_and_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path();
        let pattern_repo = repo_root.join("patterns/security-rules");
        fs::create_dir_all(pattern_repo.join("rules")).unwrap();
        fs::write(
            pattern_repo.join("rules/sql.md"),
            "Always parameterize SQL queries.",
        )
        .unwrap();

        let mut config = config::Config::default();
        config.pattern_repositories = vec![config::PatternRepositoryConfig {
            source: "patterns/security-rules".to_string(),
            include_patterns: vec!["rules/*.md".to_string()],
            max_files: 5,
            max_lines: 20,
            ..Default::default()
        }];

        let fetcher = core::ContextFetcher::new(repo_root.to_path_buf());
        let mut resolved = HashMap::new();
        resolved.insert(
            "patterns/security-rules".to_string(),
            pattern_repo.canonicalize().unwrap(),
        );
        let mut context_chunks = Vec::new();

        inject_pattern_repository_context(
            &config,
            &resolved,
            &fetcher,
            &diff_for("src/lib.rs"),
            &mut context_chunks,
        )
        .await
        .unwrap();

        assert_eq!(context_chunks.len(), 2);
        assert_eq!(
            context_chunks[0].provenance,
            Some(core::ContextProvenance::pattern_repository_source(
                "patterns/security-rules"
            ))
        );
        assert_eq!(
            context_chunks[1].provenance,
            Some(core::ContextProvenance::pattern_repository_context(
                "patterns/security-rules"
            ))
        );
        assert!(context_chunks[1]
            .content
            .starts_with("[Pattern repository: patterns/security-rules]"));
    }
}
