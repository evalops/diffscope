use anyhow::Result;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::adapters::llm::LLMAdapter;
use crate::core::code_summary::{summarize_file_symbols, SummaryCache};
use crate::core::context::{ContextType, LLMContextChunk};
use crate::core::diff_parser::{ChangeType, UnifiedDiff};
use crate::core::function_chunker::chunk_diff_by_functions;

const MAX_CODE_FILE_BYTES: usize = 512 * 1024;
const FALLBACK_EMBEDDING_DIMENSIONS: usize = 128;
const SUPPORTED_CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "pyi", "js", "jsx", "ts", "tsx", "go", "java", "kt", "cs", "rb", "php", "c", "h",
    "cc", "cpp", "cxx", "hpp", "swift", "scala",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticChunk {
    pub key: String,
    pub file_path: PathBuf,
    pub symbol_name: String,
    pub line_range: (usize, usize),
    pub summary: String,
    pub embedding_text: String,
    pub code_excerpt: String,
    pub embedding: Vec<f32>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticIndex {
    pub version: u32,
    pub entries: HashMap<String, SemanticChunk>,
}

#[derive(Debug, Clone)]
pub struct SemanticMatch {
    pub chunk: SemanticChunk,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFeedbackExample {
    pub content: String,
    pub category: String,
    pub file_patterns: Vec<String>,
    pub accepted: bool,
    pub created_at: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFeedbackStore {
    pub version: u32,
    pub examples: Vec<SemanticFeedbackExample>,
}

impl Default for SemanticIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
        }
    }
}

impl Default for SemanticFeedbackStore {
    fn default() -> Self {
        Self {
            version: 1,
            examples: Vec::new(),
        }
    }
}

impl SemanticIndex {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(content: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(content)
    }
}

impl SemanticFeedbackStore {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(content: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(content)
    }

    pub fn add_example(&mut self, example: SemanticFeedbackExample) {
        let fingerprint = feedback_example_fingerprint(
            &example.content,
            &example.category,
            &example.file_patterns,
            example.accepted,
        );
        if self.examples.iter().any(|existing| {
            feedback_example_fingerprint(
                &existing.content,
                &existing.category,
                &existing.file_patterns,
                existing.accepted,
            ) == fingerprint
        }) {
            return;
        }
        self.examples.push(example);
    }
}

pub fn default_index_path(repo_root: &Path) -> PathBuf {
    let repo_key = hash_text(&repo_root.to_string_lossy());
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("diffscope")
        .join("semantic")
        .join(format!("{}.json", &repo_key[..16]))
}

pub fn default_semantic_feedback_path(feedback_path: &Path) -> PathBuf {
    let parent = feedback_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = feedback_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("diffscope.feedback");
    parent.join(format!("{}.semantic.json", stem))
}

pub fn load_semantic_index(path: &Path) -> SemanticIndex {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| SemanticIndex::from_json(&content).ok())
        .unwrap_or_default()
}

pub fn save_semantic_index(path: &Path, index: &SemanticIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, index.to_json()?)?;
    Ok(())
}

pub fn load_semantic_feedback_store(path: &Path) -> SemanticFeedbackStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| SemanticFeedbackStore::from_json(&content).ok())
        .unwrap_or_default()
}

pub fn save_semantic_feedback_store(path: &Path, store: &SemanticFeedbackStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, store.to_json()?)?;
    Ok(())
}

pub async fn embed_texts_with_fallback(
    adapter: Option<&dyn LLMAdapter>,
    texts: &[String],
) -> Vec<Vec<f32>> {
    if texts.is_empty() {
        return Vec::new();
    }

    if let Some(adapter) = adapter {
        if adapter.supports_embeddings() {
            if let Ok(vectors) = adapter.embed(texts).await {
                if vectors.len() == texts.len() && vectors.iter().all(|vector| !vector.is_empty()) {
                    return vectors;
                }
            }
        }
    }

    texts
        .iter()
        .map(|text| local_hash_embedding(text))
        .collect()
}

