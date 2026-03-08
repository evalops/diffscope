use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::core::symbol_index::SymbolLocation;

/// Relationship between two symbols in the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolRelation {
    /// This symbol calls the target.
    Calls,
    /// This symbol is called by the target.
    CalledBy,
    /// This symbol inherits/extends the target.
    Inherits,
    /// This symbol implements the target trait/interface.
    Implements,
    /// This symbol references/uses the target type.
    Uses,
    /// This symbol is used/referenced by the target.
    UsedBy,
    /// This symbol is defined in the same file as the target.
    ColocatedWith,
}

impl SymbolRelation {
    /// Returns the inverse relation for bidirectional graph edges.
    pub fn inverse(&self) -> Self {
        match self {
            SymbolRelation::Calls => SymbolRelation::CalledBy,
            SymbolRelation::CalledBy => SymbolRelation::Calls,
            SymbolRelation::Inherits => SymbolRelation::Implements,
            SymbolRelation::Implements => SymbolRelation::Inherits,
            SymbolRelation::Uses => SymbolRelation::UsedBy,
            SymbolRelation::UsedBy => SymbolRelation::Uses,
            SymbolRelation::ColocatedWith => SymbolRelation::ColocatedWith,
        }
    }

    /// Weight used for ranking related symbols. Lower = more relevant.
    pub fn relevance_weight(&self) -> f32 {
        match self {
            SymbolRelation::Calls | SymbolRelation::CalledBy => 1.0,
            SymbolRelation::Inherits | SymbolRelation::Implements => 0.8,
            SymbolRelation::Uses | SymbolRelation::UsedBy => 1.5,
            SymbolRelation::ColocatedWith => 2.0,
        }
    }
}

/// A reference to a related symbol with the relationship type.
#[derive(Debug, Clone)]
pub struct SymbolEdge {
    pub target: String,
    pub relation: SymbolRelation,
    pub target_file: PathBuf,
    pub target_line: usize,
}

/// A node in the symbol graph representing a single symbol definition.
#[derive(Debug, Clone)]
pub struct SymbolNode {
    pub name: String,
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub kind: SymbolKind,
    pub edges: Vec<SymbolEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Class,
}

/// Graph-based symbol index that tracks relationships between symbols.
#[derive(Debug, Default)]
pub struct SymbolGraph {
    /// symbol_name -> list of nodes (same name can appear in multiple files)
    nodes: HashMap<String, Vec<SymbolNode>>,
    /// file_path -> set of symbol names defined in that file
    file_symbols: HashMap<PathBuf, HashSet<String>>,
}

