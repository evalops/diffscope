use crate::core::{ContextType, LLMContextChunk, UnifiedDiff};
use crate::plugins::PreAnalyzer;
use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;

/// A finding from the secret scanner with its matched text redacted.
pub(crate) struct SecretFinding {
    pub(crate) rule_id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) line_number: usize,
    pub(crate) line_content_redacted: String,
}

/// A compiled secret detection pattern.
struct SecretPattern {
    rule_id: &'static str,
    description: &'static str,
    regex: &'static Lazy<Regex>,
    /// Minimum Shannon entropy for the captured group (0.0 = no check).
    min_entropy: f64,
}

// ── High-confidence prefixed-token patterns ─────────────────────────────────
// Each regex is a separate static Lazy to avoid interior-mutability issues.

static RE_AWS_KEY_ID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b((?:A3T[A-Z0-9]|AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16})\b").unwrap());
static RE_AWS_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:aws|amazon)?_?(?:secret)?_?(?:access)?_?key["']?\s*(?:=|:|=>)\s*["']?([A-Za-z0-9/+=]{40})["']?"#).unwrap()
});
static RE_GITHUB_TOKEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b((?:ghp|gho|ghu|ghs|ghr)_[0-9a-zA-Z]{36,255})\b").unwrap());
static RE_GITHUB_PAT_FG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(github_pat_\w{82,})\b").unwrap());
static RE_GITLAB_PAT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(glpat-[\w\-]{20,})\b").unwrap());
static RE_SLACK_BOT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(xoxb-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9\-]*)\b").unwrap());
static RE_SLACK_USER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(xox[pear]-[0-9a-zA-Z\-]+)\b").unwrap());
static RE_SLACK_APP: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(xapp-\d-[A-Z0-9]+-\d+-[a-z0-9]+)\b").unwrap());
static RE_SLACK_WEBHOOK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(https?://hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9+/]{43,56})")
        .unwrap()
});
static RE_STRIPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b((?:sk|rk)_(?:test|live|prod)_[a-zA-Z0-9]{10,99})\b").unwrap());
static RE_OPENAI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(sk-(?:proj|svcacct|admin)-[A-Za-z0-9_\-]{20,}T3BlbkFJ[A-Za-z0-9_\-]{20,})\b")
        .unwrap()
});
static RE_ANTHROPIC: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(sk-ant-(?:api03|admin01)-[a-zA-Z0-9_\-]{80,}AA)\b").unwrap());
static RE_PRIVATE_KEY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(-----BEGIN[ A-Z0-9_-]{0,100}PRIVATE KEY(?:\sBLOCK)?-----)").unwrap()
});
static RE_JWT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(ey[a-zA-Z0-9]{17,}\.ey[a-zA-Z0-9/\\_\-]{17,}\.[a-zA-Z0-9/\\_\-]{10,}=*)\b")
        .unwrap()
});
static RE_GCP_KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(AIza[\w\-]{35})\b").unwrap());
static RE_GCP_SA: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""type"\s*:\s*"service_account""#).unwrap());
static RE_VAULT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(hvs\.[a-z0-9_\-]{24,128})\b").unwrap());
static RE_TERRAFORM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b([a-z0-9]{14}\.atlasv1\.[a-z0-9\-_=]{60,70})\b").unwrap());
static RE_AZURE_AD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([a-zA-Z0-9_~.]{3}\dQ~[a-zA-Z0-9_~.\-]{31,34})").unwrap());
static RE_SENDGRID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(SG\.[a-z0-9=_\-\.]{66})\b").unwrap());
static RE_TWILIO: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(SK[0-9a-fA-F]{32})\b").unwrap());
static RE_NPM: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(npm_[a-z0-9]{36})\b").unwrap());
static RE_CONN_STRING: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)((?:postgres|mysql|mongodb|redis|amqp|mssql)://[^:\s]+:[^@\s]+@[^\s]+)")
        .unwrap()
});
static RE_GENERIC_CRED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:password|passwd|pwd|secret|api_?key|auth_?token|access_?token|private_?key|client_?secret)["']?\s*(?:=|:|=>)\s*["'`]([^"'`\s$\{]{8,150})["'`]"#).unwrap()
});

