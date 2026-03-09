use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Churn and risk metrics for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChurnInfo {
    pub file_path: PathBuf,
    pub commit_count: usize,
    pub bug_fix_count: usize,
    pub distinct_authors: usize,
    pub last_modified: Option<String>,
    pub lines_added_total: usize,
    pub lines_removed_total: usize,
    pub age_days: Option<u64>,
}

impl FileChurnInfo {
    /// Compute a risk score (0.0-1.0) based on historical metrics.
    pub fn risk_score(&self) -> f32 {
        let mut score: f32 = 0.0;

        // High churn = higher risk
        if self.commit_count > 20 {
            score += 0.25;
        } else if self.commit_count > 10 {
            score += 0.15;
        } else if self.commit_count > 5 {
            score += 0.05;
        }

        // Bug fix frequency
        if self.commit_count > 0 {
            let bug_ratio = self.bug_fix_count as f32 / self.commit_count as f32;
            score += bug_ratio * 0.3;
        }

        // Many authors = coordination risk
        if self.distinct_authors > 5 {
            score += 0.15;
        } else if self.distinct_authors > 3 {
            score += 0.05;
        }

        // High total churn
        let total_churn = self.lines_added_total + self.lines_removed_total;
        if total_churn > 1000 {
            score += 0.15;
        } else if total_churn > 500 {
            score += 0.1;
        }

        score.clamp(0.0, 1.0)
    }

    pub fn is_high_churn(&self) -> bool {
        self.commit_count > 10
    }

    pub fn is_bug_prone(&self) -> bool {
        self.commit_count >= 5 && self.bug_fix_count as f32 / self.commit_count as f32 > 0.3
    }
}

/// A parsed git log entry.
#[derive(Debug, Clone)]
pub struct GitLogEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files_changed: Vec<FileChange>,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub file_path: PathBuf,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// Analyzes git history for risk-weighted review context.
#[derive(Debug, Default)]
pub struct GitHistoryAnalyzer {
    entries: Vec<GitLogEntry>,
    file_churn: HashMap<PathBuf, FileChurnInfo>,
}

