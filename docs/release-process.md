# Release process

This doc and GitHub Actions make releases repeatable with minimal manual steps.

## One-time per release (on main)

1. **Bump version** in a PR (or directly on main):
   - `Cargo.toml` → `[package].version = "0.5.28"`
   - `charts/diffscope/Chart.yaml` → `appVersion: "0.5.28"`
2. **Update release notes** in the same PR (or a follow-up):
   - In `RELEASE_NOTES.md`, add a new section at the top:
     ```markdown
     # Release Notes - v0.5.28
     📅 **Release Date**: YYYY-MM-DD
     ## Summary
     - Bullet points for this release.
     ## Full Changelog
     [v0.5.27...v0.5.28](https://github.com/evalops/diffscope/compare/v0.5.27...v0.5.28)
     ---
     ```
   - Optionally update `docs/ROADMAP.md` “Shipped” section.
3. **Merge to main** (e.g. “chore: bump version to 0.5.28”).

## Create the release (automated)

4. **Run the “Prepare release” workflow**
   - In the repo: **Actions** → **Prepare release** → **Run workflow**.
   - Enter the **version** (e.g. `0.5.28`). Must match `Cargo.toml` and `Chart.yaml` on main.
   - The workflow creates tag `v0.5.28`, pushes it, and triggers the **Release** workflow.
5. **Release workflow** (runs on tag push):
   - Verifies `Cargo.toml` and Chart `appVersion` match the tag.
   - Extracts the section for this version from `RELEASE_NOTES.md` and uses it in the GitHub Release body.
   - Builds binaries for Linux/macOS/Windows, builds Docker image, creates the release and uploads assets.

## PR workflow (reminder)

- Use the **PR template** (Summary, Test plan, **Closes #N**).
- Linking “Closes #28” in the PR body auto-closes the issue when the PR is merged.

## Automation summary

| Step | Automation |
|------|------------|
| Version sync (CI) | `check_version_sync.py` fails if `Cargo.toml` is behind latest tag. |
| Tag and trigger release | **Prepare release** workflow (manual run with version input). From CLI: `./scripts/gh-release.sh 0.5.28` or `gh workflow run "Prepare release" -f version=0.5.28`. See [gh-automation.md](gh-automation.md). |
| Release body | **Release** workflow reads `RELEASE_NOTES.md` for the tagged version. |
| Binaries + Docker | **Release** workflow builds and uploads. |
| Issue close | Add “Closes #N” in PR body. |
