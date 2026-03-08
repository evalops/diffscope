use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A natural-language summary of a code unit (function, struct, module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSummary {
    pub file_path: PathBuf,
    pub symbol_name: String,
    pub line_range: (usize, usize),
    pub summary: String,
    pub embedding_text: String,
}

/// Cache of code summaries, keyed by file_path:symbol_name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryCache {
    entries: HashMap<String, CodeSummary>,
    version: u32,
}

impl SummaryCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            version: 1,
        }
    }

    pub fn get(&self, file_path: &Path, symbol_name: &str) -> Option<&CodeSummary> {
        let key = cache_key(file_path, symbol_name);
        self.entries.get(&key)
    }

    pub fn insert(&mut self, summary: CodeSummary) {
        let key = cache_key(&summary.file_path, &summary.symbol_name);
        self.entries.insert(key, summary);
    }

    pub fn remove(&mut self, file_path: &Path, symbol_name: &str) -> Option<CodeSummary> {
        let key = cache_key(file_path, symbol_name);
        self.entries.remove(&key)
    }

    pub fn invalidate_file(&mut self, file_path: &Path) {
        let prefix = format!("{}:", file_path.display());
        self.entries.retain(|k, _| !k.starts_with(&prefix));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn all_summaries(&self) -> Vec<&CodeSummary> {
        self.entries.values().collect()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

fn cache_key(file_path: &Path, symbol_name: &str) -> String {
    format!("{}:{}", file_path.display(), symbol_name)
}

/// Generate a heuristic natural-language summary of a code block.
/// This uses pattern matching rather than an LLM call for speed and offline use.
pub fn summarize_code_heuristic(
    symbol_name: &str,
    code: &str,
    file_path: &Path,
    line_range: (usize, usize),
) -> CodeSummary {
    let language = detect_language(file_path);
    let kind = detect_symbol_kind(code, &language);
    let params = extract_parameters(code, &language);
    let returns = extract_return_type(code, &language);
    let docstring = extract_docstring(code, &language);
    let complexity = estimate_complexity(code);
    let operations = extract_key_operations(code);

    let mut summary_parts = Vec::new();

    // Symbol kind and name
    summary_parts.push(format!("{} `{}`", kind, symbol_name));

    // Parameters
    if !params.is_empty() {
        summary_parts.push(format!("takes {}", params.join(", ")));
    }

    // Return type
    if let Some(ret) = &returns {
        summary_parts.push(format!("returns {}", ret));
    }

    // Docstring
    if let Some(doc) = &docstring {
        summary_parts.push(doc.clone());
    }

    // Key operations
    if !operations.is_empty() {
        summary_parts.push(format!(
            "performs: {}",
            operations.into_iter().take(3).collect::<Vec<_>>().join(", ")
        ));
    }

    // Complexity
    if complexity > 5 {
        summary_parts.push(format!("complexity: high ({})", complexity));
    } else if complexity > 2 {
        summary_parts.push(format!("complexity: medium ({})", complexity));
    }

    let summary = summary_parts.join(". ");
    let embedding_text = build_embedding_text(symbol_name, &summary, code);

    CodeSummary {
        file_path: file_path.to_path_buf(),
        symbol_name: symbol_name.to_string(),
        line_range,
        summary,
        embedding_text,
    }
}

/// Build embedding text that combines NL summary with code for better similarity matching.
/// Based on Greptile's finding that NL summaries + code yield 12% better similarity.
pub fn build_embedding_text(symbol_name: &str, summary: &str, code: &str) -> String {
    let code_truncated = if code.len() > 500 {
        &code[..500]
    } else {
        code
    };
    format!(
        "Symbol: {}\nSummary: {}\nCode:\n{}",
        symbol_name, summary, code_truncated
    )
}

/// Batch-summarize all symbols in a file.
pub fn summarize_file_symbols(
    file_path: &Path,
    content: &str,
    cache: &mut SummaryCache,
) -> Vec<CodeSummary> {
    let language = detect_language(file_path);
    let blocks = extract_code_blocks(content, &language);
    let mut summaries = Vec::new();

    for (name, code, start_line, end_line) in blocks {
        if let Some(cached) = cache.get(file_path, &name) {
            summaries.push(cached.clone());
            continue;
        }

        let summary =
            summarize_code_heuristic(&name, &code, file_path, (start_line, end_line));
        cache.insert(summary.clone());
        summaries.push(summary);
    }

    summaries
}

fn detect_language(file_path: &Path) -> String {
    file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string()
}

fn detect_symbol_kind(code: &str, language: &str) -> &'static str {
    let first_line = code.lines().next().unwrap_or("");
    match language {
        "rs" => {
            if first_line.contains("fn ") {
                "Function"
            } else if first_line.contains("struct ") {
                "Struct"
            } else if first_line.contains("enum ") {
                "Enum"
            } else if first_line.contains("trait ") {
                "Trait"
            } else if first_line.contains("impl ") {
                "Implementation"
            } else {
                "Symbol"
            }
        }
        "py" | "pyi" => {
            if first_line.contains("def ") {
                "Function"
            } else if first_line.contains("class ") {
                "Class"
            } else {
                "Symbol"
            }
        }
        "js" | "ts" | "tsx" | "jsx" => {
            if first_line.contains("function ") {
                "Function"
            } else if first_line.contains("class ") {
                "Class"
            } else if first_line.contains("interface ") {
                "Interface"
            } else {
                "Symbol"
            }
        }
        _ => "Symbol",
    }
}

static RUST_PARAMS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"fn\s+\w+\s*(?:<[^>]*>)?\s*\(([^)]*)\)").unwrap());
static PY_PARAMS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"def\s+\w+\s*\(([^)]*)\)").unwrap());
static JS_PARAMS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"function\s+\w+\s*\(([^)]*)\)").unwrap());

