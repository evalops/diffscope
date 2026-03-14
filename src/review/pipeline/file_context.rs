use anyhow::Result;
use std::path::Path;

#[path = "file_context/base.rs"]
mod base;
#[path = "file_context/finalize.rs"]
mod finalize;
#[path = "file_context/sources.rs"]
mod sources;

use crate::config;
use crate::core;

use super::context::extract_symbols_from_diff;
use super::services::PipelineServices;
use super::session::ReviewSession;

pub(super) struct PreparedFileContext {
    pub active_rules: Vec<core::ReviewRule>,
    pub path_config: Option<config::PathConfig>,
    pub deterministic_comments: Vec<core::Comment>,
    pub context_chunks: Vec<core::LLMContextChunk>,
    pub graph_query_traces: Vec<core::dag::DagExecutionTrace>,
}

pub(super) async fn assemble_file_context(
    services: &PipelineServices,
    session: &ReviewSession,
    diff: &core::UnifiedDiff,
    pre_analysis_context: Vec<core::LLMContextChunk>,
    deterministic_comments: Vec<core::Comment>,
) -> Result<PreparedFileContext> {
    let path_config = services.config.get_path_config(&diff.file_path).cloned();
    let mut context_chunks =
        base::initial_context_chunks(services, diff, pre_analysis_context).await?;
    let diff_symbols = extract_symbols_from_diff(diff);
    let mut graph_query_records = Vec::new();

    add_graph_metadata_context(
        &services.repo_path,
        &diff.file_path,
        session.symbol_index.as_ref(),
        &mut context_chunks,
        &mut graph_query_records,
    );

    sources::add_symbol_context(
        services,
        session,
        diff,
        &diff_symbols,
        &mut context_chunks,
        &mut graph_query_records,
    )
    .await?;
    sources::add_related_file_context(
        services,
        session,
        diff,
        &mut context_chunks,
        &mut graph_query_records,
    );
    sources::add_semantic_context(
        services,
        session,
        diff,
        &diff_symbols,
        &mut context_chunks,
        &mut graph_query_records,
    )
    .await;
    sources::add_path_context(services, diff, path_config.as_ref(), &mut context_chunks).await?;
    sources::inject_repository_context(services, diff, &mut context_chunks).await?;

    if !diff_symbols.is_empty() && !graph_query_records.is_empty() {
        graph_query_records.insert(
            0,
            sources::trace_record(
                format!("seed_symbols={}", summarize_seed_symbols(&diff_symbols)),
                0,
            ),
        );
    }

    let graph_query_traces = sources::build_graph_query_trace(&diff.file_path, graph_query_records)
        .into_iter()
        .collect();

    Ok(finalize::finalize_file_context(
        services,
        diff,
        path_config,
        deterministic_comments,
        context_chunks,
        graph_query_traces,
    ))
}

fn summarize_seed_symbols(symbols: &[String]) -> String {
    let shown = symbols.iter().take(6).cloned().collect::<Vec<_>>();
    let mut summary = shown.join(", ");
    if symbols.len() > shown.len() {
        summary.push_str(&format!(" (+{} more)", symbols.len() - shown.len()));
    }
    summary
}

fn add_graph_metadata_context(
    repo_path: &Path,
    diff_file_path: &Path,
    index: Option<&core::SymbolIndex>,
    context_chunks: &mut Vec<core::LLMContextChunk>,
    graph_query_records: &mut Vec<core::dag::DagExecutionRecord>,
) {
    let Some(index) = index else {
        return;
    };

    context_chunks.push(
        core::LLMContextChunk::documentation(
            diff_file_path.to_path_buf(),
            index.graph_metadata_summary(repo_path),
        )
        .with_provenance(core::ContextProvenance::RepositoryGraphMetadata),
    );

    graph_query_records.extend(
        index
            .graph_trace_details(repo_path)
            .into_iter()
            .map(|detail| sources::trace_record(detail, 0)),
    );
}

#[cfg(test)]
mod tests {
    use super::add_graph_metadata_context;
    use crate::core;
    use git2::{Repository, Signature};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn commit_repo_file(repo_root: &Path, relative: &str, content: &str, message: &str) {
        let repo = Repository::open(repo_root).unwrap();
        let path = repo_root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new(relative)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = Signature::now("Test User", "test@example.com").unwrap();

        let parent_commit = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .and_then(|oid| repo.find_commit(oid).ok());

        if let Some(parent) = parent_commit.as_ref() {
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[parent],
            )
            .unwrap();
        } else {
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .unwrap();
        }
    }

    #[test]
    fn add_graph_metadata_context_injects_metadata_chunk_and_trace_records() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        commit_repo_file(
            dir.path(),
            "src/lib.rs",
            "pub fn helper() {}\n",
            "initial graph",
        );
        let index = core::SymbolIndex::build(dir.path(), 16, 128 * 1024, 8, |_path| false).unwrap();

        let mut context_chunks = Vec::new();
        let mut graph_query_records = Vec::new();
        add_graph_metadata_context(
            dir.path(),
            Path::new("src/lib.rs"),
            Some(&index),
            &mut context_chunks,
            &mut graph_query_records,
        );

        assert_eq!(context_chunks.len(), 1);
        assert_eq!(
            context_chunks[0].provenance,
            Some(core::ContextProvenance::RepositoryGraphMetadata)
        );
        assert!(context_chunks[0]
            .content
            .contains("Repository graph metadata:"));
        assert!(context_chunks[0].content.contains("freshness: fresh"));
        assert!(graph_query_records
            .iter()
            .any(|record| record.name == "graph_freshness=fresh"));
        assert!(graph_query_records
            .iter()
            .any(|record| record.name.starts_with("graph_version=")));
        assert!(graph_query_records
            .iter()
            .all(|record| record.duration_ms == 0));
        assert_eq!(context_chunks[0].file_path, PathBuf::from("src/lib.rs"));
    }
}
