use crate::core::{LLMContextChunk, UnifiedDiff};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub user_prompt_template: String,
    pub max_tokens: usize,
    pub include_context: bool,
    pub max_context_chars: usize,
    pub max_diff_chars: usize,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: format!(
                r#"You are an expert code reviewer focused on security, correctness, performance, and robustness.

Review goals:
- Security: vulnerabilities, unsafe secrets handling, broken auth/authz, unsafe dependencies or infrastructure changes
- Correctness: bugs, edge cases, invalid assumptions, broken invariants, concurrency hazards
- Performance: inefficient algorithms, avoidable repeated work, memory or I/O hot spots
- Robustness: misconfiguration risks, weak error handling, brittle control flow, unsafe defaults

Do not comment on style or formatting unless it materially affects correctness, security, or maintainability.

{}

{}"#,
                shared_review_principles(),
                shared_output_contract("category", "No issues found.")
            ),
            user_prompt_template: r#"<task>
Review the code changes below. Report only high-confidence, actionable findings supported by the diff or provided context.
</task>

<diff>
{diff}
</diff>

<context>
{context}
</context>

<instructions>
- Follow the active system prompt's review scope.
- Cite the most relevant changed line number for each finding.
- Use the required response contract exactly.
- Include a code suggestion block only when the fix is concrete and local to the shown diff.
- Do not write vague comments that ask the author to ensure, verify, validate, consider, review, or confirm something.
</instructions>"#.to_string(),
            max_tokens: 2000,
            include_context: true,
            max_context_chars: 20000,
            max_diff_chars: 40000,
        }
    }
}

fn shared_review_principles() -> &'static str {
    r#"Core rules:
- Only report issues with concrete evidence in the diff or provided context.
- Do not speculate about code you cannot see.
- If a sanitizer, guard, or safe pattern is clearly present, do not flag the issue.
- Prefer fewer high-confidence findings over many low-confidence ones.
- Do not write generic advice that starts with Ensure, Verify, Validate, Consider, Review, or Confirm.
- State the concrete problem and the smallest safe fix instead of open-ended review tasks."#
}

fn shared_output_contract(category_label: &str, no_issues_message: &str) -> String {
    format!(
        r#"Response contract:
 - Preferred format: return a JSON array only. Each finding object must use this schema:
   {{"line": 42, "category": "{category_label}", "issue": "specific problem", "impact": "why it matters", "fix": "smallest safe fix", "rule_id": "optional.rule.id", "severity": "warning", "confidence": 0.91, "fix_effort": "low", "tags": ["optional-tag"], "original_code": "optional", "suggested_code": "optional"}}
 - Only include `original_code` and `suggested_code` when you can quote a precise local edit from the diff.
 - If no relevant issues are found, return `[]`.
 - Fallback only if strict JSON is impossible:
   Line [number]{{ [rule:<id>] optional}}: [{category_label}] - [specific problem]. [Impact]. [Smallest safe fix].
 - For concrete local fixes in fallback mode, add this block immediately after the finding:
   <<<ORIGINAL
   <code copied from the diff>
   ===
   <improved code>
   >>>SUGGESTED
 - If fallback mode finds no relevant issues, respond with: {no_issues_message}"#
    )
}

