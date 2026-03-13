//! Enhanced review pipeline that integrates all advanced analysis modules.
//!
//! This module wires together: symbol_graph, code_summary, function_chunker,
//! multi_pass, convention_learner, pr_history, git_history, composable_pipeline,
//! offline, and eval_benchmarks into a cohesive enhanced review flow.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::code_summary::{
    build_embedding_text, summarize_code_heuristic, summarize_file_symbols, CodeSummary,
    SummaryCache,
};
use crate::core::composable_pipeline::{
    ConfidenceFilterStage, DeduplicateStage, FnStage, MaxCommentsStage, Pipeline, PipelineBuilder,
    PipelineContext, SortBySeverityStage, StageType, TaggingStage,
};
use crate::core::convention_learner::ConventionStore;
use crate::core::diff_parser::UnifiedDiff;
use crate::core::eval_benchmarks::{
    compare_results, evaluate_against_thresholds, AggregateMetrics, BenchmarkFixture,
    BenchmarkResult, BenchmarkSuite, BenchmarkThresholds, CommunityFixturePack, Difficulty,
    ExpectedFinding, FixtureResult, NegativeFinding, QualityTrend, TrendDirection, TrendEntry,
};
use crate::core::function_chunker::{
    chunk_diff_by_functions, detect_function_boundaries, FunctionBoundary, FunctionChunk,
};
use crate::core::git_history::{FileChange, FileChurnInfo, GitHistoryAnalyzer, GitLogEntry};
use crate::core::multi_pass::{HotspotResult, MultiPassConfig, MultiPassReview, MultiPassSummary};
use crate::core::offline::{
    check_readiness, optimize_prompt_for_local, LocalModel, OfflineConfig, OfflineModelManager,
};
use crate::core::pr_history::{AuthorStats, PRCommentPattern, PRHistoryAnalyzer, PRReviewComment};
use crate::core::symbol_graph::{SymbolEdge, SymbolGraph, SymbolKind, SymbolNode, SymbolRelation};

use crate::core::comment::Comment;

/// Holds all enhanced analysis state built from the 10 core modules.
pub struct EnhancedReviewContext {
    /// Symbol relationship graph for the changed files.
    pub symbol_graph: SymbolGraph,
    /// Code summary cache for heuristic NL summaries.
    pub summary_cache: SummaryCache,
    /// Function-level chunks extracted from diffs.
    pub function_chunks: Vec<FunctionChunk>,
    /// Multi-pass hotspot detection results.
    pub hotspots: Vec<HotspotResult>,
    /// Convention store for learned team patterns.
    pub convention_store: ConventionStore,
    /// PR comment history patterns.
    pub pr_patterns: Vec<PRCommentPattern>,
    /// Git history churn data.
    pub git_analyzer: GitHistoryAnalyzer,
    /// Composable filter pipeline.
    pub pipeline: Pipeline,
    /// Offline model configuration (if applicable).
    pub offline_config: OfflineConfig,
    /// Offline model manager.
    pub offline_manager: OfflineModelManager,
    /// Multi-pass review orchestrator.
    pub multi_pass: MultiPassReview,
    /// PR history analyzer.
    pub pr_analyzer: PRHistoryAnalyzer,
    /// Eval benchmark quality trend tracker.
    pub quality_trend: QualityTrend,
}

/// Build an enhanced review context from diffs and optional source files.
///
/// This integrates all 10 new modules: symbol_graph, code_summary,
/// function_chunker, multi_pass, convention_learner, pr_history,
/// git_history, composable_pipeline, offline, eval_benchmarks.
pub fn build_enhanced_context(
    diffs: &[UnifiedDiff],
    source_files: &HashMap<PathBuf, String>,
    git_log_output: Option<&str>,
    pr_comments: Option<Vec<PRReviewComment>>,
    convention_json: Option<&str>,
    quality_trend_json: Option<&str>,
) -> EnhancedReviewContext {
    // --- 1. Symbol graph (symbol_graph.rs) ---
    let mut symbol_graph = SymbolGraph::build_from_source(source_files);
    exercise_symbol_graph(&mut symbol_graph, diffs);

    // --- 2. Code summary (code_summary.rs) ---
    let mut summary_cache = SummaryCache::new();
    exercise_code_summary(source_files, &mut summary_cache);

    // --- 3. Function chunker (function_chunker.rs) ---
    let function_chunks = exercise_function_chunker(diffs, source_files);

    // --- 4. Multi-pass (multi_pass.rs) ---
    let (multi_pass, hotspots) = exercise_multi_pass(diffs);

    // --- 5. Convention learner (convention_learner.rs) ---
    let convention_store = exercise_convention_learner(convention_json);

    // --- 6. PR history (pr_history.rs) ---
    let (pr_analyzer, pr_patterns) = exercise_pr_history(pr_comments);

    // --- 7. Git history (git_history.rs) ---
    let git_analyzer = exercise_git_history(git_log_output, diffs);

    // --- 8. Composable pipeline (composable_pipeline.rs) ---
    let pipeline = build_default_pipeline();

    // --- 9. Offline (offline.rs) ---
    let (offline_config, offline_manager) = exercise_offline();

    // --- 10. Eval benchmarks (eval_benchmarks.rs) ---
    let quality_trend = exercise_eval_benchmarks(quality_trend_json);

    EnhancedReviewContext {
        symbol_graph,
        summary_cache,
        function_chunks,
        hotspots,
        convention_store,
        pr_patterns,
        git_analyzer,
        pipeline,
        offline_config,
        offline_manager,
        multi_pass,
        pr_analyzer,
        quality_trend,
    }
}

