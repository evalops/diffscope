use serde::{Deserialize, Serialize};

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
    #[allow(dead_code)]
    pub skipped_summary: String,
}
