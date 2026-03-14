# DiffScope roadmap

Enhancement backlog derived from open GitHub issues. Use **gh CLI** for triage and filtering.

## gh CLI workflow

```bash
# List open issues (default: 30)
gh issue list

# Filter by label
gh issue list --label "priority: high"
gh issue list --label "area: review-pipeline"

# Search (advanced filters)
gh issue list --search "no:assignee sort:created-asc"
gh issue list --search "verification OR RAG"

# View one issue
gh issue view 32

# Add/remove labels (labels must exist: gh label create "name" --color "hex" --description "desc")
gh issue edit 32 --add-label "priority: medium,area: plugins"
gh issue edit 32 --remove-label "help wanted"

# Add to project (requires project scope)
gh issue edit 32 --add-project "Roadmap"
```

Create labels once: `priority: high`, `priority: medium`, `priority: low`, `area: review-pipeline`, `area: plugins`, `area: platform`.

---

## Priority: High / Critical

| # | Title | Area | Notes |
|---|--------|------|--------|
| [27](https://github.com/evalops/diffscope/issues/27) | Embedding-based false positive filtering from developer feedback | review | Greptile-style: block if similar to 3+ downvoted; pass if 3+ upvoted. Per-team. |
| [23](https://github.com/evalops/diffscope/issues/23) | Verification pass to catch hallucinations | review | Second LLM pass validates findings vs actual code; drop below score. **Partially done** (verification in config/pipeline). |
| [22](https://github.com/evalops/diffscope/issues/22) | Embedding-based RAG pipeline with function-level chunking | review | NL summaries + pgvector; highest leverage for catch rate. |
| [24](https://github.com/evalops/diffscope/issues/24) | Agentic review loop with tool use | review | Tools: search_code, read_file, search_symbols, git_log, git_blame. |
| [21](https://github.com/evalops/diffscope/issues/21) | Multi-agent architecture: review + fix + test agents | platform | Fix Agent, Test Agent, Triage Agent; orchestration. |
| [10](https://github.com/evalops/diffscope/issues/10) | Deep codebase graph context in review prompts | review | Pre-index repo; inject callers/callees/contracts into prompt. |

## Priority: Medium

| # | Title | Area | Notes |
|---|--------|------|--------|
| [32](https://github.com/evalops/diffscope/issues/32) | In-sandbox linter/analyzer execution | plugins | ToolSandbox + AnalysisTool trait; Clippy, Ruff, Gitleaks, ShellCheck, actionlint. |
| [31](https://github.com/evalops/diffscope/issues/31) | AST-based structural pattern matching (ast-grep) | plugins | Pre-analyzer plugin; coderabbitai/ast-grep-essentials rules. |
| [30](https://github.com/evalops/diffscope/issues/30) | Adaptive patch compression for large PRs | review | Full → Compressed → Clipped → MultiCall; token budget. |
| [29](https://github.com/evalops/diffscope/issues/29) | File triage: classify before expensive review | review | NeedsReview vs Cosmetic/ConfigChange/TestOnly; heuristic + cheap model. |
| [28](https://github.com/evalops/diffscope/issues/28) | Robust LLM output parsing with fallback strategies | review | **In progress:** code-block extraction, trailing commas, diff-prefix strip. More fallbacks in parsing/llm_response.rs. |
| [25](https://github.com/evalops/diffscope/issues/25) | Dynamic context: enclosing function/class boundary | review | Search upward for boundary; reuse symbol_index patterns. |

## Priority: Low / Tier 2–3

| # | Title | Area | Notes |
|---|--------|------|--------|
| [20](https://github.com/evalops/diffscope/issues/20) | Built-in secrets detection scanner | plugins | **Done:** `secret_scanner.rs` with AWS, GitHub, Slack, JWT, PEM, etc. |
| [19](https://github.com/evalops/diffscope/issues/19) | Compliance review command | platform | `diffscope compliance` — security, secrets, rules, ticket, licenses, duplication. |
| [18](https://github.com/evalops/diffscope/issues/18) | Authentication layer for web UI (SSO/SAML) | platform | Basic → OAuth/OIDC → SAML, RBAC. |
| [17](https://github.com/evalops/diffscope/issues/17) | GitLab, Azure DevOps, Bitbucket support | platform | GitPlatform trait; GitLab first. |
| [15](https://github.com/evalops/diffscope/issues/15) | Auto-generate Mermaid sequence diagrams in PRs | review | Symbol graph → sequence diagram in PR comment. |
| [14](https://github.com/evalops/diffscope/issues/14) | VS Code / IDE extension | platform | Staged/unstaged review, inline diagnostics, Quick Fix. |
| [13](https://github.com/evalops/diffscope/issues/13) | Ticket validation (Jira/Linear/GitHub Issues) | platform | Fetch ticket, validate acceptance criteria. |
| [12](https://github.com/evalops/diffscope/issues/12) | Natural language custom review rules | review | Prose rules in YAML or `.diffscope-rules`. |
| [11](https://github.com/evalops/diffscope/issues/11) | PR analytics and review metrics dashboard | platform | Persist to PG, aggregation, dashboard. |
| [9](https://github.com/evalops/diffscope/issues/9) | Structured PR description auto-generation | platform | `pr describe` — walkthrough, labels, breaking, testing notes. |

---

## Shipped (recent)

- **Natural language rules (#12):** `review_rules_prose: [ "Rule one", "Rule two" ]` in config; injected as "Custom rules (natural language)" bullets into review guidance. Tests: `test_config_deserialize_review_rules_prose_from_yaml`, `build_review_guidance_includes_prose_rules`.
- **Triage skip deletion-only (#29):** `triage_skip_deletion_only: true` in config; when true, deletion-only diffs get `SkipDeletionOnly` and skip expensive review. Default false. Tests: `test_triage_deletion_only_with_skip_true_returns_skip_deletion_only`, config deserialize.
- **LLM parsing (#28):** Repair candidate for diff-style line prefixes (`+` on each line) in `repair_json_candidates`; test `parse_json_with_diff_prefix_artifact`.
- **Secrets (#20):** Built-in secret scanner in `plugins/builtin/secret_scanner.rs`.
- **Verification (#23):** Verification pass and config (verification.*) in pipeline.

---

## References

- [GitHub CLI manual](https://cli.github.com/manual/)
- [Advanced issue search](https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests)
- [gh issue edit](https://cli.github.com/manual/gh_issue_edit) — labels, assignees, projects, milestones
