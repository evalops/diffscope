use anyhow::Result;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::time::Instant;
use tokio::task::JoinSet;

pub trait DagNode: Clone + Eq + Hash {
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct DagNodeSpec<NodeId> {
    pub id: NodeId,
    pub dependencies: Vec<NodeId>,
    pub hints: DagNodeExecutionHints,
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
    pub satisfied: bool,
    pub ready: bool,
    pub unmet_dependencies: Vec<String>,
    pub subgraph: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagExecutionPlan {
    pub graph_name: String,
    pub completed: Vec<String>,
    pub satisfied: Vec<String>,
    pub ready: Vec<String>,
    pub nodes: Vec<DagNodePlanState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagExecutionRecord {
    pub name: String,
    pub enabled: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagExecutionTrace {
    pub graph_name: String,
    pub records: Vec<DagExecutionRecord>,
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

pub async fn execute_dag_with_parallelism<NodeId, TaskOutput, SpawnFn, ApplyFn>(
    specs: &[DagNodeSpec<NodeId>],
    mut spawn: SpawnFn,
    mut apply: ApplyFn,
) -> Result<Vec<DagExecutionRecord>>
where
    NodeId: DagNode + Send + 'static,
    TaskOutput: Send + 'static,
    SpawnFn: FnMut(NodeId) -> Result<BoxFuture<'static, Result<TaskOutput>>>,
    ApplyFn: FnMut(NodeId, TaskOutput) -> Result<()>,
{
    let mut completed = HashSet::new();
    let mut in_flight = HashSet::new();
    let mut join_set = JoinSet::new();
    let mut recorded = Vec::with_capacity(specs.len());
    let mut launch_sequence = 0usize;

    while completed.len() < specs.len() {
        let mut progressed = false;
        loop {
            let Some(spec) = specs
                .iter()
                .find(|candidate| {
                    !candidate.enabled
                        && !completed.contains(&candidate.id)
                        && !in_flight.contains(&candidate.id)
                        && candidate
                            .dependencies
                            .iter()
                            .all(|dependency| completed.contains(dependency))
                })
                .cloned()
            else {
                break;
            };

            recorded.push((
                launch_sequence,
                DagExecutionRecord {
                    name: spec.id.name().to_string(),
                    enabled: false,
                    duration_ms: 0,
                },
            ));
            launch_sequence += 1;
            completed.insert(spec.id);
            progressed = true;
        }

        if completed.len() == specs.len() {
            break;
        }

        let ready_indices = specs
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                candidate.enabled
                    && !completed.contains(&candidate.id)
                    && !in_flight.contains(&candidate.id)
                    && candidate
                        .dependencies
                        .iter()
                        .all(|dependency| completed.contains(dependency))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        if join_set.is_empty() {
            if let Some(index) = ready_indices
                .iter()
                .copied()
                .find(|index| !specs[*index].hints.parallelizable)
            {
                let spec = specs[index].clone();
                let started = Instant::now();
                let output = spawn(spec.id.clone())?.await?;
                apply(spec.id.clone(), output)?;
                recorded.push((
                    launch_sequence,
                    DagExecutionRecord {
                        name: spec.id.name().to_string(),
                        enabled: true,
                        duration_ms: started.elapsed().as_millis() as u64,
                    },
                ));
                launch_sequence += 1;
                completed.insert(spec.id);
                continue;
            }

            for index in ready_indices.iter().copied() {
                let spec = specs[index].clone();
                let sequence = launch_sequence;
                launch_sequence += 1;
                let id = spec.id.clone();
                let future = spawn(id.clone())?;
                let started = Instant::now();
                in_flight.insert(id.clone());
                join_set.spawn(async move {
                    let output = future.await;
                    (sequence, id, started.elapsed().as_millis() as u64, output)
                });
            }
        } else {
            for index in ready_indices
                .iter()
                .copied()
                .filter(|index| specs[*index].hints.parallelizable)
            {
                let spec = specs[index].clone();
                let sequence = launch_sequence;
                launch_sequence += 1;
                let id = spec.id.clone();
                let future = spawn(id.clone())?;
                let started = Instant::now();
                in_flight.insert(id.clone());
                join_set.spawn(async move {
                    let output = future.await;
                    (sequence, id, started.elapsed().as_millis() as u64, output)
                });
            }
        }

        if !join_set.is_empty() {
            let Some(joined) = join_set.join_next().await else {
                anyhow::bail!("DAG has unresolved or cyclic dependencies");
            };
            let (sequence, id, duration_ms, output) = joined
                .map_err(|error| anyhow::anyhow!("parallel DAG task failed to join: {error}"))?;
            let output = output?;
            apply(id.clone(), output)?;
            in_flight.remove(&id);
            completed.insert(id.clone());
            recorded.push((
                sequence,
                DagExecutionRecord {
                    name: id.name().to_string(),
                    enabled: true,
                    duration_ms,
                },
            ));
            continue;
        }

        if !progressed {
            anyhow::bail!("DAG has unresolved or cyclic dependencies");
        }
    }

    recorded.sort_by_key(|(sequence, _)| *sequence);
    Ok(recorded.into_iter().map(|(_, record)| record).collect())
}

pub fn plan_dag_execution(
    graph: &DagGraphContract,
    completed: &[String],
) -> Result<DagExecutionPlan> {
    let completed_set = completed.iter().cloned().collect::<HashSet<_>>();
    let satisfied_set = derive_satisfied_nodes(graph, &completed_set);

    for name in &completed_set {
        if !graph.nodes.iter().any(|node| node.name == *name) {
            anyhow::bail!("Unknown completed node '{}' for DAG '{}'", name, graph.name);
        }
    }

    let mut ready = Vec::new();
    let mut satisfied = Vec::new();
    let mut nodes = Vec::with_capacity(graph.nodes.len());
    for node in &graph.nodes {
        let unmet_dependencies = node
            .dependencies
            .iter()
            .filter(|dependency| !satisfied_set.contains(*dependency))
            .cloned()
            .collect::<Vec<_>>();
        let is_completed = completed_set.contains(&node.name);
        let is_satisfied = satisfied_set.contains(&node.name);
        let is_ready = node.enabled && !is_completed && unmet_dependencies.is_empty();
        if is_satisfied {
            satisfied.push(node.name.clone());
        }
        if is_ready {
            ready.push(node.name.clone());
        }
        nodes.push(DagNodePlanState {
            name: node.name.clone(),
            enabled: node.enabled,
            completed: is_completed,
            satisfied: is_satisfied,
            ready: is_ready,
            unmet_dependencies,
            subgraph: node.hints.subgraph.clone(),
        });
    }

    Ok(DagExecutionPlan {
        graph_name: graph.name.clone(),
        completed: completed.to_vec(),
        satisfied,
        ready,
        nodes,
    })
}

fn derive_satisfied_nodes(
    graph: &DagGraphContract,
    completed: &HashSet<String>,
) -> HashSet<String> {
    let mut satisfied = completed.clone();

    loop {
        let mut changed = false;
        for node in &graph.nodes {
            if node.enabled || satisfied.contains(&node.name) {
                continue;
            }
            if node
                .dependencies
                .iter()
                .all(|dependency| satisfied.contains(dependency))
            {
                satisfied.insert(node.name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    satisfied
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    fn hints(parallelizable: bool) -> DagNodeExecutionHints {
        DagNodeExecutionHints {
            parallelizable,
            retryable: true,
            side_effects: false,
            subgraph: None,
        }
    }

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
                hints: hints(false),
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Leaf,
                dependencies: vec![TestNode::Branch],
                hints: hints(false),
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Branch,
                dependencies: vec![TestNode::Root],
                hints: hints(false),
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
                hints: hints(false),
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Leaf,
                dependencies: vec![TestNode::Root],
                hints: hints(true),
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

        assert_eq!(plan.satisfied, vec!["root"]);
        assert_eq!(plan.ready, vec!["leaf"]);
        assert!(plan.nodes[0].completed);
        assert!(plan.nodes[0].satisfied);
        assert!(!plan.nodes[1].satisfied);
        assert_eq!(plan.nodes[1].subgraph.as_deref(), Some("child"));
    }

    #[test]
    fn plan_dag_execution_auto_satisfies_disabled_dependencies() {
        let graph = DagGraphContract {
            name: "disabled".to_string(),
            description: "disabled graph".to_string(),
            entry_nodes: vec!["skip".to_string()],
            terminal_nodes: vec!["work".to_string()],
            nodes: vec![
                DagNodeContract {
                    name: "skip".to_string(),
                    description: "skip".to_string(),
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
                    enabled: false,
                },
                DagNodeContract {
                    name: "work".to_string(),
                    description: "work".to_string(),
                    kind: DagNodeKind::Execution,
                    dependencies: vec!["skip".to_string()],
                    inputs: vec![],
                    outputs: vec![],
                    hints: DagNodeExecutionHints {
                        parallelizable: true,
                        retryable: true,
                        side_effects: false,
                        subgraph: None,
                    },
                    enabled: true,
                },
            ],
        };

        let plan = plan_dag_execution(&graph, &[]).unwrap();

        assert_eq!(plan.satisfied, vec!["skip"]);
        assert_eq!(plan.ready, vec!["work"]);
        assert!(!plan.nodes[0].completed);
        assert!(plan.nodes[0].satisfied);
        assert!(plan.nodes[1].ready);
        assert!(plan.nodes[1].unmet_dependencies.is_empty());
    }

    #[tokio::test]
    async fn execute_dag_with_parallelism_runs_ready_parallel_nodes_concurrently() {
        let specs = vec![
            DagNodeSpec {
                id: TestNode::Root,
                dependencies: vec![],
                hints: hints(false),
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Branch,
                dependencies: vec![TestNode::Root],
                hints: hints(true),
                enabled: true,
            },
            DagNodeSpec {
                id: TestNode::Leaf,
                dependencies: vec![TestNode::Root],
                hints: hints(true),
                enabled: true,
            },
        ];
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut applied = Vec::new();

        let records = execute_dag_with_parallelism(
            &specs,
            |node| {
                let active = Arc::clone(&active);
                let max_active = Arc::clone(&max_active);
                async move {
                    if node != TestNode::Root {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        let observed_max = max_active.load(Ordering::SeqCst);
                        if current > observed_max {
                            max_active.store(current, Ordering::SeqCst);
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                    Ok(node.name().to_string())
                }
                .boxed()
            },
            |_, output| {
                applied.push(output);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].name, "root");
        assert_eq!(records[1].name, "branch");
        assert_eq!(records[2].name, "leaf");
        assert!(max_active.load(Ordering::SeqCst) >= 2);
        assert!(applied.contains(&"branch".to_string()));
        assert!(applied.contains(&"leaf".to_string()));
    }
}