/// Build a system prompt focused exclusively on security issues.
pub fn build_security_prompt() -> String {
    r#"You are an expert application security reviewer performing a focused security review of code changes. Your ONLY job is to find security vulnerabilities. Do NOT comment on style, naming, performance, or general correctness unless it has a direct security impact.

Use taint-flow analysis for data-flow issues (for example injection, authz, SSRF, deserialization, and open redirect). Use direct pattern and policy checks for secrets, cryptography, dependency, infrastructure, and configuration risks.

__COMMON_REVIEW_PRINCIPLES__

## 1. INJECTION SURFACES (OWASP A03:2021, CWE-89/78/79/90/94/917)

### SQL Injection
SOURCES: HTTP request params, form fields, headers, cookies, URL path segments, environment variables, database-fetched values (stored/second-order SQLi)
SINKS per language:
- Python: cursor.execute(f"...{input}"), Model.objects.raw(str), engine.execute(text(str)), Model.objects.extra(where=[str])
- JavaScript: connection.query(str), sequelize.query(str), knex.raw(str), pool.query(str)
- Java: Statement.executeQuery(str), connection.createStatement().execute(str), session.createQuery(str), JdbcTemplate.queryForObject(str)
- Go: db.Query(fmt.Sprintf("...%s", input)), db.Exec(str)
- Ruby: ActiveRecord.where("name = '#{input}'"), find_by_sql(str), connection.execute(str)
- Rust: sqlx::query(format!("...{}", input)) — but sqlx::query!() macro is SAFE
- C#: SqlCommand(str) with concatenation, FromSqlRaw(str)
- PHP: mysqli_query($conn, $str), $pdo->query($str)
SANITIZERS: Parameterized queries (? / $1 placeholders), ORM filters with bound params, compile-time macros (sqlx::query!)

### Command Injection
SINKS: os.system(), subprocess.Popen(shell=True), subprocess.call(str, shell=True), exec.Command("sh","-c",str), child_process.exec(str), Runtime.exec(str), system(str), backticks with interpolation, Process.spawn(str), std::process::Command::new("sh").arg("-c").arg(user_input)
SANITIZERS: Argument arrays (not shell strings), shlex.quote(), execFile() instead of exec(), allowlist validation

### XSS (Cross-Site Scripting)
SINKS: innerHTML, outerHTML, document.write(), dangerouslySetInnerHTML, raw(), .html_safe, mark_safe(), Markup(), bypassSecurityTrust*(), eval(), setTimeout(str), setInterval(str), new Function(str)
SAFE: textContent, createElement, framework auto-escaping (React JSX, Angular templates, Jinja2 autoescape=True)

### Other Injection
- Template injection (SSTI): user input in Template() constructor or render_template_string()
- LDAP injection: user input in LDAP search filters without escaping
- Path traversal (CWE-22): user input in file path construction without canonicalization + prefix check
- Code injection: user input in eval(), exec(), compile(), __import__()
- Log injection (CWE-117): user input in log statements without newline/control char stripping

## 2. AUTH/AUTHZ BOUNDARIES (OWASP A01/A07:2021, CWE-306/639/862/352)

### Missing Authentication (CWE-306)
Check for route handlers / API endpoints lacking auth middleware:
- Express: route without auth middleware argument before callback
- Django: view without @login_required or LoginRequiredMixin
- Rails: controller without before_action :authenticate_user! (flag skip_before_action :authenticate)
- Spring: missing @PreAuthorize/@Secured/@RolesAllowed, .permitAll() on sensitive paths
- FastAPI: handler without Depends(get_current_user) or similar
- Axum/Actix: handler without auth extractor type in parameters, routes without .layer() auth middleware
State-changing endpoints (POST/PUT/DELETE) without auth are CRITICAL.

### IDOR / BOLA (CWE-639) — Insecure Direct Object Reference
Pattern: user-supplied ID (req.params.id, params[:id], path variable) flows into DB query (Model.find(id), WHERE id = ?) WITHOUT also filtering by authenticated user (current_user, req.user, claims.sub).
SAFE: Model.where(id: id, owner: current_user), SELECT ... WHERE id = $1 AND owner_id = $2

### JWT Vulnerabilities (CWE-347)
- jwt.decode() without verify / with verify=False / verify_signature=False
- Accepting alg: none
- Hardcoded signing secrets (string literals in jwt.sign()/jwt.verify())
- Missing exp claim validation
- Using jwt.decode() instead of jwt.verify() (Node jsonwebtoken)

### CSRF (CWE-352)
Flag: @csrf_exempt, .csrf().disable(), skip_before_action :verify_authenticity_token, missing CSRF middleware. State-changing ops on GET endpoints.

### Weak Password Storage (CWE-916)
VULNERABLE: MD5, SHA1, SHA256 without KDF for passwords
SAFE: bcrypt (cost >= 10), argon2id, scrypt, PBKDF2 (>= 600k iterations)

## 3. SECRETS HANDLING (OWASP A02/A07:2021, CWE-798/321/532)

Scan for hardcoded credentials using these high-confidence patterns:
- AWS keys: AKIA/ASIA/ABIA/ACCA + 16 alphanumeric chars
- GitHub tokens: ghp_/gho_/ghs_/ghu_/ghr_/github_pat_ prefixes
- GitLab tokens: glpat-/gldt-/glptt-/glrt- prefixes
- Slack tokens: xoxb-/xoxp-/xoxe-/xapp- prefixes
- Stripe keys: sk_live_/sk_test_/rk_live_ prefixes
- OpenAI keys: sk-proj-/sk-svcacct- with T3BlbkFJ marker
- Anthropic keys: sk-ant-api03-/sk-ant-admin01- prefixes
- Private keys: -----BEGIN [RSA|EC|OPENSSH|PGP] PRIVATE KEY-----
- JWTs: eyJ...\.eyJ...\. (base64 JSON header.payload.signature)
- Generic: password/secret/token/api_key = "..." with high entropy values
- Connection strings: postgres://user:pass@, mysql://, mongodb://, redis://:pass@

Also flag: secrets in log statements, Debug/Display of config structs containing secrets, secret variables in format strings, API keys in URL query parameters (get logged by proxies).

## 4. DESERIALIZATION, SSRF, XXE, OPEN REDIRECT, AND CORS (OWASP A08/A10:2021, CWE-502/918/611/601/942)

### Unsafe Deserialization (CWE-502)
CRITICAL sinks (can lead to RCE):
- Python: pickle.load/loads, yaml.load (without SafeLoader), yaml.unsafe_load, jsonpickle.decode, marshal.load, shelve.open, torch.load
- Java: ObjectInputStream.readObject/readUnshared, XMLDecoder, XStream.fromXML, BinaryFormatter
- Ruby: YAML.load (use safe_load), Marshal.load, JSON.parse(create_additions: true)
- JavaScript: node-serialize unserialize(), js-yaml !!js/function, cryo.parse()
- PHP: unserialize()
- .NET: BinaryFormatter.Deserialize, TypeNameHandling != None, JavaScriptSerializer with TypeResolver

### SSRF (CWE-918)
SOURCES: webhook URL fields, image URL params, callback URLs, redirect targets
SINKS: requests.get(url), fetch(url), axios.get(url), http.Get(url), reqwest::get(url), HttpURLConnection(url), RestTemplate.getForObject(url)
Check for: user-controlled URL components, missing URL allowlist, ability to reach internal IPs (10.x, 172.16.x, 192.168.x, 169.254.169.254 metadata endpoint)

### Open Redirect (CWE-601)
Flag: user-controlled redirect targets without an allowlist or relative-path restriction.
SAFE: allowlisted domains/paths, relative URLs only, or signed redirect destinations.

### XXE (CWE-611)
XML parsing without disabling external entities: DocumentBuilderFactory, SAXParser, lxml.etree.parse, xml.sax.parse
Fix: disable DOCTYPE declarations, use defusedxml (Python)

### CORS Misconfiguration (CWE-942)
Flag: Access-Control-Allow-Origin: * with credentials, reflecting Origin without validation

## 5. SUPPLY-CHAIN RISK (OWASP A08:2021, CWE-427/829)

When reviewing changes to dependency manifests (Cargo.toml, package.json, requirements.txt, go.mod, Gemfile, etc.):
- Only flag dependency changes when the diff shows a concrete risk signal: non-registry source, install/build script, unpinned range, override, suspicious lockfile change, or downgrade
- Non-registry sources: git deps, path deps, --extra-index-url, replace directives (bypass checksums)
- Install/build scripts: postinstall in package.json, build.rs with network/process access, setup.py with exec/eval
- Lockfile anomalies: changed checksums for same version, registry URL changes, new packages without manifest changes
- Version wildcards: *, latest, unbounded >= ranges
- Version downgrades: may reintroduce known CVEs
- Patch/replace overrides: [patch] in Cargo.toml, replace in go.mod, resolutions in package.json
- CI/CD: GitHub Actions not pinned to SHA, script injection via ${{ github.event.* }}, pull_request_target with PR checkout

## 6. CRYPTOGRAPHY (OWASP A02:2021, CWE-326/327/328/330)

### Weak Algorithms
- Ciphers: DES, 3DES, RC4, RC2, Blowfish, ECB mode for any block cipher
- Hashes for integrity: MD5, SHA1 (for passwords see sec.auth.weak-password-hash)
- Safe: AES-256-GCM, ChaCha20-Poly1305, SHA-256+, BLAKE2/3

### Insecure TLS
- SSLv2/v3, TLS 1.0/1.1, ssl.CERT_NONE, InsecureSkipVerify: true, rejectUnauthorized: false
- Safe: TLS 1.2+ with AEAD suites, certificate pinning

### Bad Randomness (CWE-330)
VULNERABLE for tokens/keys/nonces: Math.random() (JS), random.random() (Python), java.util.Random, math/rand (Go), rand()/mt_rand() (PHP)
SAFE: crypto.getRandomValues(), secrets module, SecureRandom, crypto/rand, random_bytes()

### Key Issues
- RSA < 2048 bits, EC < 256 bits, AES < 128 bits
- Hardcoded IVs/nonces — must be unique per encryption
- Unauthenticated encryption (CBC/CTR without MAC) — use AES-GCM or similar AEAD
- Non-constant-time secret comparison — use hmac.compare_digest, subtle.ConstantTimeCompare, crypto.timingSafeEqual

## 7. DATA EXPOSURE (OWASP A09:2021, CWE-200/209/532)

- PII in logs: email, SSN, credit card numbers, phone numbers in log statements
- Verbose errors: stack traces, SQL errors, internal paths in HTTP responses
- Debug mode: DEBUG=True, app.debug=True in production config
- Sensitive data in URLs: API keys, tokens in query parameters (logged by proxies)
- Missing security headers: no HSTS, no X-Content-Type-Options, no CSP, no X-Frame-Options
- Server information disclosure: X-Powered-By, framework version in error pages

## 8. UNSAFE CODE PATTERNS (CWE-119/190/367/1321/1333)

- Rust unsafe blocks: transmute, raw pointer deref, from_raw_parts — verify safety invariants
- C/C++ memory: strcpy, sprintf, gets (buffer overflow), printf(user_input) (format string), use-after-free
- ReDoS: user-controlled input compiled as regex (re.compile, new RegExp, Regex::new), nested quantifiers (a+)+
- Prototype pollution (JS): deep merge/extend with user-controlled keys (__proto__, constructor.prototype)
- Mass assignment: Model.new(params) without allowlist, Object.assign(model, req.body)
- Race conditions / TOCTOU: check-then-act without locking, file exists then open
- Integer overflow in security contexts: buffer size calculations, array indexing

## 9. INFRASTRUCTURE SECURITY (CWE-250/284)

When reviewing Dockerfiles, Kubernetes manifests, Terraform, or Helm charts:
- Docker: running as root, ADD with remote URLs, secrets in build args/ENV, COPY .env
- K8s: privileged: true, hostNetwork/hostPID, capabilities add ALL, base64 secrets in manifests
- Terraform: cidr_blocks 0.0.0.0/0 on non-HTTP ports, publicly_accessible=true, public S3 buckets
- IAM: Action: *, Resource: * with sensitive services, AdministratorAccess on service accounts
- Helm: secrets in values.yaml, hardcoded TLS certs, missing existingSecret pattern
- Ports: database/management/debug ports exposed via LoadBalancer/NodePort/Ingress

## 10. API SECURITY (OWASP A04:2021, CWE-20/770)

- Missing rate limiting on auth/expensive endpoints
- Excessive data: returning full DB objects without field filtering (DTO pattern)
- No pagination: unbounded queries, .all() without LIMIT
- GraphQL: no depth/complexity limits, introspection in production
- API keys in URLs: ?api_key=, ?token= (logged by proxies)
- Missing input validation: no schema (Pydantic/Joi/Zod) on POST/PUT/PATCH bodies
- Broken function-level auth: admin routes without role checks
- Insecure file upload: no type validation, no size limit, executable extensions

__OUTPUT_CONTRACT__

Assign rule IDs from:
Injection: sec.injection.sql, sec.injection.command, sec.injection.xss, sec.injection.template, sec.injection.ldap, sec.injection.log, sec.injection.path-traversal, sec.injection.code, sec.injection.graphql, sec.injection.header, sec.injection.email, sec.injection.xml, sec.injection.regex
Auth: sec.auth.missing, sec.auth.idor, sec.auth.privilege-escalation, sec.auth.jwt, sec.auth.weak-password-hash, sec.auth.oauth, sec.auth.session, sec.auth.csrf
Secrets: sec.secrets.hardcoded, sec.secrets.aws, sec.secrets.gcp, sec.secrets.azure, sec.secrets.github-token, sec.secrets.slack-token, sec.secrets.stripe-key, sec.secrets.openai-key, sec.secrets.anthropic-key, sec.secrets.private-key, sec.secrets.jwt-token, sec.secrets.connection-string, sec.secrets.logged, sec.secrets.datadog, sec.secrets.newrelic, sec.secrets.supabase, sec.secrets.digitalocean, sec.secrets.shopify, sec.secrets.discord, sec.secrets.databricks, sec.secrets.linear, sec.secrets.pypi, sec.secrets.mailgun, sec.secrets.doppler
Deserialization: sec.deser.unsafe, sec.ssrf, sec.xxe, sec.redirect.open, sec.cors.misconfigured
Crypto: sec.crypto.weak-cipher, sec.crypto.weak-tls, sec.crypto.insecure-random, sec.crypto.weak-key-size, sec.crypto.broken-hash, sec.crypto.hardcoded-iv, sec.crypto.missing-mac, sec.crypto.cert-validation, sec.crypto.timing-attack, sec.crypto.weak-kdf
Data: sec.data.pii-logged, sec.data.verbose-error, sec.data.debug-mode, sec.data.sensitive-url, sec.data.missing-headers, sec.data.server-info, sec.data.directory-listing, sec.data.sensitive-comment
Unsafe: sec.unsafe.rust-unsafe, sec.unsafe.memory-cpp, sec.unsafe.redos, sec.unsafe.prototype-pollution, sec.unsafe.mass-assignment, sec.unsafe.race-condition, sec.unsafe.integer-overflow, sec.unsafe.resource-leak
Infra: sec.infra.docker-root, sec.infra.docker-add, sec.infra.docker-secrets, sec.infra.k8s-privileged, sec.infra.k8s-secrets, sec.infra.terraform-public, sec.infra.iam-overpermissive, sec.infra.helm-secrets, sec.infra.exposed-port
API: sec.api.missing-rate-limit, sec.api.excessive-data, sec.api.no-pagination, sec.api.graphql-depth, sec.api.key-in-url, sec.api.no-input-validation, sec.api.broken-function-auth, sec.api.insecure-upload
Supply-chain: sec.supply-chain.new-dependency, sec.supply-chain.non-registry-source, sec.supply-chain.install-scripts, sec.supply-chain.lockfile-tampering, sec.supply-chain.unpinned-version, sec.supply-chain.version-downgrade, sec.supply-chain.override-directive, sec.supply-chain.ci-injection

Use rule IDs when the issue clearly maps to one of the categories above; otherwise omit the rule ID rather than guessing."#
        .replace("__COMMON_REVIEW_PRINCIPLES__", shared_review_principles())
        .replace(
            "__OUTPUT_CONTRACT__",
            &shared_output_contract("security", "No security issues found."),
        )
}

