use anyhow::Result;
#[path = "comment/identity.rs"]
mod identity;
#[path = "comment/ordering.rs"]
mod ordering;
#[path = "comment/suggestions.rs"]
mod suggestions;
#[path = "comment/summary.rs"]
mod summary;
#[path = "comment/types.rs"]
mod types;

use ordering::{
    deduplicate_comments as deduplicate_comment_list, sort_by_priority as sort_comments_by_priority,
};
#[cfg(test)]
use std::path::PathBuf;
use suggestions::generate_code_suggestion;
use summary::generate_summary as build_review_summary;

pub use identity::compute_comment_id;
pub use types::{
    Category, CodeSuggestion, Comment, FixEffort, RawComment, ReviewSummary, Severity,
};

fn is_ascii_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }

    haystack.match_indices(needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ascii_word_byte(haystack.as_bytes()[start - 1]);
        let after_ok = end == haystack.len() || !is_ascii_word_byte(haystack.as_bytes()[end]);
        before_ok && after_ok
    })
}

fn contains_any_word(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| contains_word(haystack, needle))
}

fn contains_any_phrase(haystack: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| haystack.contains(phrase))
}

fn push_unique_tag(tags: &mut Vec<String>, tag: &str) {
    if !tags.iter().any(|existing| existing == tag) {
        tags.push(tag.to_string());
    }
}

fn append_cwe_tags(tags: &mut Vec<String>, lower: &str) {
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("cwe-") {
        let pos = search_from + offset;
        let cwe_rest = &lower[pos..];
        let cwe_tag: String = cwe_rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        if cwe_tag.len() > 4 {
            push_unique_tag(tags, &cwe_tag);
        }
        search_from = pos + 4;
    }
}

fn contains_action_word(text: &str) -> bool {
    let lower = text.to_lowercase();
    contains_any_word(
        &lower,
        &[
            "add", "avoid", "check", "guard", "move", "remove", "rename", "replace", "use",
        ],
    )
}

fn mentions_weak_cipher(lower: &str) -> bool {
    lower.contains("weak cipher")
        || contains_word(lower, "des")
        || lower.contains("3des")
        || lower.contains("rc4")
        || lower.contains("blowfish")
        || lower.contains("ecb mode")
}

fn has_security_signal(lower: &str) -> bool {
    lower.contains("security")
        || lower.contains("vulnerab")
        || lower.contains("inject")
        || lower.contains("authentication")
        || lower.contains("authorization")
        || lower.contains("transmute")
        || lower.contains("overpermissive")
        || contains_any_word(
            lower,
            &[
                "xss",
                "csrf",
                "ssrf",
                "jwt",
                "idor",
                "owasp",
                "xxe",
                "cors",
                "pii",
                "hostnetwork",
                "toctou",
                "redos",
            ],
        )
        || contains_any_phrase(
            lower,
            &[
                "sql injection",
                "command injection",
                "cross-site scripting",
                "cross-site request forgery",
                "server-side request forgery",
                "deserialization",
                "path traversal",
                "directory traversal",
                "hardcoded secret",
                "hardcoded credential",
                "hardcoded password",
                "api key",
                "private key",
                "access control",
                "privilege escalation",
                "insecure direct object",
                "supply chain",
                "supply-chain",
                "dependency confusion",
                "typosquatting",
                "open redirect",
                "template injection",
                "ldap injection",
                "log injection",
                "code injection",
                "unsafe pickle",
                "unsafe yaml",
                "weak hash",
                "weak password",
                "insecure tls",
                "insecure ssl",
                "insecure random",
                "math.random",
                "weak key",
                "broken hash",
                "hardcoded iv",
                "hardcoded nonce",
                "timing attack",
                "certificate validation",
                "cert validation",
                "data exposure",
                "data leak",
                "debug mode",
                "stack trace",
                "verbose error",
                "information disclosure",
                "security header",
                "missing security header",
                "unsafe block",
                "unsafe {",
                "buffer overflow",
                "prototype pollution",
                "mass assignment",
                "race condition",
                "catastrophic backtracking",
                "running as root",
                "privileged container",
                "publicly accessible",
                "iam policy",
                "rate limit",
                "no pagination",
                "unbounded query",
                "graphql depth",
                "insecure upload",
                "unrestricted upload",
            ],
        )
        || (lower.contains("file upload")
            && contains_any_phrase(
                lower,
                &["insecure", "unrestricted", "vulnerability", "security"],
            ))
        || (lower.contains("input validation")
            && contains_any_phrase(
                lower,
                &["missing", "vulnerability", "security", "injection"],
            ))
        || mentions_weak_cipher(lower)
}

