# Release Notes - v0.5.28

📅 **Release Date**: 2026-03-15

## Summary

- **MCP integration:** Expose review and analytics tools over stdio; reusable readiness, fix-loop, and issue replay prompts; fix-agent handoff contract.
- **Fix-loop orchestration:** Fix-until-clean loop orchestrator with policy profiles, telemetry persistence, resumable loops, and recomputable analytics.
- **Symbol graph enhancements:** Graph-ranked semantic retrieval, blast radius annotations, trait contract edge traversal, similar implementation lookup, graph freshness metadata, and repository graph caching.
- **Feedback learning system:** Learn from feedback explanations, preferred comment phrasing, follow-up outcome reinforcement, stale rule decay, dismissed finding tracking, and review history backfill.
- **Eval framework expansion:** Independent auditor benchmark, single-pass vs agent-loop comparison, lifecycle/readiness/fix-loop/external-context regression fixtures, performance/API design/error-handling/infra fixture packs, feedback coverage and verification health gates.
- **Server and API:** Rate-limited API auth with audit logs, signed automation review webhooks, fix-loop policy profiles API.
- **Analytics:** Workload cost breakdown dashboards, pattern repository impact tracking, feedback learning lift measurement, PR review run comparison, context source lift tracking.
- **Operations:** Self-hosted diagnostics and replay evals, explicit review model routing, artifact pruning and trend history retention.
- **CI fixes:** Normalize path separators in blast radius summaries, prebuilt cargo-nextest for rustc version compatibility.

## Full Changelog

[v0.5.27...v0.5.28](https://github.com/evalops/diffscope/compare/v0.5.27...v0.5.28)

---

# Release Notes - v0.5.27

📅 **Release Date**: 2026-03-14

## Summary

- **Natural language review rules (#12):** `review_rules_prose` in config; prose rules section in review guidance.
- **Optional skip deletion-only (#29):** `triage_skip_deletion_only`; `TriageOptions` + `SkipDeletionOnly`; pipeline uses in `prepare_diff_analysis`.
- **LLM parsing (#28):** Single-quoted JSON repair, raw bracket span fallback, escaped apostrophe in values; diff-prefix strip.
- **Dynamic context (#25):** Documented as shipped — `find_enclosing_boundary_line` in context.
- **Test coverage:** Parsing (single-quote object/findings, escaped apostrophe), guidance (empty/single/special-char prose), triage/config defaults and reason strings.

## Full Changelog

[v0.5.26...v0.5.27](https://github.com/evalops/diffscope/compare/v0.5.26...v0.5.27)

---

# Release Notes - v0.5.0

📅 **Release Date**: 2025-06-06

## 📊 Summary

This release brings major new features inspired by CodeRabbit, including PR summary generation, interactive commands, changelog generation, and path-based configuration.

- 🎯 **Total Changes**: 4 major features
- ✨ **New Features**: 4
- 🐛 **Bug Fixes**: 0
- ⚠️ **Breaking Changes**: 0

## ✨ Highlights

### 1. PR Summary Generation
- Generate comprehensive executive summaries for pull requests
- Includes statistics, change analysis, and risk assessment
- Seamless GitHub integration with `diffscope pr --summary`

### 2. Interactive PR Commands
- Respond to PR comments with `@diffscope` commands
- Support for review, ignore, explain, generate, and help commands
- Makes code review more collaborative and interactive

### 3. Changelog & Release Notes Generation
- Automatically parse conventional commits
- Generate professional changelogs with `diffscope changelog`
- Support for both changelog and release notes formats
- Group changes by type with emoji support

### 4. Path-Based Configuration
- Configure review behavior per directory/file pattern
- Set custom focus areas, severity overrides, and prompts
- Support for exclude patterns and path-specific rules
- Example: Elevate all security issues to errors in API endpoints

## 🔧 Configuration

Create a `.diffscope.yml` file to customize behavior:

```yaml
# Path-specific rules
paths:
  "src/api/**":
    focus: [security, validation]
    severity_overrides:
      security: error
```

## 🚀 Getting Started

```bash
# Install the latest version
cargo install diffscope

# Generate a changelog
diffscope changelog --from v0.4.0

# Use path-based configuration
cp .diffscope.yml.example .diffscope.yml
```

## 👥 Contributors

- Jonathan Haas

## 📝 Full Changelog

For detailed changes, see the [full changelog](https://github.com/evalops/diffscope/compare/v0.4.4...v0.5.0).

