use super::signals::{contains_any_word, contains_word, mentions_weak_cipher};
use super::{Category, Severity};

/// `lower` must already be lowercased.
pub(super) fn calculate_confidence(lower: &str, severity: &Severity, _category: &Category) -> f32 {
    let mut confidence: f32 = 0.7;

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

    if lower.contains("missing rate limit") || lower.contains("no rate limit") {
        confidence += 0.1;
    }
    if lower.contains("insecure file upload") || lower.contains("unrestricted upload") {
        confidence += 0.2;
    }
    if lower.contains("graphql") && lower.contains("depth") {
        confidence += 0.1;
    }

    if lower.contains("null pointer") {
        confidence += 0.2;
    }
    if lower.contains("performance issue") || lower.contains("n+1") {
        confidence += 0.15;
    }

    match severity {
        Severity::Error => confidence += 0.1,
        Severity::Warning => confidence += 0.05,
        _ => {}
    }

    if lower.contains("cwe-") {
        confidence += 0.1;
    }

    confidence.clamp(0.1, 1.0)
}
