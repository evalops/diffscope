use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

use crate::adapters::llm::ToolDefinition;
use crate::core::context::ContextFetcher;
use crate::core::git_history::GitHistoryAnalyzer;
use crate::core::symbol_graph::SymbolGraph;
use crate::core::symbol_index::SymbolIndex;

/// Maximum bytes returned by any single tool execution (8 KB).
const MAX_TOOL_OUTPUT_BYTES: usize = 8 * 1024;

/// Context shared across all review tools for a single review session.
pub struct ReviewToolContext {
    pub repo_path: PathBuf,
    pub context_fetcher: Arc<ContextFetcher>,
    pub symbol_index: Option<Arc<SymbolIndex>>,
    pub symbol_graph: Option<Arc<SymbolGraph>>,
    pub git_history: Option<Arc<GitHistoryAnalyzer>>,
}

#[async_trait]
pub trait ReviewTool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, input: serde_json::Value) -> Result<String>;
}

/// Build the standard set of review tools from the given context.
pub fn build_review_tools(ctx: Arc<ReviewToolContext>) -> Vec<Box<dyn ReviewTool>> {
    let mut tools: Vec<Box<dyn ReviewTool>> = vec![
        Box::new(ReadFileTool { ctx: ctx.clone() }),
        Box::new(SearchCodebaseTool { ctx: ctx.clone() }),
    ];

    if ctx.symbol_index.is_some() {
        tools.push(Box::new(LookupSymbolTool { ctx: ctx.clone() }));
        tools.push(Box::new(GetDefinitionsTool { ctx: ctx.clone() }));
    }

    if ctx.symbol_graph.is_some() {
        tools.push(Box::new(GetRelatedSymbolsTool { ctx: ctx.clone() }));
    }

    if ctx.git_history.is_some() {
        tools.push(Box::new(GetFileHistoryTool { ctx: ctx.clone() }));
    }

    tools
}

fn truncate_output(s: String) -> String {
    if s.len() <= MAX_TOOL_OUTPUT_BYTES {
        s
    } else {
        let mut truncated = s[..MAX_TOOL_OUTPUT_BYTES].to_string();
        truncated.push_str("\n... [truncated to 8KB]");
        truncated
    }
}

// ── read_file ──────────────────────────────────────────────────────────

struct ReadFileTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file in the repository. Returns the file content with line numbers. Use start_line/end_line to read a specific range.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file relative to the repository root"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "First line to read (1-based, inclusive). Omit to start from the beginning."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Last line to read (1-based, inclusive). Omit to read to the end."
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
        let start_line = input["start_line"].as_u64().map(|n| n as usize);
        let end_line = input["end_line"].as_u64().map(|n| n as usize);

        let full_path = self.ctx.repo_path.join(file_path);
        if !full_path.exists() {
            return Ok(format!("Error: file not found: {}", file_path));
        }

        // Prevent path traversal
        let canonical = full_path.canonicalize()?;
        let repo_canonical = self.ctx.repo_path.canonicalize()?;
        if !canonical.starts_with(&repo_canonical) {
            return Ok("Error: path traversal not allowed".to_string());
        }

        let content = tokio::fs::read_to_string(&full_path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let start = start_line.unwrap_or(1).max(1);
        let end = end_line.unwrap_or(lines.len()).min(lines.len());

        let mut output = String::new();
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            if line_num >= start && line_num <= end {
                output.push_str(&format!("{:>4} | {}\n", line_num, line));
            }
        }

        Ok(truncate_output(output))
    }
}

// ── search_codebase ────────────────────────────────────────────────────

