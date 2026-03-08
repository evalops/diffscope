# Eval Fixtures

Starter fixture set for `diffscope eval`.

- `repo_regressions/` contains regression-style diffs based on realistic mistakes in this codebase.
- Each fixture can include `rule_id` as a label for rule-level precision/recall metrics.
- Set `require_rule_id: true` on a pattern if the rule id must be emitted by the model for a match.

Run:

```bash
diffscope eval --fixtures eval/fixtures --output eval-report.json
```

Notes:
- Fixtures call the configured model and API provider; they are not deterministic unit tests.
- Treat this set as a baseline and tighten `must_find`/`must_not_find` thresholds over time.
