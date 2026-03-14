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

CI runs mutation on `*storage_json*` with a timeout; see [CI job](#ci) below.

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

## Pre-push vs CI

- **Pre-push** (`.githooks/pre-push`): Runs unit tests, `cargo audit`, web build+test. Does *not* run mutation (too slow for every push).
- **CI mutation job**: Runs `cargo mutants -f '*storage_json*'` on PRs/push to main. Fails if "missed" count increases beyond the baseline in this doc.
- For a quick local push without full checks: `git push --no-verify` (use sparingly; CI will still run).

## CI

The `mutation` job in `.github/workflows/ci.yml` runs mutation on the `storage_json` crate with a timeout. Update the "allowed missed" baseline in the job when you intentionally accept new equivalent mutants (and add them to the table above).
