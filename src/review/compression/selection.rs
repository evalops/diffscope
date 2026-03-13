use crate::core::diff_parser::UnifiedDiff;

use super::metrics::{estimate_diff_tokens, CHARS_PER_TOKEN};
use super::transform::{clip_diff, compress_diff};

pub(super) struct SelectionOutcome {
    pub included_indices: Vec<usize>,
    pub skipped_indices: Vec<usize>,
    pub estimated_tokens: usize,
}

pub(super) fn select_compressed_diffs(
    diffs: &[UnifiedDiff],
    token_budget: usize,
) -> SelectionOutcome {
    let mut file_tokens = indexed_diff_tokens(diffs);
    file_tokens.sort_by(|left, right| left.1.cmp(&right.1));

    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut budget_used = 0usize;

    for (idx, _) in file_tokens {
        let Some(compressed) = compress_diff(&diffs[idx]) else {
            skipped.push(idx);
            continue;
        };
        let compressed_tokens = estimate_diff_tokens(&compressed);
        if budget_used + compressed_tokens <= token_budget {
            included.push(idx);
            budget_used += compressed_tokens;
        } else {
            skipped.push(idx);
        }
    }

    SelectionOutcome {
        included_indices: included,
        skipped_indices: skipped,
        estimated_tokens: budget_used,
    }
}

pub(super) fn select_clipped_diffs(diffs: &[UnifiedDiff], token_budget: usize) -> SelectionOutcome {
    let per_file_char_budget = (token_budget * CHARS_PER_TOKEN) / diffs.len().max(1);
    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut budget_used = 0usize;

    for (idx, diff) in diffs.iter().enumerate() {
        let Some(clipped) = clip_diff(diff, per_file_char_budget) else {
            skipped.push(idx);
            continue;
        };
        let clipped_tokens = estimate_diff_tokens(&clipped);
        if budget_used + clipped_tokens <= token_budget {
            included.push(idx);
            budget_used += clipped_tokens;
        } else {
            skipped.push(idx);
        }
    }

    SelectionOutcome {
        included_indices: included,
        skipped_indices: skipped,
        estimated_tokens: budget_used,
    }
}

pub(super) fn indexed_diff_tokens(diffs: &[UnifiedDiff]) -> Vec<(usize, usize)> {
    diffs
        .iter()
        .enumerate()
        .map(|(idx, diff)| (idx, estimate_diff_tokens(diff)))
        .collect()
}
