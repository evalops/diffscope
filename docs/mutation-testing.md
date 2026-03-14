# Mutation testing

We use [cargo-mutants](https://github.com/sourcefrog/cargo-mutants) to find gaps in test coverage. Mutants are small code changes (e.g. `*` → `/`, `>` → `>=`); if tests still pass, the mutant is "missed" and the behavior is under-tested.

## Running

```bash
# All mutants (slow; 6000+)
cargo mutants

# One file / path (faster; glob)
cargo mutants -f '*storage_json*'
cargo mutants -f '*state*'
cargo mutants -f '*storage_pg*'
cargo mutants -f '*cost*'
```

CI runs mutation on `*storage_json*` with a timeout; see [CI job](#ci) below. The `*state*` and `*storage_pg*` globs are not run in CI (too many mutants, longer runtime); run locally when auditing those areas.

## Known equivalent / accepted mutants

These mutants are either equivalent (same behavior) or accepted by design. CI does not fail on them.

### cost.rs

| Location | Mutation | Reason |
|----------|----------|--------|
| ~line 43 | `* FALLBACK_PRICE_PER_M` → `/ FALLBACK_PRICE_PER_M` | With `FALLBACK_PRICE_PER_M = 1.0`, `* 1` and `/ 1` are equivalent. No test can distinguish without changing the constant. |

### storage_json.rs

After adding targeted tests (refresh_summary, get_event_stats exact aggregates/single-event/by_repo, prune boundary), many previously missed mutants are now killed. Any remaining misses in these areas are documented here after a run:

- **refresh_summary**: Condition `summary.is_some() \|\| !comments.is_empty()` — tests now assert synthesized summary when comments exist and no summary.
- **get_event_stats**: Percentile index, avg formulas, by_model/by_repo — tests assert exact values (e.g. p50/p95/p99, avg_score).
- **prune**: Boundary `now - started_at > max_age_secs` — test asserts review exactly at boundary is not pruned; max_count test asserts oldest removed.

If new mutants appear in these regions, add assertions that would fail on the wrong operator/formula, or add them to this table with a one-line rationale.

### state.rs and storage_pg.rs (local only)

Mutation on `*state*` and `*storage_pg*` is not in CI. Summary from a local run (interrupted; full run is slow):

| Glob          | Mutants | Notes |
|---------------|---------|--------|
| `*state*`     | ~90     | Many missed: arithmetic in cost/age, `load_reviews_from_disk`, `mark_running` / `complete_review` / `fail_review` / `prune_old_reviews` (no-op or wrong operator), `current_timestamp`, `ReviewEventBuilder`. Killing these would need more unit tests or integration tests that assert side effects. |
| `*storage_pg*`| ~67     | Many missed: `migrate`, `is_empty`, `parse_comment_status`, `save_review` / `get_review` / `list_reviews` / `delete_review` / `save_event` / `list_events` (stubbed return or wrong operator). Would need PostgreSQL-backed tests or contract tests to kill. |

To re-run: `cargo mutants -f '*state*'` and `cargo mutants -f '*storage_pg*'` (allow several minutes each).

## Pre-push vs CI

- **Pre-push** (`.githooks/pre-push`): Runs unit tests, `cargo audit`, web build+test. Does *not* run mutation (too slow for every push).
- **CI mutation job**: Runs `cargo mutants -f '*storage_json*'` on PRs/push to main. Fails if "missed" count increases beyond the baseline in this doc.
- For a quick local push without full checks: `git push --no-verify` (use sparingly; CI will still run).

## CI

The `mutation` job in `.github/workflows/ci.yml` runs mutation on the `storage_json` crate with a timeout. The current **allowed-missed baseline is 15**. Update the baseline in the workflow (and optionally this doc) when you intentionally accept new equivalent mutants (and add them to the table above).
