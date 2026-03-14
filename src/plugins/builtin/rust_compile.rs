use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::core::comment::{Category, Severity};
use crate::core::diff_parser::ChangeType;
use crate::core::UnifiedDiff;
use crate::plugins::{AnalyzerFinding, PreAnalysis, PreAnalyzer};

static SHORTHAND_FIELD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*$").unwrap());
static STRUCT_LITERAL_START: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_:]*)\s*\{\s*$").unwrap());

pub struct RustCompileAnalyzer;

impl RustCompileAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn analyze_diff_with_source(
        diff: &UnifiedDiff,
        source_lines: Option<&[String]>,
    ) -> Vec<AnalyzerFinding> {
        if diff.file_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            return Vec::new();
        }

        let mut findings = Vec::new();

        for hunk in &diff.hunks {
            for (idx, change) in hunk.changes.iter().enumerate() {
                if change.change_type != ChangeType::Removed {
                    continue;
                }

                let Some(captures) = SHORTHAND_FIELD.captures(change.content.as_str()) else {
                    continue;
                };
                let Some(field_name) = captures.get(1).map(|capture| capture.as_str()) else {
                    continue;
                };
                if has_field_replacement(hunk, idx, field_name) {
                    continue;
                }

                let Some(struct_name) = nearest_struct_literal_name(
                    &hunk.changes[..idx],
                    source_lines,
                    change.old_line_no.or(change.new_line_no).unwrap_or(0),
                ) else {
                    continue;
                };
                let canonical_struct = struct_name
                    .rsplit("::")
                    .next()
                    .unwrap_or(struct_name.as_str());
                let line_number = change.old_line_no.or(change.new_line_no).unwrap_or(0);
                if line_number == 0 {
                    continue;
                }

                findings.push(AnalyzerFinding {
                    file_path: diff.file_path.clone(),
                    line_number,
                    content: format!(
                        "Removing `{field_name},` from the `{canonical_struct}` initializer leaves the required `{field_name}` field unset, so this change will not compile."
                    ),
                    rule_id: Some(format!(
                        "compile.{}.{}",
                        normalize_struct_name(canonical_struct),
                        field_name.to_ascii_lowercase()
                    )),
                    suggestion: Some(format!(
                        "Restore `{field_name}` in the `{canonical_struct}` initializer, or remove the field from `{canonical_struct}` and update every initializer together."
                    )),
                    severity: Severity::Error,
                    category: Category::Bug,
                    confidence: 0.99,
                    source: "rust-compile".to_string(),
                    tags: vec!["compile-error".to_string(), "struct-initializer".to_string()],
                    metadata: HashMap::new(),
                });
            }
        }

        findings
    }
}

#[async_trait]
impl PreAnalyzer for RustCompileAnalyzer {
    fn id(&self) -> &str {
        "rust-compile"
    }

    async fn run(&self, diff: &UnifiedDiff, repo_path: &str) -> Result<PreAnalysis> {
        let source_lines = load_source_lines(repo_path, &diff.file_path);
        Ok(PreAnalysis {
            context_chunks: Vec::new(),
            findings: Self::analyze_diff_with_source(diff, source_lines.as_deref()),
        })
    }
}

fn has_field_replacement(
    hunk: &crate::core::diff_parser::DiffHunk,
    removed_idx: usize,
    field_name: &str,
) -> bool {
    hunk.changes.iter().enumerate().any(|(idx, change)| {
        idx != removed_idx
            && change.change_type == ChangeType::Added
            && line_starts_with_field(change.content.as_str(), field_name)
    })
}

fn nearest_struct_literal_name(
    changes_before: &[crate::core::diff_parser::DiffLine],
    source_lines: Option<&[String]>,
    line_number: usize,
) -> Option<String> {
    find_struct_literal_name_in_lines(changes_before.iter().map(|change| change.content.as_str()))
        .or_else(|| {
            source_lines.and_then(|lines| {
                if line_number == 0 || lines.is_empty() {
                    return None;
                }
                let end = line_number.saturating_sub(1).min(lines.len());
                let start = end.saturating_sub(12);
                find_struct_literal_name_in_lines(lines[start..end].iter().map(String::as_str))
            })
        })
}

fn find_struct_literal_name_in_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    lines
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .filter_map(|line| STRUCT_LITERAL_START.captures(line))
        .filter_map(|captures| captures.get(1).map(|capture| capture.as_str().to_string()))
        .find(|candidate| !is_control_flow_token(candidate))
}