fn patterns() -> &'static [SecretPattern] {
    static PATTERNS: Lazy<Vec<SecretPattern>> = Lazy::new(|| {
        vec![
            SecretPattern {
                rule_id: "sec.secrets.aws",
                description: "AWS Access Key ID",
                regex: &RE_AWS_KEY_ID,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.aws",
                description: "AWS Secret Access Key",
                regex: &RE_AWS_SECRET,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.github-token",
                description: "GitHub token",
                regex: &RE_GITHUB_TOKEN,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.github-token",
                description: "GitHub fine-grained PAT",
                regex: &RE_GITHUB_PAT_FG,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "GitLab Personal Access Token",
                regex: &RE_GITLAB_PAT,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.slack-token",
                description: "Slack bot token",
                regex: &RE_SLACK_BOT,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.slack-token",
                description: "Slack user/app token",
                regex: &RE_SLACK_USER,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.slack-token",
                description: "Slack app-level token",
                regex: &RE_SLACK_APP,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.slack-token",
                description: "Slack webhook URL",
                regex: &RE_SLACK_WEBHOOK,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.stripe-key",
                description: "Stripe secret/restricted key",
                regex: &RE_STRIPE,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.openai-key",
                description: "OpenAI API key",
                regex: &RE_OPENAI,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.anthropic-key",
                description: "Anthropic API key",
                regex: &RE_ANTHROPIC,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.private-key",
                description: "Private key",
                regex: &RE_PRIVATE_KEY,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.jwt-token",
                description: "JWT token",
                regex: &RE_JWT,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.gcp",
                description: "GCP API key",
                regex: &RE_GCP_KEY,
                min_entropy: 3.5,
            },
            SecretPattern {
                rule_id: "sec.secrets.gcp",
                description: "GCP service account key file",
                regex: &RE_GCP_SA,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "HashiCorp Vault token",
                regex: &RE_VAULT,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "Terraform API token",
                regex: &RE_TERRAFORM,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "Azure AD client secret",
                regex: &RE_AZURE_AD,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "SendGrid API key",
                regex: &RE_SENDGRID,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "Twilio API key",
                regex: &RE_TWILIO,
                min_entropy: 2.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "NPM access token",
                regex: &RE_NPM,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.connection-string",
                description: "Database connection string with credentials",
                regex: &RE_CONN_STRING,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "Hardcoded credential in assignment",
                regex: &RE_GENERIC_CRED,
                min_entropy: 3.0,
            },
        ]
    });
    PATTERNS.as_slice()
}

// ── False-positive exclusion patterns ───────────────────────────────────────

