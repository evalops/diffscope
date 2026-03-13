# Pipeline Refactor TODO (Wave 2)

## Goals

- [x] Extract shared phase contracts so `prepare.rs`, `execution.rs`, and `postprocess.rs` stop depending on `execution.rs` internals.
- [x] Decompose `prepare_file_review_jobs()` into smaller context-assembly and request-building steps.
- [x] Split `session.rs` into service/bootstrap concerns and repo-support concerns.
- [x] Keep the refactor behavior-preserving and validate with `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` after each slice.
- [x] Commit and push regularly after each completed slice.

## Slice 1 — shared phase contracts

- [x] Create `src/review/pipeline/contracts.rs`.
- [x] Move shared phase structs into `contracts.rs`:
  - `PreparedReviewJobs`
  - `FileReviewJob`
  - `ReviewExecutionContext`
  - `ExecutionSummary`
- [x] Update `prepare.rs`, `execution.rs`, `postprocess.rs`, and `pipeline.rs` to use the new contracts module.
- [x] Split `execute_review_jobs()` into dispatch/reduction helpers while preserving behavior.
- [x] Validate, commit, and push.

## Slice 2 — prepare decomposition

- [x] Create `src/review/pipeline/request.rs`.
- [x] Extract request schema and prompt/request-building helpers out of `prepare.rs`.
- [x] Introduce a small per-file preparation carrier type for assembled context and request metadata.
- [x] Split `prepare_file_review_jobs()` into:
  - file eligibility / triage handling
  - context assembly
  - request/job construction
- [x] Validate, commit, and push.

## Slice 3 — session split

- [x] Create `src/review/pipeline/services.rs` for `PipelineServices` and service bootstrapping.
- [x] Create `src/review/pipeline/repo_support.rs` for repo/runtime helpers:
  - diff chunking
  - instruction file detection
  - git log gathering
  - convention store persistence
- [x] Keep `ReviewSession` focused on per-review state in `session.rs`.
- [x] Update imports in `pipeline.rs`, `prepare.rs`, and `postprocess.rs`.
- [x] Validate, commit, and push.

## Wave 3+ — ongoing carving backlog

### Working rules

- [x] Keep refactors behavior-preserving.
- [x] Validate every checkpoint with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `bash scripts/check-workflows.sh`.
- [x] Commit and push after each validated slice.
- [ ] Prefer files that still mix orchestration + parsing/formatting + persistence.
- [ ] Prefer files that remain large after the first round of carving or that keep attracting unrelated edits.

### Recently completed checkpoints

- [x] Split `src/commands/eval/report.rs`.
- [x] Split `src/commands/misc/lsp_check.rs`.
- [x] Split `src/commands/smart_review.rs`.
- [x] Split `src/commands/eval/thresholds/evaluation.rs`.
- [x] Split `src/commands/eval/runner/execute.rs`.
- [x] Split `src/commands/feedback_eval/report/build/aggregate.rs`.

### Immediate queue

- [x] `src/commands/eval/pattern/matching.rs`: split normalized rule-id helpers, matcher predicates, and focused matcher tests.
- [x] `src/commands/eval/metrics/rules.rs`: separate aggregate math, rule counting, and summary reduction helpers.
- [x] `src/commands/doctor/endpoint/inference.rs`: split request building, HTTP execution/error handling, and response parsing.
- [x] `src/commands/feedback_eval/report/build/stats.rs`: split threshold confusion-matrix scoring from bucket primitives.
- [x] `src/commands/doctor/command/display.rs`: separate header/config output, endpoint listing, and inference result rendering.
- [x] `src/commands/doctor/command/run.rs`: separate endpoint discovery, recommendation flow, and test helpers.
- [x] `src/commands/eval/runner/matching.rs`: split required-match search, unexpected-match detection, and rule metric assembly.
- [x] `src/commands/eval/runner/execute/loading.rs`: separate diff resolution from repo-path resolution if it grows again.
- [x] `src/commands/feedback_eval/report/examples.rs`: split ranking helpers from example builders.
- [x] `src/commands/doctor/system.rs`: carve environment probes vs output helpers.

### Commands backlog

