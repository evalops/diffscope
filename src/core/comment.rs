use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    pub file_path: PathBuf,
    pub line_number: usize,
    pub content: String,
    #[serde(default)]
    pub rule_id: Option<String>,
    pub severity: Severity,
    pub category: Category,
    pub suggestion: Option<String>,
    pub confidence: f32,
    pub code_suggestion: Option<CodeSuggestion>,
    pub tags: Vec<String>,
    pub fix_effort: FixEffort,
    #[serde(default)]
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSuggestion {
    pub original_code: String,
    pub suggested_code: String,
    pub explanation: String,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total_comments: usize,
    pub by_severity: HashMap<String, usize>,
    pub by_category: HashMap<String, usize>,
    pub critical_issues: usize,
    pub files_reviewed: usize,
    pub overall_score: f32,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Suggestion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Category {
    Bug,
    Security,
    Performance,
    Style,
    Documentation,
    BestPractice,
    Maintainability,
    Testing,
    Architecture,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "Error"),
            Severity::Warning => write!(f, "Warning"),
            Severity::Info => write!(f, "Info"),
            Severity::Suggestion => write!(f, "Suggestion"),
        }
    }
}

impl Severity {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Suggestion => "suggestion",
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Bug => write!(f, "Bug"),
            Category::Security => write!(f, "Security"),
            Category::Performance => write!(f, "Performance"),
            Category::Style => write!(f, "Style"),
            Category::Documentation => write!(f, "Documentation"),
            Category::BestPractice => write!(f, "BestPractice"),
            Category::Maintainability => write!(f, "Maintainability"),
            Category::Testing => write!(f, "Testing"),
            Category::Architecture => write!(f, "Architecture"),
        }
    }
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Bug => "bug",
            Category::Security => "security",
            Category::Performance => "performance",
            Category::Style => "style",
            Category::Documentation => "documentation",
            Category::BestPractice => "bestpractice",
            Category::Maintainability => "maintainability",
            Category::Testing => "testing",
            Category::Architecture => "architecture",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FixEffort {
    Low,    // < 5 minutes
    Medium, // 5-30 minutes
    High,   // > 30 minutes
}

pub struct CommentSynthesizer;

impl CommentSynthesizer {
    pub fn synthesize(raw_comments: Vec<RawComment>) -> Result<Vec<Comment>> {
        let mut comments = Vec::new();

        for raw in raw_comments {
            comments.push(Self::process_raw_comment(raw)?);
        }

        Self::deduplicate_comments(&mut comments);
        Self::sort_by_priority(&mut comments);

        Ok(comments)
    }

    pub fn generate_summary(comments: &[Comment]) -> ReviewSummary {
        let mut by_severity = HashMap::new();
        let mut by_category = HashMap::new();
        let mut files = std::collections::HashSet::new();
        let mut critical_issues = 0;

        for comment in comments {
            let severity_str = comment.severity.to_string();
            *by_severity.entry(severity_str).or_insert(0) += 1;

            let category_str = comment.category.to_string();
            *by_category.entry(category_str).or_insert(0) += 1;

            files.insert(comment.file_path.clone());

            if matches!(comment.severity, Severity::Error) {
                critical_issues += 1;
            }
        }

        let overall_score = Self::calculate_overall_score(comments);
        let recommendations = Self::generate_recommendations(comments);

        ReviewSummary {
            total_comments: comments.len(),
            by_severity,
            by_category,
            critical_issues,
            files_reviewed: files.len(),
            overall_score,
            recommendations,
        }
    }

    fn process_raw_comment(raw: RawComment) -> Result<Comment> {
        let lower = raw.content.to_lowercase();
        let severity = raw
            .severity
            .clone()
            .unwrap_or_else(|| Self::determine_severity(&lower));
        let category = raw
            .category
            .clone()
            .unwrap_or_else(|| Self::determine_category(&lower));
        let confidence = raw
            .confidence
            .unwrap_or_else(|| Self::calculate_confidence(&lower, &severity, &category));
        let confidence = confidence.clamp(0.0, 1.0);
        let tags = if raw.tags.is_empty() {
            Self::extract_tags(&lower, &category)
        } else {
            raw.tags.clone()
        };
        let fix_effort = raw
            .fix_effort
            .clone()
            .unwrap_or_else(|| Self::determine_fix_effort(&lower, &category));
        let code_suggestion = Self::generate_code_suggestion(&raw);
        let id = Self::generate_comment_id(&raw.file_path, &raw.content, &category);

        Ok(Comment {
            id,
            file_path: raw.file_path,
            line_number: raw.line_number,
            content: raw.content,
            rule_id: raw.rule_id,
            severity,
            category,
            suggestion: raw.suggestion,
            confidence,
            code_suggestion,
            tags,
            fix_effort,
            feedback: None,
        })
    }