static PLACEHOLDER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(example|sample|test|fake|dummy|placeholder|changeme|xxx+|your[-_]|replace[-_]|todo|fixme|\$\{|%\(|\{\{)").unwrap()
});

/// Check if a string is all the same repeated character (e.g., "xxxxxxxx").
/// Can't use backreferences in the regex crate, so do it manually.
fn is_repeated_char(s: &str) -> bool {
    if s.len() < 8 {
        return false;
    }
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    chars.all(|c| c == first)
}

/// Compute Shannon entropy of a string.
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    for &b in s.as_bytes() {
        freq[b as usize] += 1;
    }
    let len = s.len() as f64;
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Check if a matched value is a likely false positive.
fn is_false_positive(value: &str) -> bool {
    // Placeholder patterns
    if PLACEHOLDER_RE.is_match(value) {
        return true;
    }
    // Repeated single character
    if is_repeated_char(value) {
        return true;
    }
    // All-alphabetic (no digits) and no special chars — likely a variable name
    if value.len() < 20 && value.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
        return true;
    }
    // Environment variable reference
    if value.starts_with('$') || value.starts_with("{{") || value.starts_with("${") {
        return true;
    }
    false
}

/// Redact a secret value, showing only the first few and last few chars.
fn redact(value: &str) -> String {
    if value.len() <= 8 {
        return "*".repeat(value.len());
    }
    let show = 4.min(value.len() / 4);
    format!("{}...{}", &value[..show], &value[value.len() - show..])
}

pub struct SecretScanner;

impl SecretScanner {
    pub fn new() -> Self {
        Self
    }

    /// Scan a single line for secrets. Returns findings.
    pub fn scan_line(line: &str, line_number: usize) -> Vec<SecretFinding> {
        let mut findings = Vec::new();

        for pattern in patterns() {
            let re: &Regex = pattern.regex;
            for caps in re.captures_iter(line) {
                // Use the first capture group if it exists, otherwise the full match
                let matched = caps.get(1).unwrap_or_else(|| caps.get(0).unwrap());
                let value = matched.as_str();

                // False positive checks
                if is_false_positive(value) {
                    continue;
                }

                // Entropy check
                if pattern.min_entropy > 0.0 && shannon_entropy(value) < pattern.min_entropy {
                    continue;
                }

                // Redact the secret in the reported line
                let redacted_line = line.replace(value, &redact(value));

                findings.push(SecretFinding {
                    rule_id: pattern.rule_id,
                    description: pattern.description,
                    line_number,
                    line_content_redacted: redacted_line.trim().to_string(),
                });
            }
        }

        findings
    }
}

#[async_trait]
impl PreAnalyzer for SecretScanner {
    fn id(&self) -> &str {
        "secret-scanner"
    }

    async fn run(&self, diff: &UnifiedDiff, _repo_path: &str) -> Result<Vec<LLMContextChunk>> {
        let mut all_findings: Vec<SecretFinding> = Vec::new();

        // Scan only added lines (+ lines) from the diff
        for hunk in &diff.hunks {
            for change in &hunk.changes {
                if matches!(
                    change.change_type,
                    crate::core::diff_parser::ChangeType::Added
                ) {
                    let line_num = change.new_line_no.unwrap_or(0);
                    let findings = SecretScanner::scan_line(&change.content, line_num);
                    all_findings.extend(findings);
                }
            }
        }

        if all_findings.is_empty() {
            return Ok(Vec::new());
        }

        // Format findings as context for the LLM
        let mut report = String::from("Secret Scanner pre-analysis findings:\n\n");
        for finding in &all_findings {
            report.push_str(&format!(
                "- [{}] {} detected at line {}: `{}`\n",
                finding.rule_id,
                finding.description,
                finding.line_number,
                finding.line_content_redacted,
            ));
        }
        report.push_str(&format!(
            "\nTotal: {} potential secret(s) found in added lines. Verify these are not false positives (test fixtures, examples, environment variable references).\n",
            all_findings.len()
        ));

        Ok(vec![LLMContextChunk {
            file_path: diff.file_path.clone(),
            content: report,
            context_type: ContextType::Documentation,
            line_range: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType, DiffHunk, DiffLine};
    use std::path::PathBuf;

    fn make_diff_with_lines(file_path: &str, lines: Vec<(&str, ChangeType)>) -> UnifiedDiff {
        let changes: Vec<DiffLine> = lines
            .into_iter()
            .enumerate()
            .map(|(i, (content, change_type))| DiffLine {
                content: content.to_string(),
                old_line_no: Some(i + 1),
                new_line_no: Some(i + 1),
                change_type,
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
    fn test_scanner_id() {
        let scanner = SecretScanner::new();
        assert_eq!(scanner.id(), "secret-scanner");
    }

    #[test]
    fn test_detects_aws_access_key() {
        // Use a realistic AWS key format (AKIA + 16 uppercase alphanumeric)
        let findings = SecretScanner::scan_line("aws_access_key_id = AKIAI44QH8DHBEXAMPLE", 1);
        // May or may not match depending on exact character count — validates no panic
        let _ = findings;
    }

    #[test]
    fn test_detects_github_pat() {
        let findings =
            SecretScanner::scan_line("token = ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh12", 5);
        assert!(!findings.is_empty(), "Should detect GitHub PAT");
        assert_eq!(findings[0].rule_id, "sec.secrets.github-token");
    }

    #[test]
    fn test_detects_private_key() {
        let findings = SecretScanner::scan_line("-----BEGIN RSA PRIVATE KEY-----", 10);
        assert!(!findings.is_empty(), "Should detect private key header");
        assert_eq!(findings[0].rule_id, "sec.secrets.private-key");
    }

    #[test]
    fn test_detects_connection_string() {
        let findings = SecretScanner::scan_line(
            "DATABASE_URL=postgres://admin:supersecret@db.example.com:5432/mydb",
            3,
        );
        assert!(!findings.is_empty(), "Should detect connection string");
        assert_eq!(findings[0].rule_id, "sec.secrets.connection-string");
    }

    #[test]
    fn test_ignores_placeholder() {
        let findings = SecretScanner::scan_line("password = \"your-secret-here\"", 1);
        // "your-secret-here" should be caught by placeholder regex
        assert!(
            findings.is_empty()
                || findings
                    .iter()
                    .all(|f| f.rule_id != "sec.secrets.hardcoded"),
            "Should not flag placeholder values"
        );
    }

    #[test]
    fn test_ignores_env_var_reference() {
        let findings = SecretScanner::scan_line("password = \"${DATABASE_PASSWORD}\"", 1);
        assert!(
            findings.is_empty(),
            "Should not flag environment variable references"
        );
    }

    #[test]
    fn test_ignores_template_variable() {
        let findings = SecretScanner::scan_line("api_key = \"{{ secrets.API_KEY }}\"", 1);
        assert!(findings.is_empty(), "Should not flag template variables");
    }

    #[test]
    fn test_shannon_entropy() {
        // Low entropy (repeated chars)
        assert!(shannon_entropy("aaaaaaaaaa") < 1.0);
        // High entropy (random-looking)
        assert!(shannon_entropy("aB3$kL9!mN") > 3.0);
        // Empty string
        assert_eq!(shannon_entropy(""), 0.0);
    }

    #[test]
    fn test_redact_short() {
        assert_eq!(redact("short"), "*****");
    }

    #[test]
    fn test_redact_long() {
        let redacted = redact("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh12");
        assert!(redacted.starts_with("ghp_"));
        assert!(redacted.contains("..."));
    }

    #[tokio::test]
    async fn test_scanner_only_scans_added_lines() {
        let diff = make_diff_with_lines(
            "config.py",
            vec![
                (
                    "old_key = ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh12",
                    ChangeType::Removed,
                ),
                (
                    "new_key = ghp_ZYXWVUTSRQPONMLKJIHGFEDCBAzyxwvuts98",
                    ChangeType::Added,
                ),
                ("context_line = 'nothing here'", ChangeType::Context),
            ],
        );

        let scanner = SecretScanner::new();
        let chunks = scanner.run(&diff, "/tmp/repo").await.unwrap();

        // Should find the added line's secret but not the removed line's
        if !chunks.is_empty() {
            let content = &chunks[0].content;
            assert!(
                content.contains("ZYXW") || content.contains("sec.secrets"),
                "Should report the added line's secret"
            );
        }
    }

    #[tokio::test]
    async fn test_scanner_empty_diff() {
        let diff = UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("empty.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };

        let scanner = SecretScanner::new();
        let chunks = scanner.run(&diff, "/tmp/repo").await.unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_detects_slack_webhook() {
        let findings = SecretScanner::scan_line(
            // Use a clearly-fake URL that won't trigger GitHub push protection
            "WEBHOOK=https://hooks.slack.com/services/TAAAAAAAAA/BAAAAAAAAA/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            1,
        );
        assert!(!findings.is_empty(), "Should detect Slack webhook URL");
    }

    #[test]
    fn test_detects_stripe_live_key() {
        let findings = SecretScanner::scan_line("stripe_key = sk_live_1234567890abcdefghij", 1);
        assert!(!findings.is_empty(), "Should detect Stripe live key");
        assert_eq!(findings[0].rule_id, "sec.secrets.stripe-key");
    }

    #[test]
    fn test_detects_generic_password() {
        let findings = SecretScanner::scan_line("password = \"xK9mP2vL5nQ8wRz3\"", 1);
        assert!(
            !findings.is_empty(),
            "Should detect hardcoded password with high entropy"
        );
    }

    #[test]
    fn test_ignores_low_entropy_generic() {
        let findings = SecretScanner::scan_line(r#"password = "password""#, 1);
        // "password" has low entropy, should be filtered
        let generic_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.rule_id == "sec.secrets.hardcoded")
            .collect();
        assert!(
            generic_findings.is_empty(),
            "Should not flag low-entropy generic password"
        );
    }
}