impl GitHistoryAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest parsed git log entries.
    pub fn ingest_log(&mut self, entries: Vec<GitLogEntry>) {
        for entry in &entries {
            let is_bugfix = is_bug_fix_commit(&entry.message);

            for change in &entry.files_changed {
                let info = self
                    .file_churn
                    .entry(change.file_path.clone())
                    .or_insert_with(|| FileChurnInfo {
                        file_path: change.file_path.clone(),
                        commit_count: 0,
                        bug_fix_count: 0,
                        distinct_authors: 0,
                        last_modified: None,
                        lines_added_total: 0,
                        lines_removed_total: 0,
                        age_days: None,
                    });

                info.commit_count += 1;
                info.lines_added_total += change.lines_added;
                info.lines_removed_total += change.lines_removed;

                if is_bugfix {
                    info.bug_fix_count += 1;
                }

                if info.last_modified.as_ref().is_none_or(|d| {
                    parse_date_for_comparison(&entry.date) > parse_date_for_comparison(d)
                }) {
                    info.last_modified = Some(entry.date.clone());
                }
            }
        }

        self.entries.extend(entries);

        // Recalculate distinct authors per file from ALL entries
        let mut file_authors: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        for entry in &self.entries {
            for change in &entry.files_changed {
                file_authors
                    .entry(change.file_path.clone())
                    .or_default()
                    .insert(entry.author.clone());
            }
        }
        for (path, authors) in file_authors {
            if let Some(info) = self.file_churn.get_mut(&path) {
                info.distinct_authors = authors.len();
            }
        }
    }

    /// Parse `git log --numstat` output into entries.
    pub fn parse_git_log_numstat(output: &str) -> Vec<GitLogEntry> {
        let mut entries = Vec::new();
        let mut current: Option<GitLogEntry> = None;

        for line in output.lines() {
            if line.starts_with("commit ") {
                if let Some(entry) = current.take() {
                    entries.push(entry);
                }
                current = Some(GitLogEntry {
                    hash: line.trim_start_matches("commit ").trim().to_string(),
                    author: String::new(),
                    date: String::new(),
                    message: String::new(),
                    files_changed: Vec::new(),
                });
            } else if line.starts_with("Author: ") {
                if let Some(ref mut entry) = current {
                    entry.author = line
                        .trim_start_matches("Author: ")
                        .split('<')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
            } else if line.starts_with("Date: ") {
                if let Some(ref mut entry) = current {
                    entry.date = line.trim_start_matches("Date: ").trim().to_string();
                }
            } else if line.starts_with("    ") && !line.trim().is_empty() {
                if let Some(ref mut entry) = current {
                    if entry.message.is_empty() {
                        entry.message = line.trim().to_string();
                    }
                }
            } else if !line.trim().is_empty() {
                // numstat line: added\tremoved\tfile
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    let added = parts[0].parse::<usize>().unwrap_or(0);
                    let removed = parts[1].parse::<usize>().unwrap_or(0);
                    let file_path = PathBuf::from(parts[2]);
                    if let Some(ref mut entry) = current {
                        entry.files_changed.push(FileChange {
                            file_path,
                            lines_added: added,
                            lines_removed: removed,
                        });
                    }
                }
            }
        }

        if let Some(entry) = current {
            entries.push(entry);
        }

        entries
    }

    /// Get churn info for a specific file.
    pub fn file_info(&self, path: &Path) -> Option<&FileChurnInfo> {
        self.file_churn.get(path)
    }

    /// Get files ranked by risk score.
    pub fn ranked_by_risk(&self, max_results: usize) -> Vec<&FileChurnInfo> {
        let mut files: Vec<&FileChurnInfo> = self.file_churn.values().collect();
        files.sort_by(|a, b| {
            b.risk_score()
                .partial_cmp(&a.risk_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        files.truncate(max_results);
        files
    }

    /// Get files that are bug-prone.
    pub fn bug_prone_files(&self) -> Vec<&FileChurnInfo> {
        self.file_churn
            .values()
            .filter(|info| info.is_bug_prone())
            .collect()
    }

    /// Generate review context based on git history for a set of changed files.
    pub fn generate_history_context(&self, changed_files: &[PathBuf]) -> String {
        let mut context = String::new();
        let mut high_risk = Vec::new();
        let mut bug_prone = Vec::new();

        for file in changed_files {
            if let Some(info) = self.file_info(file) {
                if info.risk_score() > 0.3 {
                    high_risk.push(info);
                }
                if info.is_bug_prone() {
                    bug_prone.push(info);
                }
            }
        }

        if !high_risk.is_empty() {
            context.push_str("High-churn files (review carefully):\n");
            for info in &high_risk {
                context.push_str(&format!(
                    "- {} ({} commits, risk={:.2})\n",
                    info.file_path.display(),
                    info.commit_count,
                    info.risk_score()
                ));
            }
        }

        if !bug_prone.is_empty() {
            context.push_str("\nBug-prone files (extra scrutiny):\n");
            for info in &bug_prone {
                context.push_str(&format!(
                    "- {} ({}/{} commits were bug fixes)\n",
                    info.file_path.display(),
                    info.bug_fix_count,
                    info.commit_count
                ));
            }
        }

        context
    }

    pub fn file_count(&self) -> usize {
        self.file_churn.len()
    }

    pub fn total_entries(&self) -> usize {
        self.entries.len()
    }
}

/// Parse a date string into a comparable tuple (year, month, day, time).
/// Handles both git default format ("Wed Jan 1 12:00:00 2025 +0000")
/// and ISO format ("2025-01-01 12:00:00 +0000").
fn parse_date_for_comparison(date: &str) -> (i32, u32, u32, String) {
    let parts: Vec<&str> = date.split_whitespace().collect();

    // ISO format: "2025-01-01 ..."
    if parts
        .first()
        .is_some_and(|p| p.contains('-') && p.len() == 10)
    {
        // ISO dates sort correctly as strings, but parse for consistency
        let date_parts: Vec<&str> = parts[0].split('-').collect();
        if date_parts.len() == 3 {
            let year = date_parts[0].parse::<i32>().unwrap_or(0);
            let month = date_parts[1].parse::<u32>().unwrap_or(0);
            let day = date_parts[2].parse::<u32>().unwrap_or(0);
            let time = parts.get(1).unwrap_or(&"").to_string();
            return (year, month, day, time);
        }
    }

    // Git default format: "Wed Jan 1 12:00:00 2025 +0000"
    if parts.len() >= 5 {
        let month = match parts[1] {
            "Jan" => 1,
            "Feb" => 2,
            "Mar" => 3,
            "Apr" => 4,
            "May" => 5,
            "Jun" => 6,
            "Jul" => 7,
            "Aug" => 8,
            "Sep" => 9,
            "Oct" => 10,
            "Nov" => 11,
            "Dec" => 12,
            _ => 0,
        };
        let day = parts[2].parse::<u32>().unwrap_or(0);
        let year = parts[4].parse::<i32>().unwrap_or(0);
        let time = parts[3].to_string();
        return (year, month, day, time);
    }

    // Fallback: use the raw string (best effort)
    (0, 0, 0, date.to_string())
}

/// Detect if a commit message indicates a bug fix.
fn is_bug_fix_commit(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.starts_with("fix")
        || lower.starts_with("bug")
        || lower.starts_with("hotfix")
        || lower.contains("fix #")
        || lower.contains("fixes #")
        || lower.contains("fixed #")
        || lower.contains("closes #")
        || lower.contains("resolves #")
        || lower.contains("bugfix")
        || lower.contains("patch:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        hash: &str,
        author: &str,
        msg: &str,
        files: Vec<(&str, usize, usize)>,
    ) -> GitLogEntry {
        GitLogEntry {
            hash: hash.to_string(),
            author: author.to_string(),
            date: "2024-01-01".to_string(),
            message: msg.to_string(),
            files_changed: files
                .into_iter()
                .map(|(path, added, removed)| FileChange {
                    file_path: PathBuf::from(path),
                    lines_added: added,
                    lines_removed: removed,
                })
                .collect(),
        }
    }

    #[test]
    fn test_ingest_and_query() {
        let mut analyzer = GitHistoryAnalyzer::new();
        analyzer.ingest_log(vec![
            make_entry("abc", "alice", "Add feature", vec![("src/lib.rs", 50, 10)]),
            make_entry("def", "bob", "Fix bug #123", vec![("src/lib.rs", 5, 3)]),
            make_entry("ghi", "carol", "Refactor", vec![("src/lib.rs", 20, 15)]),
        ]);

        let info = analyzer.file_info(Path::new("src/lib.rs")).unwrap();
        assert_eq!(info.commit_count, 3);
        assert_eq!(info.bug_fix_count, 1);
        assert_eq!(info.distinct_authors, 3);
        assert_eq!(info.lines_added_total, 75);
    }

    #[test]
    fn test_risk_score_high_churn() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("hot.rs"),
            commit_count: 25,
            bug_fix_count: 10,
            distinct_authors: 6,
            last_modified: None,
            lines_added_total: 2000,
            lines_removed_total: 500,
            age_days: None,
        };
        let score = info.risk_score();
        assert!(score > 0.5, "Expected high risk, got {}", score);
    }

    #[test]
    fn test_risk_score_low_churn() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("stable.rs"),
            commit_count: 2,
            bug_fix_count: 0,
            distinct_authors: 1,
            last_modified: None,
            lines_added_total: 50,
            lines_removed_total: 0,
            age_days: None,
        };
        let score = info.risk_score();
        assert!(score < 0.3, "Expected low risk, got {}", score);
    }

    #[test]
    fn test_is_bug_prone() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("buggy.rs"),
            commit_count: 10,
            bug_fix_count: 5,
            distinct_authors: 2,
            last_modified: None,
            lines_added_total: 100,
            lines_removed_total: 50,
            age_days: None,
        };
        assert!(info.is_bug_prone());
    }

    #[test]
    fn test_not_bug_prone() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("clean.rs"),
            commit_count: 10,
            bug_fix_count: 1,
            distinct_authors: 2,
            last_modified: None,
            lines_added_total: 100,
            lines_removed_total: 50,
            age_days: None,
        };
        assert!(!info.is_bug_prone());
    }

    #[test]
    fn test_ranked_by_risk() {
        let mut analyzer = GitHistoryAnalyzer::new();
        analyzer.ingest_log(vec![make_entry(
            "a",
            "alice",
            "Fix critical bug",
            vec![("src/hot.rs", 100, 50)],
        )]);
        // Add more commits to hot.rs
        for i in 0..20 {
            analyzer.ingest_log(vec![make_entry(
                &format!("commit_{}", i),
                "alice",
                "Fix another bug",
                vec![("src/hot.rs", 10, 5)],
            )]);
        }
        analyzer.ingest_log(vec![make_entry(
            "z",
            "bob",
            "Add feature",
            vec![("src/cold.rs", 10, 0)],
        )]);

        let ranked = analyzer.ranked_by_risk(10);
        assert!(ranked.len() >= 2);
        assert_eq!(ranked[0].file_path, PathBuf::from("src/hot.rs"));
    }

    #[test]
    fn test_bug_prone_files() {
        let mut analyzer = GitHistoryAnalyzer::new();
        for i in 0..10 {
            let msg = if i % 2 == 0 { "Fix bug" } else { "Add feature" };
            analyzer.ingest_log(vec![make_entry(
                &format!("c{}", i),
                "alice",
                msg,
                vec![("buggy.rs", 5, 3)],
            )]);
        }

        let bug_prone = analyzer.bug_prone_files();
        assert_eq!(bug_prone.len(), 1);
        assert_eq!(bug_prone[0].file_path, PathBuf::from("buggy.rs"));
    }

    #[test]
    fn test_generate_history_context() {
        let mut analyzer = GitHistoryAnalyzer::new();
        for i in 0..15 {
            analyzer.ingest_log(vec![make_entry(
                &format!("c{}", i),
                "alice",
                "Fix issue",
                vec![("src/risky.rs", 20, 10)],
            )]);
        }

        let context = analyzer.generate_history_context(&[PathBuf::from("src/risky.rs")]);
        assert!(context.contains("High-churn"));
        assert!(context.contains("Bug-prone"));
    }

    #[test]
    fn test_parse_git_log_numstat() {
        let output = "\
commit abc123
Author: Alice Smith <alice@example.com>
Date:   Mon Jan 1 12:00:00 2024 +0000

    Fix critical bug in auth

5\t3\tsrc/auth.rs
10\t0\tsrc/handler.rs

commit def456
Author: Bob Jones <bob@example.com>
Date:   Tue Jan 2 12:00:00 2024 +0000

    Add new feature

20\t5\tsrc/feature.rs
";
        let entries = GitHistoryAnalyzer::parse_git_log_numstat(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hash, "abc123");
        assert_eq!(entries[0].author, "Alice Smith");
        assert_eq!(entries[0].message, "Fix critical bug in auth");
        assert_eq!(entries[0].files_changed.len(), 2);
        assert_eq!(entries[0].files_changed[0].lines_added, 5);
        assert_eq!(entries[0].files_changed[0].lines_removed, 3);

        assert_eq!(entries[1].hash, "def456");
        assert_eq!(entries[1].files_changed.len(), 1);
    }

    #[test]
    fn test_is_bug_fix_commit() {
        assert!(is_bug_fix_commit("Fix null pointer in auth"));
        assert!(is_bug_fix_commit("fix: handle edge case"));
        assert!(is_bug_fix_commit("Fixes #123"));
        assert!(is_bug_fix_commit("bugfix: resolve crash"));
        assert!(is_bug_fix_commit("hotfix for production"));
        assert!(is_bug_fix_commit("Closes #456"));

        assert!(!is_bug_fix_commit("Add new feature"));
        assert!(!is_bug_fix_commit("Refactor auth module"));
        assert!(!is_bug_fix_commit("Update dependencies"));
    }

    #[test]
    fn test_empty_analyzer() {
        let analyzer = GitHistoryAnalyzer::new();
        assert_eq!(analyzer.file_count(), 0);
        assert_eq!(analyzer.total_entries(), 0);
        assert!(analyzer.ranked_by_risk(10).is_empty());
        assert!(analyzer.bug_prone_files().is_empty());
    }

    #[test]
    fn test_context_empty_for_unknown_files() {
        let analyzer = GitHistoryAnalyzer::new();
        let context = analyzer.generate_history_context(&[PathBuf::from("unknown.rs")]);
        assert!(context.is_empty());
    }

    #[test]
    fn test_is_high_churn() {
        let high = FileChurnInfo {
            file_path: PathBuf::from("hot.rs"),
            commit_count: 15,
            bug_fix_count: 0,
            distinct_authors: 1,
            last_modified: None,
            lines_added_total: 100,
            lines_removed_total: 50,
            age_days: None,
        };
        assert!(high.is_high_churn());

        let low = FileChurnInfo {
            file_path: PathBuf::from("cold.rs"),
            commit_count: 3,
            bug_fix_count: 0,
            distinct_authors: 1,
            last_modified: None,
            lines_added_total: 20,
            lines_removed_total: 5,
            age_days: None,
        };
        assert!(!low.is_high_churn());
    }

    #[test]
    fn test_risk_score_clamped() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("extreme.rs"),
            commit_count: 100,
            bug_fix_count: 100,
            distinct_authors: 20,
            last_modified: None,
            lines_added_total: 10000,
            lines_removed_total: 5000,
            age_days: None,
        };
        let score = info.risk_score();
        assert!(score <= 1.0);
        assert!(score >= 0.0);
    }

    #[test]
    fn test_risk_score_zero_commits() {
        let info = FileChurnInfo {
            file_path: PathBuf::from("new.rs"),
            commit_count: 0,
            bug_fix_count: 0,
            distinct_authors: 0,
            last_modified: None,
            lines_added_total: 0,
            lines_removed_total: 0,
            age_days: None,
        };
        // Zero commits => no risk factors, score should be 0
        assert!(info.risk_score() < 0.01);
    }

    #[test]
    fn test_empty_log_parsing() {
        let entries = GitHistoryAnalyzer::parse_git_log_numstat("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_malformed_log_lines() {
        let entries = GitHistoryAnalyzer::parse_git_log_numstat("garbage data\nnot a real log\n\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_bug_fix_detection() {
        assert!(is_bug_fix_commit("Fix null pointer crash"));
        assert!(is_bug_fix_commit("bugfix: handle edge case"));
        assert!(is_bug_fix_commit("Hotfix for production issue"));
        assert!(is_bug_fix_commit("closes #123"));
        assert!(!is_bug_fix_commit("Add new feature"));
        assert!(!is_bug_fix_commit("Refactor module structure"));
    }

    // Regression: distinct_authors must accumulate across multiple ingest_log calls
    #[test]
    fn test_distinct_authors_across_ingests() {
        let mut analyzer = GitHistoryAnalyzer::new();

        analyzer.ingest_log(vec![make_entry(
            "abc1",
            "alice",
            "First commit",
            vec![("src/lib.rs", 10, 5)],
        )]);
        analyzer.ingest_log(vec![make_entry(
            "abc2",
            "bob",
            "Second commit",
            vec![("src/lib.rs", 3, 1)],
        )]);

        let info = analyzer.file_info(Path::new("src/lib.rs")).unwrap();
        assert_eq!(
            info.distinct_authors, 2,
            "Should count authors from both ingest calls, got {}",
            info.distinct_authors
        );
    }

    // Regression: last_modified must use chronological comparison, not lexicographic
    #[test]
    fn test_last_modified_chronological_order() {
        let mut analyzer = GitHistoryAnalyzer::new();
        analyzer.ingest_log(vec![
            GitLogEntry {
                hash: "newer".to_string(),
                author: "alice".to_string(),
                date: "Mon Feb 3 12:00:00 2025 +0000".to_string(), // NEWER
                message: "newer commit".to_string(),
                files_changed: vec![FileChange {
                    file_path: PathBuf::from("f.rs"),
                    lines_added: 1,
                    lines_removed: 0,
                }],
            },
            GitLogEntry {
                hash: "older".to_string(),
                author: "bob".to_string(),
                date: "Wed Jan 1 12:00:00 2025 +0000".to_string(), // OLDER
                message: "older commit".to_string(),
                files_changed: vec![FileChange {
                    file_path: PathBuf::from("f.rs"),
                    lines_added: 1,
                    lines_removed: 0,
                }],
            },
        ]);

        let info = analyzer.file_info(Path::new("f.rs")).unwrap();
        // Feb 3 is newer than Jan 1, so last_modified should contain "Feb"
        // Bug: string comparison "Wed Jan..." > "Mon Feb..." because 'W' > 'M'
        assert!(
            info.last_modified.as_ref().unwrap().contains("Feb"),
            "last_modified should be Feb 3 (newer), got: {}",
            info.last_modified.as_ref().unwrap()
        );
    }

    // Regression: distinct_authors must be cumulative like commit_count
    #[test]
    fn test_cumulative_commit_count() {
        let mut analyzer = GitHistoryAnalyzer::new();
        analyzer.ingest_log(vec![make_entry("a", "alice", "m1", vec![("f.rs", 1, 0)])]);
        analyzer.ingest_log(vec![make_entry("b", "alice", "m2", vec![("f.rs", 2, 0)])]);

        let info = analyzer.file_info(Path::new("f.rs")).unwrap();
        assert_eq!(info.commit_count, 2, "commit_count should be cumulative");
        assert_eq!(info.lines_added_total, 3, "lines_added should accumulate");
    }
}
