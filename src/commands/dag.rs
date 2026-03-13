use anyhow::Result;

use crate::config;
use crate::core::dag::{plan_dag_execution, DagCatalog, DagExecutionPlan, DagGraphContract};
use crate::review::{describe_review_pipeline_graph, describe_review_postprocess_graph};

use super::eval::describe_eval_fixture_graph;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DagGraphSelection {
    Review,
    Postprocess { convention_store_path: bool },
    Eval { repro_validate: bool },
}

pub(crate) fn describe_dag_graph(
    config: &config::Config,
    selection: DagGraphSelection,
) -> DagGraphContract {
    match selection {
        DagGraphSelection::Review => describe_review_pipeline_graph(),
        DagGraphSelection::Postprocess {
            convention_store_path,
        } => describe_review_postprocess_graph(config, convention_store_path),
        DagGraphSelection::Eval { repro_validate } => describe_eval_fixture_graph(repro_validate),
    }
}

pub(crate) fn build_dag_catalog(
    config: &config::Config,
    repro_validate: bool,
    convention_store_path: bool,
) -> DagCatalog {
    DagCatalog {
        graphs: vec![
            describe_dag_graph(config, DagGraphSelection::Review),
            describe_dag_graph(
                config,
                DagGraphSelection::Postprocess {
                    convention_store_path,
                },
            ),
            describe_dag_graph(config, DagGraphSelection::Eval { repro_validate }),
        ],
    }
}

pub(crate) fn plan_dag_graph(
    config: &config::Config,
    selection: DagGraphSelection,
    completed: &[String],
) -> Result<DagExecutionPlan> {
    let graph = describe_dag_graph(config, selection);
    plan_dag_execution(&graph, completed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_catalog_includes_nested_eval_and_review_graphs() {
        let catalog = build_dag_catalog(&config::Config::default(), true, true);

        assert_eq!(catalog.graphs.len(), 3);
        assert_eq!(catalog.graphs[0].name, "review_pipeline");
        assert_eq!(catalog.graphs[1].name, "review_postprocess");
        assert_eq!(catalog.graphs[2].name, "eval_fixture_execution");
    }

    #[test]
    fn dag_planner_reports_ready_nodes_for_review_pipeline() {
        let plan = plan_dag_graph(
            &config::Config::default(),
            DagGraphSelection::Review,
            &["initialize_services".to_string()],
        )
        .unwrap();

        assert_eq!(plan.ready, vec!["build_session"]);
    }
}