pub fn discover_source_files<F>(
    repo_root: &Path,
    should_exclude: F,
    max_files: usize,
) -> Vec<PathBuf>
where
    F: Fn(&PathBuf) -> bool,
{
    let walker = WalkBuilder::new(repo_root)
        .hidden(true)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build();

    let mut files = Vec::new();
    let max_files = max_files.max(1);

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map(|value| value.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf());
        if should_exclude(&relative) || !is_code_file(&relative) {
            continue;
        }

        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.len() as usize > MAX_CODE_FILE_BYTES {
                continue;
            }
        }

        files.push(relative);
        if files.len() >= max_files {
            break;
        }
    }

    files
}

pub async fn refresh_semantic_index<F>(
    repo_root: &Path,
    index_path: &Path,
    embedding_adapter: Option<&dyn LLMAdapter>,
    should_exclude: F,
    max_files: usize,
) -> Result<SemanticIndex>
where
    F: Fn(&PathBuf) -> bool,
{
    let mut index = load_semantic_index(index_path);
    if index.version == 0 {
        index.version = 1;
    }

    let mut summary_cache = SummaryCache::new();
    let source_files = discover_source_files(repo_root, should_exclude, max_files);
    let mut live_keys = HashSet::new();

    for relative_path in source_files {
        let full_path = repo_root.join(&relative_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let summaries = summarize_file_symbols(&relative_path, &content, &mut summary_cache);
        let mut stale_embeddings: Vec<(String, String, SemanticChunk)> = Vec::new();

        for summary in summaries {
            let code_excerpt = excerpt_for_range(&content, summary.line_range, 20);
            let chunk = SemanticChunk {
                key: semantic_key(&summary.file_path, &summary.symbol_name, summary.line_range),
                file_path: summary.file_path.clone(),
                symbol_name: summary.symbol_name.clone(),
                line_range: summary.line_range,
                summary: summary.summary.clone(),
                embedding_text: summary.embedding_text.clone(),
                code_excerpt: code_excerpt.clone(),
                embedding: Vec::new(),
                content_hash: hash_text(&format!(
                    "{}:{}:{}:{}:{}",
                    summary.file_path.display(),
                    summary.symbol_name,
                    summary.line_range.0,
                    summary.line_range.1,
                    code_excerpt
                )),
            };

            live_keys.insert(chunk.key.clone());

            match index.entries.get(&chunk.key) {
                Some(existing) if existing.content_hash == chunk.content_hash => {}
                _ => {
                    stale_embeddings.push((chunk.key.clone(), chunk.embedding_text.clone(), chunk))
                }
            }
        }

        if !stale_embeddings.is_empty() {
            let texts = stale_embeddings
                .iter()
                .map(|(_, text, _)| text.clone())
                .collect::<Vec<_>>();
            let embeddings = embed_texts_with_fallback(embedding_adapter, &texts).await;
            for ((key, _, mut chunk), embedding) in stale_embeddings.into_iter().zip(embeddings) {
                chunk.embedding = embedding;
                index.entries.insert(key, chunk);
            }
        }
    }

    index.entries.retain(|key, _| live_keys.contains(key));
    save_semantic_index(index_path, &index)?;
    Ok(index)
}

pub async fn semantic_context_for_diff(
    index: &SemanticIndex,
    diff: &UnifiedDiff,
    file_content: Option<&str>,
    embedding_adapter: Option<&dyn LLMAdapter>,
    limit: usize,
    min_similarity: f32,
) -> Vec<LLMContextChunk> {
    let query_texts = build_query_texts(diff, file_content);
    if query_texts.is_empty() {
        return Vec::new();
    }

    let query_embeddings = embed_texts_with_fallback(embedding_adapter, &query_texts).await;
    let mut matches = Vec::new();
    for query_embedding in &query_embeddings {
        matches.extend(find_related_chunks(
            index,
            query_embedding,
            Some(&diff.file_path),
            limit,
            min_similarity,
        ));
    }

    let mut seen = HashSet::new();
    let mut chunks = Vec::new();
    for semantic_match in matches {
        if !seen.insert(semantic_match.chunk.key.clone()) {
            continue;
        }
        let content = format!(
            "Semantic match (similarity {:.2})\nSymbol: {}\nSummary: {}\nCode:\n{}",
            semantic_match.similarity,
            semantic_match.chunk.symbol_name,
            semantic_match.chunk.summary,
            semantic_match.chunk.code_excerpt,
        );
        chunks.push(LLMContextChunk {
            file_path: semantic_match.chunk.file_path.clone(),
            content,
            context_type: ContextType::Reference,
            line_range: Some(semantic_match.chunk.line_range),
        });
        if chunks.len() >= limit {
            break;
        }
    }

    chunks
}

pub fn find_related_chunks(
    index: &SemanticIndex,
    query_embedding: &[f32],
    exclude_file: Option<&Path>,
    limit: usize,
    min_similarity: f32,
) -> Vec<SemanticMatch> {
    let mut matches = index
        .entries
        .values()
        .filter(|chunk| {
            exclude_file
                .map(|path| chunk.file_path.as_path() != path)
                .unwrap_or(true)
                && !chunk.embedding.is_empty()
        })
        .filter_map(|chunk| {
            let similarity = cosine_similarity(query_embedding, &chunk.embedding);
            (similarity >= min_similarity).then(|| SemanticMatch {
                chunk: chunk.clone(),
                similarity,
            })
        })
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    matches.truncate(limit.max(1));
    matches
}

pub fn find_similar_feedback_examples(
    store: &SemanticFeedbackStore,
    query_embedding: &[f32],
    category: &str,
    file_patterns: &[String],
    similarity_cutoff: f32,
    max_neighbors: usize,
) -> Vec<(SemanticFeedbackExample, f32)> {
    let mut matches = store
        .examples
        .iter()
        .filter(|example| example.category == category)
        .filter(|example| {
            example.file_patterns.is_empty()
                || file_patterns.is_empty()
                || example
                    .file_patterns
                    .iter()
                    .any(|pattern| file_patterns.iter().any(|candidate| candidate == pattern))
        })
        .filter_map(|example| {
            let similarity = cosine_similarity(query_embedding, &example.embedding);
            (similarity >= similarity_cutoff).then(|| (example.clone(), similarity))
        })
        .collect::<Vec<_>>();

    matches.sort_by(|a, b| b.1.total_cmp(&a.1));
    matches.truncate(max_neighbors);
    matches
}

pub fn build_feedback_embedding_text(content: &str, category: &str) -> String {
    format!("Category: {}\nComment: {}", category, content)
}

pub fn local_hash_embedding(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0; FALLBACK_EMBEDDING_DIMENSIONS];
    let mut seen = 0usize;

    for token in tokenize(text) {
        let hash = Sha256::digest(token.as_bytes());
        let idx = ((hash[0] as usize) << 8 | hash[1] as usize) % FALLBACK_EMBEDDING_DIMENSIONS;
        let weight = 1.0 + (hash[2] as f32 / 255.0);
        if hash[3] % 2 == 0 {
            vector[idx] += weight;
        } else {
            vector[idx] -= weight;
        }
        seen += 1;
    }

    if seen == 0 {
        return vector;
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }

    vector
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let len = left.len().min(right.len());
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for idx in 0..len {
        dot += left[idx] * right[idx];
        left_norm += left[idx] * left[idx];
        right_norm += right[idx] * right[idx];
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0)
}

