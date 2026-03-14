#!/usr/bin/env bash
# Trigger the "Prepare release" workflow from the CLI.
# Usage: ./scripts/gh-release.sh 0.5.28
# Prerequisites: Cargo.toml and charts/diffscope/Chart.yaml must already be at this version on main.

set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version>" >&2
  echo "Example: $0 0.5.28" >&2
  exit 1
fi

# Strip leading 'v' if present
VERSION="${VERSION#v}"

echo "Triggering Prepare release for v$VERSION..."
gh workflow run "Prepare release" -f version="$VERSION"
echo "Run 'gh run watch' to follow the run."