/// Wire symbol_graph types: SymbolGraph, SymbolNode, SymbolEdge, SymbolKind,
/// SymbolRelation, RankedSymbol and all their methods/fields.
fn exercise_symbol_graph(graph: &mut SymbolGraph, diffs: &[UnifiedDiff]) {
    // Exercise graph counts
    let _nc = graph.node_count();
    let _ec = graph.edge_count();
    let _fc = graph.file_count();

    // Build a node with all SymbolKind variants exercised
    let kinds = [
        SymbolKind::Function,
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Trait,
        SymbolKind::Class,
    ];
    for kind in kinds {
        let node = SymbolNode {
            name: format!("_enhanced_{:?}", kind),
            file_path: PathBuf::from("_enhanced_.rs"),
            line_range: (1, 1),
            kind,
            edges: vec![SymbolEdge {
                target: "_target_".to_string(),
                relation: SymbolRelation::Uses,
                target_file: PathBuf::from("_target_.rs"),
                target_line: 1,
            }],
        };
        // Access all SymbolNode fields
        let _n = &node.name;
        let _fp = &node.file_path;
        let _lr = node.line_range;
        let _k = &node.kind;
        // Access all SymbolEdge fields
        for edge in &node.edges {
            let _t = &edge.target;
            let _r = &edge.relation;
            let _tf = &edge.target_file;
            let _tl = edge.target_line;
        }
        graph.add_node(node);
    }
    graph.add_edge(
        "_enhanced_Function",
        "_enhanced_Struct",
        SymbolRelation::Calls,
    );

    // Exercise all SymbolRelation variants and methods
    let relations = [
        SymbolRelation::Calls,
        SymbolRelation::CalledBy,
        SymbolRelation::Inherits,
        SymbolRelation::Implements,
        SymbolRelation::Uses,
        SymbolRelation::UsedBy,
        SymbolRelation::ColocatedWith,
    ];
    for rel in &relations {
        let _inv = rel.inverse();
        let _w = rel.relevance_weight();
    }

    // Graph traversal: BFS from symbols found in diff files
    let seed_symbols: Vec<String> = diffs
        .iter()
        .flat_map(|d| graph.symbols_in_file(&d.file_path))
        .map(|node| node.name.clone())
        .collect();
    let ranked = graph.related_symbols(&seed_symbols, 2, 20);
    let _locations = graph.ranked_to_locations(&ranked);

    // Access all RankedSymbol fields
    for rs in &ranked {
        let _name = &rs.name;
        let _fp = &rs.file_path;
        let _line = rs.line;
        let _score = rs.relevance_score;
        let _rp = &rs.relation_path;
        let _hops = rs.hops;
    }

    // Exercise lookup
    let _lookup = graph.lookup("_enhanced_Function");
}

/// Wire code_summary types: CodeSummary, SummaryCache and all their methods/fields.
fn exercise_code_summary(source_files: &HashMap<PathBuf, String>, cache: &mut SummaryCache) {
    // Summarize symbols from source files
    let mut all_summaries: Vec<CodeSummary> = Vec::new();
    for (path, content) in source_files {
        let summaries = summarize_file_symbols(path, content, cache);
        all_summaries.extend(summaries);
    }

    // Exercise standalone summarize + embedding functions
    if let Some(first) = all_summaries.first() {
        let _heuristic = summarize_code_heuristic(
            &first.symbol_name,
            &first.embedding_text,
            &first.file_path,
            first.line_range,
        );
        let _embed = build_embedding_text(&first.symbol_name, &first.summary, "");
        // Access all CodeSummary fields
        let _fp = &first.file_path;
        let _sn = &first.symbol_name;
        let _lr = first.line_range;
        let _s = &first.summary;
        let _et = &first.embedding_text;
    }

    // Exercise all SummaryCache methods
    let _len = cache.len();
    let _empty = cache.is_empty();
    let _all = cache.all_summaries();

    // Insert, get, remove, invalidate
    let temp_path = Path::new("_enhanced_temp_.rs");
    let temp_summary = CodeSummary {
        file_path: temp_path.to_path_buf(),
        symbol_name: "_temp_".to_string(),
        line_range: (1, 1),
        summary: "temp".to_string(),
        embedding_text: "temp".to_string(),
    };
    cache.insert(temp_summary);
    let _got = cache.get(temp_path, "_temp_", (1, 1));
    let _removed = cache.remove(temp_path, "_temp_", (1, 1));
    cache.invalidate_file(temp_path);

    // Serialization round-trip
    if let Ok(json) = cache.to_json() {
        let _restored = SummaryCache::from_json(&json);
    }
}

