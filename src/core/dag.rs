use anyhow::Result;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::time::Instant;

pub trait DagNode: Clone + Eq + Hash {
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct DagNodeSpec<NodeId> {
    pub id: NodeId,
    pub dependencies: Vec<NodeId>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagNodeDescription {
    pub name: String,
    pub dependencies: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagNodeKind {
    Setup,
    Preparation,
    Execution,
    Validation,
    Transformation,
    Persistence,
    Analysis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNodeContract {
    pub name: String,
    pub description: String,
    pub kind: DagNodeKind,
    pub dependencies: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub hints: DagNodeExecutionHints,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagGraphContract {
    pub name: String,
    pub description: String,
    pub entry_nodes: Vec<String>,
    pub terminal_nodes: Vec<String>,
    pub nodes: Vec<DagNodeContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNodeExecutionHints {
    pub parallelizable: bool,
    pub retryable: bool,
    pub side_effects: bool,
    pub subgraph: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagCatalog {
    pub graphs: Vec<DagGraphContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNodePlanState {
    pub name: String,
    pub enabled: bool,
    pub completed: bool,
    pub ready: bool,
    pub unmet_dependencies: Vec<String>,
    pub subgraph: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagExecutionPlan {
    pub graph_name: String,
    pub completed: Vec<String>,
    pub ready: Vec<String>,
    pub nodes: Vec<DagNodePlanState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagExecutionRecord {
    pub name: String,
    pub enabled: bool,
    pub duration_ms: u64,
}

pub fn describe_dag<NodeId>(specs: &[DagNodeSpec<NodeId>]) -> Vec<DagNodeDescription>
where
    NodeId: DagNode,
{
    specs
        .iter()
        .map(|spec| DagNodeDescription {
            name: spec.id.name().to_string(),
            dependencies: spec
                .dependencies
                .iter()
                .map(|dependency| dependency.name().to_string())
                .collect(),
            enabled: spec.enabled,
        })
        .collect()
}

pub async fn execute_dag<NodeId, Context, F>(
    specs: &[DagNodeSpec<NodeId>],
    context: &mut Context,
    mut execute: F,
) -> Result<Vec<DagExecutionRecord>>
where
    NodeId: DagNode,
    Context: Send,
    F: for<'a> FnMut(NodeId, &'a mut Context) -> BoxFuture<'a, Result<()>>,
{
    let mut completed = HashSet::new();
    let mut records = Vec::with_capacity(specs.len());

    while completed.len() < specs.len() {
        let Some(spec) = specs
            .iter()
            .find(|candidate| {
                !completed.contains(&candidate.id)
                    && candidate
                        .dependencies
                        .iter()
                        .all(|dependency| completed.contains(dependency))
            })
            .cloned()
        else {
            anyhow::bail!("DAG has unresolved or cyclic dependencies");
        };

        let started = Instant::now();
        if spec.enabled {
            execute(spec.id.clone(), context).await?;
        }
        records.push(DagExecutionRecord {
            name: spec.id.name().to_string(),
            enabled: spec.enabled,
            duration_ms: started.elapsed().as_millis() as u64,
        });
        completed.insert(spec.id);
    }

    Ok(records)
}

pub fn plan_dag_execution(
    graph: &DagGraphContract,
    completed: &[String],
) -> Result<DagExecutionPlan> {
    let completed_set = completed.iter().cloned().collect::<HashSet<_>>();

    for name in &completed_set {
        if !graph.nodes.iter().any(|node| node.name == *name) {
            anyhow::bail!("Unknown completed node '{}' for DAG '{}'", name, graph.name);
        }
    }

    let mut ready = Vec::new();
    let mut nodes = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        let unmet_dependencies = node
            .dependencies
            .iter()
            .filter(|dependency| !completed_set.contains(*dependency))
            .cloned()
            .collect::<Vec<_>>();
        let is_completed = completed_set.contains(&node.name);
        let is_ready = node.enabled && !is_completed && unmet_dependencies.is_empty();
        if is_ready {
            ready.push(node.name.clone());
        }
        nodes.push(DagNodePlanState {
            name: node.name.clone(),
            enabled: node.enabled,
            completed: is_completed,
            ready: is_ready,
            unmet_dependencies,
            subgraph: node.hints.subgraph.clone(),
        });
    }

    Ok(DagExecutionPlan {
        graph_name: graph.name.clone(),
        completed: completed.to_vec(),
        ready,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestNode {
        Root,
        Branch,
        Leaf,
    }

    impl DagNode for TestNode {
        fn name(&self) -> &'static str {
            match self {
                Self::Root => "root",
                Self::Branch => "branch",
                Self::Leaf => "leaf",
            }
        }
    }

    #[tokio::test]
    async fn execute_dag_runs_nodes_in_dependency_order() {
        let specs = vec![
            DagNodeSpec {
                id: TestNode::Root,
                dependencies: vec![],
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Leaf,
                dependencies: vec![TestNode::Branch],
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Branch,
                dependencies: vec![TestNode::Root],
                enabled: true,
            },
        ];
        let mut visited = Vec::new();

        let records = execute_dag(&specs, &mut visited, |node, visited| {
            async move {
                visited.push(node.name().to_string());
                Ok(())
            }
            .boxed()
        })
        .await
        .unwrap();

        assert_eq!(visited, vec!["root", "branch", "leaf"]);
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn describe_dag_reports_names_and_dependencies() {
        let descriptions = describe_dag(&[
            DagNodeSpec {
                id: TestNode::Root,
                dependencies: vec![],
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Leaf,
                dependencies: vec![TestNode::Root],
                enabled: false,
            },
        ]);

        assert_eq!(descriptions[0].name, "root");
        assert_eq!(descriptions[1].dependencies, vec!["root"]);
        assert!(!descriptions[1].enabled);
    }

    #[test]
    fn plan_dag_execution_marks_ready_nodes() {
        let graph = DagGraphContract {
            name: "test".to_string(),
            description: "test graph".to_string(),
            entry_nodes: vec!["root".to_string()],
            terminal_nodes: vec!["leaf".to_string()],
            nodes: vec![
                DagNodeContract {
                    name: "root".to_string(),
                    description: "root".to_string(),
                    kind: DagNodeKind::Setup,
                    dependencies: vec![],
                    inputs: vec![],
                    outputs: vec![],
                    hints: DagNodeExecutionHints {
                        parallelizable: false,
                        retryable: true,
                        side_effects: false,
                        subgraph: None,
                    },
                    enabled: true,
                },
                DagNodeContract {
                    name: "leaf".to_string(),
                    description: "leaf".to_string(),
                    kind: DagNodeKind::Execution,
                    dependencies: vec!["root".to_string()],
                    inputs: vec![],
                    outputs: vec![],
                    hints: DagNodeExecutionHints {
                        parallelizable: true,
                        retryable: true,
                        side_effects: false,
                        subgraph: Some("child".to_string()),
                    },
                    enabled: true,
                },
            ],
        };

        let plan = plan_dag_execution(&graph, &["root".to_string()]).unwrap();

        assert_eq!(plan.ready, vec!["leaf"]);
        assert!(plan.nodes[0].completed);
        assert_eq!(plan.nodes[1].subgraph.as_deref(), Some("child"));
    }
}