fn extract_parameters(code: &str, language: &str) -> Vec<String> {
    let pattern = match language {
        "rs" => &*RUST_PARAMS,
        "py" | "pyi" => &*PY_PARAMS,
        "js" | "ts" | "tsx" | "jsx" => &*JS_PARAMS,
        _ => return Vec::new(),
    };

    if let Some(caps) = pattern.captures(code) {
        let params_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        params_str
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty() && p != "self" && p != "&self" && p != "&mut self")
            .collect()
    } else {
        Vec::new()
    }
}

static RUST_RETURN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"->\s*([^\{]+)").unwrap());

fn extract_return_type(code: &str, language: &str) -> Option<String> {
    match language {
        "rs" => {
            let first_lines: String = code.lines().take(3).collect::<Vec<_>>().join(" ");
            RUST_RETURN
                .captures(&first_lines)
                .map(|caps| caps[1].trim().to_string())
        }
        "py" => {
            let first_line = code.lines().next().unwrap_or("");
            if first_line.contains("->") {
                let parts: Vec<&str> = first_line.split("->").collect();
                parts.get(1).map(|s| s.trim().trim_end_matches(':').to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

static DOCSTRING_RUST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"///\s*(.+)").unwrap());
fn extract_docstring(code: &str, language: &str) -> Option<String> {
    match language {
        "rs" => {
            // Look for /// comments before the function
            for line in code.lines() {
                if let Some(caps) = DOCSTRING_RUST.captures(line) {
                    return Some(caps[1].trim().to_string());
                }
                if line.trim().starts_with("pub ")
                    || line.trim().starts_with("fn ")
                    || line.trim().starts_with("struct ")
                {
                    break;
                }
            }
            None
        }
        "py" => {
            let lines: Vec<&str> = code.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.contains("def ") || line.contains("class ") {
                    if let Some(next) = lines.get(i + 1) {
                        let trimmed = next.trim();
                        if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                            let doc = trimmed
                                .trim_start_matches("\"\"\"")
                                .trim_start_matches("'''")
                                .trim_end_matches("\"\"\"")
                                .trim_end_matches("'''")
                                .trim();
                            if !doc.is_empty() {
                                return Some(doc.to_string());
                            }
                        }
                    }
                    break;
                }
            }
            None
        }
        _ => None,
    }
}

fn estimate_complexity(code: &str) -> usize {
    let mut complexity = 1;
    let lower = code.to_lowercase();
    for keyword in &[
        "if ", "else ", "for ", "while ", "match ", "loop ", "elif ", "except ",
        "catch ", "case ", "&&", "||",
    ] {
        complexity += lower.matches(keyword).count();
    }
    complexity
}

fn extract_key_operations(code: &str) -> Vec<String> {
    let mut ops = Vec::new();
    let lower = code.to_lowercase();

    if lower.contains("unwrap") || lower.contains("expect") {
        ops.push("error handling".to_string());
    }
    if lower.contains("async") || lower.contains("await") {
        ops.push("async operations".to_string());
    }
    if lower.contains("iter()") || lower.contains("for ") {
        ops.push("iteration".to_string());
    }
    if lower.contains("hashmap") || lower.contains("vec!") || lower.contains("hashset") {
        ops.push("collection manipulation".to_string());
    }
    if lower.contains("file") || lower.contains("read") || lower.contains("write") {
        ops.push("I/O operations".to_string());
    }
    if lower.contains("parse") || lower.contains("regex") {
        ops.push("parsing".to_string());
    }
    if lower.contains("serialize") || lower.contains("json") {
        ops.push("serialization".to_string());
    }
    if lower.contains("http") || lower.contains("request") || lower.contains("fetch") {
        ops.push("network requests".to_string());
    }

    ops
}

static RUST_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?(?:async\s+)?(?:fn|struct|enum|trait)\s+([A-Za-z_]\w*)")
        .unwrap()
});
static PY_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^(?:def|class)\s+([A-Za-z_]\w*)").unwrap()
});
static JS_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^(?:export\s+)?(?:async\s+)?(?:function|class)\s+([A-Za-z_$]\w*)")
        .unwrap()
});

