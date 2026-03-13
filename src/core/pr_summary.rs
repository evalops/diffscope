use crate::adapters::llm::{LLMAdapter, LLMRequest};
use crate::core::{GitIntegration, UnifiedDiff};
use anyhow::Result;

pub struct PRSummaryGenerator;

#[derive(Debug, Clone, Default)]
pub struct SummaryOptions {
    pub include_diagram: bool,
}

impl PRSummaryGenerator {
    #[allow(dead_code)]
    pub async fn generate_summary(
        diffs: &[UnifiedDiff],
        git: &GitIntegration,
        adapter: &dyn LLMAdapter,
    ) -> Result<PRSummary> {
        Self::generate_summary_with_options(diffs, git, adapter, SummaryOptions::default()).await
    }

    pub async fn generate_summary_with_options(
        diffs: &[UnifiedDiff],
        git: &GitIntegration,
        adapter: &dyn LLMAdapter,
        options: SummaryOptions,
    ) -> Result<PRSummary> {
        let commits = git.get_recent_commits(10)?;
        Self::generate_summary_with_commits(diffs, &commits, adapter, options).await
    }

    /// Like `generate_summary_with_options`, but takes pre-fetched commit messages
    /// instead of a `GitIntegration` reference. This avoids holding `GitIntegration`
    /// (which is not `Sync`) across an `.await` boundary.
    pub async fn generate_summary_with_commits(
        diffs: &[UnifiedDiff],
        commits: &[String],
        adapter: &dyn LLMAdapter,
        options: SummaryOptions,
    ) -> Result<PRSummary> {
        let stats = Self::calculate_stats(diffs);
        let prompt = Self::build_summary_prompt(diffs, commits, &stats, &options);

        let request = LLMRequest {
            system_prompt: Self::get_system_prompt(),
            user_prompt: prompt,
            temperature: Some(0.3),
            max_tokens: Some(1000),
            response_schema: None,
        };

        let response = adapter.complete(request).await?;
        Self::parse_summary_response(&response.content, stats)
    }

    pub async fn generate_change_diagram(
        diffs: &[UnifiedDiff],
        adapter: &dyn LLMAdapter,
    ) -> Result<Option<String>> {
        let stats = Self::calculate_stats(diffs);
        let prompt = Self::build_diagram_prompt(diffs, &stats);
        let request = LLMRequest {
            system_prompt: "You create concise Mermaid diagrams for code changes. Respond with a single mermaid diagram or 'none'.".to_string(),
            user_prompt: prompt,
            temperature: Some(0.2),
            max_tokens: Some(800),
            response_schema: None,
        };

        let response = adapter.complete(request).await?;
        Ok(extract_mermaid_block(&response.content))
    }

    pub fn build_diagram_only_summary(diffs: &[UnifiedDiff], diagram: String) -> PRSummary {
        let stats = Self::calculate_stats(diffs);
        PRSummary {
            title: "Change Diagram".to_string(),
            description: String::new(),
            change_type: ChangeType::Chore,
            key_changes: Vec::new(),
            breaking_changes: None,
            testing_notes: String::new(),
            stats,
            visual_diff: Some(diagram),
        }
    }

    fn calculate_stats(diffs: &[UnifiedDiff]) -> ChangeStats {
        let mut stats = ChangeStats::default();

        for diff in diffs {
            stats.files_changed += 1;

            // Categorize file type
            let extension = diff
                .file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let file_name = diff
                .file_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");

            let file_stem = diff
                .file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            // Detect test files from common naming patterns
            let is_test = file_name.contains(".test.")
                || file_name.contains(".spec.")
                || file_name.contains("_test.")
                || file_stem.ends_with("_test")
                || file_stem.ends_with("_spec")
                || file_name.starts_with("test_");

            if is_test {
                stats.test_files += 1;
            } else {
                match extension {
                    "rs" | "py" | "js" | "ts" | "go" | "java" => stats.code_files += 1,
                    "md" | "txt" | "rst" => stats.doc_files += 1,
                    "yml" | "yaml" | "toml" | "json" => stats.config_files += 1,
                    _ => {}
                }
            }

            // Count changes
            for hunk in &diff.hunks {
                for change in &hunk.changes {
                    match change.change_type {
                        crate::core::diff_parser::ChangeType::Added => stats.lines_added += 1,
                        crate::core::diff_parser::ChangeType::Removed => stats.lines_removed += 1,
                        _ => {}
                    }
                }
            }
        }

        stats
    }

