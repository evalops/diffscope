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
}
