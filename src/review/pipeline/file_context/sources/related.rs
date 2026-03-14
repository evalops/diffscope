use std::time::Instant;

use crate::core;

use super::super::super::context::gather_related_file_context;
use super::super::super::services::PipelineServices;
use super::super::super::session::ReviewSession;
use super::traces::{trace_record, MAX_GRAPH_TRACE_DETAILS};

pub(in super::super) fn add_related_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    context_chunks: &mut Vec<core::LLMContextChunk>,
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
) {
    if let Some(index) = session.symbol_index.as_ref() {
        let lookup_started = Instant::now();
        let caller_chunks =
            gather_related_file_context(index, &diff.file_path, &services.repo_path);
        append_related_file_trace_records(
            graph_query_records,
            &caller_chunks,
            lookup_started.elapsed().as_millis() as u64,
        );
        context_chunks.extend(caller_chunks);
    }
}

fn append_related_file_trace_records(
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
    caller_chunks: &[core::LLMContextChunk],
    duration_ms: u64,
) {
    let reverse_dependencies = caller_chunks
        .iter()
        .filter(|chunk| {
            matches!(
                chunk.provenance.as_ref(),
                Some(core::ContextProvenance::ReverseDependencySummary)
            )
        })
        .map(|chunk| chunk.file_path.display().to_string())
        .collect::<Vec<_>>();

    graph_query_records.push(trace_record(
        format!("reverse_dependency_hits={}", reverse_dependencies.len()),
        duration_ms,
    ));
    for (index, file_path) in reverse_dependencies
        .iter()
        .take(MAX_GRAPH_TRACE_DETAILS)
        .enumerate()
    {
        graph_query_records.push(trace_record(
            format!("reverse_dependency_hit[{index}]={file_path}"),
            0,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::append_related_file_trace_records;
    use crate::core;
    use std::path::PathBuf;

    #[test]
    fn related_file_trace_records_only_capture_reverse_dependencies() {
        let chunks = vec![
            core::LLMContextChunk::reference(PathBuf::from("src/caller.rs"), "caller".to_string())
                .with_provenance(core::ContextProvenance::ReverseDependencySummary),
            core::LLMContextChunk::reference(PathBuf::from("tests/file.rs"), "test".to_string())
                .with_provenance(core::ContextProvenance::RelatedTestFile),
        ];

        let mut records = Vec::new();
        append_related_file_trace_records(&mut records, &chunks, 4);

        assert_eq!(records[0].name, "reverse_dependency_hits=1");
        assert_eq!(records[0].duration_ms, 4);
        assert_eq!(records[1].name, "reverse_dependency_hit[0]=src/caller.rs");
        assert_eq!(records.len(), 2);
    }
}