fn line_starts_with_field(line: &str, field_name: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix(field_name) else {
        return false;
    };
    matches!(rest.chars().next(), Some(':') | Some(','))
}

fn is_control_flow_token(candidate: &str) -> bool {
    matches!(
        candidate.rsplit("::").next().unwrap_or(candidate),
        "if" | "else" | "match" | "loop" | "for" | "while" | "fn" | "struct" | "enum"
    )
}

fn normalize_struct_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn load_source_lines(repo_path: &str, file_path: &Path) -> Option<Vec<String>> {
    let path = Path::new(repo_path).join(file_path);
    let content = fs::read_to_string(path).ok()?;
    Some(content.lines().map(|line| line.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{DiffHunk, DiffLine};
    use std::path::PathBuf;

    fn make_rust_diff(changes: Vec<DiffLine>) -> UnifiedDiff {
        UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("src/parsing/llm_response.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 155,
                old_lines: changes.len(),
                new_start: 155,
                new_lines: changes
                    .iter()
                    .filter(|change| change.change_type != ChangeType::Removed)
                    .count(),
                context: String::new(),
                changes,
            }],
        }
    }

    fn context(line: usize, content: &str) -> DiffLine {
        DiffLine {
            old_line_no: Some(line),
            new_line_no: Some(line),
            change_type: ChangeType::Context,
            content: content.to_string(),
        }
    }

    fn removed(line: usize, content: &str) -> DiffLine {
        DiffLine {
            old_line_no: Some(line),
            new_line_no: None,
            change_type: ChangeType::Removed,
            content: content.to_string(),
        }
    }

    fn added(line: usize, content: &str) -> DiffLine {
        DiffLine {
            old_line_no: None,
            new_line_no: Some(line),
            change_type: ChangeType::Added,
            content: content.to_string(),
        }
    }

    #[test]
    fn analyzer_id_is_stable() {
        assert_eq!(RustCompileAnalyzer::new().id(), "rust-compile");
    }

    #[test]
    fn detects_missing_struct_initializer_field() {
        let diff = make_rust_diff(vec![
            context(155, "    comments.push(core::comment::RawComment {"),
            context(156, "        file_path: file_path.to_path_buf(),"),
            removed(159, "        rule_id,"),
            context(160, "        suggestion,"),
        ]);

        let findings = RustCompileAnalyzer::analyze_diff_with_source(&diff, None);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line_number, 159);
        assert_eq!(
            findings[0].rule_id.as_deref(),
            Some("compile.rawcomment.rule_id")
        );
        assert!(findings[0].content.contains("will not compile"));
    }

    #[test]
    fn detects_missing_initializer_field_from_source_context() {
        let diff = make_rust_diff(vec![
            context(156, "        file_path: file_path.to_path_buf(),"),
            context(157, "        line_number,"),
            removed(159, "        rule_id,"),
            context(160, "        suggestion,"),
        ]);
        let source_lines = vec![
            "fn parse_primary() {".to_string(),
            "    comments.push(core::comment::RawComment {".to_string(),
            "        file_path: file_path.to_path_buf(),".to_string(),
            "        line_number,".to_string(),
            "        content,".to_string(),
            "        rule_id,".to_string(),
            "        suggestion,".to_string(),
            "    });".to_string(),
            "}".to_string(),
        ];

        let findings = RustCompileAnalyzer::analyze_diff_with_source(&diff, Some(&source_lines));

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].rule_id.as_deref(),
            Some("compile.rawcomment.rule_id")
        );
    }

    #[test]
    fn ignores_field_replacements_in_same_hunk() {
        let diff = make_rust_diff(vec![
            context(155, "    comments.push(core::comment::RawComment {"),
            removed(159, "        rule_id,"),
            added(159, "        rule_id: normalized_rule_id,"),
        ]);

        let findings = RustCompileAnalyzer::analyze_diff_with_source(&diff, None);

        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_non_rust_files() {
        let mut diff = make_rust_diff(vec![removed(12, "        rule_id,")]);
        diff.file_path = PathBuf::from("src/main.py");

        let findings = RustCompileAnalyzer::analyze_diff_with_source(&diff, None);

        assert!(findings.is_empty());
    }
}