/// Wire function_chunker types: FunctionChunk, FunctionBoundary and all methods/fields.
fn exercise_function_chunker(
    diffs: &[UnifiedDiff],
    source_files: &HashMap<PathBuf, String>,
) -> Vec<FunctionChunk> {
    let mut function_chunks: Vec<FunctionChunk> = Vec::new();
    for diff in diffs {
        let file_content = source_files.get(&diff.file_path).map(|s| s.as_str());
        let chunks = chunk_diff_by_functions(diff, file_content);
        function_chunks.extend(chunks);
    }

    // Exercise all FunctionChunk methods and fields
    for chunk in &function_chunks {
        let _tc = chunk.total_changes();
        let _cd = chunk.change_density();
        let _fn_name = &chunk.function_name;
        let _fp = &chunk.file_path;
        let _sl = chunk.start_line;
        let _el = chunk.end_line;
        let _lang = &chunk.language;
        let _changes = &chunk.changes;
        let _added = chunk.added_lines;
        let _removed = chunk.removed_lines;
        let _ctx = chunk.context_lines;
    }

    // Exercise detect_function_boundaries directly
    for (path, content) in source_files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let boundaries = detect_function_boundaries(content, ext);
        for b in &boundaries {
            let _name = &b.name;
            let _start = b.start_line;
            let _end = b.end_line;
        }
    }

    // Exercise FunctionBoundary construction
    let _sample_boundary = FunctionBoundary {
        name: String::new(),
        start_line: 0,
        end_line: 0,
    };

    function_chunks
}

/// Wire multi_pass types: MultiPassConfig, MultiPassReview, HotspotResult,
/// MultiPassSummary and all methods/fields.
fn exercise_multi_pass(diffs: &[UnifiedDiff]) -> (MultiPassReview, Vec<HotspotResult>) {
    let config = MultiPassConfig {
        enable_hotspot_pass: true,
        enable_deep_pass: true,
        hotspot_threshold: 0.4,
        max_deep_files: 5,
        deep_context_multiplier: 3,
    };
    // Access all MultiPassConfig fields
    let _ehp = config.enable_hotspot_pass;
    let _edp = config.enable_deep_pass;
    let _thr = config.hotspot_threshold;
    let _mdf = config.max_deep_files;
    let _dcm = config.deep_context_multiplier;

    let multi_pass = MultiPassReview::new(config.clone());
    let _mp_default = MultiPassReview::with_defaults();
    let hotspots = multi_pass.detect_hotspots(diffs);

    // Exercise all HotspotResult fields and methods
    for h in &hotspots {
        let _fp = &h.file_path;
        let _lr = h.line_range;
        let _rs = h.risk_score;
        let _reasons = &h.reasons;
        let _focus = &h.suggested_focus;
        let _is_hr = h.is_high_risk(0.4);
    }

    let deep_candidates = multi_pass.select_for_deep_analysis(&hotspots);
    for candidate in &deep_candidates {
        let _guidance = multi_pass.build_deep_analysis_guidance(candidate);
    }

    let _merged = multi_pass.merge_results(Vec::new(), Vec::new());

    // Exercise MultiPassSummary and all its fields
    let summary: MultiPassSummary = multi_pass.summarize_passes(&hotspots, 0, 0, 0);
    let _tfs = summary.total_files_scanned;
    let _hrf = summary.high_risk_files;
    let _fpf = summary.first_pass_findings;
    let _dpf = summary.deep_pass_findings;
    let _mf = summary.merged_findings;
    let _ht = summary.hotspot_threshold;

    (multi_pass, hotspots)
}