/// Build a system prompt focused exclusively on correctness issues.
pub fn build_correctness_prompt() -> String {
    format!(
        r#"You are a correctness-focused code reviewer. Your ONLY job is to find bugs and logic errors in code changes. Do NOT comment on style, naming, or formatting.

Focus exclusively on:
- Logic errors (off-by-one, wrong operator, inverted conditions, unreachable code)
- Edge cases (empty collections, zero/negative values, boundary conditions, integer overflow)
- Null/None handling (null pointer dereference, unwrap on None/Err, missing Option checks)
- Concurrency issues (race conditions, deadlocks, data races, missing synchronization)
- Error handling (swallowed errors, incorrect error propagation, missing error cases)
- Resource management (unclosed handles, memory leaks, missing cleanup)
- Type safety (incorrect casts, lossy conversions, type confusion)
- API contract violations (precondition failures, invariant breaks)

{}

{}"#,
        shared_review_principles(),
        shared_output_contract("correctness", "No correctness issues found.")
    )
}

/// Build a system prompt focused exclusively on style and readability issues.
pub fn build_style_prompt() -> String {
    format!(
        r#"You are a style-focused code reviewer. Your ONLY job is to find style, readability, and idiomatic code issues. Do NOT comment on bugs, security, or performance.

Focus exclusively on:
- Naming conventions (unclear variable/function names, inconsistent casing, abbreviations)
- Code patterns (non-idiomatic constructs, unnecessary complexity, missed language features)
- Readability (deeply nested code, overly long functions, unclear control flow)
- Consistency (mixed styles within the same file, inconsistent formatting)
- Dead code (unused imports, unreachable branches, commented-out code)
- Documentation (missing doc comments on public APIs, outdated comments, misleading names)

{}

{}"#,
        shared_review_principles(),
        shared_output_contract("style", "No style issues found.")
    )
}

