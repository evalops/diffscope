use crate::core::diff_parser::UnifiedDiff;

use super::metrics::{estimate_diff_tokens, CHARS_PER_TOKEN};
use super::selection::indexed_diff_tokens;
use super::transform::clip_diff;
use super::types::DiffBatch;

pub(super) struct MultiCallOutcome {
    pub batches: Vec<DiffBatch>,
    pub skipped_indices: Vec<usize>,
}

pub(super) fn build_multi_call_batches(
    diffs: &[UnifiedDiff],
    token_budget: usize,
    max_calls: usize,
) -> MultiCallOutcome {
    let per_batch_budget = token_budget;
    let mut batches = Vec::new();
    let mut skipped = Vec::new();
    let mut sorted_files = indexed_diff_tokens(diffs);
    sorted_files.sort_by_key(|(_, tokens)| *tokens);

    for (idx, tokens) in sorted_files {
        if tokens > per_batch_budget {
            place_clipped_file(
                &mut batches,
                &mut skipped,
                idx,
                &diffs[idx],
                per_batch_budget,
                max_calls,
            );
            continue;
        }

        if !place_in_existing_batch(&mut batches, idx, tokens, per_batch_budget)
            && !create_batch(&mut batches, idx, tokens, max_calls)
        {
            skipped.push(idx);
        }
    }

    for batch in &mut batches {
        batch.diff_indices.sort();
    }
    skipped.sort();

    MultiCallOutcome {
        batches,
        skipped_indices: skipped,
    }
}

fn place_clipped_file(
    batches: &mut Vec<DiffBatch>,
    skipped: &mut Vec<usize>,
    idx: usize,
    diff: &UnifiedDiff,
    per_batch_budget: usize,
    max_calls: usize,
) {
    let Some(clipped) = clip_diff(diff, per_batch_budget * CHARS_PER_TOKEN) else {
        skipped.push(idx);
        return;
    };
    let clipped_tokens = estimate_diff_tokens(&clipped);
    if !place_in_existing_batch(batches, idx, clipped_tokens, per_batch_budget)
        && !create_batch(batches, idx, clipped_tokens, max_calls)
    {
        skipped.push(idx);
    }
}

fn place_in_existing_batch(
    batches: &mut [DiffBatch],
    idx: usize,
    tokens: usize,
    per_batch_budget: usize,
) -> bool {
    if let Some(batch) = batches
        .iter_mut()
        .find(|batch| batch.estimated_tokens + tokens <= per_batch_budget)
    {
        batch.diff_indices.push(idx);
        batch.estimated_tokens += tokens;
        return true;
    }
    false
}

fn create_batch(batches: &mut Vec<DiffBatch>, idx: usize, tokens: usize, max_calls: usize) -> bool {
    if batches.len() >= max_calls {
        return false;
    }

    batches.push(DiffBatch {
        diff_indices: vec![idx],
        estimated_tokens: tokens,
    });
    true
}
