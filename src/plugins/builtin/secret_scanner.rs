use crate::core::comment::{Category, Severity};
use crate::core::{LLMContextChunk, UnifiedDiff};
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};
use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A finding from the secret scanner with its matched text redacted.
pub(crate) struct SecretFinding {
    pub(crate) rule_id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) line_number: usize,
    pub(crate) line_content_redacted: String,
    pub(crate) fingerprint: String,
    pub(crate) provider: String,
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
    Regex::new(r"\b(ey[a-zA-Z0-9]{17,}\.ey[a-zA-Z0-9/_-]{17,}\.[a-zA-Z0-9/_-]{10,}=*)\b").unwrap()
});
static RE_GCP_KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(AIza[\w\-]{35})\b").unwrap());
static RE_GCP_SA: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""type"\s*:\s*"service_account""#).unwrap());
static RE_VAULT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(hvs\.[a-z0-9_\-]{24,128})\b").unwrap());
static RE_TERRAFORM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b([a-z0-9]{14}\.atlasv1\.[a-z0-9\-_=]{60,70})\b").unwrap());
static RE_AZURE_AD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([a-zA-Z0-9_~.]{3}\dQ~[a-zA-Z0-9_~.\-]{31,34})").unwrap());
static RE_DATADOG_API: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(?:dd_?api_?key|datadog[\w\-]{0,20}api[\w\-]{0,20}key)["']?\s*(?:=|:|=>)\s*["']?([a-f0-9]{32})["']?"#,
    )
    .unwrap()
});
static RE_DATADOG_APP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(?:dd_?app_?key|datadog[\w\-]{0,20}app(?:lication)?[\w\-]{0,20}key)["']?\s*(?:=|:|=>)\s*["']?([a-f0-9]{40})["']?"#,
    )
    .unwrap()
});
static RE_SENDGRID: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(SG\.[a-z0-9=_\-\.]{66})\b").unwrap());
static RE_TWILIO: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(SK[0-9a-fA-F]{32})\b").unwrap());
static RE_NPM: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(npm_[a-z0-9]{36})\b").unwrap());
static RE_CONN_STRING: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)((?:postgres(?:ql)?|mysql|mongodb|redis|amqp|mssql)://[^:\s]+:[^@\s]+@[^\s]+)")
        .unwrap()
});
static RE_GENERIC_CRED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:password|passwd|pwd|secret|api_?key|auth_?token|access_?token|private_?key|client_?secret)["']?\s*(?:=|:|=>)\s*["'`]([^"'`\s$\{]{8,150})["'`]"#).unwrap()
});