fn has_performance_signal(lower: &str) -> bool {
    lower.contains("performance") || lower.contains("optimiz") || lower.contains("slow")
}

fn has_bug_signal(lower: &str) -> bool {
    contains_word(lower, "bug")
        || contains_word(lower, "error")
        || contains_word(lower, "fix")
        || lower.contains("fixed")
        || lower.contains("fixes")
        || lower.contains("fixing")
}

fn has_style_signal(lower: &str) -> bool {
    lower.contains("style") || lower.contains("format") || lower.contains("naming")
}

fn has_documentation_signal(lower: &str) -> bool {
    lower.contains("documentation")
        || lower.contains("docstring")
        || contains_word(lower, "comment")
}

fn has_testing_signal(lower: &str) -> bool {
    lower.contains("test") || lower.contains("coverage")
}

fn has_maintainability_signal(lower: &str) -> bool {
    lower.contains("maintain") || lower.contains("complex") || lower.contains("readable")
}

fn has_architecture_signal(lower: &str) -> bool {
    lower.contains("design") || lower.contains("architecture") || contains_word(lower, "pattern")
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
        build_review_summary(comments)
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
        let code_suggestion = generate_code_suggestion(&raw);
        let id = compute_comment_id(&raw.file_path, &raw.content, &category);

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
        if has_security_signal(lower) || lower.contains("cwe-") {
            Category::Security
        } else if has_performance_signal(lower) {
            Category::Performance
        } else if has_bug_signal(lower) {
            Category::Bug
        } else if has_style_signal(lower) {
            Category::Style
        } else if has_documentation_signal(lower) {
            Category::Documentation
        } else if has_testing_signal(lower) {
            Category::Testing
        } else if has_maintainability_signal(lower) {
            Category::Maintainability
        } else if has_architecture_signal(lower) {
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
        if contains_word(lower, "xss") || lower.contains("cross-site scripting") {
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
        if contains_word(lower, "idor") || lower.contains("insecure direct object") {
            confidence += 0.15;
        }
        if contains_word(lower, "csrf") || lower.contains("cross-site request forgery") {
            confidence += 0.2;
        }
        if contains_word(lower, "jwt") && (lower.contains("none") || lower.contains("verify")) {
            confidence += 0.2;
        }
        if lower.contains("privilege escalation") {
            confidence += 0.15;
        }
        if lower.contains("weak password")
            || lower.contains("weak hash")
            || contains_any_word(lower, &["md5", "sha1"])
        {
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
        if contains_word(lower, "ssrf") || lower.contains("server-side request forgery") {
            confidence += 0.15;
        }
        if contains_word(lower, "xxe") {
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
        if mentions_weak_cipher(lower) {
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
        if contains_word(lower, "pii") && contains_word(lower, "log") {
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
        if contains_word(lower, "redos") || lower.contains("catastrophic backtracking") {
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
        if contains_word(lower, "xss") || lower.contains("cross-site scripting") {
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
        if contains_word(lower, "csrf") || lower.contains("cross-site request forgery") {
            tags.push("csrf".to_string());
        }
        if contains_word(lower, "idor") || lower.contains("insecure direct object") {
            tags.push("idor".to_string());
        }
        if contains_word(lower, "jwt") {
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
        if contains_word(lower, "ssrf") || lower.contains("server-side request forgery") {
            tags.push("ssrf".to_string());
        }
        if contains_word(lower, "xxe") {
            tags.push("xxe".to_string());
        }
        if lower.contains("open redirect") {
            tags.push("open-redirect".to_string());
        }
        if contains_word(lower, "cors") {
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
        if mentions_weak_cipher(lower) {
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
        if contains_word(lower, "pii") {
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
        if contains_word(lower, "redos") || lower.contains("catastrophic backtracking") {
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
        append_cwe_tags(&mut tags, lower);

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

    fn deduplicate_comments(comments: &mut Vec<Comment>) {
        deduplicate_comment_list(comments);
    }

    fn sort_by_priority(comments: &mut [Comment]) {
        sort_comments_by_priority(comments);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_raw_comment(content: &str) -> RawComment {
        RawComment {
            file_path: PathBuf::from("src/lib.rs"),
            line_number: 10,
            content: content.to_string(),
            rule_id: None,
            suggestion: None,
            severity: None,
            category: None,
            confidence: None,
            fix_effort: None,
            tags: Vec::new(),
            code_suggestion: None,
        }
    }

    fn synthesize_single(content: &str) -> Comment {
        CommentSynthesizer::process_raw_comment(make_raw_comment(content)).unwrap()
    }

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

    #[test]
    fn test_security_regression_cases_are_classified_and_tagged() {
        let cases = [
            (
                "Running as root in Docker container (CWE-250)",
                Category::Security,
                vec!["docker", "root-container", "cwe-250"],
            ),
            (
                "Unsafe deserialization via pickle.load can trigger RCE (CWE-502)",
                Category::Security,
                vec!["deserialization", "cwe-502"],
            ),
            (
                "JWT verification is missing and enables auth bypass (CWE-347)",
                Category::Security,
                vec!["jwt", "cwe-347"],
            ),
        ];

        for (content, expected_category, expected_tags) in cases {
            let comment = synthesize_single(content);
            assert_eq!(comment.category, expected_category, "content: {content}");
            for tag in expected_tags {
                assert!(
                    comment.tags.iter().any(|existing| existing == tag),
                    "missing tag `{tag}` for content `{content}`: {:?}",
                    comment.tags
                );
            }
        }
    }

    #[test]
    fn test_extract_tags_collects_multiple_cwes() {
        let comment = synthesize_single(
            "SQL injection (CWE-89) can combine with XSS (CWE-79) in the same flow",
        );
        assert!(comment.tags.iter().any(|tag| tag == "cwe-89"));
        assert!(comment.tags.iter().any(|tag| tag == "cwe-79"));
    }

    #[test]
    fn test_deserialization_does_not_trigger_weak_cipher_tag() {
        let comment = synthesize_single("Unsafe deserialization via yaml.load on untrusted input");
        assert!(comment.tags.iter().any(|tag| tag == "deserialization"));
        assert!(!comment.tags.iter().any(|tag| tag == "weak-cipher"));
    }

    #[test]
    fn test_generate_code_suggestion_accepts_more_action_words() {
        let mut raw = make_raw_comment("Missing null check before dereference");
        raw.suggestion = Some("Add a guard clause before dereferencing the value".to_string());

        let comment = CommentSynthesizer::process_raw_comment(raw).unwrap();
        assert!(comment.code_suggestion.is_some());
    }

    #[test]
    fn test_generate_code_suggestion_ignores_non_action_words() {
        let mut raw = make_raw_comment("Suggestion parsing regression");
        raw.suggestion = Some("Reusable helper already exists for this code path".to_string());

        let comment = CommentSynthesizer::process_raw_comment(raw).unwrap();
        assert!(comment.code_suggestion.is_none());
    }
}
