use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;

use super::{SymbolEdge, SymbolGraph, SymbolKind, SymbolNode, SymbolRelation};

impl SymbolGraph {
    pub fn add_node(&mut self, node: SymbolNode) {
        self.file_symbols
            .entry(node.file_path.clone())
            .or_default()
            .insert(node.name.clone());
        self.nodes.entry(node.name.clone()).or_default().push(node);
    }

    pub fn add_edge(&mut self, from: &str, to: &str, relation: SymbolRelation) {
        let to_info = self
            .nodes
            .get(to)
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|node| (node.file_path.clone(), node.line_range.0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let from_info = self
            .nodes
            .get(from)
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|node| (node.file_path.clone(), node.line_range.0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let Some(from_nodes) = self.nodes.get_mut(from) {
            for node in from_nodes.iter_mut() {
                for (target_file, target_line) in &to_info {
                    push_edge_if_absent(
                        node,
                        SymbolEdge {
                            target: to.to_string(),
                            relation: relation.clone(),
                            target_file: target_file.clone(),
                            target_line: *target_line,
                        },
                    );
                }
            }
        }

        let inverse = relation.inverse();
        if let Some(to_nodes) = self.nodes.get_mut(to) {
            for node in to_nodes.iter_mut() {
                for (source_file, source_line) in &from_info {
                    push_edge_if_absent(
                        node,
                        SymbolEdge {
                            target: from.to_string(),
                            relation: inverse.clone(),
                            target_file: source_file.clone(),
                            target_line: *source_line,
                        },
                    );
                }
            }
        }
    }

    pub fn build_from_source(files: &HashMap<PathBuf, String>) -> Self {
        let mut graph = SymbolGraph::new();

        for (path, content) in files {
            let symbols = extract_symbol_definitions(path, content);
            for symbol in symbols {
                graph.add_node(symbol);
            }
        }

        for (path, content) in files {
            let relations = extract_relationships(path, content, &graph);
            for (from, to, relation) in relations {
                graph.add_edge(&from, &to, relation);
            }
        }

        let file_symbols_snapshot: Vec<Vec<String>> = graph
            .file_symbols
            .values()
            .map(|symbols| symbols.iter().cloned().collect())
            .collect();
        for names in &file_symbols_snapshot {
            for index in 0..names.len() {
                for other_index in (index + 1)..names.len() {
                    graph.add_edge(
                        &names[index],
                        &names[other_index],
                        SymbolRelation::ColocatedWith,
                    );
                }
            }
        }

        graph
    }

    pub fn lookup(&self, symbol: &str) -> Option<&Vec<SymbolNode>> {
        self.nodes.get(symbol)
    }

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

    pub fn node_count(&self) -> usize {
        self.nodes.values().map(|nodes| nodes.len()).sum()
    }

    pub fn edge_count(&self) -> usize {
        self.nodes
            .values()
            .flat_map(|nodes| nodes.iter())
            .map(|node| node.edges.len())
            .sum()
    }

    pub fn file_count(&self) -> usize {
        self.file_symbols.len()
    }
}

static RUST_FN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
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
static FN_CALL: Lazy<Regex> = Lazy::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap());
static TYPE_REF: Lazy<Regex> =
    Lazy::new(|| Regex::new(r":\s*(?:&\s*)?(?:mut\s+)?(?:dyn\s+)?([A-Z][A-Za-z0-9_]*)").unwrap());
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

fn push_edge_if_absent(node: &mut SymbolNode, edge: SymbolEdge) {
    if node.edges.iter().any(|existing| {
        existing.target == edge.target
            && existing.relation == edge.relation
            && existing.target_file == edge.target_file
            && existing.target_line == edge.target_line
    }) {
        return;
    }

    node.edges.push(edge);
}

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
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    let lines: Vec<&str> = content.lines().collect();
    let mut symbols = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if let Some((name, kind)) = detect_kind(line, ext) {
            if name.len() < 2 {
                continue;
            }
            let end = find_block_end(&lines, index);
            symbols.push(SymbolNode {
                name,
                file_path: path.to_path_buf(),
                line_range: (index + 1, end + 1),
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
    for (index, line) in lines.iter().enumerate().skip(start) {
        for character in line.chars() {
            if character == '{' || character == '(' {
                depth += 1;
                found_open = true;
            } else if character == '}' || character == ')' {
                depth -= 1;
            }
        }
        if found_open && depth <= 0 {
            return index;
        }
        if index > start
            && !line.trim().is_empty()
            && !line.starts_with(' ')
            && !line.starts_with('\t')
        {
            return index.saturating_sub(1);
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
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    let mut relations = Vec::new();

    let file_symbols: HashSet<String> = graph.file_symbols.get(path).cloned().unwrap_or_default();
    let known_symbols: HashSet<&String> = graph.nodes.keys().collect();

    let lines: Vec<&str> = content.lines().collect();
    let mut current_fn: Option<String> = None;

    for line in &lines {
        if ext == "rs" {
            if let Some(caps) = RUST_FN.captures(line) {
                current_fn = Some(caps[1].to_string());
            }
        } else if ext == "py" {
            if let Some(caps) = PY_DEF.captures(line) {
                current_fn = Some(caps[1].to_string());
            }
        }

        if ext == "rs" {
            if let Some(caps) = RUST_IMPL_FOR.captures(line) {
                let trait_name = caps[1].to_string();
                let struct_name = caps[2].to_string();
                if known_symbols.contains(&trait_name) && known_symbols.contains(&struct_name) {
                    relations.push((struct_name, trait_name, SymbolRelation::Implements));
                }
            }
        }

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

        if let Some(caller) = current_fn.as_ref() {
            if file_symbols.contains(caller) {
                for caps in FN_CALL.captures_iter(line) {
                    let callee = caps[1].to_string();
                    if callee != *caller && known_symbols.contains(&callee) && callee.len() >= 2 {
                        relations.push((caller.clone(), callee, SymbolRelation::Calls));
                    }
                }
            }
        }

        if let Some(owner) = current_fn.as_ref() {
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
