use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::comment::Comment;
use crate::core::diff_parser::UnifiedDiff;

/// Type alias for stage closure used in [`FnStage`].
type StageFn = dyn Fn(&mut PipelineContext) -> Result<()> + Send + Sync;

/// A named pipeline stage that transforms diff/comment data.
pub trait PipelineStage: Send + Sync {
    fn name(&self) -> &str;
    fn stage_type(&self) -> StageType;
    fn execute(&self, ctx: &mut PipelineContext) -> Result<()>;
}

/// Types of pipeline stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageType {
    /// Parses raw diff input.
    Parse,
    /// Analyzes code (pre-analyzers, linting).
    Analyze,
    /// Runs the LLM review.
    Review,
    /// Filters and transforms comments.
    Filter,
    /// Formats output.
    Format,
    /// Custom user-defined stage.
    Custom(String),
}

/// Context passed through the pipeline, accumulating results.
#[derive(Debug, Default)]
pub struct PipelineContext {
    pub diffs: Vec<UnifiedDiff>,
    pub comments: Vec<Comment>,
    pub metadata: HashMap<String, String>,
    pub stage_results: Vec<StageResult>,
    pub aborted: bool,
    pub abort_reason: Option<String>,
}

impl PipelineContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_diffs(diffs: Vec<UnifiedDiff>) -> Self {
        Self {
            diffs,
            ..Default::default()
        }
    }

    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }

    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    pub fn abort(&mut self, reason: &str) {
        self.aborted = true;
        self.abort_reason = Some(reason.to_string());
    }
}

/// Result recorded after each stage executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage_name: String,
    pub stage_type: StageType,
    pub success: bool,
    pub comments_before: usize,
    pub comments_after: usize,
    pub duration_ms: u64,
    pub message: Option<String>,
}

/// A composable pipeline built from ordered stages.
pub struct Pipeline {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn add_stage(&mut self, stage: Box<dyn PipelineStage>) {
        self.stages.push(stage);
    }

    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    pub fn stage_names(&self) -> Vec<&str> {
        self.stages.iter().map(|s| s.name()).collect()
    }

    /// Execute all stages in order, passing context through.
    pub fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        for stage in &self.stages {
            if ctx.aborted {
                break;
            }

            let comments_before = ctx.comments.len();
            let start = std::time::Instant::now();

            let result = stage.execute(ctx);
            let duration = start.elapsed().as_millis() as u64;

            let success = result.is_ok();
            let message = result.as_ref().err().map(|e| e.to_string());

            ctx.stage_results.push(StageResult {
                stage_name: stage.name().to_string(),
                stage_type: stage.stage_type(),
                success,
                comments_before,
                comments_after: ctx.comments.len(),
                duration_ms: duration,
                message,
            });

            result?;
        }

        Ok(())
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing pipelines with a fluent API.
pub struct PipelineBuilder {
    stages: Vec<Box<dyn PipelineStage>>,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    pub fn add(mut self, stage: Box<dyn PipelineStage>) -> Self {
        self.stages.push(stage);
        self
    }

    pub fn build(self) -> Pipeline {
        Pipeline {
            stages: self.stages,
        }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// --- Built-in stages ---

/// Stage that filters comments by minimum confidence.
pub struct ConfidenceFilterStage {
    min_confidence: f32,
}

impl ConfidenceFilterStage {
    pub fn new(min_confidence: f32) -> Self {
        Self { min_confidence }
    }
}

impl PipelineStage for ConfidenceFilterStage {
    fn name(&self) -> &str {
        "confidence-filter"
    }

    fn stage_type(&self) -> StageType {
        StageType::Filter
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        ctx.comments.retain(|c| c.confidence >= self.min_confidence);
        Ok(())
    }
}

/// Stage that deduplicates comments by file+line+content.
pub struct DeduplicateStage;

impl PipelineStage for DeduplicateStage {
    fn name(&self) -> &str {
        "deduplicate"
    }

    fn stage_type(&self) -> StageType {
        StageType::Filter
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        use crate::core::comment::Severity;
        let severity_rank = |s: &Severity| match s {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
            Severity::Suggestion => 3,
        };
        ctx.comments.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.line_number.cmp(&b.line_number))
                .then(a.content.cmp(&b.content))
                .then(severity_rank(&a.severity).cmp(&severity_rank(&b.severity)))
        });
        // dedup_by keeps b (the earlier element); our sort puts highest severity first
        ctx.comments.dedup_by(|a, b| {
            a.file_path == b.file_path && a.line_number == b.line_number && a.content == b.content
        });
        Ok(())
    }
}

