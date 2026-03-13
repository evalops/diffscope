use super::signals::{contains_word, mentions_weak_cipher};
use super::Category;

/// `lower` must already be lowercased.
pub(super) fn extract_tags(lower: &str, category: &Category) -> Vec<String> {
    let mut tags = vec![category.as_str().to_string()];

    if lower.contains("sql") && lower.contains("injection") {
        tags.push("sql-injection".to_string());
    } else if lower.contains("sql") {
        tags.push("sql".to_string());
    }
    if lower.contains("injection") && !tags.iter().any(|tag| tag.contains("injection")) {
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

    append_cwe_tags(&mut tags, lower);

    if lower.contains("n+1") {
        tags.push("n+1-query".to_string());
    }
    if lower.contains("memory") {
        tags.push("memory".to_string());
    }
    if lower.contains("cache") {
        tags.push("caching".to_string());
    }

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
            .take_while(|ch| ch.is_alphanumeric() || *ch == '-')
            .collect();
        if cwe_tag.len() > 4 {
            push_unique_tag(tags, &cwe_tag);
        }
        search_from = pos + 4;
    }
}