    fn generate_comment_id(file_path: &Path, content: &str, category: &Category) -> String {
        compute_comment_id(file_path, content, category)
    }

    /// `lower` must already be lowercased.
    fn determine_severity(lower: &str) -> Severity {
        if lower.contains("error") || lower.contains("critical") {
            Severity::Error
        } else if lower.contains("warning") || lower.contains("issue") {
            Severity::Warning
        } else if lower.contains("consider") || lower.contains("suggestion") {
            Severity::Suggestion
        } else {
            Severity::Info
        }
    }

    /// `lower` must already be lowercased.
    fn determine_category(lower: &str) -> Category {
        // Security — broad keyword coverage across all 5 vulnerability classes
        if lower.contains("security")
            || lower.contains("vulnerability")
            || lower.contains("injection")
            || lower.contains("sql injection")
            || lower.contains("command injection")
            || lower.contains("xss")
            || lower.contains("cross-site scripting")
            || lower.contains("csrf")
            || lower.contains("cross-site request forgery")
            || lower.contains("ssrf")
            || lower.contains("server-side request forgery")
            || lower.contains("deserialization")
            || lower.contains("path traversal")
            || lower.contains("directory traversal")
            || lower.contains("hardcoded secret")
            || lower.contains("hardcoded credential")
            || lower.contains("hardcoded password")
            || lower.contains("api key")
            || lower.contains("private key")
            || lower.contains("jwt")
            || lower.contains("authentication")
            || lower.contains("authorization")
            || lower.contains("access control")
            || lower.contains("privilege escalation")
            || lower.contains("idor")
            || lower.contains("insecure direct object")
            || lower.contains("supply chain")
            || lower.contains("supply-chain")
            || lower.contains("dependency confusion")
            || lower.contains("typosquatting")
            || lower.contains("cwe-")
            || lower.contains("owasp")
            || lower.contains("xxe")
            || lower.contains("open redirect")
            || lower.contains("cors")
            || lower.contains("template injection")
            || lower.contains("ldap injection")
            || lower.contains("log injection")
            || lower.contains("code injection")
            || lower.contains("unsafe pickle")
            || lower.contains("unsafe yaml")
            || lower.contains("weak hash")
            || lower.contains("weak password")
            // Cryptography
            || lower.contains("weak cipher")
            || lower.contains("insecure tls")
            || lower.contains("insecure ssl")
            || lower.contains("insecure random")
            || lower.contains("math.random")
            || lower.contains("weak key")
            || lower.contains("broken hash")
            || lower.contains("hardcoded iv")
            || lower.contains("hardcoded nonce")
            || lower.contains("ecb mode")
            || lower.contains("timing attack")
            || lower.contains("certificate validation")
            || lower.contains("cert validation")
            // Data exposure
            || lower.contains("pii")
            || lower.contains("data exposure")
            || lower.contains("data leak")
            || lower.contains("debug mode")
            || lower.contains("stack trace")
            || lower.contains("verbose error")
            || lower.contains("information disclosure")
            || lower.contains("security header")
            || lower.contains("missing security header")
            // Unsafe code patterns
            || lower.contains("unsafe block")
            || lower.contains("unsafe {")
            || lower.contains("transmute")
            || lower.contains("buffer overflow")
            || lower.contains("prototype pollution")
            || lower.contains("mass assignment")
            || lower.contains("race condition")
            || lower.contains("toctou")
            || lower.contains("redos")
            || lower.contains("catastrophic backtracking")
            // Infrastructure
            || lower.contains("running as root")
            || lower.contains("privileged container")
            || lower.contains("hostnetwork")
            || lower.contains("overpermissive")
            || lower.contains("publicly accessible")
            || lower.contains("iam policy")
            // API security
            || lower.contains("rate limit")
            || lower.contains("brute force")
            || lower.contains("no pagination")
            || lower.contains("unbounded query")
            || lower.contains("graphql depth")
            || lower.contains("insecure upload")
            || lower.contains("unrestricted upload")
            || (lower.contains("file upload") && (lower.contains("insecure") || lower.contains("unrestricted") || lower.contains("vulnerability") || lower.contains("security")))
            || (lower.contains("input validation") && (lower.contains("missing") || lower.contains("vulnerability") || lower.contains("security") || lower.contains("injection")))
        {
            Category::Security
        } else if lower.contains("performance")
            || lower.contains("optimization")
            || lower.contains("slow")
        {
            Category::Performance
        } else if lower.contains("bug") || lower.contains("fix") || lower.contains("error") {
            Category::Bug
        } else if lower.contains("style") || lower.contains("format") || lower.contains("naming") {
            Category::Style
        } else if lower.contains("documentation")
            || lower.contains("docstring")
            || lower.contains("comment")
        {
            Category::Documentation
        } else if lower.contains("test") || lower.contains("coverage") {
            Category::Testing
        } else if lower.contains("maintain")
            || lower.contains("complex")
            || lower.contains("readable")
        {
            Category::Maintainability
        } else if lower.contains("design")
            || lower.contains("architecture")
            || lower.contains("pattern")
        {
            Category::Architecture
        } else {
            Category::BestPractice
        }
    }