- [x] `src/commands/eval/types.rs`: split fixture, pattern, report, and run-option types if churn keeps touching unrelated structs.
- [x] `src/commands/feedback_eval/types.rs`: separate input payload types from report/output types.
- [x] `src/commands/feedback_eval/input/loading.rs`: split format detection from JSON parsing/loading.
- [x] `src/commands/feedback_eval/input/conversion.rs`: split review-session conversion from label normalization helpers.
- [x] `src/commands/pr.rs`: separate summary-only flow, full review flow, and comment-posting orchestration.
- [x] `src/commands/pr/gh.rs`: carve PR resolution, diff fetching, and metadata fetching.
- [x] `src/commands/git/suggest.rs`: split commit-message prompting from PR-title prompting and response extraction.
- [x] `src/commands/review/command.rs`: split review/check/compare entrypoints if they keep diverging.
- [x] `src/commands/misc/feedback/command.rs`: separate file loading/ID normalization from store persistence.
- [x] `src/commands/misc/feedback/apply.rs`: split acceptance/rejection counters from store mutation helpers.
- [x] `src/commands/misc/discussion/command.rs`: separate the interactive loop from single-shot execution.
- [x] `src/commands/misc/discussion/selection.rs`: split file loading/ID repair from selection rules.
- [x] `src/commands/misc/changelog.rs`: evaluate splitting changelog collection from output formatting.
- [x] `src/commands/eval/command.rs`: separate CLI option prep, fixture execution, and report lifecycle.
- [x] `src/commands/feedback_eval/command.rs`: separate input loading from report/output orchestration.

### Review pipeline backlog

- [x] `src/review/pipeline/execution/responses/processing.rs`: split raw response normalization, comment extraction, and merge logic.
- [x] `src/review/pipeline/execution/responses/validation.rs`: separate schema validation from per-comment sanitization.
- [x] `src/review/pipeline/prepare/runner.rs`: split per-diff orchestration, pre-analysis/triage decisions, and progress updates.
- [ ] `src/review/pipeline/context/symbols.rs`: split symbol search, snippet selection, and fallback behavior.
- [ ] `src/review/pipeline/context/related.rs`: separate related-file discovery from ranking/selection.
- [ ] `src/review/pipeline/guidance.rs`: carve guidance assembly, repo-support guidance, and prompt-facing formatting.
- [ ] `src/review/pipeline/session.rs`: split session construction from runtime state transitions.
- [ ] `src/review/pipeline/services.rs`: separate service wiring from optional feature initialization.
- [ ] `src/review/pipeline/file_context/sources.rs`: split repo sources, symbol sources, and supplemental context sources.
- [ ] `src/review/pipeline/comments.rs`: separate comment assembly, filtering, and metadata stamping.
- [ ] `src/review/pipeline/postprocess/dedup.rs`: split duplicate detection, scoring, and merge/rewrite behavior.
- [ ] `src/review/pipeline/postprocess/feedback.rs`: separate store lookups from suppression/annotation decisions.
- [ ] `src/review/pipeline/execution/dispatcher.rs`: carve request scheduling, concurrency control, and result collection.
- [ ] `src/review/pipeline.rs`: keep trimming top-level orchestration as helpers mature.

### Review helper backlog

- [ ] `src/review/rule_helpers/reporting.rs`: separate rendering/formatting from score/rationale helpers.
- [ ] `src/review/rule_helpers/runtime.rs`: split runtime state, caching, and dispatch helpers.
- [ ] `src/review/context_helpers/ranking.rs`: separate scoring inputs from final ranking/selection.
- [ ] `src/review/context_helpers/pattern_repositories.rs`: split pattern loading, matching, and repo fallback logic.
- [ ] `src/review/filters.rs`: carve severity/category filters, suppression filters, and dedup-like passes.
- [ ] `src/review/feedback.rs`: split persistence, semantic examples, and suppression statistics.
- [ ] `src/review/triage.rs`: separate heuristics, explanations, and scoring/reporting.
- [ ] `src/review/compression.rs`: split chunking, summarization, and token-budget planning.
- [ ] `src/review/verification/parser.rs`: separate parser stages and error handling.
- [ ] `src/review/verification/prompt.rs`: split prompt assembly from example selection.

### Core backlog

