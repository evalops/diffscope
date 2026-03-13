use crate::core::UnifiedDiff;
use crate::plugins::PreAnalysis;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;

#[async_trait]
pub trait PreAnalyzer: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis>;

    async fn run_batch(
        &self,
        diffs: &[UnifiedDiff],
        repo_path: &str,
    ) -> Result<HashMap<PathBuf, PreAnalysis>> {
        let mut results: HashMap<PathBuf, PreAnalysis> = HashMap::new();

        for diff in diffs {
            let analysis = self.run(diff, repo_path).await?;
            if analysis.context_chunks.is_empty() && analysis.findings.is_empty() {
                continue;
            }
            results
                .entry(diff.file_path.clone())
                .or_default()
                .extend(analysis);
        }

        Ok(results)
    }
}