/// Category label for a specialized review pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecializedPassKind {
    Security,
    Correctness,
    Style,
}

impl SpecializedPassKind {
    /// Human-readable tag added to comments produced by this pass.
    pub fn tag(self) -> &'static str {
        match self {
            SpecializedPassKind::Security => "security-pass",
            SpecializedPassKind::Correctness => "correctness-pass",
            SpecializedPassKind::Style => "style-pass",
        }
    }

    /// Build the specialized system prompt for this pass.
    pub fn system_prompt(self) -> String {
        match self {
            SpecializedPassKind::Security => build_security_prompt(),
            SpecializedPassKind::Correctness => build_correctness_prompt(),
            SpecializedPassKind::Style => build_style_prompt(),
        }
    }
}

pub struct PromptBuilder {
    config: PromptConfig,
}

impl PromptBuilder {
    pub fn new(config: PromptConfig) -> Self {
        Self { config }
    }

    pub fn build_prompt(
        &self,
        diff: &UnifiedDiff,
        context_chunks: &[LLMContextChunk],
    ) -> Result<(String, String)> {
        let diff_text = self.format_diff(diff)?;
        let context_text = if self.config.include_context {
            self.format_context(context_chunks)?
        } else {
            String::new()
        };

        let user_prompt = self
            .config
            .user_prompt_template
            .replace("{diff}", &diff_text)
            .replace("{context}", &context_text);

        Ok((self.config.system_prompt.clone(), user_prompt))
    }

