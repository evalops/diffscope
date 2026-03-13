# Eval Fixtures

Starter fixture set for `diffscope eval`.

- `repo_regressions/` contains regression-style diffs based on realistic mistakes in this codebase.
- Each fixture can include `rule_id` as a label for rule-level precision/recall metrics.
- Set `require_rule_id: true` on a pattern if the rule id must be emitted by the model for a match.

Run:

```bash
diffscope eval --fixtures eval/fixtures --output eval-report.json
```

Filter and label a deeper suite run:

```bash
diffscope eval \
  --fixtures eval/fixtures \
  --suite review-depth-core \
  --max-fixtures 3 \
  --label smoke \
  --output eval-report.json
```

Live OpenRouter example:

```bash
OPENROUTER_API_KEY=... \
diffscope \
  --adapter openrouter \
  --base-url https://openrouter.ai/api/v1 \
  --model anthropic/claude-opus-4.1 \
  eval \
  --fixtures eval/fixtures \
  --suite review-depth-core \
  --max-fixtures 3 \
  --label openrouter-smoke
```

Notes:
- Fixtures call the configured model and API provider; they are not deterministic unit tests.
- Treat this set as a baseline and tighten `must_find`/`must_not_find` thresholds over time.
- Benchmark-pack fixtures now preserve category/language/source metadata in the JSON report so live runs can be sliced by dimension.
