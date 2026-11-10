//! Reading back what a run did: its full trace, and the delegation tree
//! beneath it.

use agentos_core::delegation::Delegation;
use agentos_core::ids::TaskRunId;
use agentos_core::task::TaskRun;

use super::Runtime;
use crate::error::RuntimeError;

impl Runtime {
    /// Read a run's full execution trace.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn trace(&self, run_id: TaskRunId) -> Result<RunTrace, RuntimeError> {
        let run = self.database.runs().get(run_id).await?;
        let task = self.database.tasks().get(run.task_id).await?;
        let agent = self.database.agents().get(task.agent_id).await?;
        Ok(RunTrace {
            agent_name: agent.name,
            objective: task.objective,
            steps: self.database.steps().list_for_run(run_id).await?,
            executions: self.database.executions().list_for_run(run_id).await?,
            approvals: self.database.approvals().list_for_run(run_id).await?,
            delegations: self
                .database
                .delegations()
                .list_for_parent_run(run_id)
                .await?,
            delegated_by: self
                .database
                .delegations()
                .find_by_child_run(run_id)
                .await?,
            run,
        })
    }

    /// The delegation tree below a run, depth first.
    ///
    /// Read to at most the configured depth plus one level below `run_id`,
    /// and never through the same run twice, so a corrupt row that pointed a
    /// run at its own ancestor cannot make the read go on forever. A
    /// delegation whose run was never prepared has no node of its own; it is
    /// in [`RunTrace::delegations`] for the run that asked.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure, or if `run_id` does not exist.
    pub async fn delegation_tree(&self, run_id: TaskRunId) -> Result<DelegationNode, RuntimeError> {
        let delegation = self
            .database
            .delegations()
            .find_by_child_run(run_id)
            .await?;
        let levels = usize::from(self.config.delegation.max_depth) + 1;
        let mut seen = std::collections::HashSet::from([run_id]);
        self.delegation_node(run_id, delegation, levels, &mut seen)
            .await
    }

    async fn delegation_node(
        &self,
        run_id: TaskRunId,
        delegation: Option<Delegation>,
        levels: usize,
        seen: &mut std::collections::HashSet<TaskRunId>,
    ) -> Result<DelegationNode, RuntimeError> {
        let run = self.database.runs().get(run_id).await?;
        let task = self.database.tasks().get(run.task_id).await?;
        let agent = self.database.agents().get(task.agent_id).await?;

        let mut children = Vec::new();
        if levels > 0 {
            for edge in self
                .database
                .delegations()
                .list_for_parent_run(run_id)
                .await?
            {
                let Some(child_run) = edge.child_run_id else {
                    continue;
                };
                if !seen.insert(child_run) {
                    continue;
                }
                children.push(
                    Box::pin(self.delegation_node(child_run, Some(edge), levels - 1, seen)).await?,
                );
            }
        }

        Ok(DelegationNode {
            run,
            agent_name: agent.name,
            objective: task.objective,
            delegation,
            children,
        })
    }
}

/// Everything recorded about one run.
#[derive(Debug, Clone)]
pub struct RunTrace {
    /// The run itself.
    pub run: TaskRun,
    /// The agent that performed it.
    pub agent_name: String,
    /// What it was asked to do.
    pub objective: String,
    /// The ordered trace.
    pub steps: Vec<agentos_core::task::TaskStep>,
    /// Tool invocations, with their permission decisions.
    pub executions: Vec<agentos_persistence::ToolExecutionRecord>,
    /// Approvals raised during the run.
    pub approvals: Vec<agentos_core::approval::ApprovalRequest>,
    /// The delegations this run made, in the order it asked.
    pub delegations: Vec<Delegation>,
    /// The delegation that started this run, if another run asked for it.
    pub delegated_by: Option<Delegation>,
}

/// One run in a delegation tree, and the runs it delegated to.
#[derive(Debug, Clone)]
pub struct DelegationNode {
    /// The run.
    pub run: TaskRun,
    /// The agent that performed it.
    pub agent_name: String,
    /// What it was asked to do.
    pub objective: String,
    /// The edge that started it; `None` for the run the tree was read from
    /// when nothing delegated to it.
    pub delegation: Option<Delegation>,
    /// The runs it delegated to, in the order it asked.
    pub children: Vec<DelegationNode>,
}