impl SymbolGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a symbol node to the graph.
    pub fn add_node(&mut self, node: SymbolNode) {
        self.file_symbols
            .entry(node.file_path.clone())
            .or_default()
            .insert(node.name.clone());
        self.nodes
            .entry(node.name.clone())
            .or_default()
            .push(node);
    }

    /// Add a directed edge between two symbols.
    pub fn add_edge(&mut self, from: &str, to: &str, relation: SymbolRelation) {
        // Collect target info first to avoid borrow conflicts
        let to_info = self
            .nodes
            .get(to)
            .and_then(|nodes| nodes.first())
            .map(|n| (n.file_path.clone(), n.line_range.0));
        let from_info = self
            .nodes
            .get(from)
            .and_then(|nodes| nodes.first())
            .map(|n| (n.file_path.clone(), n.line_range.0));

        // Forward edge: from -> to
        if let Some((target_file, target_line)) = &to_info {
            if let Some(from_nodes) = self.nodes.get_mut(from) {
                for node in from_nodes.iter_mut() {
                    node.edges.push(SymbolEdge {
                        target: to.to_string(),
                        relation: relation.clone(),
                        target_file: target_file.clone(),
                        target_line: *target_line,
                    });
                }
            }
        }

        // Inverse edge: to -> from
        let inverse = relation.inverse();
        if let Some((source_file, source_line)) = &from_info {
            if let Some(to_nodes) = self.nodes.get_mut(to) {
                for node in to_nodes.iter_mut() {
                    node.edges.push(SymbolEdge {
                        target: from.to_string(),
                        relation: inverse.clone(),
                        target_file: source_file.clone(),
                        target_line: *source_line,
                    });
                }
            }
        }
    }

    /// Build the graph from source code using regex-based extraction.
    pub fn build_from_source(files: &HashMap<PathBuf, String>) -> Self {
        let mut graph = SymbolGraph::new();

        // First pass: extract symbol definitions
        for (path, content) in files {
            let symbols = extract_symbol_definitions(path, content);
            for sym in symbols {
                graph.add_node(sym);
            }
        }

        // Second pass: extract relationships
        for (path, content) in files {
            let relations = extract_relationships(path, content, &graph);
            for (from, to, relation) in relations {
                graph.add_edge(&from, &to, relation);
            }
        }

        // Add colocation edges
        for symbol_names in graph.file_symbols.clone().values() {
            let names: Vec<String> = symbol_names.iter().cloned().collect();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    graph.add_edge(&names[i], &names[j], SymbolRelation::ColocatedWith);
                }
            }
        }

        graph
    }

    /// Look up all nodes for a given symbol name.
    pub fn lookup(&self, symbol: &str) -> Option<&Vec<SymbolNode>> {
        self.nodes.get(symbol)
    }

    /// Get all symbols defined in a file.
    pub fn symbols_in_file(&self, file_path: &Path) -> Vec<&SymbolNode> {
        let mut result = Vec::new();
        if let Some(names) = self.file_symbols.get(file_path) {
            for name in names {
                if let Some(nodes) = self.nodes.get(name) {
                    for node in nodes {
                        if node.file_path == file_path {
                            result.push(node);
                        }
                    }
                }
            }
        }
        result
    }

    /// Find related symbols using BFS traversal with relationship awareness.
    /// Returns symbols ranked by relevance (weighted by relationship type and hop distance).
    pub fn related_symbols(
        &self,
        seed_symbols: &[String],
        max_hops: usize,
        max_results: usize,
    ) -> Vec<RankedSymbol> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize, f32)> = VecDeque::new();
        let mut results: Vec<RankedSymbol> = Vec::new();

        for seed in seed_symbols {
            visited.insert(seed.clone());
            queue.push_back((seed.clone(), 0, 0.0));
        }

        while let Some((current, depth, accumulated_cost)) = queue.pop_front() {
            if depth > max_hops {
                continue;
            }

            if let Some(nodes) = self.nodes.get(&current) {
                for node in nodes {
                    for edge in &node.edges {
                        if visited.insert(edge.target.clone()) {
                            let cost =
                                accumulated_cost + edge.relation.relevance_weight() * (depth as f32 + 1.0);
                            results.push(RankedSymbol {
                                name: edge.target.clone(),
                                file_path: edge.target_file.clone(),
                                line: edge.target_line,
                                relevance_score: 1.0 / (1.0 + cost),
                                relation_path: vec![edge.relation.clone()],
                                hops: depth + 1,
                            });
                            if depth + 1 < max_hops {
                                queue.push_back((edge.target.clone(), depth + 1, cost));
                            }
                        }
                    }
                }
            }
        }

        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);
        results
    }

    /// Convert ranked symbols into SymbolLocations for compatibility with existing pipeline.
    pub fn ranked_to_locations(&self, ranked: &[RankedSymbol]) -> Vec<SymbolLocation> {
        let mut locations = Vec::new();
        for rs in ranked {
            if let Some(nodes) = self.nodes.get(&rs.name) {
                for node in nodes {
                    if node.file_path == rs.file_path {
                        locations.push(SymbolLocation {
                            file_path: node.file_path.clone(),
                            line_range: node.line_range,
                            snippet: format!(
                                "[Graph: {:?}, relevance={:.2}]\n{}",
                                rs.relation_path.first().unwrap_or(&SymbolRelation::Uses),
                                rs.relevance_score,
                                node.name
                            ),
                        });
                    }
                }
            }
        }
        locations
    }

    pub fn node_count(&self) -> usize {
        self.nodes.values().map(|v| v.len()).sum()
    }

    pub fn edge_count(&self) -> usize {
        self.nodes
            .values()
            .flat_map(|v| v.iter())
            .map(|n| n.edges.len())
            .sum()
    }

    pub fn file_count(&self) -> usize {
        self.file_symbols.len()
    }
}

/// A symbol with a computed relevance score from graph traversal.
#[derive(Debug, Clone)]
pub struct RankedSymbol {
    pub name: String,
    pub file_path: PathBuf,
    pub line: usize,
    pub relevance_score: f32,
    pub relation_path: Vec<SymbolRelation>,
    pub hops: usize,
}