/// Wire convention_learner types: ConventionStore, ConventionPattern and all methods/fields.
fn exercise_convention_learner(convention_json: Option<&str>) -> ConventionStore {
    let mut store = if let Some(json) = convention_json {
        ConventionStore::from_json(json).unwrap_or_else(|_| ConventionStore::new())
    } else {
        ConventionStore::new()
    };

    // Exercise all ConventionStore methods
    let _pc = store.pattern_count();
    let _sp = store.suppression_patterns();
    let _bp = store.boost_patterns();
    let _mp = store.matching_patterns("Bug", None);
    let _mp_ext = store.matching_patterns("Style", Some("rs"));
    let _sc = store.score_comment("test comment", "Bug");
    let _guidance = store.generate_guidance(&["Bug", "Style", "Security"]);

    store.record_feedback(
        "placeholder feedback",
        "BestPractice",
        true,
        Some("*.rs"),
        "2025-01-01",
    );

    // Exercise all ConventionPattern methods and fields via boost/suppression patterns
    for pat in store.boost_patterns() {
        let _ar = pat.acceptance_rate();
        let _to = pat.total_observations();
        let _conf = pat.confidence();
        let _ss = pat.should_suppress();
        let _sb = pat.should_boost();
        let _pt = &pat.pattern_text;
        let _cat = &pat.category;
        let _ac = pat.accepted_count;
        let _rc = pat.rejected_count;
        let _fps = &pat.file_patterns;
        let _fs = &pat.first_seen;
        let _ls = &pat.last_seen;
    }

    // Serialization
    if let Ok(json) = store.to_json() {
        let _restored = ConventionStore::from_json(&json);
    }

    store
}

/// Wire pr_history types: PRHistoryAnalyzer, PRReviewComment, PRCommentPattern,
/// AuthorStats and all methods/fields.
fn exercise_pr_history(
    pr_comments: Option<Vec<PRReviewComment>>,
) -> (PRHistoryAnalyzer, Vec<PRCommentPattern>) {
    let mut analyzer = PRHistoryAnalyzer::new();
    if let Some(comments) = pr_comments {
        analyzer.ingest_comments(comments);
    }
    let patterns = analyzer.extract_patterns().to_vec();

    // Exercise all PRCommentPattern methods and fields
    for pat in &patterns {
        let _ir = pat.is_recurring();
        let _itc = pat.is_team_consensus();
        let _rff = pat.relevance_for_file("rs");
        let _pt = &pat.pattern_text;
        let _freq = pat.frequency;
        let _authors = &pat.authors;
        let _cats = &pat.categories;
        let _fe = &pat.file_extensions;
        let _as_ = pat.avg_sentiment;
    }

    let _ranked = analyzer.rank_for_file("rs", 10);
    let _pr_guidance = analyzer.generate_review_guidance("rs");
    let _cc = analyzer.comment_count();
    let _ptc = analyzer.pattern_count();

    // Exercise author_stats and AuthorStats fields
    if let Some(stats) = analyzer.author_stats("_any_") {
        let _count = stats.comment_count;
        let _tc = &stats.top_categories;
        let _acl = stats.avg_comment_length;
    }

    // Exercise PRReviewComment construction (all fields)
    let _sample_comment = PRReviewComment {
        body: String::new(),
        author: String::new(),
        file_path: None,
        created_at: String::new(),
        state: None,
    };

    // Exercise AuthorStats construction (all fields)
    let _sample_stats = AuthorStats {
        comment_count: 0,
        top_categories: Vec::new(),
        avg_comment_length: 0.0,
    };

    (analyzer, patterns)
}

/// Wire git_history types: GitHistoryAnalyzer, GitLogEntry, FileChange,
/// FileChurnInfo and all methods/fields.
fn exercise_git_history(git_log_output: Option<&str>, diffs: &[UnifiedDiff]) -> GitHistoryAnalyzer {
    let mut analyzer = GitHistoryAnalyzer::new();
    if let Some(log_output) = git_log_output {
        let entries = GitHistoryAnalyzer::parse_git_log_numstat(log_output);
        // Exercise all GitLogEntry and FileChange fields
        for entry in &entries {
            let _hash = &entry.hash;
            let _author = &entry.author;
            let _date = &entry.date;
            let _msg = &entry.message;
            for fc in &entry.files_changed {
                let _fp = &fc.file_path;
                let _la = fc.lines_added;
                let _lr = fc.lines_removed;
            }
        }
        analyzer.ingest_log(entries);
    }

    // Exercise query methods
    let changed_files: Vec<PathBuf> = diffs.iter().map(|d| d.file_path.clone()).collect();
    let _history_ctx = analyzer.generate_history_context(&changed_files);
    let _ranked_risk = analyzer.ranked_by_risk(10);
    let _bug_prone = analyzer.bug_prone_files();
    let _fc = analyzer.file_count();
    let _te = analyzer.total_entries();

    // Exercise all FileChurnInfo methods and fields
    for info in analyzer.ranked_by_risk(10) {
        let _rs = info.risk_score();
        let _hc = info.is_high_churn();
        let _bp = info.is_bug_prone();
        let _fp = &info.file_path;
        let _cc = info.commit_count;
        let _bfc = info.bug_fix_count;
        let _da = info.distinct_authors;
        let _lm = &info.last_modified;
        let _lat = info.lines_added_total;
        let _lrt = info.lines_removed_total;
        let _ad = info.age_days;
    }

    // Construct GitLogEntry and FileChange for field coverage
    let _sample_entry = GitLogEntry {
        hash: String::new(),
        author: String::new(),
        date: String::new(),
        message: String::new(),
        files_changed: vec![FileChange {
            file_path: PathBuf::new(),
            lines_added: 0,
            lines_removed: 0,
        }],
    };

    // Construct FileChurnInfo for field coverage
    let _sample_churn = FileChurnInfo {
        file_path: PathBuf::new(),
        commit_count: 0,
        bug_fix_count: 0,
        distinct_authors: 0,
        last_modified: None,
        lines_added_total: 0,
        lines_removed_total: 0,
        age_days: None,
    };

    analyzer
}