    fn build_summary_prompt(
        diffs: &[UnifiedDiff],
        commits: &[String],
        stats: &ChangeStats,
        options: &SummaryOptions,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str("Generate a comprehensive PR summary based on the following changes:\n\n");

        // Add statistics
        prompt.push_str("## Statistics\n");
        prompt.push_str(&format!("- Files changed: {}\n", stats.files_changed));
        prompt.push_str(&format!("- Lines added: {}\n", stats.lines_added));
        prompt.push_str(&format!("- Lines removed: {}\n", stats.lines_removed));
        prompt.push_str(&format!("- Code files: {}\n", stats.code_files));
        prompt.push_str(&format!("- Test files: {}\n", stats.test_files));
        prompt.push_str(&format!("- Documentation: {}\n\n", stats.doc_files));

        // Add recent commits
        if !commits.is_empty() {
            prompt.push_str("## Recent Commits\n");
            for commit in commits.iter().take(5) {
                prompt.push_str(&format!("- {}\n", commit));
            }
            prompt.push('\n');
        }

        // Add file changes summary
        prompt.push_str("## Files Changed\n");
        for diff in diffs {
            let path = diff.file_path.display();
            let added = diff
                .hunks
                .iter()
                .flat_map(|h| &h.changes)
                .filter(|c| matches!(c.change_type, crate::core::diff_parser::ChangeType::Added))
                .count();
            let removed = diff
                .hunks
                .iter()
                .flat_map(|h| &h.changes)
                .filter(|c| matches!(c.change_type, crate::core::diff_parser::ChangeType::Removed))
                .count();

            prompt.push_str(&format!("- {} (+{}, -{})\n", path, added, removed));
        }

        prompt.push_str("\n## Instructions\n");
        prompt.push_str("Create a structured summary with:\n");
        prompt.push_str("1. A brief one-line description\n");
        prompt.push_str("2. Key changes (3-5 bullet points)\n");
        prompt.push_str("3. Type of change (feature/fix/refactor/docs)\n");
        prompt.push_str("4. Breaking changes (if any)\n");
        prompt.push_str("5. Testing notes\n");
        if options.include_diagram {
            prompt.push_str(
                "6. A Mermaid diagram (sequence or flowchart) summarizing the change if helpful\n",
            );
        }

        prompt
    }

    fn build_diagram_prompt(diffs: &[UnifiedDiff], stats: &ChangeStats) -> String {
        let mut prompt = String::new();
        prompt.push_str(
            "Create a single Mermaid flowchart or sequence diagram that summarizes the change.\n",
        );
        prompt.push_str(
            "Use only one mermaid code block. If a diagram isn't useful, reply with 'none'.\n\n",
        );
        prompt.push_str("## Statistics\n");
        prompt.push_str(&format!("- Files changed: {}\n", stats.files_changed));
        prompt.push_str(&format!("- Lines added: {}\n", stats.lines_added));
        prompt.push_str(&format!("- Lines removed: {}\n", stats.lines_removed));

        prompt.push_str("\n## Files Changed\n");
        for diff in diffs.iter().take(20) {
            let path = diff.file_path.display();
            let added = diff
                .hunks
                .iter()
                .flat_map(|h| &h.changes)
                .filter(|c| matches!(c.change_type, crate::core::diff_parser::ChangeType::Added))
                .count();
            let removed = diff
                .hunks
                .iter()
                .flat_map(|h| &h.changes)
                .filter(|c| matches!(c.change_type, crate::core::diff_parser::ChangeType::Removed))
                .count();
            let status = if diff.is_deleted {
                "deleted"
            } else if diff.is_new {
                "new"
            } else {
                "modified"
            };
            prompt.push_str(&format!(
                "- {} ({}; +{}, -{})\n",
                path, status, added, removed
            ));
        }

        prompt
    }

    fn get_system_prompt() -> String {
        r#"You are an AI assistant that generates clear, concise PR summaries.
        
Focus on:
- What changed and why
- Impact on users/developers
- Key technical details
- Testing considerations

Format your response as:
SUMMARY: [one line description]
TYPE: [feature|fix|refactor|docs|test|chore]
KEY_CHANGES:
- [change 1]
- [change 2]
- [change 3]
BREAKING_CHANGES: [none or describe]
TESTING_NOTES: [what to test]
DIAGRAM: [optional mermaid diagram or none]"#
            .to_string()
    }

