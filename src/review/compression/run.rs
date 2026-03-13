use crate::core::diff_parser::UnifiedDiff;

use super::multicall::build_multi_call_batches;
use super::selection::{indexed_diff_tokens, select_clipped_diffs, select_compressed_diffs};
use super::summary::build_skipped_summary;
use super::types::{CompressionResult, CompressionStrategy, DiffBatch};

/// Run adaptive compression on a set of diffs.
///
/// `token_budget` is the maximum tokens available for diff content.
/// `max_calls` is the maximum number of LLM calls for multi-call splitting.
pub fn compress_diffs(
    diffs: &[UnifiedDiff],
    token_budget: usize,
    max_calls: usize,
) -> CompressionResult {
    if diffs.is_empty() {
        return CompressionResult {
            strategy: CompressionStrategy::Full,
            batches: vec![],
            skipped_indices: vec![],
            skipped_summary: String::new(),
        };
    }

    let total_tokens: usize = indexed_diff_tokens(diffs)
        .iter()
        .map(|(_, tokens)| *tokens)
        .sum();
    if total_tokens <= token_budget {
        return CompressionResult {
            strategy: CompressionStrategy::Full,
            batches: vec![DiffBatch {
                diff_indices: (0..diffs.len()).collect(),
                estimated_tokens: total_tokens,
            }],
            skipped_indices: vec![],
            skipped_summary: String::new(),
        };
    }

    let compressed = select_compressed_diffs(diffs, token_budget);
    if compressed.included_indices.is_empty() {
        let mut skipped = compressed.skipped_indices;
        skipped.sort();
        return CompressionResult {
            strategy: CompressionStrategy::Compressed,
            batches: vec![],
            skipped_indices: skipped.clone(),
            skipped_summary: build_skipped_summary(diffs, &skipped),
        };
    }

    if compressed.skipped_indices.is_empty() {
        return CompressionResult {
            strategy: CompressionStrategy::Compressed,
            batches: vec![single_batch(
                compressed.included_indices,
                compressed.estimated_tokens,
            )],
            skipped_indices: vec![],
            skipped_summary: String::new(),
        };
    }

    if max_calls <= 1 {
        let mut included = compressed.included_indices;
        let mut skipped = compressed.skipped_indices;
        included.sort();
        skipped.sort();
        return CompressionResult {
            strategy: CompressionStrategy::Compressed,
            batches: vec![single_batch(included, compressed.estimated_tokens)],
            skipped_indices: skipped.clone(),
            skipped_summary: build_skipped_summary(diffs, &skipped),
        };
    }

    let clipped = select_clipped_diffs(diffs, token_budget);
    if !clipped.included_indices.is_empty()
        && clipped.included_indices.len() > compressed.included_indices.len()
    {
        return CompressionResult {
            strategy: CompressionStrategy::Clipped,
            batches: vec![single_batch(
                clipped.included_indices,
                clipped.estimated_tokens,
            )],
            skipped_indices: clipped.skipped_indices.clone(),
            skipped_summary: build_skipped_summary(diffs, &clipped.skipped_indices),
        };
    }

    let max_calls = max_calls.max(1);
    let multi_call = build_multi_call_batches(diffs, token_budget, max_calls);
    CompressionResult {
        strategy: CompressionStrategy::MultiCall,
        batches: multi_call.batches,
        skipped_indices: multi_call.skipped_indices.clone(),
        skipped_summary: build_skipped_summary(diffs, &multi_call.skipped_indices),
    }
}

fn single_batch(mut diff_indices: Vec<usize>, estimated_tokens: usize) -> DiffBatch {
    diff_indices.sort();
    DiffBatch {
        diff_indices,
        estimated_tokens,
    }
}
