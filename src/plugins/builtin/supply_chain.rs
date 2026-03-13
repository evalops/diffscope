use crate::core::comment::{Category, Severity};
use crate::core::{LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;

/// Manifest file types we know how to analyze.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ManifestKind {
    CargoToml,
    CargoLock,
    PackageJson,
    PackageLockJson,
    YarnLock,
    PnpmLock,
    RequirementsTxt,
    PyprojectToml,
    Pipfile,
    GoMod,
    GoSum,
    Gemfile,
    GemfileLock,
    ComposerJson,
    GithubActions,
}

impl ManifestKind {
    fn from_path(path: &std::path::Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        match name {
            "Cargo.toml" => Some(Self::CargoToml),
            "Cargo.lock" => Some(Self::CargoLock),
            "package.json" => Some(Self::PackageJson),
            "package-lock.json" => Some(Self::PackageLockJson),
            "yarn.lock" => Some(Self::YarnLock),
            "pnpm-lock.yaml" => Some(Self::PnpmLock),
            "go.mod" => Some(Self::GoMod),
            "go.sum" => Some(Self::GoSum),
            "Gemfile" => Some(Self::Gemfile),
            "Gemfile.lock" => Some(Self::GemfileLock),
            "composer.json" => Some(Self::ComposerJson),
            "Pipfile" => Some(Self::Pipfile),
            "pyproject.toml" => Some(Self::PyprojectToml),
            _ => {
                if name.starts_with("requirements") && name.ends_with(".txt") {
                    return Some(Self::RequirementsTxt);
                }
                // GitHub Actions workflows
                if path
                    .to_str()
                    .map(|s| s.contains(".github/workflows/"))
                    .unwrap_or(false)
                    && (name.ends_with(".yml") || name.ends_with(".yaml"))
                {
                    return Some(Self::GithubActions);
                }
                None
            }
        }
    }
}

/// A supply-chain finding.
struct SupplyChainFinding {
    rule_id: &'static str,
    severity: &'static str,
    description: String,
    line_number: usize,
}

// ── Regex patterns for supply-chain signals ─────────────────────────────────

// Cargo.toml
static CARGO_GIT_DEP: Lazy<Regex> = Lazy::new(|| Regex::new(r#"git\s*=\s*"([^"]+)""#).unwrap());
static CARGO_PATH_DEP: Lazy<Regex> = Lazy::new(|| Regex::new(r#"path\s*=\s*"([^"]+)""#).unwrap());
static CARGO_WILDCARD: Lazy<Regex> = Lazy::new(|| Regex::new(r#"version\s*=\s*"\*""#).unwrap());
static CARGO_PATCH_SECTION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\[(?:patch|replace)\b").unwrap());

// package.json
static NPM_INSTALL_SCRIPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""(?:pre|post)?install"\s*:"#).unwrap());
static NPM_GIT_DEP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#":\s*"(?:git\+|github:|https?://)"#).unwrap());
static NPM_WILDCARD: Lazy<Regex> = Lazy::new(|| Regex::new(r#":\s*"(?:\*|latest)""#).unwrap());
static NPM_RESOLUTION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""(?:resolutions|overrides)""#).unwrap());

// requirements.txt
static PIP_EXTRA_INDEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)--(?:extra-)?index-url\s+(\S+)").unwrap());
static PIP_GIT_DEP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z0-9_-]+\s*@\s*git\+").unwrap());
static PIP_UNPINNED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9_-]+\s*$|^[a-z0-9_-]+\s*>=").unwrap());

// go.mod
static GO_REPLACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*replace\s+").unwrap());
static _GO_REQUIRE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:require\s+)?([^\s]+)\s+v[\d.]+").unwrap());

// GitHub Actions
// Matches uses: action@ref — we check in code whether ref is a pinned SHA
static GHA_USES_REF: Lazy<Regex> = Lazy::new(|| Regex::new(r"uses:\s*([^@\s]+)@(\S+)").unwrap());
static GHA_SHA_REF: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-f0-9]{40}$|^[a-f0-9]{64}$").unwrap());
static GHA_SCRIPT_INJECTION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$\{\{\s*github\.event\.[^}]+\}\}").unwrap());
static GHA_PR_TARGET: Lazy<Regex> = Lazy::new(|| Regex::new(r"pull_request_target").unwrap());

// Lockfile signals
static LOCKFILE_HTTP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)"?resolved"?\s*[:=]\s*"?http://"#).unwrap());
// Standard npm registries — used to filter non-standard registry URLs
static STANDARD_REGISTRIES: &[&str] = &["registry.npmjs.org", "registry.yarnpkg.com"];
static LOCKFILE_REGISTRY_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)"?(?:resolved|registry)"?\s*[:=]\s*"?(https?://[^\s"]+)"#).unwrap()
});

pub struct SupplyChainAnalyzer;