- [ ] `src/config.rs`: split defaulting, loading, validation, migration, and path-resolution logic.
- [ ] `src/core/comment.rs`: separate model types, ID generation, formatting helpers, and feedback-related transforms.
- [ ] `src/core/symbol_index.rs`: carve command detection, indexing, retrieval, and language-map handling.
- [ ] `src/core/symbol_graph.rs`: separate graph construction, traversal, and serialization helpers.
- [ ] `src/core/semantic.rs`: split semantic extraction, matching, and persistence boundaries.
- [ ] `src/core/pr_summary.rs`: carve stats calculation, prompt generation, response parsing, and diagram support.
- [ ] `src/core/enhanced_review.rs`: split orchestration, prompt building, and response handling.
- [ ] `src/core/eval_benchmarks.rs`: separate benchmark fixtures, thresholds, scoring, and aggregation.
- [ ] `src/core/prompt.rs`: split prompt fragments, model-specific tuning, and reusable builders.
- [ ] `src/core/context.rs`: separate context assembly, provenance handling, and formatting.
- [ ] `src/core/offline.rs`: split endpoint/model probing, metadata parsing, and recommendation helpers.
- [ ] `src/core/function_chunker.rs`: separate parsing, chunk planning, and scoring heuristics.
- [ ] `src/core/agent_tools.rs`: carve tool registry, schema building, and execution adapters.
- [ ] `src/core/agent_loop.rs`: separate loop orchestration, state transitions, and tool/result handling.
- [ ] `src/core/code_summary.rs`: split summary planning, extraction, and formatting.
- [ ] `src/core/changelog.rs`: separate git/history ingestion from final changelog rendering.
- [ ] `src/core/multi_pass.rs`: split pass planning, execution, and result merging.
- [ ] `src/core/composable_pipeline.rs`: separate stage wiring from execution semantics.
- [ ] `src/core/convention_learner.rs`: split store persistence, scoring, and feedback ingestion.
- [ ] `src/core/git_history.rs`: carve log collection, parsing, and summarization.
- [ ] `src/core/diff_parser.rs`: separate unified/text diff parsing, hunk tracking, and post-processing.
- [ ] `src/core/interactive.rs`: split REPL/input loop, commands, and output formatting.

### Server and storage backlog

- [ ] `src/server/api.rs`: split route handlers by domain plus shared request/response helpers.
- [ ] `src/server/state.rs`: separate session state, queueing, and persistence coordination.
- [ ] `src/server/storage_json.rs`: carve file I/O, indexing, migrations, and query helpers.
- [ ] `src/server/storage_pg.rs`: separate SQL-backed persistence domains and query grouping.
- [ ] `src/server/github.rs`: split webhook parsing, API interactions, and review-session orchestration.
- [ ] `src/server/metrics.rs`: separate metric registration from event emission helpers.
- [ ] `src/server/mod.rs`: keep top-level wiring thin as submodules mature.

### Adapters, parsing, and plugins backlog

- [ ] `src/adapters/llm.rs`: split request shaping, retry/policy logic, and response normalization.
- [ ] `src/adapters/openai.rs`: carve request builders, streaming handling, and schema/response parsing.
- [ ] `src/adapters/anthropic.rs`: carve request conversion, retries, and response parsing.
- [ ] `src/adapters/ollama.rs`: separate local model capabilities, request building, and response parsing.
- [ ] `src/adapters/common.rs`: split shared retry/auth/http helpers.
- [ ] `src/parsing/llm_response.rs`: separate fenced-block parsing, comment extraction, and validation.
- [ ] `src/parsing/smart_response.rs`: split structured smart-review parsing from fallbacks.
- [ ] `src/plugins/builtin/secret_scanner.rs`: carve rule loading, scanning, and finding shaping.
- [ ] `src/plugins/builtin/supply_chain.rs`: separate manifest parsing, registry lookups, and finding generation.
- [ ] `src/plugins/builtin/eslint.rs`: split command execution, parser helpers, and finding conversion.
- [ ] `src/plugins/builtin/semgrep.rs`: split command assembly, result parsing, and finding mapping.
- [ ] `src/plugins/builtin/duplicate_filter.rs`: separate fingerprinting from suppression heuristics.
- [ ] `src/plugins/plugin.rs`: split plugin traits/types from execution helpers.

### Output and entrypoint backlog

- [ ] `src/output/format.rs`: separate smart review formatting, patch output, and walkthrough generation.
- [ ] `src/main.rs`: carve CLI wiring by command group and shared config/bootstrap helpers.
- [ ] `src/vault.rs`: split vault discovery, parsing, and maintenance operations.

### Nice-to-have / monitor

- [ ] Revisit freshly split files once they cross roughly 150 LOC again, especially `src/commands/eval/pattern/matching.rs`, `src/commands/eval/metrics/rules.rs`, `src/commands/doctor/endpoint/inference.rs`, and `src/commands/feedback_eval/report/build/stats.rs`.
- [ ] Keep module roots thin: if a root file only re-exports helpers, leave it alone until child files grow again.
- [ ] Favor extracting pure helpers and test-only builders before moving async orchestration.
