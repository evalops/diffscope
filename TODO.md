# Pipeline Refactor TODO

## Goals

- [x] Split `src/review/pipeline/helpers.rs` into focused modules with clearer ownership.
- [x] Introduce shared pipeline session/services types so preparation, execution, and post-processing stop threading long parameter lists.
- [x] Extract post-processing and verification orchestration into a dedicated pipeline submodule.
- [x] Move pipeline tests out of `src/review/pipeline.rs` and colocate them with the modules that own the behavior.
- [x] Keep the refactor behavior-preserving and validate with `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test`.

## Planned module split

### 1. Shared pipeline state

- [x] Create `src/review/pipeline/types.rs` for shared pipeline result/progress types.
- [x] Create `src/review/pipeline/session.rs` for `PipelineServices` and `ReviewSession` plus runtime/repo helpers:
  - local-model optimization detection
  - diff chunking
  - instruction file detection
  - git log gathering
  - convention store path resolution and saving

### 2. Context and guidance

- [x] Create `src/review/pipeline/context.rs` for:
  - symbol extraction from diffs
  - symbol index construction
  - related-file context gathering
  - test-file discovery helper
- [x] Create `src/review/pipeline/guidance.rs` for review guidance assembly.

### 3. Comment preparation and execution support

- [x] Create `src/review/pipeline/comments.rs` for:
  - analyzer finding synthesis
  - diff-line filtering
  - analyzer comment detection
- [x] Keep execution-specific validation and metric aggregation near `execution.rs`.

### 4. Dedicated post-processing stage

- [x] Create `src/review/pipeline/postprocess.rs` for:
  - specialized-pass deduplication
  - plugin post-processing orchestration
  - verification pass orchestration
  - semantic feedback confidence adjustment
  - enhanced feedback adjustment
  - review filtering, enhanced filters, and convention suppression

### 5. Test relocation

- [x] Move symbol/context tests into `context.rs`.
- [x] Move guidance tests into `guidance.rs`.
- [x] Move diff chunking tests into `session.rs`.
- [x] Move response validation tests into `execution.rs`.
- [x] Move comment filtering/dedup tests into `comments.rs` and `postprocess.rs`.
- [x] Move prompt/config ownership tests to `src/core/prompt.rs` and `src/config.rs`.

## Execution checklist

- [x] Rewire `src/review/pipeline.rs` into a thin orchestration facade over the new modules.
- [x] Run validators.
- [x] Review git diff for scope and sensitive data.
- [x] Commit and push the refactor.