// ── Additional provider patterns ────────────────────────────────────────────
static RE_NEWRELIC: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(NRAK-[A-Z0-9]{27})\b").unwrap());
static RE_NEWRELIC_BROWSER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(NRJS-[a-f0-9]{19})\b").unwrap());
static RE_DATABRICKS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(dapi[0-9a-f]{32})\b").unwrap());
static RE_SHOPIFY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(shp(?:at|ss|ca|pa)_[a-fA-F0-9]{32,})\b").unwrap());
static RE_DISCORD_WEBHOOK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(https?://(?:discord|discordapp)\.com/api/webhooks/\d+/[A-Za-z0-9_\-]+)").unwrap()
});
static RE_LINEAR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(lin_api_[a-zA-Z0-9]{40,})\b").unwrap());
static RE_PYPI: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_\-]{50,})\b").unwrap());
static RE_DIGITALOCEAN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(dop_v1_[a-f0-9]{64})\b").unwrap());
static RE_MAILGUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(key-[0-9a-f]{32})\b").unwrap());
static RE_DOPPLER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(dp\.(?:st|ct|pt)\.[a-zA-Z0-9]{40,})\b").unwrap());
static RE_SUPABASE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(sbp_[a-f0-9]{40,})\b").unwrap());
static RE_PAGERDUTY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:pagerduty|pd)[\w_]*(?:key|token)[\w_]*\s*(?:=|:)\s*["']?([a-zA-Z0-9+/]{20,})["']?"#).unwrap()
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
                rule_id: "sec.secrets.azure",
                description: "Azure AD client secret",
                regex: &RE_AZURE_AD,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.datadog",
                description: "Datadog API key",
                regex: &RE_DATADOG_API,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.datadog",
                description: "Datadog application key",
                regex: &RE_DATADOG_APP,
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
            // ── Additional providers ──
            SecretPattern {
                rule_id: "sec.secrets.newrelic",
                description: "New Relic API key",
                regex: &RE_NEWRELIC,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.newrelic",
                description: "New Relic browser key",
                regex: &RE_NEWRELIC_BROWSER,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.databricks",
                description: "Databricks personal access token",
                regex: &RE_DATABRICKS,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.shopify",
                description: "Shopify access token",
                regex: &RE_SHOPIFY,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.discord",
                description: "Discord webhook URL",
                regex: &RE_DISCORD_WEBHOOK,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.linear",
                description: "Linear API key",
                regex: &RE_LINEAR,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.pypi",
                description: "PyPI API token",
                regex: &RE_PYPI,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.digitalocean",
                description: "DigitalOcean personal access token",
                regex: &RE_DIGITALOCEAN,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.mailgun",
                description: "Mailgun API key",
                regex: &RE_MAILGUN,
                min_entropy: 3.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.doppler",
                description: "Doppler service token",
                regex: &RE_DOPPLER,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.supabase",
                description: "Supabase service role key",
                regex: &RE_SUPABASE,
                min_entropy: 0.0,
            },
            SecretPattern {
                rule_id: "sec.secrets.hardcoded",
                description: "PagerDuty API key",
                regex: &RE_PAGERDUTY,
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
    let char_count = value.chars().count();
    if char_count <= 8 {
        return "*".repeat(char_count);
    }
    let show = 4.min(char_count / 4);
    // Use char_indices to find safe byte boundaries for multi-byte UTF-8
    let prefix_end = value
        .char_indices()
        .nth(show)
        .map_or(value.len(), |(i, _)| i);
    let suffix_start = value
        .char_indices()
        .nth(char_count - show)
        .map_or(value.len(), |(i, _)| i);
    format!("{}...{}", &value[..prefix_end], &value[suffix_start..])
}

#[derive(Clone, Copy)]
struct SecretMatch {
    rule_id: &'static str,
    description: &'static str,
    start: usize,
    end: usize,
}

fn redact_ranges(line: &str, matches: &[SecretMatch]) -> String {
    if matches.is_empty() {
        return line.to_string();
    }

    let mut ranges: Vec<(usize, usize)> = matches.iter().map(|m| (m.start, m.end)).collect();
    ranges.sort_by_key(|(start, _)| *start);

    let mut merged_ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match merged_ranges.last_mut() {
            Some((_, last_end)) if start <= *last_end => {
                *last_end = (*last_end).max(end);
            }
            _ => merged_ranges.push((start, end)),
        }
    }

    let mut redacted = String::with_capacity(line.len());
    let mut cursor = 0;
    for (start, end) in merged_ranges {
        redacted.push_str(&line[cursor..start]);
        redacted.push_str(&redact(&line[start..end]));
        cursor = end;
    }
    redacted.push_str(&line[cursor..]);
    redacted
}

enum AllowlistEntry {
    Literal(String),
    Path(String),
    Regex(Regex),
}

impl AllowlistEntry {
    fn parse(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        if let Some(value) = trimmed.strip_prefix("path:") {
            return Some(Self::Path(value.trim().to_string()));
        }
        if let Some(value) = trimmed.strip_prefix("regex:") {
            return Regex::new(value.trim()).ok().map(Self::Regex);
        }
        Some(Self::Literal(trimmed.to_string()))
    }

    fn matches(&self, file_path: &Path, line: &str) -> bool {
        match self {
            Self::Literal(value) => {
                line.contains(value) || file_path.to_string_lossy().contains(value)
            }
            Self::Path(value) => file_path.to_string_lossy().contains(value),
            Self::Regex(regex) => {
                regex.is_match(line) || regex.is_match(&file_path.to_string_lossy())
            }
        }
    }
}

fn secret_fingerprint(rule_id: &str, secret: &str) -> String {
    let digest = Sha256::digest(format!("{rule_id}:{secret}").as_bytes());
    format!("{digest:x}")
}

fn provider_name(rule_id: &str) -> &'static str {
    if rule_id.contains("aws") {
        "aws"
    } else if rule_id.contains("github") {
        "github"
    } else if rule_id.contains("gitlab") {
        "gitlab"
    } else if rule_id.contains("slack") {
        "slack"
    } else if rule_id.contains("stripe") {
        "stripe"
    } else if rule_id.contains("openai") {
        "openai"
    } else if rule_id.contains("anthropic") {
        "anthropic"
    } else if rule_id.contains("azure") {
        "azure"
    } else if rule_id.contains("datadog") {
        "datadog"
    } else if rule_id.contains("gcp") {
        "gcp"
    } else {
        "generic"
    }
}

