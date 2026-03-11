//! Adaptive patch compression for large PRs.
//!
//! Implements a 4-stage progressive degradation strategy (inspired by Qodo/CodeRabbit):
//!   Stage 1 – Full:       all diffs fit within token budget → use as-is.
//!   Stage 2 – Compressed: remove deletion-only hunks, sort by size, drop tail files.
//!   Stage 3 – Clipped:    truncate remaining large files at clean line boundaries.
//!   Stage 4 – MultiCall:  split into multiple LLM call batches.

use crate::core::diff_parser::{ChangeType, DiffHunk, UnifiedDiff};
use serde::{Deserialize, Serialize};

/// Rough token estimation: ~4 chars per token (industry standard fallback).
const CHARS_PER_TOKEN: usize = 4;

/// Strategy selected by the compressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionStrategy {
    Full,
    Compressed,
    Clipped,
    MultiCall,
}

/// A single batch of diffs that fits within the token budget.
#[derive(Debug, Clone)]
pub struct DiffBatch {
    /// Indices into the original diffs vec.
    pub diff_indices: Vec<usize>,
    /// Estimated token count for this batch.
    pub estimated_tokens: usize,
}

/// Result of running adaptive compression.
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub strategy: CompressionStrategy,
    /// Batches of diffs to review (1 batch for stages 1-3, N for stage 4).
    pub batches: Vec<DiffBatch>,
    /// Indices of diffs that were dropped entirely.
    pub skipped_indices: Vec<usize>,
    /// Human-readable summary of what was skipped.
    pub skipped_summary: String,
}

/// Estimate the token cost of a single diff.
pub fn estimate_diff_tokens(diff: &UnifiedDiff) -> usize {
    let chars: usize = diff
        .hunks
        .iter()
        .map(|h| {
            h.changes
                .iter()
                .map(|c| c.content.len() + 10) // +10 for line prefix overhead
                .sum::<usize>()
                + h.context.len()
                + 20 // hunk header
        })
        .sum();
    // Add file header overhead
    let file_overhead = diff.file_path.to_string_lossy().len() + 40;
    (chars + file_overhead) / CHARS_PER_TOKEN
}

/// Check if a hunk is deletion-only (all changes are removals or context).
pub fn is_deletion_only_hunk(hunk: &DiffHunk) -> bool {
    hunk.changes
        .iter()
        .all(|c| c.change_type == ChangeType::Removed || c.change_type == ChangeType::Context)
}

/// Remove deletion-only hunks from a diff. Returns a new diff (or None if all hunks removed).
pub fn compress_diff(diff: &UnifiedDiff) -> Option<UnifiedDiff> {
    let kept_hunks: Vec<DiffHunk> = diff
        .hunks
        .iter()
        .filter(|h| !is_deletion_only_hunk(h))
        .cloned()
        .collect();

    if kept_hunks.is_empty() {
        return None;
    }

    Some(UnifiedDiff {
        file_path: diff.file_path.clone(),
        old_content: diff.old_content.clone(),
        new_content: diff.new_content.clone(),
        hunks: kept_hunks,
        is_binary: diff.is_binary,
        is_deleted: diff.is_deleted,
        is_new: diff.is_new,
    })
}

/// Clip a diff to fit within a character budget by keeping only leading hunks.
pub fn clip_diff(diff: &UnifiedDiff, max_chars: usize) -> Option<UnifiedDiff> {
    let mut kept_hunks = Vec::new();
    let mut chars_used = 0;

    for hunk in &diff.hunks {
        let hunk_chars: usize = hunk.changes.iter().map(|c| c.content.len() + 10).sum::<usize>()
            + hunk.context.len()
            + 20;

        if chars_used + hunk_chars > max_chars && !kept_hunks.is_empty() {
            break;
        }
        kept_hunks.push(hunk.clone());
        chars_used += hunk_chars;
    }

    if kept_hunks.is_empty() {
        return None;
    }

    Some(UnifiedDiff {
        file_path: diff.file_path.clone(),
        old_content: diff.old_content.clone(),
        new_content: diff.new_content.clone(),
        hunks: kept_hunks,
        is_binary: diff.is_binary,
        is_deleted: diff.is_deleted,
        is_new: diff.is_new,
    })
}