impl SupplyChainAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn analyze_added_lines(kind: ManifestKind, lines: &[(usize, &str)]) -> Vec<SupplyChainFinding> {
        let mut findings = Vec::new();

        for &(line_num, line) in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            match kind {
                ManifestKind::CargoToml => {
                    Self::check_cargo_toml(trimmed, line_num, &mut findings);
                }
                ManifestKind::PackageJson => {
                    Self::check_package_json(trimmed, line_num, &mut findings);
                }
                ManifestKind::RequirementsTxt
                | ManifestKind::Pipfile
                | ManifestKind::PyprojectToml => {
                    Self::check_python_deps(trimmed, line_num, &mut findings);
                }
                ManifestKind::GoMod => {
                    Self::check_go_mod(trimmed, line_num, &mut findings);
                }
                ManifestKind::GithubActions => {
                    Self::check_github_actions(trimmed, line_num, &mut findings);
                }
                ManifestKind::CargoLock
                | ManifestKind::PackageLockJson
                | ManifestKind::YarnLock
                | ManifestKind::PnpmLock
                | ManifestKind::GoSum
                | ManifestKind::GemfileLock => {
                    Self::check_lockfile(trimmed, line_num, kind, &mut findings);
                }
                ManifestKind::Gemfile | ManifestKind::ComposerJson => {
                    // Basic checks — flag git sources
                    if trimmed.contains("git:") || trimmed.contains("github:") {
                        findings.push(SupplyChainFinding {
                            rule_id: "sec.supply-chain.non-registry-source",
                            severity: "warning",
                            description: format!("Git-sourced dependency: {}", trimmed),
                            line_number: line_num,
                        });
                    }
                }
            }
        }

        findings
    }

    fn check_cargo_toml(line: &str, line_num: usize, findings: &mut Vec<SupplyChainFinding>) {
        if CARGO_GIT_DEP.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.non-registry-source",
                severity: "warning",
                description: format!(
                    "Git dependency bypasses crates.io checksums — pin to specific rev: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if CARGO_PATH_DEP.is_match(line) && !line.contains("members") {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.non-registry-source",
                severity: "info",
                description: format!(
                    "Path dependency (verify this is a workspace member): {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if CARGO_WILDCARD.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.unpinned-version",
                severity: "warning",
                description: format!("Wildcard version allows arbitrary major bumps: {}", line),
                line_number: line_num,
            });
        }
        if CARGO_PATCH_SECTION.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.override-directive",
                severity: "warning",
                description: format!(
                    "[patch]/[replace] section can silently redirect dependencies: {}",
                    line
                ),
                line_number: line_num,
            });
        }
    }

    fn check_package_json(line: &str, line_num: usize, findings: &mut Vec<SupplyChainFinding>) {
        if NPM_INSTALL_SCRIPT.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.install-scripts",
                severity: "warning",
                description: format!(
                    "Install script detected — executes during npm install: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if NPM_GIT_DEP.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.non-registry-source",
                severity: "warning",
                description: format!("Non-registry dependency source: {}", line),
                line_number: line_num,
            });
        }
        if NPM_WILDCARD.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.unpinned-version",
                severity: "warning",
                description: format!(
                    "Wildcard/latest version — pin to specific version: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if NPM_RESOLUTION.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.override-directive",
                severity: "info",
                description: format!(
                    "Resolution/override section can redirect packages: {}",
                    line
                ),
                line_number: line_num,
            });
        }
    }

    fn check_python_deps(line: &str, line_num: usize, findings: &mut Vec<SupplyChainFinding>) {
        if PIP_EXTRA_INDEX.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.non-registry-source",
                severity: "warning",
                description: format!(
                    "Extra/custom index URL — dependency confusion risk: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if PIP_GIT_DEP.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.non-registry-source",
                severity: "warning",
                description: format!("Git-sourced Python dependency: {}", line),
                line_number: line_num,
            });
        }
        if PIP_UNPINNED.is_match(line) && !line.starts_with('-') && !line.starts_with('#') {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.unpinned-version",
                severity: "info",
                description: format!("Unpinned or loosely pinned dependency: {}", line),
                line_number: line_num,
            });
        }
    }

    fn check_go_mod(line: &str, line_num: usize, findings: &mut Vec<SupplyChainFinding>) {
        if GO_REPLACE.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.override-directive",
                severity: "warning",
                description: format!(
                    "Go replace directive — redirects module resolution, risky in production: {}",
                    line
                ),
                line_number: line_num,
            });
        }
    }

    fn check_github_actions(line: &str, line_num: usize, findings: &mut Vec<SupplyChainFinding>) {
        if let Some(caps) = GHA_USES_REF.captures(line) {
            let ref_str = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if !GHA_SHA_REF.is_match(ref_str) {
                findings.push(SupplyChainFinding {
                    rule_id: "sec.supply-chain.ci-injection",
                    severity: "warning",
                    description: format!(
                        "Action not pinned to full SHA — mutable tag can be hijacked: {}",
                        line
                    ),
                    line_number: line_num,
                });
            }
        }
        if GHA_SCRIPT_INJECTION.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.ci-injection",
                severity: "error",
                description: format!(
                    "Script injection via github.event context — attacker-controlled PR title/body/branch: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if GHA_PR_TARGET.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.ci-injection",
                severity: "warning",
                description: format!(
                    "pull_request_target trigger — if it checks out PR code, it executes untrusted code with write permissions: {}",
                    line
                ),
                line_number: line_num,
            });
        }
    }

    fn check_lockfile(
        line: &str,
        line_num: usize,
        _kind: ManifestKind,
        findings: &mut Vec<SupplyChainFinding>,
    ) {
        if LOCKFILE_HTTP.is_match(line) {
            findings.push(SupplyChainFinding {
                rule_id: "sec.supply-chain.lockfile-tampering",
                severity: "error",
                description: format!(
                    "HTTP (not HTTPS) registry URL in lockfile — MITM risk: {}",
                    line
                ),
                line_number: line_num,
            });
        }
        if let Some(caps) = LOCKFILE_REGISTRY_URL.captures(line) {
            let url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let is_standard = STANDARD_REGISTRIES.iter().any(|r| url.contains(r));
            if !is_standard {
                findings.push(SupplyChainFinding {
                    rule_id: "sec.supply-chain.lockfile-tampering",
                    severity: "warning",
                    description: format!(
                        "Non-standard registry URL in lockfile — verify this is intentional: {}",
                        line
                    ),
                    line_number: line_num,
                });
            }
        }
    }
}

