use crate::core::UnifiedDiff;
use crate::plugins::PreAnalysis;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait PreAnalyzer: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis>;
}