/// Stage that sorts comments by severity then category.
pub struct SortBySeverityStage;

impl PipelineStage for SortBySeverityStage {
    fn name(&self) -> &str {
        "sort-by-severity"
    }

    fn stage_type(&self) -> StageType {
        StageType::Filter
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        ctx.comments.sort_by(|a, b| {
            let severity_rank = |s: &crate::core::comment::Severity| match s {
                crate::core::comment::Severity::Error => 0,
                crate::core::comment::Severity::Warning => 1,
                crate::core::comment::Severity::Info => 2,
                crate::core::comment::Severity::Suggestion => 3,
            };
            severity_rank(&a.severity)
                .cmp(&severity_rank(&b.severity))
                .then_with(|| a.file_path.cmp(&b.file_path))
                .then_with(|| a.line_number.cmp(&b.line_number))
        });
        Ok(())
    }
}

/// Stage that limits the total number of comments.
pub struct MaxCommentsStage {
    max: usize,
}

impl MaxCommentsStage {
    pub fn new(max: usize) -> Self {
        Self { max }
    }
}

impl PipelineStage for MaxCommentsStage {
    fn name(&self) -> &str {
        "max-comments"
    }

    fn stage_type(&self) -> StageType {
        StageType::Filter
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        ctx.comments.truncate(self.max);
        Ok(())
    }
}

/// Stage that adds metadata tags based on comment patterns.
pub struct TaggingStage;

impl PipelineStage for TaggingStage {
    fn name(&self) -> &str {
        "auto-tagger"
    }

    fn stage_type(&self) -> StageType {
        StageType::Analyze
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        for comment in &mut ctx.comments {
            let lower = comment.content.to_lowercase();
            if lower.contains("security") && !comment.tags.contains(&"security".to_string()) {
                comment.tags.push("security".to_string());
            }
            if lower.contains("performance") && !comment.tags.contains(&"performance".to_string()) {
                comment.tags.push("performance".to_string());
            }
            if lower.contains("breaking") && !comment.tags.contains(&"breaking-change".to_string())
            {
                comment.tags.push("breaking-change".to_string());
            }
        }
        Ok(())
    }
}

/// A custom stage defined by a closure.
pub struct FnStage {
    name: String,
    stage_type: StageType,
    func: Box<StageFn>,
}

impl FnStage {
    pub fn new<F>(name: &str, stage_type: StageType, func: F) -> Self
    where
        F: Fn(&mut PipelineContext) -> Result<()> + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            stage_type,
            func: Box::new(func),
        }
    }
}

impl PipelineStage for FnStage {
    fn name(&self) -> &str {
        &self.name
    }

    fn stage_type(&self) -> StageType {
        self.stage_type.clone()
    }