    fn parse_summary_response(content: &str, stats: ChangeStats) -> Result<PRSummary> {
        let mut summary = PRSummary {
            title: String::new(),
            description: String::new(),
            change_type: ChangeType::Feature,
            key_changes: Vec::new(),
            breaking_changes: None,
            testing_notes: String::new(),
            stats,
            visual_diff: extract_mermaid_diagram(content),
        };

        // Parse structured response
        let lines: Vec<&str> = content.lines().collect();
        let mut current_section = "";

        for line in lines {
            let line = line.trim();

            if line.starts_with("SUMMARY:") {
                summary.title = line
                    .strip_prefix("SUMMARY:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
            } else if line.starts_with("TYPE:") {
                let type_str = line.strip_prefix("TYPE:").unwrap_or("").trim();
                summary.change_type = match type_str {
                    "fix" => ChangeType::Fix,
                    "refactor" => ChangeType::Refactor,
                    "docs" => ChangeType::Docs,
                    "test" => ChangeType::Test,
                    "chore" => ChangeType::Chore,
                    _ => ChangeType::Feature,
                };
            } else if line.starts_with("KEY_CHANGES:") {
                current_section = "changes";
            } else if line.starts_with("BREAKING_CHANGES:") {
                let breaking = line.strip_prefix("BREAKING_CHANGES:").unwrap_or("").trim();
                if breaking != "none" && !breaking.is_empty() {
                    summary.breaking_changes = Some(breaking.to_string());
                }
            } else if line.starts_with("TESTING_NOTES:") {
                summary.testing_notes = line
                    .strip_prefix("TESTING_NOTES:")
                    .unwrap_or("")
                    .trim()
                    .to_string();
            } else if current_section == "changes" && line.starts_with("- ") {
                summary
                    .key_changes
                    .push(line.strip_prefix("- ").unwrap_or("").to_string());
            }
        }

        // Build description from key changes
        if !summary.key_changes.is_empty() {
            summary.description = summary.key_changes.join("\n");
        }

        Ok(summary)
    }
}

#[derive(Debug, Clone)]
pub struct PRSummary {
    pub title: String,
    pub description: String,
    pub change_type: ChangeType,
    pub key_changes: Vec<String>,
    pub breaking_changes: Option<String>,
    pub testing_notes: String,
    pub stats: ChangeStats,
    pub visual_diff: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ChangeType {
    Feature,
    Fix,
    Refactor,
    Docs,
    Test,
    Chore,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeStats {
    pub files_changed: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub code_files: usize,
    pub test_files: usize,
    pub doc_files: usize,
    pub config_files: usize,
}

impl PRSummary {
    pub fn to_markdown(&self) -> String {
        let mut output = String::new();

        // Title and type badge
        let type_emoji = match self.change_type {
            ChangeType::Feature => "✨",
            ChangeType::Fix => "🐛",
            ChangeType::Refactor => "♻️",
            ChangeType::Docs => "📚",
            ChangeType::Test => "🧪",
            ChangeType::Chore => "🔧",
        };

        output.push_str(&format!("# {} {}\n\n", type_emoji, self.title));

        // Description
        if !self.description.is_empty() {
            output.push_str(&format!("{}\n\n", self.description));
        }

        // Key changes
        if !self.key_changes.is_empty() {
            output.push_str("## 🎯 Key Changes\n\n");
            for change in &self.key_changes {
                output.push_str(&format!("- {}\n", change));
            }
            output.push('\n');
        }

        // Statistics
        output.push_str("## 📊 Change Statistics\n\n");
        output.push_str(&format!(
            "- **Files Changed:** {}\n",
            self.stats.files_changed
        ));
        output.push_str(&format!(
            "- **Lines Added:** {} +++\n",
            self.stats.lines_added
        ));
        output.push_str(&format!(
            "- **Lines Removed:** {} ---\n",
            self.stats.lines_removed
        ));

        if self.stats.test_files > 0 {
            output.push_str(&format!(
                "- **Tests Modified:** {} files\n",
                self.stats.test_files
            ));
        }
        if self.stats.doc_files > 0 {
            output.push_str(&format!(
                "- **Docs Updated:** {} files\n",
                self.stats.doc_files
            ));
        }
        output.push('\n');

        // Breaking changes
        if let Some(breaking) = &self.breaking_changes {
            output.push_str("## ⚠️ Breaking Changes\n\n");
            output.push_str(&format!("{}\n\n", breaking));
        }

        // Testing notes
        if !self.testing_notes.is_empty() {
            output.push_str("## 🧪 Testing Notes\n\n");
            output.push_str(&format!("{}\n\n", self.testing_notes));
        }

        if let Some(diagram) = &self.visual_diff {
            if !diagram.trim().is_empty() {
                output.push_str("## 🗺️ Change Diagram\n\n");
                output.push_str("```mermaid\n");
                output.push_str(diagram.trim());
                output.push_str("\n```\n\n");
            }
        }

        output
    }
}

fn extract_mermaid_diagram(content: &str) -> Option<String> {
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("DIAGRAM:") {
            if trimmed.to_lowercase().contains("none") {
                return None;
            }

            // Seek to mermaid block
            for next_line in lines.by_ref() {
                let next_trimmed = next_line.trim();
                if next_trimmed.starts_with("```") && next_trimmed.contains("mermaid") {
                    break;
                }
            }

            let mut diagram_lines = Vec::new();
            for block_line in lines.by_ref() {
                let block_trimmed = block_line.trim();
                if block_trimmed.starts_with("```") {
                    break;
                }
                diagram_lines.push(block_line);
            }

            let diagram = diagram_lines.join("\n").trim().to_string();
            if diagram.is_empty() {
                return None;
            }
            return Some(diagram);
        }
    }

    None
}

fn extract_mermaid_block(content: &str) -> Option<String> {
    if content.to_lowercase().contains("none") {
        return None;
    }
    if let Some(diagram) = extract_mermaid_diagram(content) {
        return Some(diagram);
    }

    let mut in_block = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && trimmed.contains("mermaid") {
            in_block = true;
            continue;
        }
        if trimmed.starts_with("```") && in_block {
            break;
        }
        if in_block {
            lines.push(line);
        }
    }
    let diagram = lines.join("\n").trim().to_string();
    if !diagram.is_empty() {
        return Some(diagram);
    }

