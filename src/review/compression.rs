//! Adaptive patch compression for large PRs.
//!
//! Implements a 4-stage progressive degradation strategy (inspired by Qodo/CodeRabbit):
//!   Stage 1 – Full:       all diffs fit within token budget → use as-is.
//!   Stage 2 – Compressed: remove deletion-only hunks, sort by size, drop tail files.
//!   Stage 3 – Clipped:    truncate remaining large files at clean line boundaries.
//!   Stage 4 – MultiCall:  split into multiple LLM call batches.

#![allow(dead_code)]

#[path = "compression/metrics.rs"]
mod metrics;
#[path = "compression/multicall.rs"]
mod multicall;
#[path = "compression/run.rs"]
mod run;
#[path = "compression/selection.rs"]
mod selection;
#[path = "compression/summary.rs"]
mod summary;
#[path = "compression/transform.rs"]
mod transform;
#[path = "compression/types.rs"]
mod types;

#[allow(unused_imports)]
pub use metrics::estimate_diff_tokens;
#[allow(unused_imports)]
pub use run::compress_diffs;
#[allow(unused_imports)]
pub use summary::build_skipped_summary;
#[allow(unused_imports)]
pub use transform::{clip_diff, compress_diff, is_deletion_only_hunk};
#[allow(unused_imports)]
pub use types::{CompressionResult, CompressionStrategy, DiffBatch};

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
        assert!(tokens >= 80, "tokens={tokens} too low");
        assert!(tokens <= 200, "tokens={tokens} too high");
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
            vec![make_hunk(vec![make_line(
                ChangeType::Added,
                &"x".repeat(1000),
            )])],
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
        let diffs = vec![make_simple_diff("a.rs", 100), make_simple_diff("b.rs", 100)];
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
        let diffs = vec![make_simple_diff("a.rs", 100), make_simple_diff("b.rs", 100)];
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
            .map(|i| make_simple_diff(&format!("file{i}.rs"), 2000))
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
            .map(|i| make_simple_diff(&format!("f{i}.rs"), 1000))
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
            .map(|i| make_simple_diff(&format!("f{i}.rs"), 1000))
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

    // ── Mutation-testing gap fills ─────────────────────────────────────

    #[test]
    fn test_estimate_tokens_includes_overhead() {
        // Even an empty hunk should have overhead from file path + hunk header
        let diff = make_diff("a.rs", vec![make_hunk(vec![])]);
        let tokens = estimate_diff_tokens(&diff);
        assert!(tokens > 0, "Empty hunk should still have overhead tokens");
    }

    #[test]
    fn test_estimate_tokens_content_proportional() {
        // Adding more content should always increase token count
        let small = make_simple_diff("a.rs", 100);
        let medium = make_simple_diff("a.rs", 500);
        let large = make_simple_diff("a.rs", 2000);
        let t_small = estimate_diff_tokens(&small);
        let t_medium = estimate_diff_tokens(&medium);
        let t_large = estimate_diff_tokens(&large);
        assert!(t_small < t_medium, "small={t_small} >= medium={t_medium}");
        assert!(t_medium < t_large, "medium={t_medium} >= large={t_large}");
    }

    #[test]
    fn test_clip_diff_reduces_size() {
        let diff = make_diff(
            "file.rs",
            vec![
                make_hunk(vec![make_line(ChangeType::Added, &"a".repeat(500))]),
                make_hunk(vec![make_line(ChangeType::Added, &"b".repeat(500))]),
                make_hunk(vec![make_line(ChangeType::Added, &"c".repeat(500))]),
            ],
        );
        let original_tokens = estimate_diff_tokens(&diff);
        let clipped = clip_diff(&diff, 600).unwrap(); // budget fits ~1 hunk
        let clipped_tokens = estimate_diff_tokens(&clipped);
        assert!(
            clipped_tokens < original_tokens,
            "Clipped ({clipped_tokens}) should be smaller than original ({original_tokens})"
        );
    }

    #[test]
    fn test_compress_stage2_budget_accounting() {
        // Verify that included files actually fit within the budget
        let diffs = vec![
            make_simple_diff("small.rs", 200),
            make_simple_diff("medium.rs", 500),
            make_simple_diff("large.rs", 2000),
        ];
        let budget = estimate_diff_tokens(&diffs[0]) + estimate_diff_tokens(&diffs[1]) + 10;
        let result = compress_diffs(&diffs, budget, 1);
        for batch in &result.batches {
            assert!(
                batch.estimated_tokens <= budget,
                "Batch tokens {} exceeds budget {}",
                batch.estimated_tokens,
                budget
            );
        }
    }

    #[test]
    fn test_skipped_summary_out_of_bounds_index() {
        let diffs = vec![make_simple_diff("a.rs", 100)];
        // Index 99 is out of bounds — should not panic
        let summary = build_skipped_summary(&diffs, &[99]);
        // Out-of-bounds index is silently ignored
        assert!(summary.is_empty());
    }

    // ── Adversarial edge cases ──────────────────────────────────────────

    #[test]
    fn test_zero_token_budget() {
        let diffs = vec![make_simple_diff("a.rs", 100)];
        // Budget of 0 — should not panic, everything skipped or clipped
        let result = compress_diffs(&diffs, 0, 5);
        // With 0 budget nothing should fit in Full/Compressed, but the algorithm
        // should still produce some output without panicking
        assert!(
            result.batches.is_empty() || result.batches.iter().all(|b| b.diff_indices.is_empty()),
            "Nothing should fit in zero budget"
        );
    }

    #[test]
    fn test_max_calls_zero_treated_as_one() {
        let diffs = vec![make_simple_diff("a.rs", 100)];
        // max_calls=0 should be clamped to 1, not panic
        let result = compress_diffs(&diffs, 10000, 0);
        assert_eq!(result.strategy, CompressionStrategy::Full);
    }

    #[test]
    fn test_diff_with_empty_hunks() {
        let diff = make_diff("empty.rs", vec![]);
        let tokens = estimate_diff_tokens(&diff);
        assert!(tokens < 50, "Empty diff should have minimal tokens");
        let result = compress_diffs(&[diff], 10000, 5);
        assert_eq!(result.strategy, CompressionStrategy::Full);
    }

    #[test]
    fn test_all_diffs_deletion_only_compressed_away() {
        // Every diff has only deletion hunks — compress_diff returns None for all.
        // Use a budget smaller than total tokens to force Stage 2 (where compression happens).
        let diffs = vec![
            make_diff(
                "a.rs",
                vec![make_hunk(vec![make_line(
                    ChangeType::Removed,
                    &"x".repeat(200),
                )])],
            ),
            make_diff(
                "b.rs",
                vec![make_hunk(vec![make_line(
                    ChangeType::Removed,
                    &"y".repeat(200),
                )])],
            ),
        ];
        let total = diffs.iter().map(estimate_diff_tokens).sum::<usize>();
        // Budget smaller than total forces Stage 2, where deletion-only hunks get removed
        let result = compress_diffs(&diffs, total / 2, 5);
        // All files should be skipped since they're deletion-only after compression
        assert_eq!(
            result.skipped_indices.len(),
            2,
            "Both deletion-only diffs should be skipped, got {result:?}"
        );
    }

    #[test]
    fn test_no_duplicate_indices_across_batches() {
        let diffs: Vec<_> = (0..8)
            .map(|i| make_simple_diff(&format!("f{i}.rs"), 2000))
            .collect();
        let single_tokens = estimate_diff_tokens(&diffs[0]);
        let result = compress_diffs(&diffs, single_tokens * 2, 4);

        let mut seen = std::collections::HashSet::new();
        for batch in &result.batches {
            for &idx in &batch.diff_indices {
                assert!(seen.insert(idx), "Duplicate index {idx} across batches");
            }
        }
        for &idx in &result.skipped_indices {
            assert!(
                seen.insert(idx),
                "Index {idx} appears in both batches and skipped"
            );
        }
    }

    #[test]
    fn test_budget_of_one_token() {
        let diffs = vec![make_simple_diff("a.rs", 100), make_simple_diff("b.rs", 100)];
        // Budget so tiny nothing should fit via compressed
        let result = compress_diffs(&diffs, 1, 1);
        // Should not panic; files end up skipped or in a clipped batch
        let total_accounted = result
            .batches
            .iter()
            .map(|b| b.diff_indices.len())
            .sum::<usize>()
            + result.skipped_indices.len();
        assert_eq!(total_accounted, 2, "All files must be accounted for");
    }
}