fn extract_code_blocks(content: &str, language: &str) -> Vec<(String, String, usize, usize)> {
    let pattern = match language {
        "rs" => &*RUST_BLOCK,
        "py" | "pyi" => &*PY_BLOCK,
        "js" | "ts" | "tsx" | "jsx" => &*JS_BLOCK,
        _ => return Vec::new(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let mut blocks = Vec::new();
    let mut matches: Vec<(String, usize)> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if let Some(caps) = pattern.captures(line) {
            matches.push((caps[1].to_string(), idx));
        }
    }

    for (i, (name, start)) in matches.iter().enumerate() {
        let end = if i + 1 < matches.len() {
            matches[i + 1].1.saturating_sub(1)
        } else {
            lines.len().saturating_sub(1)
        };
        let code = lines[*start..=end.min(lines.len() - 1)].join("\n");
        blocks.push((name.clone(), code, start + 1, end + 1));
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_rust_function() {
        let code = r#"pub fn validate_token(token: &str, secret: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    token.len() > 10 && token.starts_with(secret)
}"#;
        let summary = summarize_code_heuristic(
            "validate_token",
            code,
            Path::new("auth.rs"),
            (1, 6),
        );

        assert_eq!(summary.symbol_name, "validate_token");
        assert!(summary.summary.contains("Function"));
        assert!(summary.summary.contains("validate_token"));
        assert!(!summary.embedding_text.is_empty());
    }

    #[test]
    fn test_summarize_rust_struct() {
        let code = "pub struct Config {\n    pub name: String,\n    pub value: usize,\n}";
        let summary =
            summarize_code_heuristic("Config", code, Path::new("config.rs"), (1, 4));

        assert!(summary.summary.contains("Struct"));
    }

    #[test]
    fn test_summarize_python_function() {
        let code = "def process_data(items, threshold=0.5):\n    \"\"\"Process items above threshold.\"\"\"\n    return [i for i in items if i > threshold]\n";
        let summary =
            summarize_code_heuristic("process_data", code, Path::new("utils.py"), (1, 3));

        assert!(summary.summary.contains("Function"));
        assert!(summary.summary.contains("process_data"));
        // Should extract docstring
        assert!(summary.summary.contains("Process items above threshold"));
    }

    #[test]
    fn test_embedding_text_format() {
        let text = build_embedding_text("foo", "A function that does stuff", "fn foo() {}");
        assert!(text.contains("Symbol: foo"));
        assert!(text.contains("Summary: A function that does stuff"));
        assert!(text.contains("Code:"));
    }

    #[test]
    fn test_embedding_text_truncates_long_code() {
        let long_code = "x".repeat(1000);
        let text = build_embedding_text("foo", "summary", &long_code);
        assert!(text.len() < 600); // 500 code + headers
    }

    #[test]
    fn test_summary_cache_basic() {
        let mut cache = SummaryCache::new();
        assert!(cache.is_empty());

        let summary = CodeSummary {
            file_path: PathBuf::from("test.rs"),
            symbol_name: "foo".to_string(),
            line_range: (1, 5),
            summary: "A test function".to_string(),
            embedding_text: "test".to_string(),
        };

        cache.insert(summary.clone());
        assert_eq!(cache.len(), 1);

        let retrieved = cache.get(Path::new("test.rs"), "foo");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().summary, "A test function");
    }

    #[test]
    fn test_summary_cache_invalidate_file() {
        let mut cache = SummaryCache::new();
        cache.insert(CodeSummary {
            file_path: PathBuf::from("a.rs"),
            symbol_name: "foo".to_string(),
            line_range: (1, 5),
            summary: "".to_string(),
            embedding_text: "".to_string(),
        });
        cache.insert(CodeSummary {
            file_path: PathBuf::from("a.rs"),
            symbol_name: "bar".to_string(),
            line_range: (6, 10),
            summary: "".to_string(),
            embedding_text: "".to_string(),
        });
        cache.insert(CodeSummary {
            file_path: PathBuf::from("b.rs"),
            symbol_name: "baz".to_string(),
            line_range: (1, 5),
            summary: "".to_string(),
            embedding_text: "".to_string(),
        });

        assert_eq!(cache.len(), 3);
        cache.invalidate_file(Path::new("a.rs"));
        assert_eq!(cache.len(), 1);
        assert!(cache.get(Path::new("b.rs"), "baz").is_some());
    }

    #[test]
    fn test_summary_cache_serialization() {
        let mut cache = SummaryCache::new();
        cache.insert(CodeSummary {
            file_path: PathBuf::from("test.rs"),
            symbol_name: "foo".to_string(),
            line_range: (1, 5),
            summary: "test summary".to_string(),
            embedding_text: "embedding".to_string(),
        });

        let json = cache.to_json().unwrap();
        let restored = SummaryCache::from_json(&json).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(
            restored.get(Path::new("test.rs"), "foo").unwrap().summary,
            "test summary"
        );
    }

    #[test]
    fn test_extract_parameters_rust() {
        let code = "fn process(items: &[String], count: usize) -> bool { true }";
        let params = extract_parameters(code, "rs");
        assert_eq!(params.len(), 2);
        assert!(params[0].contains("items"));
        assert!(params[1].contains("count"));
    }

    #[test]
    fn test_extract_parameters_python() {
        let code = "def process(self, items, count=10):\n    pass";
        let params = extract_parameters(code, "py");
        // self is filtered out
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_extract_return_type_rust() {
        let code = "fn validate(token: &str) -> Result<bool> {\n    Ok(true)\n}";
        let ret = extract_return_type(code, "rs");
        assert!(ret.is_some());
        assert!(ret.unwrap().contains("Result<bool>"));
    }

    #[test]
    fn test_complexity_estimation() {
        let simple = "fn foo() { return 1; }";
        let complex = "fn bar() { if a { for x in items { if b { match c { _ => {} } } } } }";
        assert!(estimate_complexity(complex) > estimate_complexity(simple));
    }

    #[test]
    fn test_extract_code_blocks_rust() {
        let content = "pub fn alpha() {\n    1\n}\n\npub struct Beta {\n    x: i32,\n}\n\npub fn gamma() {\n    2\n}\n";
        let blocks = extract_code_blocks(content, "rs");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].0, "alpha");
        assert_eq!(blocks[1].0, "Beta");
        assert_eq!(blocks[2].0, "gamma");
    }

    #[test]
    fn test_summarize_file_symbols_with_cache() {
        let content = "pub fn hello() {\n    println!(\"hi\");\n}\n\npub fn world() {\n    println!(\"world\");\n}\n";
        let mut cache = SummaryCache::new();

        let summaries = summarize_file_symbols(Path::new("test.rs"), content, &mut cache);
        assert_eq!(summaries.len(), 2);
        assert_eq!(cache.len(), 2);

        // Second call should use cache
        let summaries2 = summarize_file_symbols(Path::new("test.rs"), content, &mut cache);
        assert_eq!(summaries2.len(), 2);
        assert_eq!(cache.len(), 2); // no new entries
    }

    #[test]
    fn test_key_operations_detection() {
        let code = "async fn fetch_data() { let resp = reqwest::get(url).await?; let data = resp.json().await?; }";
        let ops = extract_key_operations(code);
        assert!(ops.contains(&"async operations".to_string()));
        assert!(ops.contains(&"network requests".to_string()));
        assert!(ops.contains(&"serialization".to_string()));
    }

    #[test]
    fn test_all_summaries() {
        let mut cache = SummaryCache::new();
        cache.insert(CodeSummary {
            file_path: PathBuf::from("a.rs"),
            symbol_name: "foo".to_string(),
            line_range: (1, 5),
            summary: "first".to_string(),
            embedding_text: "".to_string(),
        });
        cache.insert(CodeSummary {
            file_path: PathBuf::from("b.rs"),
            symbol_name: "bar".to_string(),
            line_range: (1, 10),
            summary: "second".to_string(),
            embedding_text: "".to_string(),
        });

        let all = cache.all_summaries();
        assert_eq!(all.len(), 2);
        let summaries: Vec<&str> = all.iter().map(|s| s.summary.as_str()).collect();
        assert!(summaries.contains(&"first"));
        assert!(summaries.contains(&"second"));
    }

    #[test]
    fn test_cache_remove() {
        let mut cache = SummaryCache::new();
        cache.insert(CodeSummary {
            file_path: PathBuf::from("test.rs"),
            symbol_name: "foo".to_string(),
            line_range: (1, 5),
            summary: "".to_string(),
            embedding_text: "".to_string(),
        });
        assert_eq!(cache.len(), 1);
        let removed = cache.remove(Path::new("test.rs"), "foo");
        assert!(removed.is_some());
        assert_eq!(cache.len(), 0);
    }
}
