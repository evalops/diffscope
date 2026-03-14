#[path = "dedup/matching.rs"]
mod matching;
#[path = "dedup/merge.rs"]
mod merge;

use crate::core;

use matching::find_dominated_comment_index;
use merge::merge_specialized_comment;

pub(super) fn deduplicate_specialized_comments(
    mut comments: Vec<core::Comment>,
) -> Vec<core::Comment> {
    if comments.len() <= 1 {
        return comments;
    }

    comments.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });

    let mut deduped: Vec<core::Comment> = Vec::with_capacity(comments.len());
    for comment in comments {
        if let Some(index) = find_dominated_comment_index(&deduped, &comment) {
            merge_specialized_comment(&mut deduped[index], comment);
        } else {
            deduped.push(comment);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_comment(file: &str, line: usize, content: &str, tag: &str) -> core::Comment {
        core::Comment {
            id: format!("cmt_{line}"),
            file_path: PathBuf::from(file),
            line_number: line,
            content: content.to_string(),
            rule_id: None,
            severity: core::comment::Severity::Warning,
            category: core::comment::Category::BestPractice,
            suggestion: None,
            confidence: 0.7,
            code_suggestion: None,
            tags: vec![tag.to_string()],
            fix_effort: core::comment::FixEffort::Medium,
            feedback: None,
            status: crate::core::comment::CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn dedup_removes_similar_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                10,
                "Missing null check on user input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 1);
        assert!(deduped[0].tags.contains(&"security-pass".to_string()));
    }

    #[test]
    fn dedup_removes_semantic_supply_chain_duplicates() {
        let comments = vec![
            make_comment(
                "Dockerfile",
                4,
                "Piping a remote install script to bash executes unverified code during the build.",
                "security-pass",
            ),
            make_comment(
                "Dockerfile",
                4,
                "This downloads a script and runs it without checksum or signature verification, creating a supply chain risk.",
                "verification-pass",
            ),
        ];

        let deduped = deduplicate_specialized_comments(comments);

        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn dedup_merges_same_rule_comments_on_same_line() {
        let mut first = make_comment(
            ".github/workflows/build.yml",
            9,
            "Action not pinned to full SHA.",
            "supply-chain",
        );
        first.rule_id = Some("sec.supply-chain.ci-injection".to_string());

        let mut second = make_comment(
            ".github/workflows/build.yml",
            9,
            "GitHub Action not pinned to immutable SHA and should use a commit hash.",
            "github-actions",
        );
        second.rule_id = Some("sec.supply-chain.ci-injection".to_string());
        second.suggestion = Some("Pin the action to a full commit SHA.".to_string());

        let deduped = deduplicate_specialized_comments(vec![first, second]);

        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].rule_id.as_deref(),
            Some("sec.supply-chain.ci-injection")
        );
        assert_eq!(
            deduped[0].suggestion.as_deref(),
            Some("Pin the action to a full commit SHA.")
        );
    }

    #[test]
    fn dedup_keeps_different_comments_on_same_line() {
        let comments = vec![
            make_comment("a.rs", 10, "SQL injection vulnerability", "security-pass"),
            make_comment("a.rs", 10, "Off-by-one error in loop", "correctness-pass"),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_keeps_similar_comments_on_different_lines() {
        let comments = vec![
            make_comment("a.rs", 10, "Missing null check on input", "security-pass"),
            make_comment(
                "a.rs",
                20,
                "Missing null check on input",
                "correctness-pass",
            ),
        ];
        let deduped = deduplicate_specialized_comments(comments);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_handles_empty_input() {
        let deduped = deduplicate_specialized_comments(vec![]);
        assert!(deduped.is_empty());
    }
}