struct SearchCodebaseTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for SearchCodebaseTool {
    fn name(&self) -> &str {
        "search_codebase"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_codebase".to_string(),
            description: "Search the codebase for a regex pattern. Returns matching lines with file paths and line numbers.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "file_glob": {
                        "type": "string",
                        "description": "Optional glob to filter files (e.g. '*.rs', 'src/**/*.ts')"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of matching lines to return (default: 20)"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("pattern is required"))?;
        let file_glob = input["file_glob"].as_str();
        let max_results = input["max_results"].as_u64().unwrap_or(20) as usize;

        let mut cmd = tokio::process::Command::new("grep");
        cmd.arg("-rn")
            .arg("--include")
            .arg(file_glob.unwrap_or("*"))
            .arg("-E")
            .arg(pattern)
            .arg(".")
            .current_dir(&self.ctx.repo_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let lines: Vec<&str> = stdout.lines().take(max_results).collect();
        if lines.is_empty() {
            return Ok("No matches found.".to_string());
        }

        Ok(truncate_output(lines.join("\n")))
    }
}

// ── lookup_symbol ──────────────────────────────────────────────────────

struct LookupSymbolTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for LookupSymbolTool {
    fn name(&self) -> &str {
        "lookup_symbol"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "lookup_symbol".to_string(),
            description: "Look up a symbol (function, struct, class, etc.) in the codebase index. Returns definition locations and code snippets.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol_name": {
                        "type": "string",
                        "description": "The name of the symbol to look up"
                    }
                },
                "required": ["symbol_name"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let symbol_name = input["symbol_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("symbol_name is required"))?;

        let index = self
            .ctx
            .symbol_index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Symbol index not available"))?;

        match index.lookup(symbol_name) {
            Some(locations) => {
                let mut output = format!(
                    "Found {} location(s) for '{}':\n\n",
                    locations.len(),
                    symbol_name
                );
                for loc in locations.iter().take(5) {
                    output.push_str(&format!(
                        "{}:{}-{}\n{}\n\n",
                        loc.file_path.display(),
                        loc.line_range.0,
                        loc.line_range.1,
                        loc.snippet
                    ));
                }
                Ok(truncate_output(output))
            }
            None => Ok(format!("Symbol '{}' not found in index.", symbol_name)),
        }
    }
}

// ── get_related_symbols ────────────────────────────────────────────────

struct GetRelatedSymbolsTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for GetRelatedSymbolsTool {
    fn name(&self) -> &str {
        "get_related_symbols"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_related_symbols".to_string(),
            description: "Find symbols related to the given symbols (callers, callees, implementations, etc.) using the symbol graph.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbols": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of symbol names to find relations for"
                    },
                    "max_hops": {
                        "type": "integer",
                        "description": "Maximum graph traversal depth (default: 2)"
                    }
                },
                "required": ["symbols"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let symbols: Vec<String> = input["symbols"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("symbols array is required"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let max_hops = input["max_hops"].as_u64().unwrap_or(2) as usize;

        let graph = self
            .ctx
            .symbol_graph
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Symbol graph not available"))?;

        let related = graph.related_symbols(&symbols, max_hops, 20);
        if related.is_empty() {
            return Ok("No related symbols found.".to_string());
        }

        let mut output = format!("Found {} related symbol(s):\n\n", related.len());
        for sym in &related {
            output.push_str(&format!(
                "- {} ({}:{}, relevance: {:.2}, hops: {})\n",
                sym.name,
                sym.file_path.display(),
                sym.line,
                sym.relevance_score,
                sym.hops
            ));
        }

        Ok(truncate_output(output))
    }
}

// ── get_file_history ───────────────────────────────────────────────────

struct GetFileHistoryTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for GetFileHistoryTool {
    fn name(&self) -> &str {
        "get_file_history"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_file_history".to_string(),
            description: "Get git history and churn metrics for a file: commit count, bug fix count, distinct authors, risk score.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file relative to the repository root"
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;

        let history = self
            .ctx
            .git_history
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Git history not available"))?;

        let path = std::path::Path::new(file_path);
        match history.file_info(path) {
            Some(info) => Ok(format!(
                "File: {}\nCommits: {}\nBug fixes: {}\nDistinct authors: {}\nLast modified: {}\nLines added (total): {}\nLines removed (total): {}\nRisk score: {:.2}\nHigh churn: {}\nBug prone: {}",
                file_path,
                info.commit_count,
                info.bug_fix_count,
                info.distinct_authors,
                info.last_modified.as_deref().unwrap_or("unknown"),
                info.lines_added_total,
                info.lines_removed_total,
                info.risk_score(),
                info.is_high_churn(),
                info.is_bug_prone()
            )),
            None => Ok(format!("No history found for '{}'.", file_path)),
        }
    }
}

// ── get_definitions ────────────────────────────────────────────────────

struct GetDefinitionsTool {
    ctx: Arc<ReviewToolContext>,
}

#[async_trait]
impl ReviewTool for GetDefinitionsTool {
    fn name(&self) -> &str {
        "get_definitions"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_definitions".to_string(),
            description: "Get the definitions of specific symbols as they appear in a given file, using the symbol index for precise lookup.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file relative to the repository root"
                    },
                    "symbols": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of symbol names to get definitions for"
                    }
                },
                "required": ["file_path", "symbols"]
            }),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let file_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;
        let symbols: Vec<String> = input["symbols"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("symbols array is required"))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        let index = self
            .ctx
            .symbol_index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Symbol index not available"))?;

        let path_buf = PathBuf::from(file_path);
        let chunks = self
            .ctx
            .context_fetcher
            .fetch_related_definitions_with_index(&path_buf, &symbols, index, 10, 2, 5)
            .await?;

        if chunks.is_empty() {
            return Ok(format!(
                "No definitions found for {:?} in context of '{}'.",
                symbols, file_path
            ));
        }

        let mut output = format!("Found {} definition chunk(s):\n\n", chunks.len());
        for chunk in &chunks {
            output.push_str(&format!(
                "── {} ({:?}) ──\n{}\n\n",
                chunk.file_path.display(),
                chunk.context_type,
                chunk.content
            ));
        }

        Ok(truncate_output(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_output_short() {
        let s = "hello".to_string();
        assert_eq!(truncate_output(s.clone()), s);
    }

    #[test]
    fn test_truncate_output_long() {
        let s = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 100);
        let result = truncate_output(s);
        assert!(result.len() < MAX_TOOL_OUTPUT_BYTES + 50);
        assert!(result.contains("[truncated to 8KB]"));
    }

    #[test]
    fn test_build_review_tools_minimal() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tools = build_review_tools(ctx);
        // At minimum: read_file + search_codebase
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name(), "read_file");
        assert_eq!(tools[1].name(), "search_codebase");
    }

    #[test]
    fn test_build_review_tools_all() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: Some(Arc::new(SymbolIndex::default())),
            symbol_graph: Some(Arc::new(SymbolGraph::new())),
            git_history: Some(Arc::new(GitHistoryAnalyzer::new())),
        });
        let tools = build_review_tools(ctx);
        assert_eq!(tools.len(), 6);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"search_codebase"));
        assert!(names.contains(&"lookup_symbol"));
        assert!(names.contains(&"get_definitions"));
        assert!(names.contains(&"get_related_symbols"));
        assert!(names.contains(&"get_file_history"));
    }

    #[test]
    fn test_tool_definitions_have_required_fields() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: Some(Arc::new(SymbolIndex::default())),
            symbol_graph: Some(Arc::new(SymbolGraph::new())),
            git_history: Some(Arc::new(GitHistoryAnalyzer::new())),
        });
        let tools = build_review_tools(ctx);
        for tool in &tools {
            let def = tool.definition();
            assert!(!def.name.is_empty());
            assert!(!def.description.is_empty());
            assert!(def.input_schema.is_object());
        }
    }

    #[tokio::test]
    async fn test_read_file_tool_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = ReadFileTool { ctx };
        let result = tool
            .execute(json!({"file_path": "nonexistent.rs"}))
            .await
            .unwrap();
        assert!(result.contains("not found"));
    }

    #[tokio::test]
    async fn test_read_file_tool_success() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = ReadFileTool { ctx };
        let result = tool.execute(json!({"file_path": "test.rs"})).await.unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
    }

    #[tokio::test]
    async fn test_read_file_tool_line_range() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = ReadFileTool { ctx };
        let result = tool
            .execute(json!({"file_path": "test.rs", "start_line": 2, "end_line": 4}))
            .await
            .unwrap();
        assert!(!result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(!result.contains("line5"));
    }

    #[tokio::test]
    async fn test_read_file_path_traversal_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = ReadFileTool { ctx };
        let result = tool
            .execute(json!({"file_path": "../../../etc/passwd"}))
            .await;
        // Either returns error or a "not allowed" / "not found" message
        if let Ok(msg) = result {
            assert!(
                msg.contains("not allowed") || msg.contains("not found"),
                "Got: {}",
                msg
            );
        }
    }

    #[tokio::test]
    async fn test_lookup_symbol_not_found() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: Some(Arc::new(SymbolIndex::default())),
            symbol_graph: None,
            git_history: None,
        });
        let tool = LookupSymbolTool { ctx };
        let result = tool
            .execute(json!({"symbol_name": "nonexistent"}))
            .await
            .unwrap();
        assert!(result.contains("not found"));
    }

    #[tokio::test]
    async fn test_search_codebase_tool_finds_pattern() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("hello.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = SearchCodebaseTool { ctx };
        let result = tool
            .execute(json!({"pattern": "println", "max_results": 5}))
            .await
            .unwrap();
        assert!(
            result.contains("println"),
            "Should find println in results: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_search_codebase_tool_no_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.rs"), "fn main() {}\n").unwrap();

        let ctx = Arc::new(ReviewToolContext {
            repo_path: dir.path().to_path_buf(),
            context_fetcher: Arc::new(ContextFetcher::new(dir.path().to_path_buf())),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = SearchCodebaseTool { ctx };
        let result = tool
            .execute(json!({"pattern": "zzz_nonexistent_pattern_zzz"}))
            .await
            .unwrap();
        assert!(
            result.contains("No matches"),
            "Should indicate no matches: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_truncate_output_preserves_exact_boundary() {
        // Exactly at the limit should not truncate
        let s = "x".repeat(MAX_TOOL_OUTPUT_BYTES);
        let result = truncate_output(s.clone());
        assert_eq!(result.len(), MAX_TOOL_OUTPUT_BYTES);
        assert!(!result.contains("truncated"));

        // One byte over should truncate
        let s = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 1);
        let result = truncate_output(s);
        assert!(result.contains("[truncated to 8KB]"));
    }

    #[tokio::test]
    async fn test_get_file_history_no_analyzer() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = GetFileHistoryTool { ctx };
        let result = tool.execute(json!({"file_path": "test.rs"})).await;
        assert!(result.is_err(), "Should error when git_history is None");
    }

    #[tokio::test]
    async fn test_get_file_history_with_empty_analyzer() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: None,
            git_history: Some(Arc::new(GitHistoryAnalyzer::new())),
        });
        let tool = GetFileHistoryTool { ctx };
        let result = tool
            .execute(json!({"file_path": "nonexistent.rs"}))
            .await
            .unwrap();
        assert!(
            result.contains("No history"),
            "Should indicate no history: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_get_related_symbols_no_graph() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = GetRelatedSymbolsTool { ctx };
        let result = tool.execute(json!({"symbols": ["foo"]})).await;
        assert!(result.is_err(), "Should error when symbol_graph is None");
    }

    #[tokio::test]
    async fn test_get_related_symbols_empty_graph() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: Some(Arc::new(SymbolGraph::new())),
            git_history: None,
        });
        let tool = GetRelatedSymbolsTool { ctx };
        let result = tool
            .execute(json!({"symbols": ["nonexistent_symbol"]}))
            .await
            .unwrap();
        assert!(
            result.contains("No related"),
            "Should indicate no related symbols: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_get_definitions_no_index() {
        let ctx = Arc::new(ReviewToolContext {
            repo_path: PathBuf::from("/tmp/test"),
            context_fetcher: Arc::new(ContextFetcher::new(PathBuf::from("/tmp/test"))),
            symbol_index: None,
            symbol_graph: None,
            git_history: None,
        });
        let tool = GetDefinitionsTool { ctx };
        let result = tool
            .execute(json!({"file_path": "test.rs", "symbols": ["foo"]}))
            .await;
        assert!(result.is_err(), "Should error when symbol_index is None");
    }
}