fn build_query_texts(diff: &UnifiedDiff, file_content: Option<&str>) -> Vec<String> {
    let chunks = chunk_diff_by_functions(diff, file_content);
    let mut queries = Vec::new();

    for chunk in chunks {
        let changed_code = chunk
            .changes
            .iter()
            .filter(|change| matches!(change.change_type, ChangeType::Added | ChangeType::Removed))
            .map(|change| change.content.as_str())
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        if changed_code.trim().is_empty() {
            continue;
        }
        queries.push(format!(
            "File: {}\nFunction: {}\nLanguage: {}\nChanged code:\n{}",
            diff.file_path.display(),
            chunk.function_name,
            chunk.language,
            changed_code,
        ));
    }

    if queries.is_empty() {
        let fallback = diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.changes.iter())
            .filter(|change| matches!(change.change_type, ChangeType::Added | ChangeType::Removed))
            .map(|change| change.content.as_str())
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        if !fallback.trim().is_empty() {
            queries.push(format!(
                "File: {}\nChanged code:\n{}",
                diff.file_path.display(),
                fallback,
            ));
        }
    }

    queries
}

fn excerpt_for_range(content: &str, line_range: (usize, usize), padding: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let start = line_range.0.saturating_sub(1 + padding);
    let end = (line_range.1 + padding).min(lines.len());
    lines[start..end].join("\n")
}

