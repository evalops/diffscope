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
  --trend-file eval/trends/review-depth-core.json \
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
  --label openrouter-smoke \
  --trend-file eval/trends/openrouter-smoke.json
```

Baseline-gated regression check:

```bash
diffscope eval \
  --fixtures eval/fixtures \
  --suite review-depth-core \
  --baseline eval/baselines/review-depth-core.json \
  --max-micro-f1-drop 0.03 \
  --max-suite-f1-drop 0.05 \
  --max-category-f1-drop 0.05 \
  --max-language-f1-drop 0.05 \
  --output eval-report.json
```

Notes:
- Fixtures call the configured model and API provider; they are not deterministic unit tests.
- Treat this set as a baseline and tighten `must_find`/`must_not_find` thresholds over time.
- Benchmark-pack fixtures now preserve category/language/source metadata in the JSON report so live runs can be sliced by dimension.
- Use `--baseline` together with the dimension drop flags when you want regressions to fail on shared suites, categories, or languages instead of only on the whole run.
- Use `--trend-file` with `--label` to append comparable live-run checkpoints into a reusable `QualityTrend` JSON history, including suite/category/language micro-F1 series and verifier-health counters.
