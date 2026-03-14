use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::config;
use crate::core;

use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;
use super::traces::{trace_record, MAX_GRAPH_TRACE_DETAILS};

pub(in super::super) async fn add_semantic_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    symbols: &[String],
    context_chunks: &mut Vec<core::LLMContextChunk>,
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
) {
    let Some(index) = session.semantic_index.as_ref() else {
        return;
    };
    let should_trace_graph_ranking = session.symbol_index.is_some() && !symbols.is_empty();
    let ranking_started = Instant::now();
    let preferred_files = graph_ranked_semantic_files(services, session, diff, symbols);
    if should_trace_graph_ranking {
        append_semantic_preference_trace_records(
            graph_query_records,
            &preferred_files,
            ranking_started.elapsed().as_millis() as u64,
        );
    }

    let semantic_chunks = core::semantic_context_for_diff(
        index,
        diff,
        session
            .source_files
            .get(&diff.file_path)
            .map(|content| content.as_str()),
        services.embedding_adapter.as_deref(),
        services.config.semantic_rag_top_k,
        services.config.semantic_rag_min_similarity,
        &preferred_files,
    )
    .await;
    append_similar_implementation_trace_records(graph_query_records, &semantic_chunks);
    context_chunks.extend(semantic_chunks);
}

fn graph_ranked_semantic_files(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    symbols: &[String],
) -> Vec<PathBuf> {
    let Some(index) = session.symbol_index.as_ref() else {
        return Vec::new();
    };

    if symbols.is_empty() {
        return Vec::new();
    }

    let retriever = core::SymbolContextRetriever::new(
        index,
        core::SymbolRetrievalPolicy::new(
            services.config.symbol_index_max_locations,
            services.config.symbol_index_graph_hops,
            services.config.symbol_index_graph_max_files,
        ),
    );
    let related_locations = retriever.related_symbol_locations(&diff.file_path, symbols);

    let mut preferred_files = Vec::new();
    let mut seen = HashSet::new();

    for location in related_locations.definition_locations {
        if location.file_path == diff.file_path || !seen.insert(location.file_path.clone()) {
            continue;
        }
        preferred_files.push(location.file_path);
    }

    let mut reference_files = related_locations
        .reference_locations
        .into_iter()
        .map(|location| location.file_path)
        .filter(|file_path| file_path != &diff.file_path)
        .filter(|file_path| seen.insert(file_path.clone()))
        .collect::<Vec<_>>();
    reference_files.sort();
    preferred_files.extend(reference_files);

    preferred_files
}

fn append_semantic_preference_trace_records(
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
    preferred_files: &[PathBuf],
    duration_ms: u64,
) {
    graph_query_records.push(trace_record(
        format!("semantic_preferred_files={}", preferred_files.len()),
        duration_ms,
    ));
    for (index, file_path) in preferred_files
        .iter()
        .take(MAX_GRAPH_TRACE_DETAILS)
        .enumerate()
    {
        graph_query_records.push(trace_record(
            format!("semantic_preferred_file[{index}]={}", file_path.display()),
            0,
        ));
    }
}

fn append_similar_implementation_trace_records(
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
    semantic_chunks: &[core::LLMContextChunk],
) {
    let similar_implementation_files = semantic_chunks
        .iter()
        .filter(|chunk| {
            matches!(
                chunk.provenance.as_ref(),
                Some(core::ContextProvenance::SimilarImplementation { .. })
            )
        })
        .map(|chunk| chunk.file_path.display().to_string())
        .collect::<Vec<_>>();

    graph_query_records.push(trace_record(
        format!(
            "similar_implementation_matches={}",
            similar_implementation_files.len()
        ),
        0,
    ));
    for (index, file_path) in similar_implementation_files
        .iter()
        .take(MAX_GRAPH_TRACE_DETAILS)
        .enumerate()
    {
        graph_query_records.push(trace_record(
            format!("similar_implementation_file[{index}]={file_path}"),
            0,
        ));
    }
}

pub(in super::super) async fn add_path_context(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    path_config: Option<&config::PathConfig>,
    context_chunks: &mut Vec<core::LLMContextChunk>,
) -> Result<()> {
    let Some(path_config) = path_config else {
        return Ok(());
    };

    if !path_config.focus.is_empty() {
        context_chunks.push(
            core::LLMContextChunk::documentation(
                diff.file_path.clone(),
                format!(
                    "Focus areas for this file: {}",
                    path_config.focus.join(", ")
                ),
            )
            .with_provenance(core::ContextProvenance::PathSpecificFocusAreas),
        );
    }

    if !path_config.extra_context.is_empty() {
        let extra_chunks = services
            .context_fetcher
            .fetch_additional_context(&path_config.extra_context)
            .await?;
        context_chunks.extend(extra_chunks);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        append_semantic_preference_trace_records, append_similar_implementation_trace_records,
    };
    use crate::core;
    use std::path::PathBuf;

    #[test]
    fn semantic_preference_trace_records_keep_ranked_order() {
        let preferred_files = vec![PathBuf::from("src/auth.rs"), PathBuf::from("src/db.rs")];

        let mut records = Vec::new();
        append_semantic_preference_trace_records(&mut records, &preferred_files, 7);

        assert_eq!(records[0].name, "semantic_preferred_files=2");
        assert_eq!(records[0].duration_ms, 7);
        assert_eq!(records[1].name, "semantic_preferred_file[0]=src/auth.rs");
        assert_eq!(records[2].name, "semantic_preferred_file[1]=src/db.rs");
    }

    #[test]
    fn similar_implementation_trace_records_capture_explicit_matches() {
        let chunks = vec![
            core::LLMContextChunk::reference(
                PathBuf::from("src/auth_guard.rs"),
                "Similar implementation".to_string(),
            )
            .with_provenance(core::ContextProvenance::similar_implementation(
                0.92,
                "validate_admin",
            )),
            core::LLMContextChunk::reference(
                PathBuf::from("src/other.rs"),
                "Semantic match".to_string(),
            )
            .with_provenance(core::ContextProvenance::semantic_retrieval(
                0.81,
                "sanitize_name",
            )),
        ];

        let mut records = Vec::new();
        append_similar_implementation_trace_records(&mut records, &chunks);

        assert_eq!(records[0].name, "similar_implementation_matches=1");
        assert_eq!(
            records[1].name,
            "similar_implementation_file[0]=src/auth_guard.rs"
        );
    }
}