fn semantic_key(file_path: &Path, symbol_name: &str, line_range: (usize, usize)) -> String {
    format!(
        "{}:{}:{}:{}",
        file_path.display(),
        symbol_name,
        line_range.0,
        line_range.1
    )
}

fn feedback_example_fingerprint(
    content: &str,
    category: &str,
    file_patterns: &[String],
    accepted: bool,
) -> String {
    hash_text(&format!(
        "{}|{}|{}|{}",
        category,
        accepted,
        file_patterns.join(","),
        content
    ))
}

fn hash_text(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    format!("{:x}", digest)
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn is_code_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| SUPPORTED_CODE_EXTENSIONS.contains(&extension))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::diff_parser::{DiffHunk, DiffLine};

    #[test]
    fn local_hash_embedding_is_stable_and_normalized() {
        let first = local_hash_embedding("fn validate_token(user: &User) -> bool");
        let second = local_hash_embedding("fn validate_token(user: &User) -> bool");
        assert_eq!(first, second);
        let norm = first.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.001 || norm == 0.0);
    }

    #[test]
    fn cosine_similarity_prefers_related_text() {
        let query = local_hash_embedding("sql injection in query builder");
        let close = local_hash_embedding("query builder vulnerable to sql injection");
        let far = local_hash_embedding("rename variable for readability");
        assert!(cosine_similarity(&query, &close) > cosine_similarity(&query, &far));
    }

    #[test]
    fn semantic_feedback_store_deduplicates_examples() {
        let mut store = SemanticFeedbackStore::default();
        let example = SemanticFeedbackExample {
            content: "Style nit".to_string(),
            category: "Style".to_string(),
            file_patterns: vec!["*.rs".to_string()],
            accepted: false,
            created_at: "2026-03-13T00:00:00Z".to_string(),
            embedding: local_hash_embedding("Style nit"),
        };
        store.add_example(example.clone());
        store.add_example(example);
        assert_eq!(store.examples.len(), 1);
    }

    #[tokio::test]
    async fn semantic_context_returns_related_chunks() {
        let mut index = SemanticIndex::default();
        let embedding =
            local_hash_embedding("format sql query select from users where id equals user_id");
        index.entries.insert(
            "src/db.rs:build_query:1:10".to_string(),
            SemanticChunk {
                key: "src/db.rs:build_query:1:10".to_string(),
                file_path: PathBuf::from("src/db.rs"),
                symbol_name: "build_query".to_string(),
                line_range: (1, 10),
                summary: "Function `build_query` performs SQL string construction".to_string(),
                embedding_text: "format sql query select from users where id equals user_id"
                    .to_string(),
                code_excerpt: "fn build_query() {}".to_string(),
                embedding,
                content_hash: "abc".to_string(),
            },
        );

        let diff = UnifiedDiff {
            old_content: None,
            new_content: None,
            file_path: PathBuf::from("src/api.rs"),
            is_new: false,
            is_deleted: false,
            is_binary: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                context: String::new(),
                changes: vec![DiffLine {
                    old_line_no: Some(1),
                    new_line_no: Some(1),
                    change_type: ChangeType::Added,
                    content: "let query = format!(\"SELECT * FROM users WHERE id = {}\", user_id);"
                        .to_string(),
                }],
            }],
        };

        let chunks = semantic_context_for_diff(&index, &diff, None, None, 3, 0.1).await;
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Semantic match"));
    }
}