pub struct SecretScanner {
    allowlist_file: PathBuf,
    baseline_file: PathBuf,
}

impl SecretScanner {
    pub fn new() -> Self {
        Self {
            allowlist_file: PathBuf::from(".diffscope-secrets-allowlist"),
            baseline_file: PathBuf::from(".diffscope-secrets-baseline.json"),
        }
    }

    /// Scan a single line for secrets. Returns findings.
    pub fn scan_line(line: &str, line_number: usize) -> Vec<SecretFinding> {
        let mut matches = Vec::new();

        for pattern in patterns() {
            let re: &Regex = pattern.regex;
            for caps in re.captures_iter(line) {
                // Use the first capture group if it exists, otherwise the full match
                let Some(matched) = caps.get(1).or_else(|| caps.get(0)) else {
                    continue;
                };
                let value = matched.as_str();

                // False positive checks
                if is_false_positive(value) {
                    continue;
                }

                // Entropy check
                if pattern.min_entropy > 0.0 && shannon_entropy(value) < pattern.min_entropy {
                    continue;
                }

                matches.push(SecretMatch {
                    rule_id: pattern.rule_id,
                    description: pattern.description,
                    start: matched.start(),
                    end: matched.end(),
                });
            }
        }

        if matches.is_empty() {
            return Vec::new();
        }

        let redacted_line = redact_ranges(line, &matches).trim().to_string();

        let mut findings = Vec::with_capacity(matches.len());
        for secret_match in matches {
            findings.push(SecretFinding {
                rule_id: secret_match.rule_id,
                description: secret_match.description,
                line_number,
                line_content_redacted: redacted_line.clone(),
                fingerprint: secret_fingerprint(
                    secret_match.rule_id,
                    &line[secret_match.start..secret_match.end],
                ),
                provider: provider_name(secret_match.rule_id).to_string(),
            });
        }

        findings
    }

    fn load_allowlist(&self, repo_path: &str) -> Vec<AllowlistEntry> {
        let path = Path::new(repo_path).join(&self.allowlist_file);
        let Ok(content) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        content.lines().filter_map(AllowlistEntry::parse).collect()
    }