    let fallback = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("flowchart")
                || trimmed.starts_with("graph")
                || trimmed.starts_with("sequenceDiagram")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if fallback.trim().is_empty() {
        None
    } else {
        Some(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{ChangeType as DiffChangeType, DiffHunk, DiffLine};
    use std::path::PathBuf;

    fn make_diff(path: &str) -> UnifiedDiff {
        UnifiedDiff {
            file_path: PathBuf::from(path),
            old_content: None,
            new_content: None,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                context: String::new(),
                changes: vec![
                    DiffLine {
                        content: "old line".to_string(),
                        change_type: DiffChangeType::Removed,
                        old_line_no: Some(1),
                        new_line_no: None,
                    },
                    DiffLine {
                        content: "new line 1".to_string(),
                        change_type: DiffChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(1),
                    },
                    DiffLine {
                        content: "new line 2".to_string(),
                        change_type: DiffChangeType::Added,
                        old_line_no: None,
                        new_line_no: Some(2),
                    },
                ],
            }],
            is_binary: false,
            is_deleted: false,
            is_new: false,
        }
    }

    // ── Bug: test file detection was checking for ".test"/".spec" extensions ──
    //
    // Real test files use patterns like `foo.test.js`, `foo_test.rs`,
    // `foo.spec.ts`, `test_utils.py`, etc. Their extension() is still
    // `.js`, `.rs`, `.ts`, `.py` — NOT `.test` or `.spec`.
    // The old code used `match extension { "test" | "spec" => ... }` which
    // never matched real test files.

