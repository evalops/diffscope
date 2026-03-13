use crate::core;

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
        let dominated = deduped.iter_mut().find(|existing| {
            existing.file_path == comment.file_path
                && existing.line_number == comment.line_number
                && core::multi_pass::content_similarity(&existing.content, &comment.content) > 0.6
        });
        if let Some(existing) = dominated {
            if comment.confidence > existing.confidence {
                existing.content = comment.content;
                existing.confidence = comment.confidence;
                existing.severity = comment.severity;
            }
            for tag in &comment.tags {
                if !existing.tags.contains(tag) {
                    existing.tags.push(tag.clone());
                }
            }
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
            id: format!("cmt_{}", line),
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