    fn execute(&self, ctx: &mut PipelineContext) -> Result<()> {
        (self.func)(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::comment::{Category, FixEffort, Severity};
    use std::path::PathBuf;

    fn make_comment(file: &str, line: usize, content: &str, confidence: f32) -> Comment {
        Comment {
            id: format!("cmt_{}_{}", file, line),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::BestPractice,
            suggestion: None,
            confidence,
            code_suggestion: None,
            tags: vec![],
            fix_effort: FixEffort::Medium,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
        }
    }

    #[test]
    fn test_empty_pipeline() {
        let pipeline = Pipeline::new();
        let mut ctx = PipelineContext::new();
        assert!(pipeline.execute(&mut ctx).is_ok());
        assert_eq!(ctx.stage_results.len(), 0);
    }

    #[test]
    fn test_confidence_filter() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(ConfidenceFilterStage::new(0.5)))
            .build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "High confidence", 0.9),
            make_comment("a.rs", 2, "Low confidence", 0.3),
            make_comment("a.rs", 3, "Medium confidence", 0.6),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 2);
        assert!(ctx.comments.iter().all(|c| c.confidence >= 0.5));
    }

    #[test]
    fn test_deduplicate_stage() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(DeduplicateStage))
            .build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "Duplicate issue", 0.8),
            make_comment("a.rs", 1, "Duplicate issue", 0.8),
            make_comment("a.rs", 2, "Different issue", 0.7),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 2);
    }

    #[test]
    fn test_sort_by_severity() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(SortBySeverityStage))
            .build();

        let mut ctx = PipelineContext::new();
        let mut c1 = make_comment("a.rs", 1, "Info", 0.5);
        c1.severity = Severity::Info;
        let mut c2 = make_comment("a.rs", 2, "Error", 0.9);
        c2.severity = Severity::Error;
        let mut c3 = make_comment("a.rs", 3, "Warning", 0.7);
        c3.severity = Severity::Warning;

        ctx.comments = vec![c1, c2, c3];
        pipeline.execute(&mut ctx).unwrap();

        assert!(matches!(ctx.comments[0].severity, Severity::Error));
        assert!(matches!(ctx.comments[1].severity, Severity::Warning));
        assert!(matches!(ctx.comments[2].severity, Severity::Info));
    }

    #[test]
    fn test_max_comments_stage() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(MaxCommentsStage::new(2)))
            .build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "First", 0.9),
            make_comment("a.rs", 2, "Second", 0.8),
            make_comment("a.rs", 3, "Third", 0.7),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 2);
    }

    #[test]
    fn test_tagging_stage() {
        let pipeline = PipelineBuilder::new().add(Box::new(TaggingStage)).build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "This is a security vulnerability", 0.9),
            make_comment("a.rs", 2, "Performance could be improved", 0.7),
            make_comment("a.rs", 3, "This is a breaking change in API", 0.8),
            make_comment("a.rs", 4, "Simple style issue", 0.5),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert!(ctx.comments[0].tags.contains(&"security".to_string()));
        assert!(ctx.comments[1].tags.contains(&"performance".to_string()));
        assert!(ctx.comments[2]
            .tags
            .contains(&"breaking-change".to_string()));
        assert!(ctx.comments[3].tags.is_empty());
    }

    #[test]
    fn test_multi_stage_pipeline() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(TaggingStage))
            .add(Box::new(ConfidenceFilterStage::new(0.5)))
            .add(Box::new(SortBySeverityStage))
            .add(Box::new(MaxCommentsStage::new(10)))
            .build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "Security issue", 0.9),
            make_comment("a.rs", 2, "Low confidence", 0.2),
            make_comment("a.rs", 3, "Performance hit", 0.7),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 2); // low confidence filtered
        assert_eq!(ctx.stage_results.len(), 4); // 4 stages executed
        assert!(ctx.stage_results.iter().all(|r| r.success));
    }

    #[test]
    fn test_fn_stage() {
        let custom = FnStage::new("custom-counter", StageType::Custom("count".into()), |ctx| {
            ctx.set_metadata("comment_count", &ctx.comments.len().to_string());
            Ok(())
        });

        let pipeline = PipelineBuilder::new().add(Box::new(custom)).build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "Test", 0.8),
            make_comment("b.rs", 2, "Test2", 0.9),
        ];

        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.get_metadata("comment_count"), Some("2"));
    }

    #[test]
    fn test_abort_stops_pipeline() {
        let abort_stage = FnStage::new("aborter", StageType::Custom("abort".into()), |ctx| {
            ctx.abort("Testing abort");
            Ok(())
        });

        let should_not_run = FnStage::new("never", StageType::Filter, |ctx| {
            ctx.set_metadata("ran", "true");
            Ok(())
        });

        let pipeline = PipelineBuilder::new()
            .add(Box::new(abort_stage))
            .add(Box::new(should_not_run))
            .build();

        let mut ctx = PipelineContext::new();
        pipeline.execute(&mut ctx).unwrap();

        assert!(ctx.aborted);
        assert_eq!(ctx.abort_reason.as_deref(), Some("Testing abort"));
        assert!(ctx.get_metadata("ran").is_none());
    }

    #[test]
    fn test_stage_results_tracking() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(ConfidenceFilterStage::new(0.5)))
            .build();

        let mut ctx = PipelineContext::new();
        ctx.comments = vec![
            make_comment("a.rs", 1, "Keep", 0.8),
            make_comment("a.rs", 2, "Drop", 0.3),
        ];

        pipeline.execute(&mut ctx).unwrap();

        assert_eq!(ctx.stage_results.len(), 1);
        let result = &ctx.stage_results[0];
        assert_eq!(result.stage_name, "confidence-filter");
        assert_eq!(result.stage_type, StageType::Filter);
        assert!(result.success);
        assert_eq!(result.comments_before, 2);
        assert_eq!(result.comments_after, 1);
    }

    #[test]
    fn test_add_stage_and_stage_count() {
        let mut pipeline = Pipeline::new();
        assert_eq!(pipeline.stage_count(), 0);

        pipeline.add_stage(Box::new(TaggingStage));
        assert_eq!(pipeline.stage_count(), 1);

        pipeline.add_stage(Box::new(DeduplicateStage));
        assert_eq!(pipeline.stage_count(), 2);
    }

    #[test]
    fn test_pipeline_stage_names() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(TaggingStage))
            .add(Box::new(DeduplicateStage))
            .add(Box::new(SortBySeverityStage))
            .build();

        let names = pipeline.stage_names();
        assert_eq!(
            names,
            vec!["auto-tagger", "deduplicate", "sort-by-severity"]
        );
    }

    #[test]
    fn test_pipeline_context_metadata() {
        let mut ctx = PipelineContext::new();
        assert!(ctx.get_metadata("key").is_none());

        ctx.set_metadata("key", "value");
        assert_eq!(ctx.get_metadata("key"), Some("value"));

        ctx.set_metadata("key", "updated");
        assert_eq!(ctx.get_metadata("key"), Some("updated"));
    }

    #[test]
    fn test_pipeline_with_diffs() {
        let diffs = vec![UnifiedDiff {
            file_path: PathBuf::from("test.rs"),
            old_content: None,
            new_content: None,
            hunks: vec![],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }];

        let ctx = PipelineContext::with_diffs(diffs);
        assert_eq!(ctx.diffs.len(), 1);
        assert!(ctx.comments.is_empty());
    }

    #[test]
    fn test_empty_pipeline_no_stages() {
        let pipeline = Pipeline::new();
        let mut ctx = PipelineContext::new();
        let result = pipeline.execute(&mut ctx);
        assert!(result.is_ok());
        assert!(ctx.stage_results.is_empty());
    }

    #[test]
    fn test_pipeline_abort_stops_later_stages() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(FnStage::new(
                "aborter",
                StageType::Custom("test".to_string()),
                |ctx| {
                    ctx.abort("test abort");
                    Ok(())
                },
            )))
            .add(Box::new(TaggingStage))
            .build();

        let mut ctx = PipelineContext::new();
        let result = pipeline.execute(&mut ctx);
        assert!(result.is_ok());
        assert!(ctx.aborted);
        assert_eq!(ctx.abort_reason.as_deref(), Some("test abort"));
        // Tagging stage should not have run
        assert_eq!(ctx.stage_results.len(), 1);
    }

    #[test]
    fn test_max_comments_truncates() {
        let mut ctx = PipelineContext::new();
        for i in 0..10 {
            ctx.comments
                .push(make_comment("test.rs", i + 1, &format!("comment {i}"), 0.8));
        }

        let stage = MaxCommentsStage::new(5);
        stage.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 5);
    }

    #[test]
    fn test_confidence_filter_removes_low() {
        let mut ctx = PipelineContext::new();
        ctx.comments
            .push(make_comment("test.rs", 1, "low confidence", 0.3));
        ctx.comments
            .push(make_comment("test.rs", 2, "high confidence", 0.9));

        let stage = ConfidenceFilterStage::new(0.5);
        stage.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 1);
    }

    // Regression: DeduplicateStage must preserve different-content comments at the same line
    #[test]
    fn test_deduplicate_preserves_different_content_same_line() {
        let mut ctx = PipelineContext::new();
        ctx.comments
            .push(make_comment("test.rs", 5, "Missing null check", 0.8));
        ctx.comments
            .push(make_comment("test.rs", 5, "Potential memory leak", 0.9));

        let stage = DeduplicateStage;
        stage.execute(&mut ctx).unwrap();
        // Different content at same line should be preserved
        assert_eq!(ctx.comments.len(), 2);
    }

    // Regression: DeduplicateStage must keep the highest severity when deduplicating
    #[test]
    fn test_deduplicate_preserves_highest_severity() {
        let mut ctx = PipelineContext::new();
        let mut info = make_comment("test.rs", 10, "duplicate issue", 0.7);
        info.severity = Severity::Info;
        let mut error = make_comment("test.rs", 10, "duplicate issue", 0.7);
        error.severity = Severity::Error;
        // Insert lower severity first to ensure sort fixes order
        ctx.comments.push(info);
        ctx.comments.push(error);

        let stage = DeduplicateStage;
        stage.execute(&mut ctx).unwrap();
        assert_eq!(ctx.comments.len(), 1);
        assert_eq!(
            ctx.comments[0].severity,
            Severity::Error,
            "DeduplicateStage should keep the highest severity"
        );
    }

    // Regression: SortBySeverityStage must sort Error > Warning > Info
    #[test]
    fn test_sort_by_severity_order() {
        let mut ctx = PipelineContext::new();
        ctx.comments.push(make_comment("b.rs", 1, "info", 0.5));
        ctx.comments[0].severity = Severity::Info;
        ctx.comments.push(make_comment("a.rs", 1, "error", 0.9));
        ctx.comments[1].severity = Severity::Error;
        ctx.comments.push(make_comment("a.rs", 2, "warning", 0.7));
        ctx.comments[2].severity = Severity::Warning;

        let stage = SortBySeverityStage;
        stage.execute(&mut ctx).unwrap();

        // Error should come first, then Warning, then Info
        assert_eq!(ctx.comments[0].severity, Severity::Error);
        assert_eq!(ctx.comments[1].severity, Severity::Warning);
        assert_eq!(ctx.comments[2].severity, Severity::Info);
    }

    // Test that pipeline records stage results correctly
    #[test]
    fn test_pipeline_records_all_stage_results() {
        let pipeline = PipelineBuilder::new()
            .add(Box::new(TaggingStage))
            .add(Box::new(DeduplicateStage))
            .add(Box::new(SortBySeverityStage))
            .build();

        let mut ctx = PipelineContext::new();
        pipeline.execute(&mut ctx).unwrap();
        assert_eq!(ctx.stage_results.len(), 3);
        assert_eq!(ctx.stage_results[0].stage_name, "auto-tagger");
        assert_eq!(ctx.stage_results[1].stage_name, "deduplicate");
        assert_eq!(ctx.stage_results[2].stage_name, "sort-by-severity");
    }
}