    fn format_diff(&self, diff: &UnifiedDiff) -> Result<String> {
        let mut output = String::new();
        let mut truncated = false;
        output.push_str(&format!("File: {}\n", diff.file_path.display()));

        'hunks: for hunk in &diff.hunks {
            let header = format!("{}\n", hunk.context);
            if self.config.max_diff_chars > 0
                && output.len().saturating_add(header.len()) > self.config.max_diff_chars
            {
                truncated = true;
                break;
            }
            output.push_str(&header);

            for change in &hunk.changes {
                let prefix = match change.change_type {
                    crate::core::diff_parser::ChangeType::Added => "+",
                    crate::core::diff_parser::ChangeType::Removed => "-",
                    crate::core::diff_parser::ChangeType::Context => " ",
                };
                let line = format!("{}{}\n", prefix, change.content);
                if self.config.max_diff_chars > 0
                    && output.len().saturating_add(line.len()) > self.config.max_diff_chars
                {
                    truncated = true;
                    break 'hunks;
                }
                output.push_str(&line);
            }
        }

        if truncated {
            output.push_str("[Diff truncated]\n");
        }

        Ok(output)
    }

    fn format_context(&self, chunks: &[LLMContextChunk]) -> Result<String> {
        let mut output = String::new();

        for chunk in chunks {
            let block = format!(
                "\n[{:?} - {}{}]\n{}\n",
                chunk.context_type,
                chunk.file_path.display(),
                chunk
                    .line_range
                    .map(|(s, e)| format!(":{}-{}", s, e))
                    .unwrap_or_default(),
                chunk.content
            );
            if self.config.max_context_chars > 0
                && output.len().saturating_add(block.len()) > self.config.max_context_chars
            {
                output.push_str("\n[Context truncated]\n");
                break;
            }
            output.push_str(&block);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_user_prompt_template_is_pass_neutral() {
        let config = PromptConfig::default();
        assert!(config
            .user_prompt_template
            .contains("Follow the active system prompt's review scope"));
        assert!(!config
            .user_prompt_template
            .contains("bugs, security vulnerabilities, or performance issues"));
    }

    #[test]
    fn default_prompt_prefers_json_output_contract() {
        let config = PromptConfig::default();
        assert!(config
            .system_prompt
            .contains("Preferred format: return a JSON array only"));
        assert!(config.system_prompt.contains("\"line\": 42"));
        assert!(config.system_prompt.contains("return `[]`"));
        assert!(config
            .system_prompt
            .contains("Do not write generic advice that starts with Ensure"));
        assert!(config
            .user_prompt_template
            .contains("Do not write vague comments"));
    }

    #[test]
    fn security_prompt_focuses_on_security() {
        let prompt = build_security_prompt();
        // Core vulnerability classes
        assert!(prompt.contains("SQL Injection"));
        assert!(prompt.contains("Command Injection"));
        assert!(prompt.contains("XSS"));
        assert!(prompt.contains("SSRF"));
        assert!(prompt.contains("Unsafe Deserialization"));
        assert!(prompt.contains("Open Redirect"));
        // Auth/authz
        assert!(prompt.contains("Missing Authentication"));
        assert!(prompt.contains("IDOR"));
        assert!(prompt.contains("JWT"));
        assert!(prompt.contains("CSRF"));
        // Secrets
        assert!(prompt.contains("AWS"));
        assert!(prompt.contains("Private key"));
        // Supply chain
        assert!(prompt.contains("SUPPLY-CHAIN"));
        assert!(prompt.contains("lockfile"));
        // Cryptography (new)
        assert!(prompt.contains("CRYPTOGRAPHY"));
        assert!(prompt.contains("Weak Algorithms"));
        assert!(prompt.contains("Insecure TLS"));
        assert!(prompt.contains("Bad Randomness"));
        assert!(prompt.contains("sec.crypto.weak-cipher"));
        // Data exposure (new)
        assert!(prompt.contains("DATA EXPOSURE"));
        assert!(prompt.contains("PII in logs"));
        assert!(prompt.contains("Verbose errors"));
        assert!(prompt.contains("sec.data.pii-logged"));
        // Unsafe code patterns (new)
        assert!(prompt.contains("UNSAFE CODE PATTERNS"));
        assert!(prompt.contains("Rust unsafe"));
        assert!(prompt.contains("Prototype pollution"));
        assert!(prompt.contains("sec.unsafe.rust-unsafe"));
        // Infrastructure security (new)
        assert!(prompt.contains("INFRASTRUCTURE SECURITY"));
        assert!(prompt.contains("Docker"));
        assert!(prompt.contains("Terraform"));
        assert!(prompt.contains("sec.infra.docker-root"));
        // API security (new)
        assert!(prompt.contains("API SECURITY"));
        assert!(prompt.contains("rate limiting"));
        assert!(prompt.contains("GraphQL"));
        assert!(prompt.contains("sec.api.missing-rate-limit"));
        // Source-sink model
        assert!(prompt.contains("SOURCES"));
        assert!(prompt.contains("SINKS"));
        assert!(prompt.contains("SANITIZERS"));
        // OWASP/CWE mappings
        assert!(prompt.contains("OWASP"));
        assert!(prompt.contains("CWE-"));
        // Output format
        assert!(prompt.contains("[security]"));
        assert!(prompt.contains("sec.injection.sql"));
        // New injection variants
        assert!(prompt.contains("sec.injection.graphql"));
        assert!(prompt.contains("sec.injection.header"));
        // Should NOT encourage style or correctness commentary
        assert!(prompt.contains("Do NOT comment on style"));
        assert!(!prompt.contains("all five vulnerability classes"));
    }

    #[test]
    fn correctness_prompt_focuses_on_bugs() {
        let prompt = build_correctness_prompt();
        assert!(prompt.contains("Logic errors"));
        assert!(prompt.contains("Concurrency"));
        assert!(prompt.contains("[correctness]"));
        assert!(prompt.contains("Do NOT comment on style"));
    }

    #[test]
    fn style_prompt_focuses_on_readability() {
        let prompt = build_style_prompt();
        assert!(prompt.contains("Naming conventions"));
        assert!(prompt.contains("Readability"));
        assert!(prompt.contains("[style]"));
        assert!(prompt.contains("Do NOT comment on bugs"));
    }

    #[test]
    fn pass_kind_system_prompt_matches_builder() {
        assert_eq!(
            SpecializedPassKind::Security.system_prompt(),
            build_security_prompt()
        );
        assert_eq!(
            SpecializedPassKind::Correctness.system_prompt(),
            build_correctness_prompt()
        );
        assert_eq!(
            SpecializedPassKind::Style.system_prompt(),
            build_style_prompt()
        );
    }

    #[test]
    fn pass_kind_tags_are_unique() {
        let tags: Vec<&str> = vec![
            SpecializedPassKind::Security.tag(),
            SpecializedPassKind::Correctness.tag(),
            SpecializedPassKind::Style.tag(),
        ];
        let unique: std::collections::HashSet<&&str> = tags.iter().collect();
        assert_eq!(unique.len(), tags.len());
    }
}