    /// `lower` must already be lowercased.
    fn calculate_confidence(lower: &str, severity: &Severity, _category: &Category) -> f32 {
        let mut confidence: f32 = 0.7; // Base confidence

        // ── Injection (high-confidence, well-defined patterns) ──
        if lower.contains("sql injection") {
            confidence += 0.2;
        }
        if lower.contains("command injection") || lower.contains("shell injection") {
            confidence += 0.2;
        }
        if lower.contains("xss") || lower.contains("cross-site scripting") {
            confidence += 0.2;
        }
        if lower.contains("path traversal") || lower.contains("directory traversal") {
            confidence += 0.2;
        }
        if lower.contains("code injection") || lower.contains("eval(") {
            confidence += 0.2;
        }
        if lower.contains("template injection") || lower.contains("ssti") {
            confidence += 0.15;
        }
        if lower.contains("ldap injection") {
            confidence += 0.15;
        }

        // ── Auth/AuthZ ──
        if lower.contains("missing authentication") || lower.contains("no auth") {
            confidence += 0.2;
        }
        if lower.contains("idor") || lower.contains("insecure direct object") {
            confidence += 0.15;
        }
        if lower.contains("csrf") || lower.contains("cross-site request forgery") {
            confidence += 0.2;
        }
        if lower.contains("jwt") && (lower.contains("none") || lower.contains("verify")) {
            confidence += 0.2;
        }
        if lower.contains("privilege escalation") {
            confidence += 0.15;
        }
        if lower.contains("weak password") || lower.contains("weak hash") || lower.contains("md5") {
            confidence += 0.15;
        }

        // ── Secrets ──
        if lower.contains("hardcoded")
            && (lower.contains("secret")
                || lower.contains("credential")
                || lower.contains("password")
                || lower.contains("key"))
        {
            confidence += 0.25;
        }
        if lower.contains("private key") {
            confidence += 0.25;
        }
        if lower.contains("api key") && lower.contains("hardcoded") {
            confidence += 0.2;
        }
        if lower.contains("connection string") && lower.contains("credential") {
            confidence += 0.2;
        }

        // ── Deserialization / SSRF ──
        if lower.contains("deserialization")
            || lower.contains("pickle")
            || lower.contains("unsafe yaml")
        {
            confidence += 0.2;
        }
        if lower.contains("ssrf") || lower.contains("server-side request forgery") {
            confidence += 0.15;
        }
        if lower.contains("xxe") {
            confidence += 0.15;
        }

        // ── Supply chain ──
        if lower.contains("dependency confusion") {
            confidence += 0.15;
        }
        if lower.contains("install script") || lower.contains("postinstall") {
            confidence += 0.1;
        }
        if lower.contains("lockfile") && lower.contains("tamper") {
            confidence += 0.2;
        }
        if lower.contains("unpinned") && lower.contains("action") {
            confidence += 0.1;
        }

        // ── Cryptography ──
        if lower.contains("weak cipher")
            || lower.contains(" des ")
            || lower.starts_with("des ")
            || lower.contains("3des")
            || lower.contains("rc4")
            || lower.contains("ecb mode")
        {
            confidence += 0.2;
        }
        if lower.contains("insecure tls")
            || lower.contains("sslv2")
            || lower.contains("sslv3")
            || lower.contains("tls 1.0")
        {
            confidence += 0.2;
        }
        if lower.contains("math.random") || lower.contains("insecure random") {
            confidence += 0.15;
        }
        if lower.contains("hardcoded iv")
            || lower.contains("hardcoded nonce")
            || lower.contains("static iv")
        {
            confidence += 0.2;
        }
        if lower.contains("timing attack") || lower.contains("constant-time") {
            confidence += 0.15;
        }
        if lower.contains("certificate validation") && lower.contains("disabled") {
            confidence += 0.2;
        }

        // ── Data exposure ──
        if lower.contains("pii") && lower.contains("log") {
            confidence += 0.15;
        }
        if lower.contains("stack trace") && lower.contains("response") {
            confidence += 0.15;
        }
        if lower.contains("debug") && lower.contains("production") {
            confidence += 0.2;
        }
        if lower.contains("missing") && lower.contains("security header") {
            confidence += 0.1;
        }

        // ── Unsafe code ──
        if lower.contains("transmute") || lower.contains("from_raw_parts") {
            confidence += 0.15;
        }
        if lower.contains("prototype pollution") {
            confidence += 0.2;
        }
        if lower.contains("mass assignment") {
            confidence += 0.15;
        }
        if lower.contains("redos") || lower.contains("catastrophic backtracking") {
            confidence += 0.15;
        }
        if lower.contains("buffer overflow") {
            confidence += 0.2;
        }
        if lower.contains("race condition") || lower.contains("toctou") {
            confidence += 0.15;
        }

        // ── Infrastructure ──
        if lower.contains("privileged") && lower.contains("container") {
            confidence += 0.2;
        }
        if lower.contains("running as root") {
            confidence += 0.15;
        }
        if lower.contains("publicly accessible") || lower.contains("0.0.0.0/0") {
            confidence += 0.2;
        }
        if lower.contains("iam") && (lower.contains("admin") || lower.contains("*")) {
            confidence += 0.15;
        }

        // ── API security ──
        if lower.contains("missing rate limit") || lower.contains("no rate limit") {
            confidence += 0.1;
        }
        if lower.contains("insecure file upload") || lower.contains("unrestricted upload") {
            confidence += 0.2;
        }
        if lower.contains("graphql") && lower.contains("depth") {
            confidence += 0.1;
        }

        // ── Correctness ──
        if lower.contains("null pointer") {
            confidence += 0.2;
        }
        if lower.contains("performance issue") || lower.contains("n+1") {
            confidence += 0.15;
        }

        // Adjust based on severity
        match severity {
            Severity::Error => confidence += 0.1,
            Severity::Warning => confidence += 0.05,
            _ => {}
        }

        // CWE references indicate high-confidence structured findings
        if lower.contains("cwe-") {
            confidence += 0.1;
        }

        confidence.clamp(0.1, 1.0)
    }