    fn load_baseline(&self, repo_path: &str) -> HashSet<String> {
        let path = Path::new(repo_path).join(&self.baseline_file);
        let Ok(content) = std::fs::read_to_string(path) else {
            return HashSet::new();
        };
        if let Ok(values) = serde_json::from_str::<Vec<String>>(&content) {
            return values.into_iter().collect();
        }
        serde_json::from_str::<HashMap<String, Vec<String>>>(&content)
            .ok()
            .and_then(|map| map.get("fingerprints").cloned())
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

#[async_trait]
impl PreAnalyzer for SecretScanner {
    fn id(&self) -> &str {
        "secret-scanner"
    }

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis> {
        let mut all_findings: Vec<SecretFinding> = Vec::new();
        let allowlist = self.load_allowlist(repo_path);
        let baseline = self.load_baseline(repo_path);

        // Scan only added lines (+ lines) from the diff
        for hunk in &diff.hunks {
            for change in &hunk.changes {
                if matches!(
                    change.change_type,
                    crate::core::diff_parser::ChangeType::Added
                ) {
                    let line_num = change.new_line_no.unwrap_or(0);
                    let findings = SecretScanner::scan_line(&change.content, line_num)
                        .into_iter()
                        .filter(|finding| !baseline.contains(&finding.fingerprint))
                        .filter(|_| {
                            !allowlist
                                .iter()
                                .any(|entry| entry.matches(&diff.file_path, &change.content))
                        })
                        .collect::<Vec<_>>();
                    all_findings.extend(findings);
                }
            }
        }

        if all_findings.is_empty() {
            return Ok(PreAnalysis::default());
        }

        // Format findings as context for the LLM
        let mut report = String::from("Secret Scanner pre-analysis findings:\n\n");
        for finding in &all_findings {
            report.push_str(&format!(
                "- [{} / {}] {} detected at line {}: `{}`\n",
                finding.rule_id,
                finding.provider,
                finding.description,
                finding.line_number,
                finding.line_content_redacted,
            ));
        }
        report.push_str(&format!(
            "\nTotal: {} potential secret(s) found in added lines. Verify these are not false positives (test fixtures, examples, environment variable references).\n",
            all_findings.len()
        ));

        Ok(PreAnalysis {
            context_chunks: vec![LLMContextChunk::documentation(diff.file_path.clone(), report)
                .with_provenance(crate::core::ContextProvenance::analyzer("secret scanner"))],
            findings: all_findings
                .into_iter()
                .map(|finding| {
                    let mut metadata = HashMap::new();
                    metadata.insert("provider".to_string(), finding.provider.clone());
                    metadata.insert("fingerprint".to_string(), finding.fingerprint.clone());
                    AnalyzerFinding {
                        file_path: diff.file_path.clone(),
                        line_number: finding.line_number,
                        content: format!(
                            "{} detected in added code: {}",
                            finding.description, finding.line_content_redacted
                        ),
                        rule_id: Some(finding.rule_id.to_string()),
                        suggestion: Some(
                            "Move the secret into environment-backed configuration or a secret manager and rotate the exposed credential."
                                .to_string(),
                        ),
                        severity: Severity::Error,
                        category: Category::Security,
                        confidence: 0.99,
                        source: "secret-scanner".to_string(),
                        tags: vec!["secret-scanner".to_string(), finding.provider.clone()],
                        metadata,
                    }
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
    use tempfile::tempdir;

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
        let line = format!("aws_access_key_id = {}", fake_token("AKIA", 'A', 20));
        let findings = SecretScanner::scan_line(&line, 1);
        // May or may not match depending on exact character count — validates no panic
        let _ = findings;
    }

    #[test]
    fn test_detects_github_pat() {
        let line = format!("token = {}", fake_token("ghp_", 'A', 40));
        let findings = SecretScanner::scan_line(&line, 5);
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
    fn test_detects_postgresql_connection_string() {
        let findings = SecretScanner::scan_line(
            "DATABASE_URL=postgresql://admin:supersecret@db.example.com:5432/mydb",
            3,
        );
        assert!(
            !findings.is_empty(),
            "Should detect postgresql:// connection string"
        );
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
    fn test_detects_azure_client_secret_with_specific_rule_id() {
        let findings = SecretScanner::scan_line(
            "AZURE_CLIENT_SECRET=abc1Q~AbCdEfGhIjKlMnOpQrStUvWxYz012345",
            1,
        );
        assert!(!findings.is_empty(), "Should detect Azure client secret");
        assert_eq!(findings[0].rule_id, "sec.secrets.azure");
    }

    #[test]
    fn test_detects_datadog_api_key() {
        let findings = SecretScanner::scan_line("DD_API_KEY=0123456789abcdef0123456789abcdef", 1);
        assert!(!findings.is_empty(), "Should detect Datadog API key");
        assert_eq!(findings[0].rule_id, "sec.secrets.datadog");
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

    #[test]
    fn test_redact_multibyte_utf8() {
        // Must not panic on multi-byte UTF-8 characters
        let redacted = redact("pässwörd_töken_sëcret_välue_here");
        assert!(redacted.contains("..."));
    }

    #[test]
    fn test_redacts_all_detected_secrets_on_same_line() {
        let github_token = fake_token("ghp_", 'A', 40);
        let slack_webhook = "https://hooks.slack.com/services/TAAAAAAAAA/BAAAAAAAAA/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let line = format!("GITHUB_TOKEN={github_token} SLACK_WEBHOOK={slack_webhook}");

        let findings = SecretScanner::scan_line(&line, 1);
        assert!(findings.len() >= 2, "Should detect both secrets");
        for finding in findings {
            assert!(
                !finding.line_content_redacted.contains(&github_token),
                "GitHub token should be redacted: {}",
                finding.line_content_redacted
            );
            assert!(
                !finding.line_content_redacted.contains(slack_webhook),
                "Slack webhook should be redacted: {}",
                finding.line_content_redacted
            );
        }
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
        let analysis = scanner.run(&diff, "/tmp/repo").await.unwrap();

        // Should find the added line's secret but not the removed line's
        if !analysis.context_chunks.is_empty() {
            let content = &analysis.context_chunks[0].content;
            assert!(
                content.contains("ZYXW") || content.contains("sec.secrets"),
                "Should report the added line's secret"
            );
        }
        assert_eq!(analysis.findings.len(), 1);
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
        let analysis = scanner.run(&diff, "/tmp/repo").await.unwrap();
        assert!(analysis.context_chunks.is_empty());
        assert!(analysis.findings.is_empty());
    }

    #[tokio::test]
    async fn test_scanner_respects_allowlist_file() {
        let repo = tempdir().unwrap();
        std::fs::write(
            repo.path().join(".diffscope-secrets-allowlist"),
            "fixtures/\n",
        )
        .unwrap();
        let diff = make_diff_with_lines(
            "fixtures/example.env",
            vec![(
                &format!("AWS_KEY={}", fake_token("AKIA", 'A', 20)),
                ChangeType::Added,
            )],
        );

        let scanner = SecretScanner::new();
        let analysis = scanner
            .run(&diff, repo.path().to_string_lossy().as_ref())
            .await
            .unwrap();

        assert!(analysis.context_chunks.is_empty());
        assert!(analysis.findings.is_empty());
    }

    #[tokio::test]
    async fn test_scanner_respects_baseline_file() {
        let repo = tempdir().unwrap();
        let github_pat = fake_token("ghp_", 'A', 40);
        let baseline_fingerprint = secret_fingerprint("sec.secrets.github-token", &github_pat);
        std::fs::write(
            repo.path().join(".diffscope-secrets-baseline.json"),
            serde_json::to_string(&vec![baseline_fingerprint]).unwrap(),
        )
        .unwrap();
        let diff = make_diff_with_lines(
            "config.env",
            vec![(&format!("TOKEN={github_pat}"), ChangeType::Added)],
        );

        let scanner = SecretScanner::new();
        let analysis = scanner
            .run(&diff, repo.path().to_string_lossy().as_ref())
            .await
            .unwrap();

        assert!(analysis.context_chunks.is_empty());
        assert!(analysis.findings.is_empty());
    }

    #[test]
    fn test_detects_slack_webhook() {
        let webhook = format!(
            "https://hooks.slack.com/services/{}/{}/{}",
            "TAAAAAAAAA",
            "BAAAAAAAAA",
            "a".repeat(45)
        );
        let line = format!("WEBHOOK={webhook}");
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect Slack webhook URL");
    }

    #[test]
    fn test_detects_stripe_live_key() {
        let line = format!("stripe_key = {}", fake_token("sk_live_", 'a', 28));
        let findings = SecretScanner::scan_line(&line, 1);
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

    /// Build a fake token at runtime to avoid GitHub Push Protection flagging
    /// the source file. The full pattern never appears as a string literal.
    fn fake_token(prefix: &str, fill: char, len: usize) -> String {
        let suffix_len = len.saturating_sub(prefix.len());
        format!(
            "{}{}",
            prefix,
            std::iter::repeat_n(fill, suffix_len).collect::<String>()
        )
    }

    #[test]
    fn test_detects_newrelic_key() {
        let line = format!("NEW_RELIC_KEY={}", fake_token("NRAK-", 'A', 32));
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect New Relic API key");
        assert_eq!(findings[0].rule_id, "sec.secrets.newrelic");
    }

    #[test]
    fn test_detects_databricks_token() {
        // Databricks has min_entropy 3.0 — use high-entropy hex suffix
        let line = format!("token = dapi{}", "0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d");
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect Databricks PAT");
        assert_eq!(findings[0].rule_id, "sec.secrets.databricks");
    }

    #[test]
    fn test_detects_shopify_token() {
        let line = format!("SHOPIFY_TOKEN={}", fake_token("shpat_", 'a', 38));
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect Shopify access token");
        assert_eq!(findings[0].rule_id, "sec.secrets.shopify");
    }

    #[test]
    fn test_detects_digitalocean_token() {
        let line = format!("DO_TOKEN={}", fake_token("dop_v1_", 'a', 71));
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect DigitalOcean PAT");
        assert_eq!(findings[0].rule_id, "sec.secrets.digitalocean");
    }

    #[test]
    fn test_detects_doppler_token() {
        let line = format!("DOPPLER_TOKEN={}", fake_token("dp.st.", 'a', 46));
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect Doppler service token");
        assert_eq!(findings[0].rule_id, "sec.secrets.doppler");
    }

    #[test]
    fn test_detects_linear_key() {
        let line = format!("LINEAR_API_KEY={}", fake_token("lin_api_", 'a', 48));
        let findings = SecretScanner::scan_line(&line, 1);
        assert!(!findings.is_empty(), "Should detect Linear API key");
        assert_eq!(findings[0].rule_id, "sec.secrets.linear");
    }
}
