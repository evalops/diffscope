#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

if [ ! -d ".github/workflows" ]; then
    exit 0
fi

if command -v actionlint >/dev/null 2>&1; then
    actionlint
    exit 0
fi

python3 - <<'PY'
from pathlib import Path
import re
import sys

workflow_dir = Path(".github/workflows")
paths = sorted(workflow_dir.glob("*.yml")) + sorted(workflow_dir.glob("*.yaml"))
secret_if_pattern = re.compile(r"^\s*if:\s*\$\{\{\s*secrets\.", re.MULTILINE)

failures = []
for path in paths:
    contents = path.read_text(encoding="utf-8")
    if secret_if_pattern.search(contents):
        failures.append(str(path))

if failures:
    print("ERROR: GitHub Actions does not allow the `secrets` context directly in `if:` expressions.")
    print("Use a prior step to expose a boolean output instead.")
    print("")
    print("Affected workflow files:")
    for path in failures:
        print(f" - {path}")
    print("")
    print("Install `actionlint` for full local workflow lint coverage.")
    sys.exit(1)
PY
