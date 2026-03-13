pub(super) fn contains_word(haystack: &str, needle: &str) -> bool {
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

pub(super) fn contains_any_word(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| contains_word(haystack, needle))
}

pub(super) fn contains_any_phrase(haystack: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| haystack.contains(phrase))
}

pub(super) fn contains_action_word(text: &str) -> bool {
    let lower = text.to_lowercase();
    contains_any_word(
        &lower,
        &[
            "add", "avoid", "check", "guard", "move", "remove", "rename", "replace", "use",
        ],
    )
}

pub(super) fn mentions_weak_cipher(lower: &str) -> bool {
    lower.contains("weak cipher")
        || contains_word(lower, "des")
        || lower.contains("3des")
        || lower.contains("rc4")
        || lower.contains("blowfish")
        || lower.contains("ecb mode")
}

pub(super) fn has_security_signal(lower: &str) -> bool {
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

pub(super) fn has_performance_signal(lower: &str) -> bool {
    lower.contains("performance") || lower.contains("optimiz") || lower.contains("slow")
}

pub(super) fn has_bug_signal(lower: &str) -> bool {
    contains_word(lower, "bug")
        || contains_word(lower, "error")
        || contains_word(lower, "fix")
        || lower.contains("fixed")
        || lower.contains("fixes")
        || lower.contains("fixing")
}

pub(super) fn has_style_signal(lower: &str) -> bool {
    lower.contains("style") || lower.contains("format") || lower.contains("naming")
}

pub(super) fn has_documentation_signal(lower: &str) -> bool {
    lower.contains("documentation")
        || lower.contains("docstring")
        || contains_word(lower, "comment")
}

pub(super) fn has_testing_signal(lower: &str) -> bool {
    lower.contains("test") || lower.contains("coverage")
}

pub(super) fn has_maintainability_signal(lower: &str) -> bool {
    lower.contains("maintain") || lower.contains("complex") || lower.contains("readable")
}

pub(super) fn has_architecture_signal(lower: &str) -> bool {
    lower.contains("design") || lower.contains("architecture") || contains_word(lower, "pattern")
}

fn is_ascii_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
