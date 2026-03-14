use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    assert!(graph.node_count() >= 6);
}

#[test]
fn test_symbols_in_file() {
    let files = sample_rust_files();
    let graph = SymbolGraph::build_from_source(&files);

    let auth_symbols = graph.symbols_in_file(Path::new("src/auth.rs"));
    let names: HashSet<&str> = auth_symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
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
    let names: HashSet<&str> = related.iter().map(|symbol| symbol.name.as_str()).collect();
    assert!(
        names.contains("Role") || names.contains("Authenticator"),
        "Expected colocated symbols, got: {names:?}"
    );
}

#[test]
fn test_related_symbols_retains_multi_hop_relation_path() {
    let mut graph = SymbolGraph::new();
    for (name, file) in [
        ("entrypoint", "src/entry.rs"),
        ("validate_token", "src/auth.rs"),
        ("Role", "src/models.rs"),
    ] {
        graph.add_node(SymbolNode {
            name: name.to_string(),
            file_path: PathBuf::from(file),
            line_range: (1, 5),
            kind: if name == "Role" {
                SymbolKind::Struct
            } else {
                SymbolKind::Function
            },
            edges: Vec::new(),
        });
    }

    graph.add_edge("entrypoint", "validate_token", SymbolRelation::Calls);
    graph.add_edge("validate_token", "Role", SymbolRelation::Uses);

    let related = graph.related_symbols(&["entrypoint".to_string()], 3, 10);
    let role = related
        .iter()
        .find(|symbol| symbol.name == "Role")
        .expect("expected multi-hop result for Role");

    assert_eq!(role.hops, 2);
    assert_eq!(role.relation_path.len(), 2);
    assert_eq!(role.relation_path[0], SymbolRelation::Calls);
    assert_eq!(role.relation_path[1], SymbolRelation::Uses);
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
    for location in &locations {
        assert!(!location.file_path.as_os_str().is_empty());
        assert!(location.snippet.contains("Graph:"));
    }
}

#[test]
fn test_graph_json_roundtrip_preserves_edges() {
    let files = sample_rust_files();
    let graph = SymbolGraph::build_from_source(&files);

    let reloaded = SymbolGraph::from_json(&graph.to_json().unwrap()).unwrap();
    let related = reloaded.related_symbols(&["User".to_string()], 2, 10);

    assert_eq!(reloaded.node_count(), graph.node_count());
    assert_eq!(reloaded.edge_count(), graph.edge_count());
    assert!(!related.is_empty());
}

#[test]
fn test_symbol_kind_detection() {
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("lib.rs"),
        "pub fn foo() {}\npub struct Bar {}\npub enum Baz {}\npub trait Qux {}".to_string(),
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
    assert_eq!(
        SymbolRelation::Inherits.inverse(),
        SymbolRelation::Implements
    );
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
    for ranked_symbol in &related {
        assert!(
            ranked_symbol.line >= 1,
            "Expected line >= 1, got {}",
            ranked_symbol.line
        );
        assert!(
            ranked_symbol.hops >= 1,
            "Expected hops >= 1, got {}",
            ranked_symbol.hops
        );
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
    assert_eq!(bar_edges[0].relation, SymbolRelation::Inherits);
}

#[test]
fn test_related_symbols_keep_trait_and_impl_methods_distinct() {
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("routes.rs"),
        r#"
use crate::request::Request;
use crate::search::QueryRunner;

pub fn get_profile(runner: &dyn QueryRunner, request: &Request) -> String {
    runner.find_user(request.name())
}
"#
        .to_string(),
    );
    files.insert(
        PathBuf::from("search.rs"),
        r#"
pub trait QueryRunner {
    fn find_user(&self, name: &str) -> String;
}
"#
        .to_string(),
    );
    files.insert(
        PathBuf::from("db.rs"),
        r#"
use crate::search::QueryRunner;

pub struct PostgresQueryRunner;

impl QueryRunner for PostgresQueryRunner {
    fn find_user(&self, name: &str) -> String {
        format!("SELECT * FROM users WHERE name = '{}'", name)
    }
}
"#
        .to_string(),
    );
    files.insert(
        PathBuf::from("request.rs"),
        r#"
pub struct Request {
    name: String,
}

impl Request {
    pub fn name(&self) -> &str {
        &self.name
    }
}
"#
        .to_string(),
    );

    let graph = SymbolGraph::build_from_source(&files);
    let related = graph.related_symbols(&["get_profile".to_string()], 2, 10);

    let find_user_files = related
        .iter()
        .filter(|symbol| symbol.name == "find_user")
        .map(|symbol| symbol.file_path.clone())
        .collect::<HashSet<_>>();

    assert!(find_user_files.contains(Path::new("search.rs")));
    assert!(find_user_files.contains(Path::new("db.rs")));
    assert!(related
        .iter()
        .any(|symbol| symbol.name == "QueryRunner" && symbol.file_path == Path::new("search.rs")));
}