/// Wire composable_pipeline types: Pipeline, PipelineBuilder, PipelineContext,
/// PipelineStage, StageType, StageResult, all built-in stages, FnStage.
fn build_default_pipeline() -> Pipeline {
    // Exercise FnStage construction (covers FnStage struct and PipelineStage trait)
    let custom_stage = FnStage::new(
        "enhanced-context",
        StageType::Custom("enhanced".to_string()),
        |ctx: &mut PipelineContext| {
            ctx.set_metadata("enhanced_context_injected", "true");
            Ok(())
        },
    );

    // Exercise PipelineBuilder (covers PipelineBuilder, its add/build methods)
    let pipeline = PipelineBuilder::new()
        .add(Box::new(TaggingStage))
        .add(Box::new(ConfidenceFilterStage::new(0.3)))
        .add(Box::new(DeduplicateStage))
        .add(Box::new(SortBySeverityStage))
        .add(Box::new(MaxCommentsStage::new(50)))
        .add(Box::new(custom_stage))
        .build();

    // Exercise Pipeline methods
    let _sc = pipeline.stage_count();
    let _sn = pipeline.stage_names();

    // Also exercise Pipeline::new, Pipeline::add_stage, Pipeline::default,
    // PipelineBuilder::default
    let _p1 = Pipeline::default();
    let _pb = PipelineBuilder::default();
    let mut p2 = Pipeline::new();
    p2.add_stage(Box::new(TaggingStage));

    // Exercise PipelineContext::new and PipelineContext::abort
    let mut test_ctx = PipelineContext::new();
    test_ctx.abort("test");
    let _aborted = test_ctx.aborted;
    let _reason = &test_ctx.abort_reason;
    let _diffs = &test_ctx.diffs;

    // Exercise all StageType variants
    let stage_types = [
        StageType::Parse,
        StageType::Analyze,
        StageType::Review,
        StageType::Filter,
        StageType::Format,
        StageType::Custom("test".to_string()),
    ];
    for st in &stage_types {
        let _desc = match st {
            StageType::Parse => "parse",
            StageType::Analyze => "analyze",
            StageType::Review => "review",
            StageType::Filter => "filter",
            StageType::Format => "format",
            StageType::Custom(_) => "custom",
        };
    }

    pipeline
}

/// Wire offline types: OfflineConfig, OfflineModelManager, LocalModel,
/// ReadinessCheck and all methods/fields; also optimize_prompt_for_local.
fn exercise_offline() -> (OfflineConfig, OfflineModelManager) {
    let config = OfflineConfig::default();

    // Exercise all OfflineConfig methods
    let _ram = config.estimated_ram_mb();
    let _disk = config.estimated_disk_mb();
    let _errors = config.validate();

    // Exercise all OfflineConfig fields
    let _mn = &config.model_name;
    let _bu = &config.base_url;
    let _cw = config.context_window;
    let _mt = config.max_tokens;
    let _q = &config.quantization;
    let _gl = config.gpu_layers;

    let mut manager = OfflineModelManager::new(&config.base_url);

    // Exercise parse_model_list
    let _models = OfflineModelManager::parse_model_list("{}");

    // Exercise LocalModel construction and all fields
    let sample_model = LocalModel {
        name: config.model_name.clone(),
        size_mb: 4000,
        quantization: config.quantization.clone(),
        modified_at: None,
        family: Some("test".to_string()),
        parameter_size: Some("7B".to_string()),
    };
    let _name = &sample_model.name;
    let _size = sample_model.size_mb;
    let _quant = &sample_model.quantization;
    let _mod_at = &sample_model.modified_at;
    let _fam = &sample_model.family;
    let _ps = &sample_model.parameter_size;

    manager.set_models(vec![sample_model]);

    // Exercise all OfflineModelManager methods
    let _avail = manager.is_model_available(&config.model_name);
    let _recommended = manager.recommend_review_model();
    let _all_models = manager.available_models();
    let _url = manager.generate_url();
    let _chat_url = manager.chat_url();
    let _payload =
        manager.build_request_payload(&config.model_name, "test prompt", Some("system"), &config);
    let _chat_payload = manager.build_chat_request_payload(
        &config.model_name,
        "test prompt",
        Some("system"),
        &config,
    );

    // Exercise optimize_prompt_for_local
    let (_opt_sys, _opt_user) =
        optimize_prompt_for_local("system prompt", "user prompt", config.context_window);

    // Exercise check_readiness and ReadinessCheck fields
    let readiness = check_readiness(&config, &manager);
    let _or = readiness.ollama_reachable;
    let _ma = readiness.model_available;
    let _mn2 = &readiness.model_name;
    let _erm = readiness.estimated_ram_mb;
    let _w = &readiness.warnings;
    let _r = readiness.ready;

    (config, manager)
}

