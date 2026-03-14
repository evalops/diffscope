use std::path::PathBuf;

use crate::core;

const MIN_BLAST_RADIUS_FILES: usize = 3;
const MAX_LISTED_DEPENDENTS: usize = 3;
const BLAST_RADIUS_TAG: &str = "blast-radius";
const BLAST_RADIUS_PREFIX: &str = "Blast radius:";

pub(super) fn apply_blast_radius_summaries(
    mut comments: Vec<core::Comment>,
    symbol_index: Option<&core::SymbolIndex>,
) -> Vec<core::Comment> {
    let Some(symbol_index) = symbol_index else {
        return comments;
    };

    for comment in &mut comments {
        let mut dependents = symbol_index.reverse_deps(&comment.file_path);
        dependents.sort();
        dependents.dedup();

        if dependents.len() < MIN_BLAST_RADIUS_FILES {
            continue;
        }

        if !comment.content.contains(BLAST_RADIUS_PREFIX) {
            comment.content.push_str("\n\n");
            comment
                .content
                .push_str(&format_blast_radius_summary(&dependents));
        }

        push_unique_tag(&mut comment.tags, BLAST_RADIUS_TAG);
        push_unique_tag(
            &mut comment.tags,
            &format!("blast-radius:{}", dependents.len()),
        );
    }

    comments
}

fn format_blast_radius_summary(dependents: &[PathBuf]) -> String {
    let listed = dependents
        .iter()
        .take(MAX_LISTED_DEPENDENTS)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let remaining = dependents.len().saturating_sub(listed.len());

    let mut summary = format!(
        "{BLAST_RADIUS_PREFIX} {} dependent files reference this file ({})",
        dependents.len(),
        listed.join(", ")
    );
    if remaining > 0 {
        summary.push_str(&format!(", +{remaining} more"));
    }
    summary.push('.');
    summary
}

fn push_unique_tag(tags: &mut Vec<String>, tag: &str) {
    if !tags.iter().any(|existing| existing == tag) {
        tags.push(tag.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::apply_blast_radius_summaries;
    use crate::core;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn build_index(repo_root: &Path) -> core::SymbolIndex {
        core::SymbolIndex::build(repo_root, 32, 128 * 1024, 8, |_path| false).unwrap()
    }

    fn write_repo_file(repo_root: &Path, relative: &str, content: &str) {
        let path = repo_root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn make_comment(file_path: &str) -> core::Comment {
        core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from(file_path),
            line_number: 1,
            content: "Shared helper has a correctness bug.".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.9,
            code_suggestion: None,
            tags: Vec::new(),
            fix_effort: FixEffort::Medium,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn applies_blast_radius_summary_for_many_dependents() {
        let dir = tempfile::tempdir().unwrap();
        write_repo_file(dir.path(), "src/shared.rs", "pub fn helper() {}\n");
        write_repo_file(
            dir.path(),
            "src/a.rs",
            "const SHARED: &str = include_str!(\"./shared.rs\");\n",
        );
        write_repo_file(
            dir.path(),
            "src/b.rs",
            "const SHARED: &str = include_str!(\"./shared.rs\");\n",
        );
        write_repo_file(
            dir.path(),
            "src/c.rs",
            "const SHARED: &str = include_str!(\"./shared.rs\");\n",
        );

        let index = build_index(dir.path());
        let comments =
            apply_blast_radius_summaries(vec![make_comment("src/shared.rs")], Some(&index));

        assert_eq!(comments.len(), 1);
        assert!(comments[0]
            .content
            .contains("Blast radius: 3 dependent files reference this file"));
        assert!(comments[0].content.contains("src/a.rs"));
        assert!(comments[0].tags.iter().any(|tag| tag == "blast-radius"));
        assert!(comments[0].tags.iter().any(|tag| tag == "blast-radius:3"));
    }

    #[test]
    fn skips_blast_radius_summary_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        write_repo_file(dir.path(), "src/shared.rs", "pub fn helper() {}\n");
        write_repo_file(
            dir.path(),
            "src/a.rs",
            "const SHARED: &str = include_str!(\"./shared.rs\");\n",
        );
        write_repo_file(
            dir.path(),
            "src/b.rs",
            "const SHARED: &str = include_str!(\"./shared.rs\");\n",
        );

        let index = build_index(dir.path());
        let comments =
            apply_blast_radius_summaries(vec![make_comment("src/shared.rs")], Some(&index));

        assert_eq!(comments[0].content, "Shared helper has a correctness bug.");
        assert!(comments[0].tags.is_empty());
    }
}
