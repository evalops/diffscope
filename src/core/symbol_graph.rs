use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod construction;
mod persistence;
mod ranking;
mod traversal;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolRelation {
    Calls,
    CalledBy,
    Inherits,
    Implements,
    Uses,
    UsedBy,
    ColocatedWith,
}

impl SymbolRelation {
    pub fn as_label(&self) -> &'static str {
        match self {
            SymbolRelation::Calls => "calls",
            SymbolRelation::CalledBy => "called-by",
            SymbolRelation::Inherits => "inherits",
            SymbolRelation::Implements => "implements",
            SymbolRelation::Uses => "uses",
            SymbolRelation::UsedBy => "used-by",
            SymbolRelation::ColocatedWith => "colocated-with",
        }
    }

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

    pub fn relevance_weight(&self) -> f32 {
        match self {
            SymbolRelation::Calls | SymbolRelation::CalledBy => 1.0,
            SymbolRelation::Inherits | SymbolRelation::Implements => 0.8,
            SymbolRelation::Uses | SymbolRelation::UsedBy => 1.5,
            SymbolRelation::ColocatedWith => 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEdge {
    pub target: String,
    pub relation: SymbolRelation,
    pub target_file: PathBuf,
    pub target_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolNode {
    pub name: String,
    pub file_path: PathBuf,
    pub line_range: (usize, usize),
    pub kind: SymbolKind,
    pub edges: Vec<SymbolEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Class,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SymbolGraph {
    nodes: HashMap<String, Vec<SymbolNode>>,
    file_symbols: HashMap<PathBuf, HashSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NodeKey {
    name: String,
    file_path: PathBuf,
    line: usize,
}

impl NodeKey {
    fn from_node(node: &SymbolNode) -> Self {
        Self {
            name: node.name.clone(),
            file_path: node.file_path.clone(),
            line: node.line_range.0,
        }
    }

    fn from_edge(edge: &SymbolEdge) -> Self {
        Self {
            name: edge.target.clone(),
            file_path: edge.target_file.clone(),
            line: edge.target_line,
        }
    }
}

impl SymbolGraph {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct RankedSymbol {
    pub name: String,
    pub file_path: PathBuf,
    pub line: usize,
    pub relevance_score: f32,
    pub relation_path: Vec<SymbolRelation>,
    pub hops: usize,
}