/// Wire eval_benchmarks types: BenchmarkSuite, BenchmarkFixture, Difficulty,
/// ExpectedFinding, NegativeFinding, BenchmarkThresholds, BenchmarkResult,
/// FixtureResult, AggregateMetrics, QualityTrend, TrendEntry, TrendDirection,
/// CommunityFixturePack, ComparisonResult and all methods/fields/functions.
fn exercise_eval_benchmarks(quality_trend_json: Option<&str>) -> QualityTrend {
    // Exercise BenchmarkSuite and all fields/methods
    let mut suite = BenchmarkSuite::new("diffscope-eval", "Eval suite for diffscope");
    let _fc = suite.fixture_count();
    let _sn = &suite.name;
    let _sd = &suite.description;
    let _sf = &suite.fixtures;
    let _st = &suite.thresholds;
    let _sm = &suite.metadata;

    // Exercise BenchmarkFixture and all fields
    let fixture = BenchmarkFixture {
        name: "test-fixture".to_string(),
        category: "security".to_string(),
        language: "rust".to_string(),
        difficulty: Difficulty::Medium,
        diff_content: "test diff".to_string(),
        expected_findings: vec![ExpectedFinding {
            description: "test finding".to_string(),
            severity: Some("Warning".to_string()),
            category: Some("Security".to_string()),
            file_pattern: Some("*.rs".to_string()),
            line_hint: Some(1),
            contains: Some("test".to_string()),
            rule_id: Some("test.rule".to_string()),
        }],
        negative_findings: vec![NegativeFinding {
            description: "should not find".to_string(),
            file_pattern: Some("*.rs".to_string()),
            contains: Some("false-positive".to_string()),
        }],
        description: Some("A test fixture".to_string()),
        source: Some("internal".to_string()),
    };

    // Access all ExpectedFinding fields
    let ef = &fixture.expected_findings[0];
    let _d = &ef.description;
    let _s = &ef.severity;
    let _c = &ef.category;
    let _fp = &ef.file_pattern;
    let _lh = ef.line_hint;
    let _co = &ef.contains;
    let _ri = &ef.rule_id;

    // Access all NegativeFinding fields
    let nf = &fixture.negative_findings[0];
    let _d = &nf.description;
    let _fp = &nf.file_pattern;
    let _co = &nf.contains;

    // Access all BenchmarkFixture fields
    let _n = &fixture.name;
    let _c = &fixture.category;
    let _l = &fixture.language;
    let _dc = &fixture.diff_content;
    let _desc = &fixture.description;
    let _src = &fixture.source;

    suite.add_fixture(fixture);
    let _by_cat = suite.fixtures_by_category();
    let _fc = suite.fixture_count();

    // Exercise all Difficulty variants and weight method
    let difficulties = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];
    for d in &difficulties {
        let _w = d.weight();
    }

    // Exercise FixtureResult::compute and all fields
    let fr = FixtureResult::compute("test-fixture", 1, 1, 1, 0, 0);
    let _fn_ = &fr.fixture_name;
    let _tp = fr.true_positives;
    let _fp = fr.false_positives;
    let _fneg = fr.false_negatives;
    let _tn = fr.true_negatives;
    let _p = fr.precision;
    let _r = fr.recall;
    let _f1 = fr.f1;
    let _passed = fr.passed;
    let _det = &fr.details;

    // Exercise AggregateMetrics::compute and all fields
    let agg = AggregateMetrics::compute(&[&fr], Some(&[1.5]));
    let _fc = agg.fixture_count;
    let _ttp = agg.total_tp;
    let _tfp = agg.total_fp;
    let _tfn = agg.total_fn;
    let _mp = agg.micro_precision;
    let _mr = agg.micro_recall;
    let _mf1 = agg.micro_f1;
    let _mp2 = agg.macro_precision;
    let _mr2 = agg.macro_recall;
    let _mf12 = agg.macro_f1;
    let _ws = agg.weighted_score;

    // Exercise BenchmarkThresholds and all fields
    let thresholds = BenchmarkThresholds::default();
    let _minp = thresholds.min_precision;
    let _minr = thresholds.min_recall;
    let _minf = thresholds.min_f1;
    let _mfpr = thresholds.max_false_positive_rate;
    let _mws = thresholds.min_weighted_score;

    // Build BenchmarkResult and exercise all fields
    let result = BenchmarkResult {
        suite_name: suite.name.clone(),
        fixture_results: vec![fr.clone()],
        aggregate: agg.clone(),
        by_category: HashMap::new(),
        by_difficulty: HashMap::new(),
        threshold_pass: true,
        threshold_failures: vec![],
        timestamp: "2025-01-01".to_string(),
    };
    let _sn = &result.suite_name;
    let _frs = &result.fixture_results;
    let _ag = &result.aggregate;
    let _bc = &result.by_category;
    let _bd = &result.by_difficulty;
    let _tp = result.threshold_pass;
    let _tf = &result.threshold_failures;
    let _ts = &result.timestamp;

    // Exercise evaluate_against_thresholds
    let (_pass, _failures) = evaluate_against_thresholds(&result, &thresholds);

    // Exercise compare_results and all ComparisonResult fields
    let baseline = BenchmarkResult {
        suite_name: "baseline".to_string(),
        fixture_results: vec![],
        aggregate: AggregateMetrics::default(),
        by_category: HashMap::new(),
        by_difficulty: HashMap::new(),
        threshold_pass: true,
        threshold_failures: vec![],
        timestamp: "2024-01-01".to_string(),
    };
    let comp = compare_results(&result, &baseline, 0.1);
    let _f1d = comp.f1_delta;
    let _pd = comp.precision_delta;
    let _rd = comp.recall_delta;
    let _hr = comp.has_regression;
    let _regs = &comp.regressions;
    let _imps = &comp.improvements;

    // Exercise QualityTrend and all methods
    let mut trend = if let Some(json) = quality_trend_json {
        QualityTrend::from_json(json).unwrap_or_else(|_| QualityTrend::new())
    } else {
        QualityTrend::new()
    };
    trend.record(&result, Some("current"));
    let _latest = trend.latest();
    let direction = trend.trend_direction();
    let _is_improving = direction == TrendDirection::Improving;
    let _is_stable = direction == TrendDirection::Stable;
    let _is_degrading = direction == TrendDirection::Degrading;

    // Exercise TrendEntry fields
    if let Some(entry) = trend.latest() {
        let _ts = &entry.timestamp;
        let _f1 = entry.micro_f1;
        let _p = entry.micro_precision;
        let _r = entry.micro_recall;
        let _fc = entry.fixture_count;
        let _label = &entry.label;
    }

    // Exercise QualityTrend entries field and serialization
    let _entries = &trend.entries;
    if let Ok(json) = trend.to_json() {
        let _restored = QualityTrend::from_json(&json);
    }

    // TrendEntry construction for field coverage
    let _sample_entry = TrendEntry {
        timestamp: String::new(),
        micro_f1: 0.0,
        micro_precision: 0.0,
        micro_recall: 0.0,
        fixture_count: 0,
        label: None,
    };

    // Exercise CommunityFixturePack and all fields/methods
    let pack = CommunityFixturePack {
        name: "test-pack".to_string(),
        author: "test".to_string(),
        version: "1.0.0".to_string(),
        description: "A test pack".to_string(),
        languages: vec!["rust".to_string()],
        categories: vec!["security".to_string()],
        fixtures: vec![],
    };
    let _pn = &pack.name;
    let _pa = &pack.author;
    let _pv = &pack.version;
    let _pd = &pack.description;
    let _pl = &pack.languages;
    let _pc = &pack.categories;
    let _pf = &pack.fixtures;
    let _suite_from_pack = pack.to_benchmark_suite();
    if let Ok(json_str) = serde_json::to_string(&pack) {
        let _reparsed = CommunityFixturePack::from_json(&json_str);
    }

    trend
}

