use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::core;

pub(super) fn apply_context_source_tags(
    mut comments: Vec<core::Comment>,
    verification_context: &HashMap<PathBuf, Vec<core::LLMContextChunk>>,
) -> Vec<core::Comment> {
    let tags_by_file = verification_context
        .iter()
        .filter_map(|(file_path, chunks)| {
            let tags = artifact_tags_for_chunks(chunks);
            (!tags.is_empty()).then(|| (file_path.clone(), tags))
        })
        .collect::<HashMap<_, _>>();

    for comment in &mut comments {
        let Some(tags) = tags_by_file.get(&comment.file_path) else {
            continue;
        };

        for tag in tags {
            push_unique_tag(&mut comment.tags, tag);
        }
    }

    comments
}

fn artifact_tags_for_chunks(chunks: &[core::LLMContextChunk]) -> Vec<String> {
    chunks
        .iter()
        .filter_map(|chunk| chunk.provenance.as_ref())
        .filter_map(core::ContextProvenance::artifact_tag)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn push_unique_tag(tags: &mut Vec<String>, tag: &str) {
    if !tags.iter().any(|existing| existing == tag) {
        tags.push(tag.to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::core::comment::{Category, CommentStatus, FixEffort, Severity};

    fn make_comment(file_path: &str) -> core::Comment {
        core::Comment {
            id: "comment-1".to_string(),
            file_path: PathBuf::from(file_path),
            line_number: 7,
            content: "Missing validation.".to_string(),
            rule_id: None,
            severity: Severity::Warning,
            category: Category::Bug,
            suggestion: None,
            confidence: 0.8,
            code_suggestion: None,
            tags: vec!["security".to_string()],
            fix_effort: FixEffort::Medium,
            feedback: None,
            status: CommentStatus::Open,
            resolved_at: None,
        }
    }

    #[test]
    fn applies_unique_context_source_tags_for_matching_file() {
        let mut verification_context = HashMap::new();
        verification_context.insert(
            PathBuf::from("src/api.rs"),
            vec![
                core::LLMContextChunk::documentation(
                    PathBuf::from("src/api.rs"),
                    "Custom context notes".to_string(),
                )
                .with_provenance(core::ContextProvenance::CustomContextNotes),
                core::LLMContextChunk::reference(
                    PathBuf::from("patterns/sql.md"),
                    "Pattern repository rule".to_string(),
                )
                .with_provenance(
                    core::ContextProvenance::pattern_repository_context("patterns/security-rules"),
                ),
                core::LLMContextChunk::reference(
                    PathBuf::from("patterns/sql.md"),
                    "Pattern repository rule".to_string(),
                )
                .with_provenance(
                    core::ContextProvenance::pattern_repository_source("patterns/security-rules"),
                ),
                core::LLMContextChunk::documentation(
                    PathBuf::from("src/api.rs"),
                    "Analyzer guidance".to_string(),
                )
                .with_provenance(core::ContextProvenance::analyzer("contracts")),
            ],
        );

        let comments = apply_context_source_tags(
            vec![make_comment("src/api.rs"), make_comment("src/other.rs")],
            &verification_context,
        );

        assert_eq!(
            comments[0].tags,
            vec![
                "security".to_string(),
                "context-source:custom-context".to_string(),
                "context-source:pattern-repository:patterns/security-rules".to_string(),
            ]
        );
        assert_eq!(comments[1].tags, vec!["security".to_string()]);
    }
}
