# Deep Refactor TODO

## Working Rules

- Keep refactors behavior-preserving.
- Validate every checkpoint with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `bash scripts/check-workflows.sh`.
- Commit and push after each validated slice.
- Prefer extracting pure helpers and formatter/parsing boundaries before moving async orchestration.
- Keep module roots thin; if a root becomes mostly re-exports, let children carry the logic.

## Improvement Queue

- [ ] `src/commands/eval/`
  - Add suite/category/language baseline comparisons instead of only whole-run threshold gates.
  - Expand `review-depth-core` with authz, supply-chain, and async-correctness benchmark packs.
- [ ] `src/commands/feedback_eval/`
  - Correlate feedback calibration with eval-suite category and rule-level performance.

## Immediate Queue

- [ ] `src/core/semantic.rs`
  - Split source-file discovery and excerpt/query builders from index refresh bookkeeping.
  - Split semantic diff retrieval and feedback-example matching from feedback-store maintenance.
- [ ] `src/core/symbol_index.rs`
  - Split LSP command detection and extension scanning from index-building entry points.
  - Split regex-based symbol extraction and dependency-hint parsing from graph/file-summary registration.
  - Split `LspClient` protocol transport from symbol-result decoding and path/URI utilities.
  - Keep `build()` and `build_with_lsp()` as thin orchestration entry points.

## Core Backlog

- [ ] `src/core/semantic.rs`
  - Split semantic chunk hashing/key generation from summary/excerpt assembly.
  - Split changed-range filtering and per-query match scoring from context chunk rendering.
  - Split feedback fingerprint helpers from feedback-store reconciliation.
- [ ] `src/config.rs`
  - Split defaults/model-role conversion from load/deserialize paths.
  - Split env/path resolution from validation/migration logic.
  - Split serialization-focused test helpers from production config code.
- [ ] `src/core/symbol_index.rs`
  - Split language-pattern tables and path candidate expansion from dependency resolution.
  - Split file collection and byte-size filtering from index population.
  - Split symbol graph and reverse-dependency registration from symbol storage.
  - Split LSP symbol collection/range extraction from request/notification plumbing.
- [ ] `src/core/symbol_graph.rs`
  - Split graph construction from traversal/query helpers.
  - Split serialization/persistence helpers from graph algorithms.
- [ ] `src/core/pr_summary.rs`
  - Split stats aggregation, prompt generation, response parsing, and diagram helpers.
- [ ] `src/core/enhanced_review.rs`
  - Split context construction, guidance generation, and response handling.
- [ ] `src/core/eval_benchmarks.rs`
  - Split fixture loading, threshold selection, scoring, and aggregation/reporting.
- [ ] `src/core/prompt.rs`
  - Split prompt fragments, model-specific tuning, and reusable prompt builders.
- [ ] `src/core/context.rs`
  - Split context chunk construction, provenance helpers, and formatting/rendering.
- [ ] `src/core/offline.rs`
  - Split endpoint/model probing, metadata parsing, and recommendation helpers.
- [ ] `src/core/function_chunker.rs`
  - Split parsing, chunk planning, and scoring heuristics.
- [ ] `src/core/agent_tools.rs`
  - Split tool registry/definitions from execution adapters and tool-context helpers.
- [ ] `src/core/agent_loop.rs`
  - Split loop orchestration, state transitions, and tool/result handling.
- [ ] `src/core/code_summary.rs`
  - Split summary planning, extraction, cache helpers, and formatting.
- [ ] `src/core/changelog.rs`
  - Split git/history ingestion from final changelog rendering.
- [ ] `src/core/multi_pass.rs`
  - Split pass planning, execution bookkeeping, and result merging.
- [ ] `src/core/composable_pipeline.rs`
  - Split stage wiring from execution semantics and result transport.
- [ ] `src/core/convention_learner.rs`
  - Split store persistence, scoring, and feedback ingestion helpers.
- [ ] `src/core/git_history.rs`
  - Split log collection, parsing, and summarization.
- [ ] `src/core/diff_parser.rs`
  - Split unified diff parsing, text diff parsing, hunk assembly, and post-processing helpers.
- [ ] `src/core/interactive.rs`
  - Split REPL/input loop, commands, and output formatting.

## Server and Storage Backlog

- [ ] `src/server/api.rs`
  - Split route handlers by domain plus shared request/response and error helpers.
- [ ] `src/server/state.rs`
  - Split session state, queueing, and persistence coordination.
- [ ] `src/server/storage_json.rs`
  - Split file I/O, indexing, migrations, and query helpers.
- [ ] `src/server/storage_pg.rs`
  - Split SQL-backed persistence by domain and query grouping.
- [ ] `src/server/github.rs`
  - Split webhook parsing, API interactions, and review-session orchestration.
- [ ] `src/server/metrics.rs`
  - Split metric registration from event emission helpers.
- [ ] `src/server/mod.rs`
  - Keep top-level wiring thin as submodules mature.

## Adapters, Parsing, and Plugins Backlog

- [ ] `src/adapters/llm.rs`
  - Split request shaping, retry/policy logic, and response normalization.
- [ ] `src/adapters/openai.rs`
  - Split request builders, streaming handling, and schema/response parsing.
- [ ] `src/adapters/anthropic.rs`
  - Split request conversion, retries, and response parsing.
- [ ] `src/adapters/ollama.rs`
  - Split local model capabilities, request building, and response parsing.
- [ ] `src/adapters/common.rs`
  - Split shared retry/auth/http helpers.
- [ ] `src/parsing/llm_response.rs`
  - Split fenced-block parsing, comment extraction, structured JSON handling, and validation.
- [ ] `src/parsing/smart_response.rs`
  - Split structured smart-review parsing from fallback parsing paths.
- [ ] `src/plugins/builtin/secret_scanner.rs`
  - Split rule loading, scanning, and finding shaping.
- [ ] `src/plugins/builtin/supply_chain.rs`
  - Split manifest parsing, registry lookups, and finding generation.
- [ ] `src/plugins/builtin/eslint.rs`
  - Split command execution, parser helpers, and finding conversion.
- [ ] `src/plugins/builtin/semgrep.rs`
  - Split command assembly, result parsing, and finding mapping.
- [ ] `src/plugins/builtin/duplicate_filter.rs`
  - Split fingerprinting from suppression heuristics.
- [ ] `src/plugins/plugin.rs`
  - Split plugin traits/types from execution helpers.

## Output and Entrypoint Backlog

- [ ] `src/output/format.rs`
  - Split smart review formatting, patch output, and walkthrough generation.
- [ ] `src/main.rs`
  - Split CLI wiring by command group and shared config/bootstrap helpers.
- [ ] `src/vault.rs`
  - Split vault discovery, parsing, and maintenance operations.

## Ongoing Watchlist

- [ ] Revisit freshly split files once they cross roughly 150 LOC again, especially `src/review/pipeline/execution/dispatcher/job.rs`, `src/review/pipeline/session/build.rs`, `src/review/pipeline/services/support.rs`, and `src/review/pipeline/postprocess/feedback/lookup.rs`.
- [ ] Keep module roots thin; if a root becomes only re-exports plus tests, leave it alone until children regrow.