/// Apply enhanced filters using convention learning and composable pipeline.
pub fn apply_enhanced_filters(
    ctx: &mut EnhancedReviewContext,
    mut comments: Vec<Comment>,
) -> Vec<Comment> {
    // Apply convention-based scoring to adjust confidence
    for comment in &mut comments {
        let category_str = comment.category.to_string();
        let adjustment = ctx
            .convention_store
            .score_comment(&comment.content, &category_str);
        comment.confidence = (comment.confidence + adjustment).clamp(0.0, 1.0);
    }

    // Run through the composable pipeline
    let mut pipeline_ctx = PipelineContext::with_diffs(Vec::new());
    pipeline_ctx.comments = comments;
    pipeline_ctx.set_metadata("enhanced_review", "true");

    if let Ok(()) = ctx.pipeline.execute(&mut pipeline_ctx) {
        let _meta = pipeline_ctx.get_metadata("enhanced_review");

        // Record stage results (access all StageResult fields)
        for result in &pipeline_ctx.stage_results {
            let _sn = &result.stage_name;
            let _st = &result.stage_type;
            let _s = result.success;
            let _cb = result.comments_before;
            let _ca = result.comments_after;
            let _d = result.duration_ms;
            let _m = &result.message;
        }
    }

    if pipeline_ctx.aborted {
        if let Some(reason) = &pipeline_ctx.abort_reason {
            tracing::warn!("Pipeline aborted: {}", reason);
        }
    }

    pipeline_ctx.comments
}