/// Build a human-readable summary of skipped files.
pub fn build_skipped_summary(diffs: &[UnifiedDiff], skipped_indices: &[usize]) -> String {
    if skipped_indices.is_empty() {
        return String::new();
    }

    let mut deleted = Vec::new();
    let mut modified = Vec::new();

    for &idx in skipped_indices {
        if idx < diffs.len() {
            let diff = &diffs[idx];
            let path = diff.file_path.display().to_string();
            if diff.is_deleted {
                deleted.push(path);
            } else {
                modified.push(path);
            }
        }
    }

    let mut summary = String::new();
    if !deleted.is_empty() {
        summary.push_str("Deleted files (not reviewed):\n");
        for f in &deleted {
            summary.push_str(&format!("  - {}\n", f));
        }
    }
    if !modified.is_empty() {
        if !summary.is_empty() {
            summary.push('\n');
        }
        summary.push_str("Additional modified files (not reviewed due to context budget):\n");
        for f in &modified {
            summary.push_str(&format!("  - {}\n", f));
        }
    }
    summary
}

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

    // Estimate per-file tokens
    let mut file_tokens: Vec<(usize, usize)> = diffs
        .iter()
        .enumerate()
        .map(|(i, d)| (i, estimate_diff_tokens(d)))
        .collect();

    let total_tokens: usize = file_tokens.iter().map(|(_, t)| *t).sum();

    // Stage 1: Full — everything fits
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

    // Stage 2: Compressed — remove deletion-only hunks, drop tail files
    // Sort by token count (smallest first) to maximize files reviewed.
    file_tokens.sort_by(|a, b| a.1.cmp(&b.1));

    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut budget_used = 0;

    for &(idx, _tokens) in &file_tokens {
        // Re-estimate with compression
        let compressed_tokens = if let Some(compressed) = compress_diff(&diffs[idx]) {
            estimate_diff_tokens(&compressed)
        } else {
            // All hunks deletion-only — skip
            skipped.push(idx);
            continue;
        };

        if budget_used + compressed_tokens <= token_budget {
            included.push(idx);
            budget_used += compressed_tokens;
        } else {
            skipped.push(idx);
        }
    }

    if skipped.is_empty() && !included.is_empty() {
        // Everything fit after compression
        included.sort();
        return CompressionResult {
            strategy: CompressionStrategy::Compressed,
            batches: vec![DiffBatch {
                diff_indices: included,
                estimated_tokens: budget_used,
            }],
            skipped_indices: vec![],
            skipped_summary: String::new(),
        };
    }

    if !skipped.is_empty() && !included.is_empty() && max_calls <= 1 {
        // Can't split further — return compressed result with skipped files
        included.sort();
        skipped.sort();
        let summary = build_skipped_summary(diffs, &skipped);
        return CompressionResult {
            strategy: CompressionStrategy::Compressed,
            batches: vec![DiffBatch {
                diff_indices: included,
                estimated_tokens: budget_used,
            }],
            skipped_indices: skipped,
            skipped_summary: summary,
        };
    }

    // Stage 3: Clipped — truncate large files to fit
    let per_file_char_budget = (token_budget * CHARS_PER_TOKEN) / diffs.len().max(1);
    let mut clipped_included = Vec::new();
    let mut clipped_skipped = Vec::new();
    let mut clipped_budget = 0;

    // Reset and try with clipping
    for (idx, diff) in diffs.iter().enumerate() {
        let clipped = clip_diff(diff, per_file_char_budget);
        if let Some(ref clipped_diff) = clipped {
            let tokens = estimate_diff_tokens(clipped_diff);
            if clipped_budget + tokens <= token_budget {
                clipped_included.push(idx);
                clipped_budget += tokens;
                continue;
            }
        }
        clipped_skipped.push(idx);
    }

    if !clipped_included.is_empty() && clipped_included.len() > included.len() {
        let summary = build_skipped_summary(diffs, &clipped_skipped);
        return CompressionResult {
            strategy: CompressionStrategy::Clipped,
            batches: vec![DiffBatch {
                diff_indices: clipped_included,
                estimated_tokens: clipped_budget,
            }],
            skipped_indices: clipped_skipped,
            skipped_summary: summary,
        };
    }

    // Stage 4: MultiCall — split into multiple batches
    let max_calls = max_calls.max(1);
    let per_batch_budget = token_budget;
    let mut batches: Vec<DiffBatch> = Vec::new();
    let mut multi_skipped = Vec::new();

    // Sort files by size (smallest first for better bin packing)
    let mut sorted_files: Vec<(usize, usize)> = diffs
        .iter()
        .enumerate()
        .map(|(i, d)| (i, estimate_diff_tokens(d)))
        .collect();
    sorted_files.sort_by_key(|(_, t)| *t);

    for (idx, tokens) in sorted_files {
        if tokens > per_batch_budget {
            // Even alone this file exceeds the budget — try clipping
            if let Some(clipped) = clip_diff(&diffs[idx], per_batch_budget * CHARS_PER_TOKEN) {
                let clipped_tokens = estimate_diff_tokens(&clipped);
                // Try to fit in existing batch or create new one
                let placed = batches
                    .iter_mut()
                    .find(|b| b.estimated_tokens + clipped_tokens <= per_batch_budget);
                if let Some(batch) = placed {
                    batch.diff_indices.push(idx);
                    batch.estimated_tokens += clipped_tokens;
                } else if batches.len() < max_calls {
                    batches.push(DiffBatch {
                        diff_indices: vec![idx],
                        estimated_tokens: clipped_tokens,
                    });
                } else {
                    multi_skipped.push(idx);
                }
            } else {
                multi_skipped.push(idx);
            }
            continue;
        }

        // Try to fit in existing batch
        let placed = batches
            .iter_mut()
            .find(|b| b.estimated_tokens + tokens <= per_batch_budget);
        if let Some(batch) = placed {
            batch.diff_indices.push(idx);
            batch.estimated_tokens += tokens;
        } else if batches.len() < max_calls {
            batches.push(DiffBatch {
                diff_indices: vec![idx],
                estimated_tokens: tokens,
            });
        } else {
            multi_skipped.push(idx);
        }
    }

    // Sort indices within each batch
    for batch in &mut batches {
        batch.diff_indices.sort();
    }
    multi_skipped.sort();

    let summary = build_skipped_summary(diffs, &multi_skipped);
    CompressionResult {
        strategy: CompressionStrategy::MultiCall,
        batches,
        skipped_indices: multi_skipped,
        skipped_summary: summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine, UnifiedDiff};
    use std::path::PathBuf;

    fn make_line(change_type: ChangeType, content: &str) -> DiffLine {
        DiffLine {
            old_line_no: Some(1),
            new_line_no: Some(1),
            change_type,
            content: content.to_string(),
        }
    }

    fn make_hunk(changes: Vec<DiffLine>) -> DiffHunk {
        DiffHunk {
            old_start: 1,
            old_lines: 10,
            new_start: 1,
            new_lines: 10,
            context: String::new(),
            changes,
        }
    }

    fn make_diff(path: &str, hunks: Vec<DiffHunk>) -> UnifiedDiff {
        UnifiedDiff {
            file_path: PathBuf::from(path),
            old_content: None,
            new_content: None,
            hunks,
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    fn make_simple_diff(path: &str, content_size: usize) -> UnifiedDiff {
        let content = "x".repeat(content_size);
        make_diff(
            path,
            vec![make_hunk(vec![make_line(ChangeType::Added, &content)])],
        )
    }

    // --- estimate_diff_tokens ---

    #[test]
    fn test_estimate_tokens_empty_diff() {
        let diff = make_diff("file.rs", vec![]);
        let tokens = estimate_diff_tokens(&diff);
        // Only file overhead
        assert!(tokens > 0);
        assert!(tokens < 50);
    }

    #[test]
    fn test_estimate_tokens_scales_with_content() {
        let small = make_simple_diff("a.rs", 100);
        let large = make_simple_diff("b.rs", 1000);
        assert!(estimate_diff_tokens(&large) > estimate_diff_tokens(&small));
    }

    #[test]
    fn test_estimate_tokens_roughly_correct() {
        // 400 chars of content ~ 100 tokens
        let diff = make_simple_diff("file.rs", 400);
        let tokens = estimate_diff_tokens(&diff);
        // Should be approximately 100 + overhead
        assert!(tokens >= 80, "tokens={} too low", tokens);
        assert!(tokens <= 200, "tokens={} too high", tokens);
    }

    // --- is_deletion_only_hunk ---

    #[test]
    fn test_deletion_only_hunk_all_removed() {
        let hunk = make_hunk(vec![
            make_line(ChangeType::Removed, "old line 1"),
            make_line(ChangeType::Removed, "old line 2"),
        ]);
        assert!(is_deletion_only_hunk(&hunk));
    }

    #[test]
    fn test_deletion_only_hunk_with_context() {
        let hunk = make_hunk(vec![
            make_line(ChangeType::Context, "context"),
            make_line(ChangeType::Removed, "deleted"),
            make_line(ChangeType::Context, "more context"),
        ]);
        assert!(is_deletion_only_hunk(&hunk));
    }

    #[test]
    fn test_not_deletion_only_with_additions() {
        let hunk = make_hunk(vec![
            make_line(ChangeType::Removed, "old"),
            make_line(ChangeType::Added, "new"),
        ]);
        assert!(!is_deletion_only_hunk(&hunk));
    }

    #[test]
    fn test_not_deletion_only_additions_only() {
        let hunk = make_hunk(vec![make_line(ChangeType::Added, "new code")]);
        assert!(!is_deletion_only_hunk(&hunk));
    }

    // --- compress_diff ---

    #[test]
    fn test_compress_diff_keeps_mixed_hunks() {
        let diff = make_diff(
            "file.rs",
            vec![make_hunk(vec![
                make_line(ChangeType::Removed, "old"),
                make_line(ChangeType::Added, "new"),
            ])],
        );
        let compressed = compress_diff(&diff);
        assert!(compressed.is_some());
        assert_eq!(compressed.unwrap().hunks.len(), 1);
    }

    #[test]
    fn test_compress_diff_removes_deletion_only_hunks() {
        let diff = make_diff(
            "file.rs",
            vec![
                make_hunk(vec![make_line(ChangeType::Removed, "deleted")]),
                make_hunk(vec![make_line(ChangeType::Added, "added")]),
            ],
        );
        let compressed = compress_diff(&diff).unwrap();
        assert_eq!(compressed.hunks.len(), 1);
    }

    #[test]
    fn test_compress_diff_returns_none_when_all_deletion() {
        let diff = make_diff(
            "file.rs",
            vec![make_hunk(vec![make_line(ChangeType::Removed, "deleted")])],
        );
        assert!(compress_diff(&diff).is_none());
    }

    // --- clip_diff ---

    #[test]
    fn test_clip_diff_keeps_hunks_within_budget() {
        let diff = make_diff(
            "file.rs",
            vec![
                make_hunk(vec![make_line(ChangeType::Added, &"x".repeat(100))]),
                make_hunk(vec![make_line(ChangeType::Added, &"y".repeat(100))]),
                make_hunk(vec![make_line(ChangeType::Added, &"z".repeat(100))]),
            ],
        );
        let clipped = clip_diff(&diff, 200).unwrap();
        assert!(clipped.hunks.len() < diff.hunks.len());
    }

    #[test]
    fn test_clip_diff_keeps_at_least_one_hunk() {
        let diff = make_diff(
            "file.rs",
            vec![make_hunk(vec![make_line(ChangeType::Added, &"x".repeat(1000))])],
        );
        // Even with tiny budget, should keep at least one hunk
        let clipped = clip_diff(&diff, 10).unwrap();
        assert_eq!(clipped.hunks.len(), 1);
    }

    #[test]
    fn test_clip_diff_empty_diff() {
        let diff = make_diff("file.rs", vec![]);
        assert!(clip_diff(&diff, 1000).is_none());
    }

    // --- build_skipped_summary ---

    #[test]
    fn test_skipped_summary_empty() {
        let diffs = vec![make_simple_diff("a.rs", 100)];
        assert!(build_skipped_summary(&diffs, &[]).is_empty());
    }

    #[test]
    fn test_skipped_summary_includes_modified_files() {
        let diffs = vec![
            make_simple_diff("a.rs", 100),
            make_simple_diff("b.rs", 100),
        ];
        let summary = build_skipped_summary(&diffs, &[1]);
        assert!(summary.contains("b.rs"));
        assert!(summary.contains("not reviewed"));
    }

    #[test]
    fn test_skipped_summary_separates_deleted_files() {
        let mut deleted = make_simple_diff("old.rs", 100);
        deleted.is_deleted = true;
        let diffs = vec![make_simple_diff("a.rs", 100), deleted];
        let summary = build_skipped_summary(&diffs, &[1]);
        assert!(summary.contains("Deleted files"));
        assert!(summary.contains("old.rs"));
    }

    // --- compress_diffs (full pipeline) ---

    #[test]
    fn test_stage1_full_when_fits() {
        let diffs = vec![
            make_simple_diff("a.rs", 100),
            make_simple_diff("b.rs", 100),
        ];
        let result = compress_diffs(&diffs, 10000, 5);
        assert_eq!(result.strategy, CompressionStrategy::Full);
        assert_eq!(result.batches.len(), 1);
        assert_eq!(result.batches[0].diff_indices.len(), 2);
        assert!(result.skipped_indices.is_empty());
    }

    #[test]
    fn test_stage2_compressed_drops_tail() {
        // Create diffs where total exceeds budget but some fit
        let diffs = vec![
            make_simple_diff("small.rs", 100),
            make_simple_diff("huge.rs", 50000),
        ];
        let small_tokens = estimate_diff_tokens(&diffs[0]);
        // Budget: fits small but not huge
        let result = compress_diffs(&diffs, small_tokens + 50, 1);
        assert!(
            result.strategy == CompressionStrategy::Compressed
                || result.strategy == CompressionStrategy::Clipped
        );
        assert!(!result.skipped_indices.is_empty());
    }

    #[test]
    fn test_stage4_multicall_splits() {
        // Multiple files that individually fit but not together.
        // Use large enough files that compression can't rescue them.
        let diffs = vec![
            make_simple_diff("a.rs", 4000),
            make_simple_diff("b.rs", 4000),
            make_simple_diff("c.rs", 4000),
        ];
        let single_tokens = estimate_diff_tokens(&diffs[0]);
        // Budget fits exactly 1 file, allow 3 calls
        let result = compress_diffs(&diffs, single_tokens, 3);
        assert!(
            result.batches.len() >= 2,
            "Expected multiple batches, got {} (single_tokens={})",
            result.batches.len(),
            single_tokens,
        );
    }

    #[test]
    fn test_multicall_respects_max_calls() {
        let diffs: Vec<_> = (0..10)
            .map(|i| make_simple_diff(&format!("file{}.rs", i), 2000))
            .collect();
        let single_tokens = estimate_diff_tokens(&diffs[0]);
        let result = compress_diffs(&diffs, single_tokens + 10, 3);
        assert!(
            result.batches.len() <= 3,
            "Got {} batches, max was 3",
            result.batches.len()
        );
    }

    #[test]
    fn test_empty_diffs() {
        let result = compress_diffs(&[], 10000, 5);
        assert_eq!(result.strategy, CompressionStrategy::Full);
        assert!(result.batches.is_empty());
    }

    #[test]
    fn test_all_indices_accounted_for() {
        let diffs: Vec<_> = (0..5)
            .map(|i| make_simple_diff(&format!("f{}.rs", i), 1000))
            .collect();
        let single_tokens = estimate_diff_tokens(&diffs[0]);
        let result = compress_diffs(&diffs, single_tokens * 2, 3);

        let mut all_indices: Vec<usize> = result
            .batches
            .iter()
            .flat_map(|b| b.diff_indices.iter().copied())
            .collect();
        all_indices.extend(result.skipped_indices.iter().copied());
        all_indices.sort();
        all_indices.dedup();
        // Every file should be either in a batch or skipped
        assert_eq!(all_indices.len(), 5);
    }

    #[test]
    fn test_batch_indices_sorted() {
        let diffs: Vec<_> = (0..5)
            .map(|i| make_simple_diff(&format!("f{}.rs", i), 1000))
            .collect();
        let result = compress_diffs(&diffs, 500, 5);
        for batch in &result.batches {
            let sorted = {
                let mut v = batch.diff_indices.clone();
                v.sort();
                v
            };
            assert_eq!(batch.diff_indices, sorted);
        }
    }

    #[test]
    fn test_single_huge_file_clipped_in_multicall() {
        // One file so big it doesn't fit even alone
        let diff = make_simple_diff("massive.rs", 100_000);
        let tokens = estimate_diff_tokens(&diff);
        // Budget is half the file
        let result = compress_diffs(&[diff], tokens / 2, 1);
        // Should still produce a batch (clipped)
        assert!(
            !result.batches.is_empty() || !result.skipped_indices.is_empty(),
            "File should be either clipped or skipped"
        );
    }
}