    /// `lower` must already be lowercased.
    fn extract_tags(lower: &str, category: &Category) -> Vec<String> {
        let mut tags = vec![category.as_str().to_string()];

        // ── Injection tags ──
        if lower.contains("sql") && lower.contains("injection") {
            tags.push("sql-injection".to_string());
        } else if lower.contains("sql") {
            tags.push("sql".to_string());
        }
        if lower.contains("injection") && !tags.iter().any(|t| t.contains("injection")) {
            tags.push("injection".to_string());
        }
        if lower.contains("command injection") || lower.contains("shell injection") {
            tags.push("command-injection".to_string());
        }
        if lower.contains("xss") || lower.contains("cross-site scripting") {
            tags.push("xss".to_string());
        }
        if lower.contains("template injection") || lower.contains("ssti") {
            tags.push("template-injection".to_string());
        }
        if lower.contains("ldap injection") {
            tags.push("ldap-injection".to_string());
        }
        if lower.contains("path traversal") || lower.contains("directory traversal") {
            tags.push("path-traversal".to_string());
        }
        if lower.contains("log injection") {
            tags.push("log-injection".to_string());
        }
        if lower.contains("code injection") {
            tags.push("code-injection".to_string());
        }

        // ── Auth/AuthZ tags ──
        if lower.contains("authentication") || lower.contains("missing auth") {
            tags.push("authentication".to_string());
        }
        if lower.contains("authorization") || lower.contains("access control") {
            tags.push("authorization".to_string());
        }
        if lower.contains("csrf") || lower.contains("cross-site request forgery") {
            tags.push("csrf".to_string());
        }
        if lower.contains("idor") || lower.contains("insecure direct object") {
            tags.push("idor".to_string());
        }
        if lower.contains("jwt") {
            tags.push("jwt".to_string());
        }
        if lower.contains("privilege escalation") {
            tags.push("privilege-escalation".to_string());
        }
        if lower.contains("session") && (lower.contains("fixation") || lower.contains("cookie")) {
            tags.push("session-management".to_string());
        }
        if lower.contains("oauth") {
            tags.push("oauth".to_string());
        }
        if lower.contains("password")
            && (lower.contains("weak")
                || lower.contains("hash")
                || lower.contains("md5")
                || lower.contains("sha1"))
        {
            tags.push("weak-password-hash".to_string());
        }

        // ── Secrets tags ──
        if lower.contains("hardcoded")
            && (lower.contains("secret")
                || lower.contains("credential")
                || lower.contains("key")
                || lower.contains("password")
                || lower.contains("token"))
        {
            tags.push("hardcoded-credential".to_string());
        }
        if lower.contains("private key") {
            tags.push("private-key".to_string());
        }
        if lower.contains("api key") {
            tags.push("api-key".to_string());
        }
        if lower.contains("connection string") {
            tags.push("connection-string".to_string());
        }

        // ── Deserialization / SSRF / XXE tags ──
        if lower.contains("deserialization") || lower.contains("pickle") {
            tags.push("deserialization".to_string());
        }
        if lower.contains("ssrf") || lower.contains("server-side request forgery") {
            tags.push("ssrf".to_string());
        }
        if lower.contains("xxe") {
            tags.push("xxe".to_string());
        }
        if lower.contains("open redirect") {
            tags.push("open-redirect".to_string());
        }
        if lower.contains("cors") {
            tags.push("cors".to_string());
        }

        // ── Supply-chain tags ──
        if lower.contains("supply chain") || lower.contains("supply-chain") {
            tags.push("supply-chain".to_string());
        }
        if lower.contains("dependency confusion") {
            tags.push("dependency-confusion".to_string());
        }
        if lower.contains("typosquat") {
            tags.push("typosquatting".to_string());
        }
        if lower.contains("install script") || lower.contains("postinstall") {
            tags.push("install-scripts".to_string());
        }
        if lower.contains("lockfile") {
            tags.push("lockfile".to_string());
        }
        if lower.contains("unpinned") {
            tags.push("unpinned-version".to_string());
        }

        // ── Cryptography tags ──
        if lower.contains("weak cipher")
            || lower.contains(" des ")
            || lower.starts_with("des ")
            || lower.contains("3des")
            || lower.contains("rc4")
            || lower.contains("blowfish")
        {
            tags.push("weak-cipher".to_string());
        }
        if lower.contains("ecb") && lower.contains("mode") {
            tags.push("ecb-mode".to_string());
        }
        if lower.contains("insecure tls") || (lower.contains("ssl") && lower.contains("insecure")) {
            tags.push("insecure-tls".to_string());
        }
        if lower.contains("insecure random")
            || lower.contains("math.random")
            || lower.contains("math/rand")
        {
            tags.push("insecure-random".to_string());
        }
        if lower.contains("weak key") {
            tags.push("weak-key-size".to_string());
        }
        if lower.contains("hardcoded iv")
            || lower.contains("hardcoded nonce")
            || lower.contains("static iv")
        {
            tags.push("hardcoded-iv".to_string());
        }
        if lower.contains("timing attack") {
            tags.push("timing-attack".to_string());
        }
        if lower.contains("certificate") && lower.contains("validation") {
            tags.push("cert-validation".to_string());
        }

        // ── Data exposure tags ──
        if lower.contains("pii") {
            tags.push("pii".to_string());
        }
        if lower.contains("stack trace") || lower.contains("verbose error") {
            tags.push("verbose-error".to_string());
        }
        if lower.contains("debug mode") {
            tags.push("debug-mode".to_string());
        }
        if lower.contains("security header") || lower.contains("missing security header") {
            tags.push("security-headers".to_string());
        }
        if lower.contains("information disclosure") || lower.contains("data exposure") {
            tags.push("information-disclosure".to_string());
        }
        if lower.contains("directory listing") {
            tags.push("directory-listing".to_string());
        }

        // ── Unsafe code tags ──
        if lower.contains("unsafe") && lower.contains("rust") {
            tags.push("rust-unsafe".to_string());
        }
        if lower.contains("transmute") {
            tags.push("transmute".to_string());
        }
        if lower.contains("buffer overflow") {
            tags.push("buffer-overflow".to_string());
        }
        if lower.contains("prototype pollution") {
            tags.push("prototype-pollution".to_string());
        }
        if lower.contains("mass assignment") {
            tags.push("mass-assignment".to_string());
        }
        if lower.contains("race condition") || lower.contains("toctou") {
            tags.push("race-condition".to_string());
        }
        if lower.contains("redos") || lower.contains("catastrophic backtracking") {
            tags.push("redos".to_string());
        }
        if lower.contains("integer overflow") {
            tags.push("integer-overflow".to_string());
        }
        if lower.contains("resource leak") || lower.contains("handle leak") {
            tags.push("resource-leak".to_string());
        }

        // ── Infrastructure tags ──
        if lower.contains("docker") {
            tags.push("docker".to_string());
        }
        if lower.contains("kubernetes") || lower.contains("k8s") {
            tags.push("kubernetes".to_string());
        }
        if lower.contains("terraform") {
            tags.push("terraform".to_string());
        }
        if lower.contains("helm") {
            tags.push("helm".to_string());
        }
        if lower.contains("privileged") && lower.contains("container") {
            tags.push("privileged-container".to_string());
        }
        if lower.contains("iam") && (lower.contains("policy") || lower.contains("permission")) {
            tags.push("iam".to_string());
        }
        if lower.contains("running as root") {
            tags.push("root-container".to_string());
        }

        // ── API security tags ──
        if lower.contains("rate limit") {
            tags.push("rate-limiting".to_string());
        }
        if lower.contains("pagination") || lower.contains("unbounded query") {
            tags.push("pagination".to_string());
        }
        if lower.contains("graphql") {
            tags.push("graphql".to_string());
        }
        if lower.contains("file upload") {
            tags.push("file-upload".to_string());
        }
        if lower.contains("input validation") {
            tags.push("input-validation".to_string());
        }

        // ── CWE / OWASP tags ──
        // Extract all CWE numbers from content
        {
            let mut search_from = 0;
            while let Some(offset) = lower[search_from..].find("cwe-") {
                let pos = search_from + offset;
                let cwe_rest = &lower[pos..];
                let cwe_tag: String = cwe_rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-')
                    .collect();
                if cwe_tag.len() > 4 && !tags.contains(&cwe_tag) {
                    tags.push(cwe_tag);
                }
                search_from = pos + 4; // skip past "cwe-" to find next
            }
        }

        // ── Performance tags ──
        if lower.contains("n+1") {
            tags.push("n+1-query".to_string());
        }
        if lower.contains("memory") {
            tags.push("memory".to_string());
        }
        if lower.contains("cache") {
            tags.push("caching".to_string());
        }

        // ── Code quality tags ──
        if lower.contains("duplicate") {
            tags.push("duplication".to_string());
        }
        if lower.contains("complex") {
            tags.push("complexity".to_string());
        }
        if lower.contains("deprecated") {
            tags.push("deprecated".to_string());
        }

        tags
    }

