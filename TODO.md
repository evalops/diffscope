# Pipeline Refactor TODO (Wave 2)

## Goals

- [ ] Extract shared phase contracts so `prepare.rs`, `execution.rs`, and `postprocess.rs` stop depending on `execution.rs` internals.
- [ ] Decompose `prepare_file_review_jobs()` into smaller context-assembly and request-building steps.
- [ ] Split `session.rs` into service/bootstrap concerns and repo-support concerns.
- [ ] Keep the refactor behavior-preserving and validate with `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` after each slice.
- [ ] Commit and push regularly after each completed slice.

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
- [ ] Validate, commit, and push.

## Slice 3 — session split

- [ ] Create `src/review/pipeline/services.rs` for `PipelineServices` and service bootstrapping.
- [ ] Create `src/review/pipeline/repo_support.rs` for repo/runtime helpers:
  - diff chunking
  - instruction file detection
  - git log gathering
  - convention store persistence
- [ ] Keep `ReviewSession` focused on per-review state in `session.rs`.
- [ ] Update imports in `pipeline.rs`, `prepare.rs`, and `postprocess.rs`.
- [ ] Validate, commit, and push.