#[test]
fn test_empty_graph_traversal() {
    let graph = SymbolGraph::new();
    let results = graph.related_symbols(&["nonexistent".to_string()], 2, 10);
    assert!(results.is_empty());
}

#[test]
fn test_empty_seed_symbols() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "Foo".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    let results = graph.related_symbols(&[], 2, 10);
    assert!(results.is_empty());
}

#[test]
fn test_nonexistent_seed_symbols() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "Foo".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    let results = graph.related_symbols(&["DoesNotExist".to_string()], 2, 10);
    assert!(results.is_empty());
}

#[test]
fn test_duplicate_edges() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "A".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_node(SymbolNode {
        name: "B".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (11, 20),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_edge("A", "B", SymbolRelation::Calls);
    graph.add_edge("A", "B", SymbolRelation::Calls);
    let a_edges = &graph.lookup("A").unwrap()[0].edges;
    assert_eq!(a_edges.len(), 1);
}

#[test]
fn test_max_results_zero() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "A".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_node(SymbolNode {
        name: "B".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (11, 20),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_edge("A", "B", SymbolRelation::Calls);
    let results = graph.related_symbols(&["A".to_string()], 2, 0);
    assert!(results.is_empty());
}

#[test]
fn test_cyclic_graph() {
    let mut graph = SymbolGraph::new();
    for name in &["A", "B", "C"] {
        graph.add_node(SymbolNode {
            name: name.to_string(),
            file_path: PathBuf::from("cycle.rs"),
            line_range: (1, 10),
            kind: SymbolKind::Function,
            edges: Vec::new(),
        });
    }
    graph.add_edge("A", "B", SymbolRelation::Calls);
    graph.add_edge("B", "C", SymbolRelation::Calls);
    graph.add_edge("C", "A", SymbolRelation::Calls);

    let results = graph.related_symbols(&["A".to_string()], 5, 10);
    assert!(!results.is_empty());
    let names: Vec<&str> = results.iter().map(|symbol| symbol.name.as_str()).collect();
    assert!(names.contains(&"B"));
    assert!(names.contains(&"C"));
}

#[test]
fn test_ranked_to_locations_simple() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "target".to_string(),
        file_path: PathBuf::from("t.rs"),
        line_range: (5, 15),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_node(SymbolNode {
        name: "caller".to_string(),
        file_path: PathBuf::from("c.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_edge("caller", "target", SymbolRelation::Calls);

    let ranked = graph.related_symbols(&["caller".to_string()], 1, 10);
    let locations = graph.ranked_to_locations(&ranked);
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].file_path, PathBuf::from("t.rs"));
}

#[test]
fn test_related_symbols_max_hops_zero() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "A".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_node(SymbolNode {
        name: "B".to_string(),
        file_path: PathBuf::from("b.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_edge("A", "B", SymbolRelation::Calls);

    let results = graph.related_symbols(&["A".to_string()], 0, 10);
    assert!(
        results.is_empty(),
        "max_hops=0 should return no results, but got {} results: {:?}",
        results.len(),
        results
            .iter()
            .map(|symbol| &symbol.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_add_edge_nonexistent_target() {
    let mut graph = SymbolGraph::new();
    graph.add_node(SymbolNode {
        name: "A".to_string(),
        file_path: PathBuf::from("a.rs"),
        line_range: (1, 10),
        kind: SymbolKind::Function,
        edges: Vec::new(),
    });
    graph.add_edge("A", "MISSING", SymbolRelation::Calls);
    let a_edges = &graph.lookup("A").unwrap()[0].edges;
    assert!(a_edges.is_empty());
}