use once_cell::sync::Lazy;
use regex::Regex;

static RUST_FN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});
static RUST_STRUCT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static RUST_ENUM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static RUST_TRAIT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static RUST_IMPL_FOR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*impl(?:<[^>]*>)?\s+([A-Za-z_][A-Za-z0-9_]*)\s+for\s+([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap()
});
static FN_CALL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap());
static TYPE_REF: Lazy<Regex> =
    Lazy::new(|| Regex::new(r":\s*(?:&\s*)?(?:mut\s+)?([A-Z][A-Za-z0-9_]*)").unwrap());
static PY_DEF: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
static PY_CLASS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)(?:\(([^)]*)\))?").unwrap());
static JS_FN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)").unwrap()
});
static JS_CLASS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:export\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)(?:\s+extends\s+([A-Za-z_$][A-Za-z0-9_$]*))?").unwrap()
});

fn detect_kind(line: &str, ext: &str) -> Option<(String, SymbolKind)> {
    match ext {
        "rs" => {
            if let Some(caps) = RUST_FN.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Function));
            }
            if let Some(caps) = RUST_STRUCT.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Struct));
            }
            if let Some(caps) = RUST_ENUM.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Enum));
            }
            if let Some(caps) = RUST_TRAIT.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Trait));
            }
            None
        }
        "py" | "pyi" => {
            if let Some(caps) = PY_DEF.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Function));
            }
            if let Some(caps) = PY_CLASS.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Class));
            }
            None
        }
        "js" | "jsx" | "ts" | "tsx" => {
            if let Some(caps) = JS_FN.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Function));
            }
            if let Some(caps) = JS_CLASS.captures(line) {
                return Some((caps[1].to_string(), SymbolKind::Class));
            }
            None
        }
        _ => None,
    }
}

fn extract_symbol_definitions(path: &Path, content: &str) -> Vec<SymbolNode> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let lines: Vec<&str> = content.lines().collect();
    let mut symbols = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if let Some((name, kind)) = detect_kind(line, ext) {
            if name.len() < 2 {
                continue;
            }
            let end = find_block_end(&lines, idx);
            symbols.push(SymbolNode {
                name,
                file_path: path.to_path_buf(),
                line_range: (idx + 1, end + 1),
                kind,
                edges: Vec::new(),
            });
        }
    }

    symbols
}

fn find_block_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0i32;
    let mut found_open = false;
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in line.chars() {
            if ch == '{' || ch == '(' {
                depth += 1;
                found_open = true;
            } else if ch == '}' || ch == ')' {
                depth -= 1;
            }
        }
        if found_open && depth <= 0 {
            return i;
        }
        // For languages without braces (Python), use indentation
        if i > start && !line.trim().is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            return i.saturating_sub(1);
        }
    }
    lines.len().saturating_sub(1)
}

