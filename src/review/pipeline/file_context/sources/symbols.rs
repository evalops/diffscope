use std::time::Instant;

use anyhow::Result;

use crate::core;

use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;
use super::traces::{trace_record, MAX_GRAPH_TRACE_DETAILS};

pub(in super::super) async fn add_symbol_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    symbols: &[String],
    context_chunks: &mut Vec<core::LLMContextChunk>,
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
) -> Result<()> {
    if symbols.is_empty() {
        return Ok(());
    }

    let definition_chunks = services
        .context_fetcher
        .fetch_related_definitions(&diff.file_path, symbols)
        .await?;
    context_chunks.extend(definition_chunks);

    if let Some(index) = session.symbol_index.as_ref() {
        let lookup_started = Instant::now();
        let index_chunks = services
            .context_fetcher
            .fetch_related_definitions_with_index(
                &diff.file_path,
                symbols,
                index,
                services.config.symbol_index_max_locations,
                services.config.symbol_index_graph_hops,
                services.config.symbol_index_graph_max_files,
            )
            .await?;
        append_symbol_graph_trace_records(
            graph_query_records,
            &index_chunks,
            lookup_started.elapsed().as_millis() as u64,
        );
        context_chunks.extend(index_chunks);
    }

    Ok(())
}

fn append_symbol_graph_trace_records(
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
    index_chunks: &[core::LLMContextChunk],
    duration_ms: u64,
) {
    let graph_hits = index_chunks
        .iter()
        .filter_map(|chunk| match chunk.provenance.as_ref() {
            Some(core::ContextProvenance::SymbolGraphPath { .. })
            | Some(core::ContextProvenance::DependencyGraphNeighborhood) => Some(format!(
                "{} | {}",
                chunk.file_path.display(),
                chunk.provenance_label().unwrap_or_default()
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    graph_query_records.push(trace_record(
        format!("symbol_graph_hits={}", graph_hits.len()),
        duration_ms,
    ));
    for (index, hit) in graph_hits.iter().take(MAX_GRAPH_TRACE_DETAILS).enumerate() {
        graph_query_records.push(trace_record(format!("symbol_graph_hit[{index}]={hit}"), 0));
    }
}

#[cfg(test)]
mod tests {
    use super::append_symbol_graph_trace_records;
    use crate::core;
    use std::path::PathBuf;

    #[test]
    fn symbol_graph_trace_records_capture_graph_provenance_only() {
        let chunks = vec![
            core::LLMContextChunk::definition(PathBuf::from("src/auth.rs"), "auth".to_string())
                .with_provenance(core::ContextProvenance::symbol_graph_path(
                    vec!["calls".to_string()],
                    1,
                    0.5,
                )),
            core::LLMContextChunk::reference(PathBuf::from("src/dep.rs"), "dep".to_string())
                .with_provenance(core::ContextProvenance::DependencyGraphNeighborhood),
            core::LLMContextChunk::definition(PathBuf::from("src/plain.rs"), "plain".to_string()),
        ];

        let mut records = Vec::new();
        append_symbol_graph_trace_records(&mut records, &chunks, 12);

        assert_eq!(records[0].name, "symbol_graph_hits=2");
        assert_eq!(records[0].duration_ms, 12);
        assert!(records[1].name.contains("src/auth.rs"));
        assert!(records[2].name.contains("dependency graph neighborhood"));
        assert_eq!(records.len(), 3);
    }
}