#[async_trait]
impl PreAnalyzer for SupplyChainAnalyzer {
    fn id(&self) -> &str {
        "supply-chain"
    }

    async fn run(&self, diff: &UnifiedDiff, _repo_path: &str) -> Result<PreAnalysis> {
        let kind = match ManifestKind::from_path(&diff.file_path) {
            Some(k) => k,
            None => return Ok(PreAnalysis::default()),
        };

        // Collect added lines with their line numbers
        let added_lines: Vec<(usize, &str)> = diff
            .hunks
            .iter()
            .flat_map(|h| h.changes.iter())
            .filter(|c| matches!(c.change_type, crate::core::diff_parser::ChangeType::Added))
            .map(|c| (c.new_line_no.unwrap_or(0), c.content.as_str()))
            .collect();

        if added_lines.is_empty() {
            return Ok(PreAnalysis::default());
        }

        let findings = Self::analyze_added_lines(kind, &added_lines);

        if findings.is_empty() {
            return Ok(PreAnalysis::default());
        }

        let mut report = format!(
            "Supply-chain analysis for {:?} ({}):\n\n",
            kind,
            diff.file_path.display()
        );
        for f in &findings {
            report.push_str(&format!(
                "- [{} / {}] Line {}: {}\n",
                f.rule_id, f.severity, f.line_number, f.description
            ));
        }
        report.push_str(&format!(
            "\nTotal: {} supply-chain signal(s) found.\n",
            findings.len()
        ));

        Ok(PreAnalysis {
            context_chunks: vec![
                LLMContextChunk::documentation(diff.file_path.clone(), report)
                    .with_provenance(crate::core::ContextProvenance::analyzer("supply-chain")),
            ],
            findings: findings
                .into_iter()
                .map(|finding| AnalyzerFinding {
                    file_path: diff.file_path.clone(),
                    line_number: finding.line_number,
                    content: finding.description,
                    rule_id: Some(finding.rule_id.to_string()),
                    suggestion: None,
                    severity: match finding.severity {
                        "error" => Severity::Error,
                        "warning" => Severity::Warning,
                        "suggestion" => Severity::Suggestion,
                        _ => Severity::Info,
                    },
                    category: Category::Security,
                    confidence: 0.98,
                    source: "supply-chain".to_string(),
                    tags: vec!["supply-chain".to_string()],
                    metadata: Default::default(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine};
    use std::path::PathBuf;

    fn make_manifest_diff(file_path: &str, added_lines: Vec<&str>) -> UnifiedDiff {
        let changes: Vec<DiffLine> = added_lines
            .into_iter()
            .enumerate()
            .map(|(i, content)| DiffLine {
                content: content.to_string(),
                old_line_no: Some(i + 1),
                new_line_no: Some(i + 1),
                change_type: ChangeType::Added,
            })
            .collect();

        UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from(file_path),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: changes.len(),
                new_start: 1,
                new_lines: changes.len(),
                context: String::new(),
                changes,
            }],
        }
    }

    #[test]
    fn test_analyzer_id() {
        assert_eq!(SupplyChainAnalyzer::new().id(), "supply-chain");
    }

    #[test]
    fn test_manifest_kind_detection() {
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("Cargo.toml")),
            Some(ManifestKind::CargoToml)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("package.json")),
            Some(ManifestKind::PackageJson)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("requirements.txt")),
            Some(ManifestKind::RequirementsTxt)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("requirements-dev.txt")),
            Some(ManifestKind::RequirementsTxt)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("go.mod")),
            Some(ManifestKind::GoMod)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new(".github/workflows/ci.yml")),
            Some(ManifestKind::GithubActions)
        );
        assert_eq!(
            ManifestKind::from_path(std::path::Path::new("src/main.rs")),
            None
        );
    }

    #[test]
    fn test_cargo_git_dep() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::CargoToml,
            &[(5, r#"my-crate = { git = "https://github.com/foo/bar" }"#)],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.non-registry-source");
    }

    #[test]
    fn test_cargo_wildcard_version() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::CargoToml,
            &[(3, r#"serde = { version = "*" }"#)],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.unpinned-version");
    }

    #[test]
    fn test_cargo_patch_section() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::CargoToml,
            &[(10, "[patch.crates-io]")],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.override-directive");
    }

    #[test]
    fn test_npm_install_script() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::PackageJson,
            &[(8, r#"    "postinstall": "node setup.js""#)],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.install-scripts");
    }

    #[test]
    fn test_npm_wildcard() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::PackageJson,
            &[(5, r#"    "lodash": "*""#)],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.unpinned-version");
    }

    #[test]
    fn test_pip_extra_index() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::RequirementsTxt,
            &[(1, "--extra-index-url https://pypi.internal.corp/simple")],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.non-registry-source");
    }

    #[test]
    fn test_go_replace() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::GoMod,
            &[(10, "replace github.com/foo/bar => ../local-bar")],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.override-directive");
    }

    #[test]
    fn test_gha_unpinned_action() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::GithubActions,
            &[(15, "      uses: actions/checkout@v4")],
        );
        assert!(!findings.is_empty());
        assert_eq!(findings[0].rule_id, "sec.supply-chain.ci-injection");
    }

    #[test]
    fn test_gha_pinned_action_ok() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::GithubActions,
            &[(
                15,
                "      uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11",
            )],
        );
        // Should NOT flag pinned-to-SHA actions
        let unpinned: Vec<_> = findings
            .iter()
            .filter(|f| f.description.contains("not pinned"))
            .collect();
        assert!(unpinned.is_empty(), "Should not flag SHA-pinned actions");
    }

    #[test]
    fn test_gha_script_injection() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::GithubActions,
            &[(
                20,
                "        run: echo ${{ github.event.pull_request.title }}",
            )],
        );
        assert!(!findings.is_empty());
        assert!(findings
            .iter()
            .any(|f| f.description.contains("Script injection")));
    }

    #[test]
    fn test_lockfile_http_registry() {
        let findings = SupplyChainAnalyzer::analyze_added_lines(
            ManifestKind::PackageLockJson,
            &[(
                100,
                r#"      "resolved": "http://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz""#,
            )],
        );
        assert!(!findings.is_empty());
        assert!(findings.iter().any(|f| f.description.contains("HTTP")));
    }

    #[tokio::test]
    async fn test_non_manifest_returns_empty() {
        let diff = make_manifest_diff("src/main.rs", vec!["fn main() {}"]);
        let analyzer = SupplyChainAnalyzer::new();
        let analysis = analyzer.run(&diff, "/tmp/repo").await.unwrap();
        assert!(analysis.context_chunks.is_empty());
        assert!(analysis.findings.is_empty());
    }

    #[tokio::test]
    async fn test_clean_manifest_returns_empty() {
        let diff = make_manifest_diff(
            "Cargo.toml",
            vec![
                r#"[dependencies]"#,
                r#"serde = { version = "1.0", features = ["derive"] }"#,
            ],
        );
        let analyzer = SupplyChainAnalyzer::new();
        let analysis = analyzer.run(&diff, "/tmp/repo").await.unwrap();
        assert!(
            analysis.context_chunks.is_empty() && analysis.findings.is_empty(),
            "Clean manifest should produce no findings"
        );
    }

    #[tokio::test]
    async fn test_manifest_with_findings_produces_context() {
        let diff = make_manifest_diff(
            "Cargo.toml",
            vec![r#"sketchy = { git = "https://github.com/evil/crate" }"#],
        );
        let analyzer = SupplyChainAnalyzer::new();
        let analysis = analyzer.run(&diff, "/tmp/repo").await.unwrap();
        assert!(!analysis.context_chunks.is_empty());
        assert!(!analysis.findings.is_empty());
        assert!(analysis.context_chunks[0].content.contains("supply-chain"));
    }
}
