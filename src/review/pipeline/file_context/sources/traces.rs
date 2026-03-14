use std::path::Path;

use crate::core::dag::{DagExecutionRecord, DagExecutionTrace};

pub(in crate::review::pipeline::file_context) const MAX_GRAPH_TRACE_DETAILS: usize = 6;

pub(in crate::review::pipeline::file_context) fn build_graph_query_trace(
    file_path: &Path,
    records: Vec<DagExecutionRecord>,
) -> Option<DagExecutionTrace> {
    if records.is_empty() {
        return None;
    }

    Some(DagExecutionTrace {
        graph_name: format!("graph_query:{}", file_path.display()),
        records,
    })
}

pub(in crate::review::pipeline::file_context) fn trace_record(
    name: impl Into<String>,
    duration_ms: u64,
) -> DagExecutionRecord {
    DagExecutionRecord {
        name: name.into(),
        enabled: true,
        duration_ms,
    }
}
