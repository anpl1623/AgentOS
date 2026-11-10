//! Clearing up after a process that exited mid-run.
//!
//! A run, the delegations it opened and the approval requests it raised,
//! left looking alive by a process that is gone, are marked as having ended,
//! so that nothing goes on reporting work that nobody is doing.

use agentos_core::delegation::DelegationState;
use agentos_core::ids::{AgentId, TaskRunId};
use agentos_core::task::{TaskRun, TaskState, TaskStatus, TaskTrigger};

use super::Runtime;
use crate::error::RuntimeError;
use crate::state::RunStateMachine;

impl Runtime {
    /// Mark runs abandoned by a previous process as failed, and close the
    /// approval requests they left behind.
    ///
    /// Called at startup: a run that was executing when the process died is not
    /// executing now, and leaving it looking alive would misreport the system's
    /// state indefinitely.
    ///
    /// Its pending requests are the same lie in the approvals queue, and the
    /// more dangerous one: a card a person can still answer, for a run that will
    /// never act on the answer. Every pending request whose run has ended —
    /// those reaped here, and any an earlier crash left on a run that had
    /// already finished — is marked expired, with a note saying why.
    ///
    /// An unfinished run is only abandoned if no process is driving it, and
    /// the desktop application, a terminal run and `agentos doctor` can all
    /// share one data directory. So nothing is reaped while any process,
    /// this one included, holds the data directory's run lock; the call
    /// returns zero and the runs are left to whoever is driving them. A run
    /// this process is driving is never reaped either way.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure, [`RuntimeError::Io`] if the run
    /// lock cannot be consulted.
    pub async fn reap_abandoned_runs(&self) -> Result<usize, RuntimeError> {
        // Held until the reap is done, so no run can start meanwhile.
        let _exclusive = match &self.runs_lock {
            Some(lock) => match lock.try_exclusive()? {
                Some(exclusive) => Some(exclusive),
                None => {
                    tracing::info!(
                        "another process is driving runs; leaving unfinished runs to it"
                    );
                    return Ok(0);
                }
            },
            None => None,
        };

        let live = self.running_runs().await;
        let abandoned: Vec<TaskRun> = self
            .database
            .runs()
            .list_unfinished()
            .await?
            .into_iter()
            .filter(|run| !live.contains(&run.id))
            .collect();
        let count = abandoned.len();

        for mut run in abandoned {
            // The transition is written to the audit chain under the agent
            // whose run it was. The run row does not carry the agent; its
            // task does. A task that cannot be found leaves the record with
            // an identity that matches no agent, which is still better than
            // leaving the run looking alive.
            let agent_id = match self.database.tasks().find(run.task_id).await? {
                Some(task) => task.agent_id,
                None => {
                    tracing::warn!(run = %run.id, "reaping a run whose task is missing");
                    AgentId::new()
                }
            };
            let machine = RunStateMachine::new(
                agent_id,
                run.task_id,
                run.id,
                run.state,
                self.database.clone(),
                self.audit.clone(),
            );
            machine.try_apply(TaskTrigger::UnrecoverableError).await;

            run.state = TaskState::Failed;
            run.failure = Some(agentos_core::task::TaskFailure::Runtime {
                message: "the process exited while this run was in progress".to_owned(),
            });
            run.completed_at = Some(agentos_core::now());
            self.database.runs().update(&run).await?;
            self.database
                .tasks()
                .set_status(run.task_id, TaskStatus::Failed)
                .await?;
            // A delegated run's edge would otherwise say it is still running
            // for as long as the record is kept.
            if let Some(edge) = self
                .database
                .delegations()
                .find_by_child_run(run.id)
                .await?
                && !edge.state.is_terminal()
            {
                self.database
                    .delegations()
                    .complete(edge.id, DelegationState::Failed, run.steps_taken)
                    .await?;
            }
            // And so would the edges it opened, which nothing is left to
            // conclude. One still `requested` names no run, so it is found
            // only from this end: the process died before the child's run was
            // attached, or before a refusal to prepare one was written.
            self.close_abandoned_delegations(run.id).await?;
            // Whatever the tools still hold for it is released, as the loop
            // would have released it had the run ended normally.
            self.registry.end_run(run.id).await;
        }

        let expired = self
            .database
            .approvals()
            .expire_for_finished_runs(
                "the run that asked had ended, so nobody could act on an answer; most often \
                 the process exited while this request was waiting",
            )
            .await?;
        if expired > 0 {
            tracing::warn!(expired, "closed approval requests left by runs that ended");
        }

        Ok(count)
    }

    /// End every delegation an abandoned run left open.
    ///
    /// A child run that has itself ended keeps its ending: the work was done
    /// or stopped, whatever became of the run that asked for it. Every other
    /// edge failed, since no run is left to carry it out.
    async fn close_abandoned_delegations(&self, parent: TaskRunId) -> Result<(), RuntimeError> {
        let delegations = self.database.delegations();
        for edge in delegations.list_open_for_parent_run(parent).await? {
            let child = match edge.child_run_id {
                Some(id) => self.database.runs().find(id).await?,
                None => None,
            };
            let (state, steps) = match &child {
                Some(run) => (
                    match run.state {
                        TaskState::Completed => DelegationState::Completed,
                        TaskState::Cancelled => DelegationState::Cancelled,
                        _ => DelegationState::Failed,
                    },
                    run.steps_taken,
                ),
                None => (DelegationState::Failed, 0),
            };
            delegations.complete(edge.id, state, steps).await?;
        }
        Ok(())
    }
}
