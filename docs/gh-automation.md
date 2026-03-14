# GitHub CLI (gh) automation

Use `gh` from the terminal for issues, PRs, releases, and CI. See also [ROADMAP.md](ROADMAP.md) (issue labels, filters) and [release-process.md](release-process.md) (release steps).

## Prerequisites

- Install: <https://cli.github.com/>
- Auth: `gh auth login`

## Issues

```bash
# List (default: open, 30)
gh issue list

# By label
gh issue list --label "priority: high"
gh issue list --label "area: review-pipeline"

# Search
gh issue list --search "verification OR RAG"
gh issue list --search "no:assignee sort:created-asc"

# View / edit
gh issue view 32
gh issue edit 32 --add-label "priority: medium"
gh issue edit 32 --remove-label "help wanted"
gh issue close 32 --comment "Fixed in #44"
```

## Pull requests

```bash
# List
gh pr list
gh pr list --state merged --limit 10

# Create (uses PR template)
gh pr create --base main --title "feat: something" --body "Summary here. Closes #28"

# Status and merge
gh pr view 46
gh pr checks 46              # CI status
gh pr checks 46 --watch      # Watch until done
gh pr merge 46 --squash
gh pr merge 46 --merge
```

## Releases and workflows

```bash
# Trigger Prepare release (creates tag, runs Release workflow)
gh workflow run "Prepare release" -f version=0.5.28

# List workflow runs
gh run list
gh run list --workflow "Release"

# Watch latest run
gh run watch

# View run details and logs
gh run view
gh run view 12345 --log
```

## One-line release from terminal

After version and RELEASE_NOTES are merged to main:

```bash
./scripts/gh-release.sh 0.5.28
# or
gh workflow run "Prepare release" -f version=0.5.28
```

## CI and runs

- **Lint**: actionlint, frontend build, `cargo fmt`, `cargo clippy`.
- **Test**: `cargo nextest run` on ubuntu/macos/windows.
- **Coverage**: `cargo llvm-cov` → upload to Codecov. Set repo secret `CODECOV_TOKEN` to enable uploads; CI does not fail if missing.
- **PR merge comment**: When a PR is merged, a workflow comments on each issue linked via “Closes #N”, “Fixes #N”, or “Resolves #N” in the PR body.

```bash
# Download artifacts from latest run
gh run download

# Re-run failed jobs
gh run rerun <run-id> --failed
```

## Quick reference

| Task              | Command |
|-------------------|--------|
| Open issues by label | `gh issue list -l "priority: high"` |
| Create PR         | `gh pr create -B main -t "title" -b "body"` |
| Merge PR           | `gh pr merge <number> --squash` |
| Close issue with comment | `gh issue close <number> --comment "Done in #44"` |
| Run Prepare release | `gh workflow run "Prepare release" -f version=0.5.28` |
| Watch CI           | `gh pr checks <number> --watch` |
