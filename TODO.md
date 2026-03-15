# Deep Research Improvement Roadmap

This roadmap is derived from deep research into Greptile's public docs, blog, MCP surface, self-hosted architecture, and GitHub repos, then mapped onto DiffScope's current architecture and gaps.

## Research Signals

- Greptile treats review as a full-codebase intelligence product, not just a PR comment bot.
- Their learning loop is explicit: thumbs, replies, and addressed/not-addressed outcomes reshape future comments.
- Their `v3` review flow is agentic and tool-using, not a rigid single-pass flowchart.
- They productize workflow state: unresolved comments, review completeness, weekly reports, merge readiness.
- They pull in external intent via Jira/Notion/Docs and cross-repo context via pattern repositories.
- They expose review operations back into IDE/agent workflows through MCP and skills.
- They sell an operational platform: self-hosted, queued workflows, analytics, and enterprise controls.

## Working Rules

- Keep changes additive and behavior-preserving unless an item explicitly requires workflow changes.
- Validate each checkpoint with `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `bash scripts/check-workflows.sh`, `npm --prefix web run lint`, `npm --prefix web run build`, and `npm --prefix web run test` when frontend code changes.
- Commit and push after each validated slice.
- Prefer turning existing primitives into first-class product surfaces before inventing brand new subsystems.
- Optimize for independent validation, tight feedback loops, and high-signal comments over superficial feature parity.

## 1. Feedback, Memory, and Outcomes

1. [x] Add first-class comment outcome states beyond thumbs: `new`, `accepted`, `rejected`, `addressed`, `stale`, `auto_fixed`.
2. [x] Infer "addressed by later commit" by diffing follow-up pushes against the original commented lines.
3. [x] Feed addressed/not-addressed outcomes into the reinforcement store alongside thumbs.
4. [x] Separate false-positive rejections from "valid but won't fix" dismissals in stored feedback.
5. [ ] Weight reinforcement by reviewer role or trust level when GitHub identity is available.
6. [x] Add rule-level reinforcement decay so old team preferences do not dominate forever.
7. [x] Add path-scoped reinforcement buckets so teams can prefer different standards in `tests/`, `scripts/`, and production code.
8. [ ] Persist explanation text from follow-up feedback replies and mine it into reusable review guidance.
9. [x] Learn "preferred phrasing" for accepted comments so comment tone and specificity improve over time.
10. [x] Backfill existing stored reviews into the new outcome-aware feedback store for cold-start reduction.

## 2. Review Lifecycle and Merge Readiness

11. [x] Track unresolved vs resolved findings for PR reviews as a first-class lifecycle state.
12. [x] Add review completeness metrics: total findings, acknowledged findings, fixed findings, stale findings.
13. [x] Compute merge-readiness summaries for GitHub PR reviews using severity, unresolved count, and verification state.
14. [x] Add stale-review detection when new commits land after the latest completed review.
15. [x] Show "needs re-review" state in review detail and history pages for incremental PR workflows.
16. [x] Distinguish informational findings from blocking findings in lifecycle and readiness calculations.
17. [x] Add "critical blockers" summary cards for unresolved `Error` and `Warning` comments.
18. [x] Add per-PR readiness timelines showing when a review became mergeable.
19. [x] Store resolution timestamps for findings so mean-time-to-fix can be measured.
20. [x] Add CLI and API surfaces to query PR readiness without opening the web UI.

## 3. Agentic Validation Loops

21. [x] Build a first-class `fix until clean` loop that can run review, apply fixes, rerun review, and stop on convergence.
22. [ ] Reuse the existing DAG runtime to model iterative review/fix loops as resumable workflow nodes.
23. [x] Add a max-iteration policy and loop budget controls for autonomous review convergence.
24. [x] Add "issue replay" prompts that hand unresolved findings back to a coding agent with file-local context.
25. [x] Add a handoff contract from reviewer findings to fix agents with rule IDs, evidence, and suggested diffs.
26. [x] Persist loop-level telemetry: iterations, fixes attempted, findings cleared, findings reopened.
27. [x] Add "challenge the finding" verification loops where a validator tries to falsify a suspected issue before keeping it.
28. [x] Add caching between iterations so repeated codebase retrieval and verification runs are cheaper.
29. [x] Allow loop policies to differ by profile: conservative auditor, high-autonomy fixer, or report-only.
30. [x] Add eval fixtures specifically for loop convergence and reopened-issue regressions.

## 4. Code Graph and Repository Intelligence

31. [x] Turn the current symbol graph into a persisted repository graph with durable storage and reload support.
32. [x] Add caller/callee expansion APIs for multi-hop impact analysis from changed symbols.
33. [x] Add contract edges between interfaces, implementations, and API endpoints.
34. [x] Add "similar implementation" lookup so repeated patterns and divergences are explicit.
35. [x] Add cross-file blast-radius summaries to findings when a change affects many callers.
36. [x] Add graph freshness/version metadata so reviews know whether they are using stale repository intelligence.
37. [x] Add graph-backed ranking of related files before semantic RAG retrieval.
38. [x] Add graph query traces to `dag_traces` or review artifacts for explainability and debugging.
39. [x] Add graph-aware eval fixtures that require multi-hop code understanding to pass.
40. [x] Split `src/core/symbol_graph.rs` into construction, persistence, traversal, and ranking modules as it grows.

## 5. External Context and Pattern Repositories

41. [x] Surface pattern repository sources in the Settings UI with validation and defaults.
42. [x] Surface review rule file sources in the Settings UI instead of requiring config edits by hand.
43. [x] Add structured UI editing for custom context notes, files, and scopes.
44. [x] Add per-path scoped review instructions in the Settings UI for common repo areas.
45. [x] Support Jira/Linear issue context ingestion for PR-linked reviews.
46. [ ] Support document-backed context ingestion for design docs, RFCs, and runbooks.
47. [ ] Add explicit "intent mismatch" review checks comparing PR changes to ticket acceptance criteria.
48. [x] Add review artifacts that show which external context sources influenced a finding.
49. [x] Add tests for pattern repository resolution across local paths, Git URLs, and broken sources.
50. [ ] Add analytics on which context sources actually improve acceptance and fix rates.

## 6. Review UX and Workflow Integration

51. [x] Add visible accepted/rejected/dismissed badges to comments throughout the UI, not just icon state.
52. [x] Add comment grouping by unresolved, fixed, stale, and informational sections in `ReviewView`.
53. [x] Add a "show only blockers" mode for large reviews.
54. [x] Add keyboard actions for thumbs, resolve, and jump-to-next-finding workflows.
55. [x] Add file-level readiness summaries in the diff sidebar.
56. [x] Add lifecycle-aware PR summaries that explain what still blocks merge.
57. [x] Add a "train the reviewer" callout when thumbs coverage on a review is low.
58. [x] Add review-change comparisons so users can diff one review run against the next on the same PR.
59. [x] Add better surfacing for incremental PR reviews so users know when only the delta was reviewed.
60. [x] Add discussion workflows that can convert repeated human comments into candidate rules or context snippets.

## 7. Analytics, Reporting, and Quality Dashboards

61. [x] Add feedback coverage metrics: percent of findings with thumbs or explicit disposition.
62. [x] Add acceptance/rejection trend lines over time for recent reviews.
63. [x] Add top accepted categories/rules and top rejected categories/rules to Analytics.
64. [x] Add unresolved blocker counts per repository and per PR.
65. [x] Add review completeness and mean-time-to-resolution charts.
66. [x] Add feedback-learning effectiveness metrics: did reranked findings get higher acceptance after rollout?
67. [x] Add pattern-repository utilization analytics showing when extra context actually affected findings.
68. [x] Add eval-vs-production dashboards comparing benchmark strength against real-world acceptance.
69. [x] Add drill-downs from trend charts directly into the affected reviews, findings, and rules.
70. [x] Add exportable JSON/CSV reports for review quality, lifecycle, and reinforcement metrics.

## 8. APIs, Automation, and MCP-Like Surfaces

71. [x] Expose unresolved/resolved comment search through the HTTP API.
72. [x] Expose PR readiness through the HTTP API for CI and agent integrations.
73. [x] Add API endpoints to fetch learned rules, attention gaps, and top rejected patterns.
74. [x] Add machine-friendly APIs to fetch findings grouped by severity, file, and lifecycle state.
75. [x] Add a "trigger re-review" API that reuses existing PR metadata and loop policy.
76. [x] Add APIs for comment resolution and lifecycle updates, not just thumbs.
77. [x] Add an MCP server for DiffScope with review, analytics, and rule-management tools.
78. [x] Add reusable agent skills/workflows for checking PR readiness and running fix loops.
79. [x] Add signed webhook or event-stream integration for downstream automation consumers.
80. [x] Add rate-limited API auth and audit trails for automation-heavy deployments.

## 9. Infra, Self-Hosting, and Enterprise Operations

81. [x] Split `src/server/api.rs` by domain so the growing platform API stays maintainable.
82. [x] Split `src/server/state.rs` into session lifecycle, persistence, progress, and GitHub coordination modules.
83. [x] Add queue depth and worker saturation metrics for long-running review and eval jobs.
84. [x] Add retention policies for review artifacts, eval artifacts, and trend histories.
85. [x] Add storage migrations for richer comment lifecycle and reinforcement schemas.
86. [ ] Add deployment docs for self-hosted review + analytics + trend retention setups.
87. [ ] Add secret-management guidance and validation for multi-provider enterprise installs.
88. [ ] Add background jobs for recomputing analytics after schema or scoring changes.
89. [ ] Add cost dashboards by provider/model/role for review, verification, and eval workloads.
90. [ ] Add failure forensics bundles for self-hosted users when review or eval jobs degrade.

## 10. Eval, Benchmarking, and Model Governance

91. [x] Add eval fixtures for external-context alignment, not just diff-local correctness.
92. [x] Add eval fixtures for merge-readiness judgments and unresolved-blocker classification.
93. [x] Add eval fixtures for addressed-vs-stale finding lifecycle inference.
94. [x] Add eval fixtures for multi-hop graph reasoning across call chains and contract edges.
95. [x] Add eval runs that compare single-pass review against agentic loop review.
96. [ ] Add production replay evals using anonymized accepted/rejected review outcomes.
97. [x] Add leaderboard reporting for reviewer usefulness metrics, not just precision/recall.
98. [x] Add regression gates for feedback coverage, verifier health, and lifecycle-state accuracy.
99. [ ] Add model-routing policies that explicitly separate generation, verification, and auditing roles.
100. [x] Publish a repeatable "independent auditor" benchmark story in the UI and CLI so DiffScope's differentiation is measurable.

## Current Execution Slice

- [x] Rewrite this roadmap into the active backlog and keep it updated as slices ship.
- [x] Productize the learning loop in Analytics with reaction coverage and acceptance trends.
- [x] Surface repository rule sources and pattern repository sources in Settings.
- [x] Ship first-pass finding lifecycle state and lightweight merge readiness through the backend, API, CLI summaries, and review UI.
- [x] Make merge readiness verification-aware and surface stale PR reviews as needs re-review in history/detail views.
- [x] Make stale-review detection compare PR head SHAs so same-head reruns do not look stale.
- [x] Split open findings into blocking vs informational buckets and surface critical blocker cards in review detail.
- [x] Add PR readiness query surfaces in the CLI and HTTP API for non-UI workflows.
- [x] Surface lifecycle-aware PR readiness summaries in the GitHub PR detail workflow.
- [x] Surface unresolved blocker counts in repo and PR GitHub discovery views.
- [x] Add a blocker-only review mode that narrows large reviews to open Error and Warning findings.
- [x] Add file-level readiness summaries to the review diff sidebar.
- [x] Add visible feedback badges on comments so accepted and rejected states are not icon-only.
- [x] Add a train-the-reviewer callout on review detail when thumbs coverage is low.
- [x] Add structured custom context and per-path instruction editors to the Settings review context workflow.
- [x] Expose latest-review PR comment search with unresolved, resolved, and dismissed lifecycle filters through the API.
- [x] Close TODO drift for existing comment lifecycle update APIs now that read and write surfaces are both shipped.
- [x] Make PR readiness explicitly call out incremental review coverage when newer commits were not part of the latest pass.
- [x] Add grouped PR findings API responses for severity, file, and lifecycle automation workflows.
- [x] Add Analytics JSON/CSV exports covering review quality, lifecycle, and reinforcement metrics.
- [x] Add learned-rules, attention-gap, and rejected-pattern analytics API endpoints for automation consumers.
- [x] Add a PR re-review API that reuses stored review metadata and posting policy.
- [x] Add pattern repository resolution coverage for repo-local directories, Git sources, and broken-source skips.
- [x] Group ReviewView list-mode findings into unresolved, fixed, stale, and informational sections.
- [x] Add ReviewView keyboard shortcuts for next-finding navigation plus accept/reject/resolve actions.
- [x] Add path-scoped reinforcement buckets so feedback can distinguish `tests/**`, `scripts/**`, and broader code areas.
- [x] Add review completeness metrics across summaries, PR readiness, and ReviewView surfaces.
- [x] Add per-PR readiness timelines that preserve historical mergeability checkpoints.
- [x] Commit and push each validated checkpoint before moving to the next epic.
