#[path = "ranking/dedupe.rs"]
mod dedupe;
#[path = "ranking/run.rs"]
mod run;
#[path = "ranking/scoring.rs"]
mod scoring;
#[path = "ranking/selection.rs"]
mod selection;

pub use run::rank_and_trim_context_chunks;

#[cfg(test)]
mod tests {
    use super::scoring::ranges_overlap;
    use super::*;
    use crate::core;
    use crate::core::diff_parser::DiffHunk;
    use std::path::PathBuf;

    #[test]
    fn ranges_overlap_true() {
        assert!(ranges_overlap((1, 10), (5, 15)));
        assert!(ranges_overlap((5, 15), (1, 10)));
        assert!(ranges_overlap((1, 10), (1, 10)));
        assert!(ranges_overlap((1, 10), (10, 20)));
    }

    #[test]
    fn ranges_overlap_false() {
        assert!(!ranges_overlap((1, 5), (6, 10)));
        assert!(!ranges_overlap((10, 20), (1, 5)));
    }

    #[test]
    fn rank_and_trim_empty_chunks() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let result = rank_and_trim_context_chunks(&diff, vec![], 10, 10000);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_and_trim_deduplicates() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunk = core::LLMContextChunk {
            content: "duplicate content".to_string(),
            context_type: core::ContextType::Documentation,
            file_path: PathBuf::from("test.rs"),
            line_range: None,
            provenance: None,
        };
        let chunks = vec![chunk.clone(), chunk];
        let result = rank_and_trim_context_chunks(&diff, chunks, 10, 100000);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn rank_and_trim_respects_max_chunks() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks: Vec<core::LLMContextChunk> = (0..5)
            .map(|i| core::LLMContextChunk {
                content: format!("chunk {}", i),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
                provenance: None,
            })
            .collect();
        let result = rank_and_trim_context_chunks(&diff, chunks, 2, 100000);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn rank_and_trim_respects_max_chars() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks: Vec<core::LLMContextChunk> = (0..5)
            .map(|i| core::LLMContextChunk {
                content: format!("chunk {} with some content here", i),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
                provenance: None,
            })
            .collect();
        let result = rank_and_trim_context_chunks(&diff, chunks, 100, 60);
        assert!(result.len() <= 2);
    }

    #[test]
    fn rank_and_trim_prioritizes_same_file() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("target.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks = vec![
            core::LLMContextChunk {
                content: "other file content".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("other.rs"),
                line_range: None,
                provenance: None,
            },
            core::LLMContextChunk {
                content: "target file content".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("target.rs"),
                line_range: None,
                provenance: None,
            },
        ];
        let result = rank_and_trim_context_chunks(&diff, chunks, 1, 100000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, PathBuf::from("target.rs"));
    }

    #[test]
    fn rank_and_trim_rule_chunks_ranked_high() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("test.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks = vec![
            core::LLMContextChunk {
                content: "some reference".to_string(),
                context_type: core::ContextType::Reference,
                file_path: PathBuf::from("other.rs"),
                line_range: None,
                provenance: None,
            },
            core::LLMContextChunk {
                content: "Active review rules. Check these rules.".to_string(),
                context_type: core::ContextType::Documentation,
                file_path: PathBuf::from("test.rs"),
                line_range: None,
                provenance: None,
            },
        ];
        let result = rank_and_trim_context_chunks(&diff, chunks, 1, 100000);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.starts_with("Active review rules."));
    }

    #[test]
    fn rank_and_trim_prioritizes_graph_provenance() {
        let diff = core::UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("target.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![],
        };
        let chunks = vec![
            core::LLMContextChunk {
                content: "plain reference".to_string(),
                context_type: core::ContextType::Reference,
                file_path: PathBuf::from("auth.rs"),
                line_range: Some((10, 20)),
                provenance: None,
            },
            core::LLMContextChunk {
                content: "graph reference".to_string(),
                context_type: core::ContextType::Reference,
                file_path: PathBuf::from("auth.rs"),
                line_range: Some((10, 20)),
                provenance: Some(core::ContextProvenance::symbol_graph_path(
                    vec!["calls".to_string()],
                    1,
                    0.50,
                )),
            },
        ];

        let result = rank_and_trim_context_chunks(&diff, chunks, 1, 100000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "graph reference");
    }

    #[test]
    fn rank_and_trim_deletion_only_hunk_does_not_boost_all_chunks() {
        let diff = core::UnifiedDiff {
            old_content: Some("old content".to_string()),
            new_content: None,
            file_path: PathBuf::from("target.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 10,
                old_lines: 5,
                new_start: 10,
                new_lines: 0,
                context: String::new(),
                changes: vec![],
            }],
        };

        let distant_chunk = core::LLMContextChunk {
            content: "distant content".to_string(),
            context_type: core::ContextType::Reference,
            file_path: PathBuf::from("other.rs"),
            line_range: Some((9000, 9100)),
            provenance: None,
        };

        let nearby_chunk = core::LLMContextChunk {
            content: "nearby content".to_string(),
            context_type: core::ContextType::Reference,
            file_path: PathBuf::from("other.rs"),
            line_range: Some((8, 12)),
            provenance: None,
        };

        let result =
            rank_and_trim_context_chunks(&diff, vec![distant_chunk, nearby_chunk], 2, 100000);

        assert_eq!(result.len(), 2);
    }
}