fn extract_relationships(
    path: &Path,
    content: &str,
    graph: &SymbolGraph,
) -> Vec<(String, String, SymbolRelation)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mut relations = Vec::new();

    // Get symbols defined in this file
    let file_symbols: HashSet<String> = graph
        .file_symbols
        .get(path)
        .cloned()
        .unwrap_or_default();

    let known_symbols: HashSet<&String> = graph.nodes.keys().collect();

    let lines: Vec<&str> = content.lines().collect();
    let mut current_fn: Option<String> = None;

    for line in &lines {
        // Track current function context
        if ext == "rs" {
            if let Some(caps) = RUST_FN.captures(line) {
                current_fn = Some(caps[1].to_string());
            }
        } else if ext == "py" {
            if let Some(caps) = PY_DEF.captures(line) {
                current_fn = Some(caps[1].to_string());
            }
        }

        // Rust: impl Trait for Struct
        if ext == "rs" {
            if let Some(caps) = RUST_IMPL_FOR.captures(line) {
                let trait_name = caps[1].to_string();
                let struct_name = caps[2].to_string();
                if known_symbols.contains(&trait_name) && known_symbols.contains(&struct_name) {
                    relations.push((struct_name, trait_name, SymbolRelation::Implements));
                }
            }
        }

        // Python: class Foo(Bar)
        if ext == "py" {
            if let Some(caps) = PY_CLASS.captures(line) {
                let class_name = caps[1].to_string();
                if let Some(bases) = caps.get(2) {
                    for base in bases.as_str().split(',') {
                        let base = base.trim().to_string();
                        if known_symbols.contains(&base) {
                            relations.push((class_name.clone(), base, SymbolRelation::Inherits));
                        }
                    }
                }
            }
        }

        // JS: class Foo extends Bar
        if ext == "js" || ext == "ts" || ext == "tsx" || ext == "jsx" {
            if let Some(caps) = JS_CLASS.captures(line) {
                let class_name = caps[1].to_string();
                if let Some(parent) = caps.get(2) {
                    let parent_name = parent.as_str().to_string();
                    if known_symbols.contains(&parent_name) {
                        relations.push((class_name, parent_name, SymbolRelation::Inherits));
                    }
                }
            }
        }

        // Function calls (within a known function)
        if let Some(ref caller) = current_fn {
            if file_symbols.contains(caller) {
                for caps in FN_CALL.captures_iter(line) {
                    let callee = caps[1].to_string();
                    if callee != *caller
                        && known_symbols.contains(&callee)
                        && callee.len() >= 2
                    {
                        relations.push((caller.clone(), callee, SymbolRelation::Calls));
                    }
                }
            }
        }

        // Type references
        if let Some(ref owner) = current_fn {
            if file_symbols.contains(owner) {
                for caps in TYPE_REF.captures_iter(line) {
                    let type_name = caps[1].to_string();
                    if type_name != *owner && known_symbols.contains(&type_name) {
                        relations.push((owner.clone(), type_name, SymbolRelation::Uses));
                    }
                }
            }
        }
    }

    relations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rust_files() -> HashMap<PathBuf, String> {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/auth.rs"),
            r#"
pub struct User {
    pub name: String,
    pub role: Role,
}

pub enum Role {
    Admin,
    Reader,
}

pub trait Authenticator {
    fn authenticate(&self, user: &User) -> bool;
}

pub fn validate_token(token: &str) -> bool {
    token.len() > 10
}
"#
            .to_string(),
        );

        files.insert(
            PathBuf::from("src/handler.rs"),
            r#"
pub struct RequestHandler {
    auth: Box<dyn Authenticator>,
}

impl RequestHandler {
    pub fn handle(&self, user: &User) -> String {
        if validate_token("abc") {
            format!("Welcome {}", user.name)
        } else {
            "Denied".to_string()
        }
    }
}
"#
            .to_string(),
        );

        files.insert(
            PathBuf::from("src/admin.rs"),
            r#"
pub struct AdminAuth;

impl Authenticator for AdminAuth {
    fn authenticate(&self, user: &User) -> bool {
        matches!(user.role, Role::Admin)
    }
}
"#
            .to_string(),
        );

        files
    }

    #[test]
    fn test_build_graph_extracts_symbols() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        assert!(graph.lookup("User").is_some());
        assert!(graph.lookup("Role").is_some());
        assert!(graph.lookup("Authenticator").is_some());
        assert!(graph.lookup("validate_token").is_some());
        assert!(graph.lookup("RequestHandler").is_some());
        assert!(graph.lookup("AdminAuth").is_some());
    }

    #[test]
    fn test_node_count() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);
        // User, Role, Authenticator, validate_token, authenticate,
        // RequestHandler, handle, AdminAuth, authenticate(admin)
        assert!(graph.node_count() >= 6);
    }

    #[test]
    fn test_symbols_in_file() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let auth_symbols = graph.symbols_in_file(Path::new("src/auth.rs"));
        let names: HashSet<&str> = auth_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("User"));
        assert!(names.contains("Role"));
        assert!(names.contains("Authenticator"));
    }

    #[test]
    fn test_related_symbols_bfs() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let related = graph.related_symbols(&["User".to_string()], 2, 10);
        assert!(!related.is_empty());
        // User should have colocation edges to Role, Authenticator, validate_token
        let names: HashSet<&str> = related.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains("Role") || names.contains("Authenticator"),
            "Expected colocated symbols, got: {:?}",
            names
        );
    }

    #[test]
    fn test_related_symbols_respects_max() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let related = graph.related_symbols(&["User".to_string()], 3, 2);
        assert!(related.len() <= 2);
    }

    #[test]
    fn test_relevance_scoring() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let related = graph.related_symbols(&["User".to_string()], 3, 20);
        // Results should be sorted by relevance (descending)
        for window in related.windows(2) {
            assert!(window[0].relevance_score >= window[1].relevance_score);
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = SymbolGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(graph.file_count(), 0);
        assert!(graph.lookup("anything").is_none());
    }

    #[test]
    fn test_ranked_to_locations() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let related = graph.related_symbols(&["User".to_string()], 2, 5);
        let locations = graph.ranked_to_locations(&related);
        assert!(!locations.is_empty());
        for loc in &locations {
            assert!(!loc.file_path.as_os_str().is_empty());
            assert!(loc.snippet.contains("Graph:"));
        }
    }

    #[test]
    fn test_symbol_kind_detection() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib.rs"),
            "pub fn foo() {}\npub struct Bar {}\npub enum Baz {}\npub trait Qux {}"
                .to_string(),
        );
        let graph = SymbolGraph::build_from_source(&files);

        let foo = &graph.lookup("foo").unwrap()[0];
        assert_eq!(foo.kind, SymbolKind::Function);
        let bar = &graph.lookup("Bar").unwrap()[0];
        assert_eq!(bar.kind, SymbolKind::Struct);
        let baz = &graph.lookup("Baz").unwrap()[0];
        assert_eq!(baz.kind, SymbolKind::Enum);
        let qux = &graph.lookup("Qux").unwrap()[0];
        assert_eq!(qux.kind, SymbolKind::Trait);
    }

    #[test]
    fn test_python_symbols() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app.py"),
            "class Animal:\n    pass\n\nclass Dog(Animal):\n    def bark(self):\n        pass\n\ndef helper():\n    pass\n"
                .to_string(),
        );
        let graph = SymbolGraph::build_from_source(&files);

        assert!(graph.lookup("Animal").is_some());
        assert!(graph.lookup("Dog").is_some());
        assert!(graph.lookup("bark").is_some());
        assert!(graph.lookup("helper").is_some());
    }

    #[test]
    fn test_javascript_symbols() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app.js"),
            "export class Base {}\nexport class Child extends Base {}\nexport function render() {}\n"
                .to_string(),
        );
        let graph = SymbolGraph::build_from_source(&files);

        assert!(graph.lookup("Base").is_some());
        assert!(graph.lookup("Child").is_some());
        assert!(graph.lookup("render").is_some());
    }

    #[test]
    fn test_inverse_relations() {
        assert_eq!(SymbolRelation::Calls.inverse(), SymbolRelation::CalledBy);
        assert_eq!(SymbolRelation::CalledBy.inverse(), SymbolRelation::Calls);
        assert_eq!(SymbolRelation::Inherits.inverse(), SymbolRelation::Implements);
        assert_eq!(SymbolRelation::Uses.inverse(), SymbolRelation::UsedBy);
        assert_eq!(
            SymbolRelation::ColocatedWith.inverse(),
            SymbolRelation::ColocatedWith
        );
    }

    #[test]
    fn test_ranked_symbol_line_and_hops() {
        let files = sample_rust_files();
        let graph = SymbolGraph::build_from_source(&files);

        let related = graph.related_symbols(&["User".to_string()], 3, 20);
        assert!(!related.is_empty());
        for rs in &related {
            // line should be a valid source line (1-indexed)
            assert!(rs.line >= 1, "Expected line >= 1, got {}", rs.line);
            // hops should be at least 1 (direct neighbor)
            assert!(rs.hops >= 1, "Expected hops >= 1, got {}", rs.hops);
        }
    }

    #[test]
    fn test_add_edge_bidirectional() {
        let mut graph = SymbolGraph::new();
        graph.add_node(SymbolNode {
            name: "Foo".to_string(),
            file_path: PathBuf::from("a.rs"),
            line_range: (1, 5),
            kind: SymbolKind::Struct,
            edges: Vec::new(),
        });
        graph.add_node(SymbolNode {
            name: "Bar".to_string(),
            file_path: PathBuf::from("b.rs"),
            line_range: (1, 5),
            kind: SymbolKind::Trait,
            edges: Vec::new(),
        });
        graph.add_edge("Foo", "Bar", SymbolRelation::Implements);

        let foo_edges = &graph.lookup("Foo").unwrap()[0].edges;
        assert_eq!(foo_edges.len(), 1);
        assert_eq!(foo_edges[0].relation, SymbolRelation::Implements);

        let bar_edges = &graph.lookup("Bar").unwrap()[0].edges;
        assert_eq!(bar_edges.len(), 1);
        assert_eq!(bar_edges[0].relation, SymbolRelation::Inherits); // inverse
    }
}
