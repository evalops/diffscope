use std::path::Path;

use super::super::types::FileMetric;
use super::responses::ProcessedJobResult;

#[derive(Default)]
pub(super) struct UsageTotals {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl UsageTotals {
    pub(super) fn add(&mut self, result: &ProcessedJobResult) {
        self.prompt_tokens += result.prompt_tokens;
        self.completion_tokens += result.completion_tokens;
        self.total_tokens += result.total_tokens;
    }
}

pub(super) fn record_file_metric(file_metrics: &mut Vec<FileMetric>, result: &ProcessedJobResult) {
    merge_file_metric(
        file_metrics,
        &result.file_path,
        result.latency_ms,
        result.prompt_tokens,
        result.completion_tokens,
        result.total_tokens,
        result.comment_count,
    );
}

fn merge_file_metric(
    file_metrics: &mut Vec<FileMetric>,
    file_path: &Path,
    latency_ms: u64,
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
    comment_count: usize,
) {
    if let Some(existing) = file_metrics
        .iter_mut()
        .find(|metric| metric.file_path == file_path)
    {
        existing.prompt_tokens += prompt_tokens;
        existing.completion_tokens += completion_tokens;
        existing.total_tokens += total_tokens;
        existing.comment_count += comment_count;
        if latency_ms > existing.latency_ms {
            existing.latency_ms = latency_ms;
        }
        return;
    }

    file_metrics.push(FileMetric {
        file_path: file_path.to_path_buf(),
        latency_ms,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        comment_count,
    });
}