    /// `lower` must already be lowercased.
    fn determine_fix_effort(lower: &str, category: &Category) -> FixEffort {
        // High effort indicators
        if lower.contains("architecture")
            || lower.contains("refactor")
            || lower.contains("redesign")
        {
            return FixEffort::High;
        }

        // Security issues often require careful consideration
        if matches!(category, Category::Security)
            && (lower.contains("injection") || lower.contains("vulnerability"))
        {
            return FixEffort::Medium;
        }

        // Performance issues might need investigation
        if matches!(category, Category::Performance) && lower.contains("n+1") {
            return FixEffort::Medium;
        }

        // Style and documentation are usually quick fixes
        if matches!(category, Category::Style | Category::Documentation) {
            return FixEffort::Low;
        }

        FixEffort::Medium
    }

    fn generate_code_suggestion(raw: &RawComment) -> Option<CodeSuggestion> {
        // Prefer the structured code suggestion parsed from the LLM response
        if let Some(cs) = &raw.code_suggestion {
            return Some(cs.clone());
        }

        // Fallback: generate a basic suggestion from the textual suggestion field
        if let Some(suggestion) = &raw.suggestion {
            let has_action_word = suggestion
                .split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("use") || w.eq_ignore_ascii_case("replace"));
            if has_action_word {
                return Some(CodeSuggestion {
                    original_code: "// Original code would be extracted from context".to_string(),
                    suggested_code: suggestion.clone(),
                    explanation: "Improved implementation following best practices".to_string(),
                    diff: format!("- original\n+ {}", suggestion),
                });
            }
        }
        None
    }

    fn calculate_overall_score(comments: &[Comment]) -> f32 {
        if comments.is_empty() {
            return 10.0;
        }

        let mut score: f32 = 10.0;
        for comment in comments {
            let penalty = match comment.severity {
                Severity::Error => 2.0,
                Severity::Warning => 1.0,
                Severity::Info => 0.3,
                Severity::Suggestion => 0.1,
            };
            score -= penalty;
        }

        score.clamp(0.0, 10.0)
    }

    fn generate_recommendations(comments: &[Comment]) -> Vec<String> {
        let mut recommendations = Vec::new();
        let mut security_count = 0;
        let mut performance_count = 0;
        let mut style_count = 0;

        for comment in comments {
            match comment.category {
                Category::Security => security_count += 1,
                Category::Performance => performance_count += 1,
                Category::Style => style_count += 1,
                _ => {}
            }
        }

        if security_count > 0 {
            recommendations.push(format!(
                "Address {} security issue(s) immediately",
                security_count
            ));
        }
        if performance_count > 2 {
            recommendations.push(
                "Consider a performance audit - multiple optimization opportunities found"
                    .to_string(),
            );
        }
        if style_count > 5 {
            recommendations
                .push("Consider setting up automated linting to catch style issues".to_string());
        }

        recommendations
    }

    fn deduplicate_comments(comments: &mut Vec<Comment>) {
        let severity_rank = |s: &Severity| match s {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
            Severity::Suggestion => 3,
        };

        // Sort by file/line/content, then by severity (highest first)
        comments.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then(a.line_number.cmp(&b.line_number))
                .then(a.content.cmp(&b.content))
                .then(severity_rank(&a.severity).cmp(&severity_rank(&b.severity)))
        });
        // dedup_by keeps the first element (b) of consecutive duplicates,
        // which is the highest severity due to our sort order
        comments.dedup_by(|a, b| {
            a.file_path == b.file_path && a.line_number == b.line_number && a.content == b.content
        });
    }

    fn sort_by_priority(comments: &mut [Comment]) {
        comments.sort_by(|a, b| {
            let severity_rank = |s: &Severity| match s {
                Severity::Error => 0,
                Severity::Warning => 1,
                Severity::Info => 2,
                Severity::Suggestion => 3,
            };
            let category_rank = |c: &Category| match c {
                Category::Security => 0,
                Category::Bug => 1,
                Category::Performance => 2,
                Category::BestPractice => 3,
                Category::Style => 4,
                Category::Documentation => 5,
                Category::Maintainability => 6,
                Category::Testing => 7,
                Category::Architecture => 8,
            };
            severity_rank(&a.severity)
                .cmp(&severity_rank(&b.severity))
                .then_with(|| category_rank(&a.category).cmp(&category_rank(&b.category)))
                .then_with(|| a.file_path.cmp(&b.file_path))
                .then_with(|| a.line_number.cmp(&b.line_number))
        });
    }
}

