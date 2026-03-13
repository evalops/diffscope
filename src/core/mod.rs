pub mod agent_loop;
pub mod agent_tools;
pub mod changelog;
pub mod code_summary;
pub mod comment;
pub mod commit_prompt;
pub mod composable_pipeline;
pub mod context;
pub mod convention_learner;
pub mod diff_parser;
pub mod enhanced_review;
pub mod eval_benchmarks;
pub mod function_chunker;
pub mod git;
pub mod git_history;
pub mod interactive;
pub mod multi_pass;
pub mod offline;
pub mod pr_history;
pub mod pr_summary;
pub mod prompt;
pub mod rules;
pub mod semantic;
pub mod smart_review_prompt;
pub mod symbol_graph;
pub mod symbol_index;

pub use changelog::ChangelogGenerator;
pub use comment::{Comment, CommentSynthesizer};
pub use commit_prompt::CommitPromptBuilder;
pub use context::{ContextFetcher, ContextType, LLMContextChunk};
pub use diff_parser::{DiffParser, UnifiedDiff};
pub use enhanced_review::{
    apply_enhanced_filters, build_enhanced_context, generate_enhanced_guidance,
};
pub use git::{validate_ref_name, GitIntegration};
pub use pr_summary::{PRSummaryGenerator, SummaryOptions};
pub use prompt::{PromptBuilder, SpecializedPassKind};
pub use rules::{active_rules_for_file, load_rules_from_patterns, ReviewRule};
pub use semantic::{
    build_feedback_embedding_text, default_index_path, default_semantic_feedback_path,
    embed_texts_with_fallback, find_similar_feedback_examples, load_semantic_feedback_store,
    refresh_semantic_index, save_semantic_feedback_store, semantic_context_for_diff,
    SemanticFeedbackExample, SemanticFeedbackStore,
};
pub use smart_review_prompt::SmartReviewPromptBuilder;
pub use symbol_index::SymbolIndex;