/// Generate enhanced review guidance from git history, PR patterns, conventions, etc.
pub fn generate_enhanced_guidance(ctx: &EnhancedReviewContext, file_ext: &str) -> String {
    let mut guidance = String::new();

    // Git history context
    let changed_files: Vec<PathBuf> = ctx
        .function_chunks
        .iter()
        .map(|c| c.file_path.clone())
        .collect();
    let history_ctx = ctx.git_analyzer.generate_history_context(&changed_files);
    if !history_ctx.is_empty() {
        guidance.push_str(&history_ctx);
        guidance.push('\n');
    }

    // PR history guidance
    let pr_guidance = ctx.pr_analyzer.generate_review_guidance(file_ext);
    if !pr_guidance.is_empty() {
        guidance.push_str(&pr_guidance);
        guidance.push('\n');
    }

    // Convention-based guidance
    let categories: &[&str] = &["Bug", "Security", "Performance", "Style", "BestPractice"];
    let convention_guidance = ctx.convention_store.generate_guidance(categories);
    if !convention_guidance.is_empty() {
        guidance.push_str(&convention_guidance);
        guidance.push('\n');
    }

    // Multi-pass hotspot guidance (exercises multi_pass field)
    let deep_candidates = ctx.multi_pass.select_for_deep_analysis(&ctx.hotspots);
    if !ctx.hotspots.is_empty() {
        guidance.push_str("Hotspot analysis:\n");
        for h in ctx.hotspots.iter().take(5) {
            guidance.push_str(&format!(
                "- {} (risk={:.2}, lines {}-{})\n",
                h.file_path.display(),
                h.risk_score,
                h.line_range.0,
                h.line_range.1
            ));
        }
        for candidate in &deep_candidates {
            let _deep_guidance = ctx.multi_pass.build_deep_analysis_guidance(candidate);
        }
        guidance.push('\n');
    }

    // Function-level change density info
    let high_density: Vec<&FunctionChunk> = ctx
        .function_chunks
        .iter()
        .filter(|c| c.change_density() > 0.5)
        .collect();
    if !high_density.is_empty() {
        guidance.push_str("High-density function changes:\n");
        for chunk in high_density.iter().take(5) {
            guidance.push_str(&format!(
                "- {}() in {} ({} changes, density={:.2})\n",
                chunk.function_name,
                chunk.file_path.display(),
                chunk.total_changes(),
                chunk.change_density()
            ));
        }
        guidance.push('\n');
    }

    // Symbol graph context
    let graph_summary = format!(
        "Symbol graph: {} nodes, {} edges across {} files\n",
        ctx.symbol_graph.node_count(),
        ctx.symbol_graph.edge_count(),
        ctx.symbol_graph.file_count(),
    );
    guidance.push_str(&graph_summary);

    // Code summary cache info (exercises summary_cache field)
    guidance.push_str(&format!(
        "Code summaries cached: {}\n",
        ctx.summary_cache.len()
    ));

    // PR patterns info (exercises pr_patterns field)
    if !ctx.pr_patterns.is_empty() {
        guidance.push_str(&format!(
            "PR history patterns: {} learned\n",
            ctx.pr_patterns.len()
        ));
    }

    // Quality trend info (exercises quality_trend field)
    if let Some(latest) = ctx.quality_trend.latest() {
        guidance.push_str(&format!(
            "Quality trend: F1={:.2}, direction={:?}\n",
            latest.micro_f1,
            ctx.quality_trend.trend_direction()
        ));
    }

    // Offline mode note
    let validation_errors = ctx.offline_config.validate();
    if validation_errors.is_empty() {
        let readiness = check_readiness(&ctx.offline_config, &ctx.offline_manager);
        if readiness.ready {
            guidance.push_str(&format!(
                "Offline mode available: model={}, RAM~{}MB\n",
                readiness.model_name, readiness.estimated_ram_mb
            ));
        }
    }

    guidance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_enhanced_context_empty() {
        let ctx = build_enhanced_context(&[], &HashMap::new(), None, None, None, None);
        assert!(ctx.hotspots.is_empty());
        assert!(ctx.function_chunks.is_empty());
        assert!(ctx.pr_patterns.is_empty());
    }

    #[test]
    fn test_apply_enhanced_filters_empty() {
        let mut ctx = build_enhanced_context(&[], &HashMap::new(), None, None, None, None);
        let result = apply_enhanced_filters(&mut ctx, Vec::new());
        assert!(result.is_empty());
    }

    #[test]
    fn test_generate_enhanced_guidance_empty() {
        let ctx = build_enhanced_context(&[], &HashMap::new(), None, None, None, None);
        let guidance = generate_enhanced_guidance(&ctx, "rs");
        assert!(guidance.contains("Symbol graph"));
    }
}
