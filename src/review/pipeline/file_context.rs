use anyhow::Result;

use crate::config;
use crate::core;

use super::context::{extract_symbols_from_diff, gather_related_file_context};
use super::services::PipelineServices;
use super::session::ReviewSession;

pub(super) struct PreparedFileContext {
    pub active_rules: Vec<core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub deterministic_comments: Vec<core::Comment>,
    pub context_chunks: Vec<core::LLMContextChunk>,
}

pub(super) async fn assemble_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    pre_analysis_context: Vec<core::LLMContextChunk>,
    deterministic_comments: Vec<core::Comment>,
) -> Result<PreparedFileContext> {
    let mut builder =
        FileContextBuilder::new(services, session, diff, pre_analysis_context).await?;
    builder.add_symbol_context().await?;
    builder.add_related_file_context();
    builder.add_semantic_context().await;
    builder.add_path_context().await?;
    builder.inject_repository_context().await?;
    Ok(builder.finalize(deterministic_comments))
}

struct FileContextBuilder<'a> {
    services: &'a PipelineServices,
    session: &'a ReviewSession,
    diff: &'a core::UnifiedDiff,
    context_chunks: Vec<core::LLMContextChunk>,
    path_config: Option<config::PathConfig>,
}

impl<'a> FileContextBuilder<'a> {
    async fn new(
        services: &'a PipelineServices,
        session: &'a ReviewSession,
        diff: &'a core::UnifiedDiff,
        pre_analysis_context: Vec<core::LLMContextChunk>,
    ) -> Result<Self> {
        let mut context_chunks = services
            .context_fetcher
            .fetch_context_for_file(&diff.file_path, &changed_line_ranges(diff))
            .await?;
        context_chunks.extend(pre_analysis_context);

        Ok(Self {
            services,
            session,
            diff,
            context_chunks,
            path_config: services.config.get_path_config(&diff.file_path).cloned(),
        })
    }

    async fn add_symbol_context(&mut self) -> Result<()> {
        let symbols = extract_symbols_from_diff(self.diff);
        if symbols.is_empty() {
            return Ok(());
        }

        let definition_chunks = self
            .services
            .context_fetcher
            .fetch_related_definitions(&self.diff.file_path, &symbols)
            .await?;
        self.context_chunks.extend(definition_chunks);

        if let Some(index) = self.session.symbol_index.as_ref() {
            let index_chunks = self
                .services
                .context_fetcher
                .fetch_related_definitions_with_index(
                    &self.diff.file_path,
                    &symbols,
                    index,
                    self.services.config.symbol_index_max_locations,
                    self.services.config.symbol_index_graph_hops,
                    self.services.config.symbol_index_graph_max_files,
                )
                .await?;
            self.context_chunks.extend(index_chunks);
        }

        Ok(())
    }

    fn add_related_file_context(&mut self) {
        if let Some(index) = self.session.symbol_index.as_ref() {
            let caller_chunks =
                gather_related_file_context(index, &self.diff.file_path, &self.services.repo_path);
            self.context_chunks.extend(caller_chunks);
        }
    }

    async fn add_semantic_context(&mut self) {
        let Some(index) = self.session.semantic_index.as_ref() else {
            return;
        };

        let semantic_chunks = core::semantic_context_for_diff(
            index,
            self.diff,
            self.session
                .source_files
                .get(&self.diff.file_path)
                .map(|content| content.as_str()),
            self.services.embedding_adapter.as_deref(),
            self.services.config.semantic_rag_top_k,
            self.services.config.semantic_rag_min_similarity,
        )
        .await;
        self.context_chunks.extend(semantic_chunks);
    }

    async fn add_path_context(&mut self) -> Result<()> {
        let Some(path_config) = self.path_config.as_ref() else {
            return Ok(());
        };

        if !path_config.focus.is_empty() {
            self.context_chunks.push(
                core::LLMContextChunk::documentation(
                    self.diff.file_path.clone(),
                    format!(
                        "Focus areas for this file: {}",
                        path_config.focus.join(", ")
                    ),
                )
                .with_provenance(core::ContextProvenance::PathSpecificFocusAreas),
            );
        }

        if !path_config.extra_context.is_empty() {
            let extra_chunks = self
                .services
                .context_fetcher
                .fetch_additional_context(&path_config.extra_context)
                .await?;
            self.context_chunks.extend(extra_chunks);
        }

        Ok(())
    }

    async fn inject_repository_context(&mut self) -> Result<()> {
        super::super::context_helpers::inject_custom_context(
            &self.services.config,
            &self.services.context_fetcher,
            self.diff,
            &mut self.context_chunks,
        )
        .await?;
        super::super::context_helpers::inject_pattern_repository_context(
            &self.services.config,
            &self.services.pattern_repositories,
            &self.services.context_fetcher,
            self.diff,
            &mut self.context_chunks,
        )
        .await?;

        Ok(())
    }

    fn finalize(mut self, deterministic_comments: Vec<core::Comment>) -> PreparedFileContext {
        let active_rules = core::active_rules_for_file(
            &self.services.review_rules,
            &self.diff.file_path,
            self.services.config.max_active_rules,
        );
        super::super::rule_helpers::inject_rule_context(
            self.diff,
            &active_rules,
            &mut self.context_chunks,
        );
        self.context_chunks = super::super::context_helpers::rank_and_trim_context_chunks(
            self.diff,
            self.context_chunks,
            self.services.config.context_max_chunks,
            self.services.config.context_budget_chars,
        );

        PreparedFileContext {
            active_rules,
            path_config: self.path_config,
            deterministic_comments,
            context_chunks: self.context_chunks,
        }
    }
}

fn changed_line_ranges(diff: &core::UnifiedDiff) -> Vec<(usize, usize)> {
    diff.hunks
        .iter()
        .map(|hunk| {
            (
                hunk.new_start,
                hunk.new_start + hunk.new_lines.saturating_sub(1),
            )
        })
        .collect()
}