    #[test]
    fn test_calculate_stats_js_test_file() {
        let diffs = vec![make_diff("src/utils.test.js")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(
            stats.test_files, 1,
            "foo.test.js should be counted as a test file, not code"
        );
        assert_eq!(
            stats.code_files, 0,
            "foo.test.js should NOT be counted as a code file"
        );
    }

    #[test]
    fn test_calculate_stats_rs_test_file() {
        let diffs = vec![make_diff("src/parser_test.rs")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(
            stats.test_files, 1,
            "parser_test.rs should be counted as a test file"
        );
    }

    #[test]
    fn test_calculate_stats_spec_file() {
        let diffs = vec![make_diff("tests/auth.spec.ts")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(
            stats.test_files, 1,
            "auth.spec.ts should be counted as a test file"
        );
    }

    #[test]
    fn test_calculate_stats_python_test_file() {
        let diffs = vec![make_diff("tests/test_parser.py")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(
            stats.test_files, 1,
            "test_parser.py should be counted as a test file"
        );
    }

    #[test]
    fn test_calculate_stats_regular_code_file() {
        let diffs = vec![make_diff("src/main.rs")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(stats.code_files, 1);
        assert_eq!(stats.test_files, 0);
    }

    #[test]
    fn test_calculate_stats_doc_file() {
        let diffs = vec![make_diff("docs/README.md")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(stats.doc_files, 1);
    }

    #[test]
    fn test_calculate_stats_config_file() {
        let diffs = vec![make_diff("config.yml")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(stats.config_files, 1);
    }

    #[test]
    fn test_calculate_stats_line_counts() {
        let diffs = vec![make_diff("src/lib.rs")];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(stats.lines_added, 2);
        assert_eq!(stats.lines_removed, 1);
        assert_eq!(stats.files_changed, 1);
    }

    #[test]
    fn test_calculate_stats_mixed_files() {
        let diffs = vec![
            make_diff("src/main.rs"),
            make_diff("src/main_test.rs"),
            make_diff("README.md"),
            make_diff("config.toml"),
        ];
        let stats = PRSummaryGenerator::calculate_stats(&diffs);
        assert_eq!(stats.files_changed, 4);
        assert_eq!(stats.code_files, 1);
        assert_eq!(stats.test_files, 1);
        assert_eq!(stats.doc_files, 1);
        assert_eq!(stats.config_files, 1);
    }

    #[test]
    fn test_parse_summary_response() {
        let content = "SUMMARY: Add new auth flow\nTYPE: feature\nKEY_CHANGES:\n- Added OAuth2 support\n- Removed old session tokens\nBREAKING_CHANGES: Old session tokens are no longer valid\nTESTING_NOTES: Test login flow end-to-end";
        let stats = ChangeStats::default();
        let summary = PRSummaryGenerator::parse_summary_response(content, stats).unwrap();
        assert_eq!(summary.title, "Add new auth flow");
        assert_eq!(summary.key_changes.len(), 2);
        assert!(summary.breaking_changes.is_some());
        assert!(!summary.testing_notes.is_empty());
    }

    #[test]
    fn test_parse_summary_response_no_breaking() {
        let content = "SUMMARY: Minor fix\nTYPE: fix\nKEY_CHANGES:\n- Fixed null check\nBREAKING_CHANGES: none\nTESTING_NOTES: Run unit tests";
        let stats = ChangeStats::default();
        let summary = PRSummaryGenerator::parse_summary_response(content, stats).unwrap();
        assert!(summary.breaking_changes.is_none());
    }

    #[test]
    fn test_extract_mermaid_diagram() {
        let content = "DIAGRAM: Here's the flow\n```mermaid\nflowchart LR\nA --> B\n```\n";
        let diagram = extract_mermaid_diagram(content);
        assert!(diagram.is_some());
        assert!(diagram.unwrap().contains("flowchart"));
    }

    #[test]
    fn test_extract_mermaid_diagram_none() {
        let content = "DIAGRAM: none\n";
        let diagram = extract_mermaid_diagram(content);
        assert!(diagram.is_none());
    }

    #[test]
    fn test_extract_mermaid_block_from_code() {
        let content = "```mermaid\nsequenceDiagram\nA->>B: Hello\n```\n";
        let block = extract_mermaid_block(content);
        assert!(block.is_some());
        assert!(block.unwrap().contains("sequenceDiagram"));
    }

    #[test]
    fn test_extract_mermaid_block_none_keyword() {
        let content = "There is none to show here.";
        let block = extract_mermaid_block(content);
        assert!(block.is_none());
    }

    #[test]
    fn test_pr_summary_to_markdown() {
        let summary = PRSummary {
            title: "Test PR".to_string(),
            description: "Description".to_string(),
            change_type: ChangeType::Feature,
            key_changes: vec!["Change 1".to_string()],
            breaking_changes: Some("API changed".to_string()),
            testing_notes: "Test all endpoints".to_string(),
            stats: ChangeStats {
                files_changed: 3,
                lines_added: 50,
                lines_removed: 10,
                code_files: 2,
                test_files: 1,
                doc_files: 0,
                config_files: 0,
            },
            visual_diff: None,
        };
        let md = summary.to_markdown();
        assert!(md.contains("Test PR"));
        assert!(md.contains("Change 1"));
        assert!(md.contains("Breaking Changes"));
        assert!(md.contains("Test all endpoints"));
        assert!(md.contains("Tests Modified"));
    }
}
