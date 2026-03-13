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
            let extra_chunks = context_fetcher
                .fetch_additional_context(&entry.files)
                .await?;
            context_chunks.extend(extra_chunks);
        }
    }

    Ok(())
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