pub fn compute_comment_id(file_path: &Path, content: &str, category: &Category) -> String {
    let normalized = normalize_content(content);
    let key = format!("{}|{}|{}", file_path.display(), category, normalized);
    let hash = fnv1a64(key.as_bytes());
    format!("cmt_{:016x}", hash)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn normalize_content(content: &str) -> String {
    let mut normalized = String::new();
    let mut last_space = false;

    for ch in content.chars() {
        let ch = if ch.is_ascii_digit() {
            '#'
        } else {
            ch.to_ascii_lowercase()
        };

        if ch.is_whitespace() {
            if !last_space {
                normalized.push(' ');
                last_space = true;
            }
        } else {
            normalized.push(ch);
            last_space = false;
        }
    }

    normalized.trim().to_string()
}

#[derive(Debug)]
pub struct RawComment {
    pub file_path: PathBuf,
    pub line_number: usize,
    pub content: String,
    pub rule_id: Option<String>,
    pub suggestion: Option<String>,
    pub severity: Option<Severity>,
    pub category: Option<Category>,
    pub confidence: Option<f32>,
    pub fix_effort: Option<FixEffort>,
    pub tags: Vec<String>,
    pub code_suggestion: Option<CodeSuggestion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplicate_preserves_highest_severity() {
        // Regression: dedup_by keeps the first element of a consecutive pair,
        // but doesn't consider severity. If Warning comes before Error
        // (due to stable sort on file/line/content), the Error is dropped.
        let raw_comments = vec![
            RawComment {
                file_path: PathBuf::from("src/lib.rs"),
                line_number: 10,
                content: "Missing null check".to_string(),
                rule_id: None,
                suggestion: None,
                severity: Some(Severity::Warning),
                category: Some(Category::Bug),
                confidence: Some(0.8),
                fix_effort: None,
                tags: Vec::new(),
                code_suggestion: None,
            },
            RawComment {
                file_path: PathBuf::from("src/lib.rs"),
                line_number: 10,
                content: "Missing null check".to_string(),
                rule_id: None,
                suggestion: None,
                severity: Some(Severity::Error),
                category: Some(Category::Bug),
                confidence: Some(0.9),
                fix_effort: None,
                tags: Vec::new(),
                code_suggestion: None,
            },
        ];

        let comments = CommentSynthesizer::synthesize(raw_comments).unwrap();
        assert_eq!(comments.len(), 1, "Should deduplicate to one comment");
        assert_eq!(
            comments[0].severity,
            Severity::Error,
            "Should keep the higher severity (Error), not the lower (Warning)"
        );
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Error.to_string(), "Error");
        assert_eq!(Severity::Warning.to_string(), "Warning");
        assert_eq!(Severity::Info.to_string(), "Info");
        assert_eq!(Severity::Suggestion.to_string(), "Suggestion");
    }

    #[test]
    fn test_category_display() {
        assert_eq!(Category::Bug.to_string(), "Bug");
        assert_eq!(Category::Security.to_string(), "Security");
        assert_eq!(Category::Performance.to_string(), "Performance");
        assert_eq!(Category::Style.to_string(), "Style");
        assert_eq!(Category::Documentation.to_string(), "Documentation");
        assert_eq!(Category::BestPractice.to_string(), "BestPractice");
        assert_eq!(Category::Maintainability.to_string(), "Maintainability");
        assert_eq!(Category::Testing.to_string(), "Testing");
        assert_eq!(Category::Architecture.to_string(), "Architecture");
    }

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Info.as_str(), "info");
        assert_eq!(Severity::Suggestion.as_str(), "suggestion");
    }

    #[test]
    fn test_category_as_str() {
        assert_eq!(Category::Bug.as_str(), "bug");
        assert_eq!(Category::Security.as_str(), "security");
        assert_eq!(Category::BestPractice.as_str(), "bestpractice");
    }
}
