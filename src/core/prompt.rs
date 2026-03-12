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
            system_prompt: r#"You are an expert code reviewer with deep knowledge of software security, performance optimization, and best practices. Your role is to identify critical issues in code changes that could impact:
- Security (vulnerabilities, data exposure, injection risks)
- Correctness (bugs, logic errors, edge cases)
- Performance (inefficiencies, memory leaks, algorithmic complexity)
- Maintainability (code clarity, error handling, documentation)

Focus only on actionable issues. Do not comment on code style or formatting unless it impacts functionality."#.to_string(),
            user_prompt_template: r#"<task>
Review the code changes below and identify specific issues. Focus on problems that could cause bugs, security vulnerabilities, or performance issues.
</task>

<diff>
{diff}
</diff>

<context>
{context}
</context>

<instructions>
1. Analyze the changes systematically
2. For each issue found, provide:
   - Line number where the issue occurs
   - Clear description of the problem
   - Impact if not addressed
   - Optional rule id when a scoped review rule applies
   - Suggested fix (if applicable)
3. For every issue where a concrete code fix is possible, include a code suggestion block immediately after the issue line using this exact format:

<<<ORIGINAL
<the problematic code, copied verbatim from the diff>
===
<the fixed code>
>>>SUGGESTED

Format each issue as:
Line [number] [rule:<id> optional]: [Issue type] - [Description]. [Impact]. [Suggestion if applicable].

Then, if a fix applies, add the code suggestion block on the next lines.

Examples:
Line 42 [rule:sec.sql.injection]: Security - User input passed directly to SQL query. Risk of SQL injection. Use parameterized queries.
<<<ORIGINAL
query = "SELECT * FROM users WHERE id = " + user_id
===
query = "SELECT * FROM users WHERE id = ?"
cursor.execute(query, (user_id,))
>>>SUGGESTED
Line 13: Bug - Missing null check before dereferencing pointer. May cause crash. Add null validation.
<<<ORIGINAL
value = obj.get_data()
result = value.process()
===
value = obj.get_data()
if value is not None:
    result = value.process()
>>>SUGGESTED
Line 28: Performance - O(n²) algorithm for large dataset. Will be slow with many items. Consider using a hash map.
</instructions>"#.to_string(),
            max_tokens: 2000,
            include_context: true,
            max_context_chars: 20000,
            max_diff_chars: 40000,
        }
    }
}

/// Build a system prompt focused exclusively on security issues.
pub fn build_security_prompt() -> String {
    r#"You are an expert application security engineer performing a focused security review of code changes. Your ONLY job is to find security vulnerabilities. Do NOT comment on style, naming, performance, or general correctness unless it has a direct security impact.

You MUST analyze all five vulnerability classes below using the taint-flow model: trace whether user-controlled data (SOURCES) can reach dangerous operations (SINKS) without passing through proper validation (SANITIZERS).

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

## 4. UNSAFE DESERIALIZATION, SSRF, XSS, CSRF (OWASP A08/A10:2021, CWE-502/918)

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

### XXE (CWE-611)
XML parsing without disabling external entities: DocumentBuilderFactory, SAXParser, lxml.etree.parse, xml.sax.parse
Fix: disable DOCTYPE declarations, use defusedxml (Python)

### CORS Misconfiguration (CWE-942)
Flag: Access-Control-Allow-Origin: * with credentials, reflecting Origin without validation

## 5. SUPPLY-CHAIN RISK (OWASP A08:2021, CWE-427/829)

When reviewing changes to dependency manifests (Cargo.toml, package.json, requirements.txt, go.mod, Gemfile, etc.):
- New dependencies: flag for awareness, especially low-download-count or very new packages
- Non-registry sources: git deps, path deps, --extra-index-url, replace directives (bypass checksums)
- Install/build scripts: postinstall in package.json, build.rs with network/process access, setup.py with exec/eval
- Lockfile anomalies: changed checksums for same version, registry URL changes, new packages without manifest changes
- Version wildcards: *, latest, unbounded >= ranges
- Version downgrades: may reintroduce known CVEs
- Patch/replace overrides: [patch] in Cargo.toml, replace in go.mod, resolutions in package.json
- CI/CD: GitHub Actions not pinned to SHA, script injection via ${{ github.event.* }}, pull_request_target with PR checkout

## OUTPUT FORMAT

For each finding use this format, mapping to CWE where possible:
Line [number] [rule:<rule_id>]: [security] - [Description with specific CWE]. [Impact]. [Suggested fix].

Assign rule IDs from: sec.injection.sql, sec.injection.command, sec.injection.xss, sec.injection.template, sec.injection.ldap, sec.injection.log, sec.injection.path-traversal, sec.injection.code, sec.auth.missing, sec.auth.idor, sec.auth.privilege-escalation, sec.auth.jwt, sec.auth.weak-password-hash, sec.auth.oauth, sec.auth.session, sec.auth.csrf, sec.secrets.hardcoded, sec.secrets.aws, sec.secrets.github-token, sec.secrets.private-key, sec.secrets.jwt-token, sec.secrets.connection-string, sec.secrets.logged, sec.deser.unsafe, sec.ssrf, sec.xxe, sec.redirect.open, sec.cors.misconfigured, sec.supply-chain.new-dependency, sec.supply-chain.non-registry-source, sec.supply-chain.install-scripts, sec.supply-chain.lockfile-tampering, sec.supply-chain.unpinned-version, sec.supply-chain.version-downgrade, sec.supply-chain.override-directive, sec.supply-chain.ci-injection.

IMPORTANT: Only report issues with concrete evidence in the diff. Do not speculate about code you cannot see. If a sanitizer is present, do not flag the issue. Prefer fewer high-confidence findings over many low-confidence ones.

If no security issues are found, respond with: No security issues found."#.to_string()
}

/// Build a system prompt focused exclusively on correctness issues.
pub fn build_correctness_prompt() -> String {
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

Tag every finding with [correctness] at the start of the issue type.
If no correctness issues are found, respond with: No correctness issues found."#.to_string()
}

/// Build a system prompt focused exclusively on style and readability issues.
pub fn build_style_prompt() -> String {
    r#"You are a style-focused code reviewer. Your ONLY job is to find style, readability, and idiomatic code issues. Do NOT comment on bugs, security, or performance.

Focus exclusively on:
- Naming conventions (unclear variable/function names, inconsistent casing, abbreviations)
- Code patterns (non-idiomatic constructs, unnecessary complexity, missed language features)
- Readability (deeply nested code, overly long functions, unclear control flow)
- Consistency (mixed styles within the same file, inconsistent formatting)
- Dead code (unused imports, unreachable branches, commented-out code)
- Documentation (missing doc comments on public APIs, outdated comments, misleading names)

Tag every finding with [style] at the start of the issue type.
If no style issues are found, respond with: No style issues found."#.to_string()
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
    fn security_prompt_focuses_on_security() {
        let prompt = build_security_prompt();
        // Core vulnerability classes
        assert!(prompt.contains("SQL Injection"));
        assert!(prompt.contains("Command Injection"));
        assert!(prompt.contains("XSS"));
        assert!(prompt.contains("SSRF"));
        assert!(prompt.contains("Unsafe Deserialization"));
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
        // Should NOT encourage style or correctness commentary
        assert!(prompt.contains("Do NOT comment on style"));
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
