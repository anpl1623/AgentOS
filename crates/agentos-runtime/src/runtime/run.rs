//! Driving a run, and stopping one.
//!
//! A prepared run is driven to a terminal state here, by its caller or on a
//! task of its own. The runs this process is driving are tracked here too,
//! each with the token that stops it, from the moment its claim lands until
//! it is done with, however it ends.

use std::sync::Arc;

use agentos_core::ids::{AgentId, TaskRunId};
use agentos_core::task::Task;
use agentos_tools::ApprovalGate;
use tokio_util::sync::CancellationToken;

use super::{PreparedRun, Runtime};
use crate::agent_loop::{AgentLoop, RunOutcome};
use crate::delegation::DelegationBroker;
use crate::error::RuntimeError;
use crate::liveness;

impl Runtime {
    /// Execute a task and return what it produced.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] for runtime failures. A task that merely fails returns
    /// `Ok` with a failed [`RunOutcome`].
    pub async fn run_task(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<RunOutcome, RuntimeError> {
        let prepared = self.prepare_run(task, approvals, cancel).await?;
        self.drive(prepared).await
    }

    /// Begin a run and hand back its identity immediately.
    ///
    /// The run proceeds in the background. A user interface needs this: it has
    /// to show the trace of a run that may take minutes, so it cannot wait for
    /// the run to finish before learning what to show.
    ///
    /// The returned handle resolves to the same outcome [`Self::run_task`]
    /// would produce. Dropping it does not stop the run — use
    /// [`Self::cancel_run`], which is what the operator's stop button does.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] if the run cannot be started. Once started, failures
    /// are reported through the outcome rather than here.
    pub async fn start_task(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<
        (
            TaskRunId,
            tokio::task::JoinHandle<Result<RunOutcome, RuntimeError>>,
        ),
        RuntimeError,
    > {
        let prepared = self.prepare_run(task, approvals, cancel).await?;
        let run_id = prepared.run.id;
        let runtime = self.clone();
        let handle = tokio::spawn(async move { runtime.drive(prepared).await });
        Ok((run_id, handle))
    }

    /// Drive a prepared run to a terminal state.
    ///
    /// The run is forgotten as live, and as able to delegate, however it
    /// ends: by the guard the prepared run carries, which goes when this
    /// returns or when this future is dropped unfinished. One that ended in a
    /// runtime error, or whose driver was dropped, is not being driven by
    /// anyone, and left in the live set it would hold a scheduler slot, be
    /// spared by the reaper and keep its runtime alive in the broker, for the
    /// life of the process.
    pub(crate) async fn drive(&self, prepared: PreparedRun) -> Result<RunOutcome, RuntimeError> {
        let PreparedRun {
            objective,
            agent,
            run,
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            delegated,
            cancel,
            live,
        } = prepared;
        let mut agent_loop = AgentLoop::new(
            agent,
            run,
            self.database.clone(),
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            cancel,
        );
        if delegated {
            agent_loop = agent_loop.delegated();
        }

        let outcome = agent_loop.run(&objective).await;
        drop(live);
        outcome
    }

    /// Create and immediately execute a task.
    ///
    /// # Errors
    ///
    /// As [`Self::run_task`].
    pub async fn run_objective(
        &self,
        agent_id: AgentId,
        objective: &str,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<RunOutcome, RuntimeError> {
        let task = self.create_task(agent_id, objective, &[], None).await?;
        self.run_task(&task, approvals, cancel).await
    }

    /// Stop a run that is currently executing.
    ///
    /// Returns whether a live run was found. The operator must always be able
    /// to stop an agent, so this cancels the token the loop and every tool
    /// inside it are watching. Every run it delegated to watches a token
    /// descended from that one, so they stop with it.
    ///
    /// The run stays live until it has actually stopped, so that what counts
    /// live runs, the scheduler among them, does not count a run that is
    /// still winding down as already gone.
    pub async fn cancel_run(&self, run_id: TaskRunId) -> bool {
        match self.running.lock().get(&run_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Take this data directory's scheduler lease, for as long as the
    /// returned handle is held.
    ///
    /// `None` for an in-memory runtime, whose database no other process can
    /// see and which therefore has no other scheduler to exclude.
    pub(crate) fn scheduler_lease(&self) -> Result<liveness::SchedulerLease, RuntimeError> {
        if self.runs_lock.is_none() {
            return Ok(liveness::SchedulerLease { _held: None });
        }
        liveness::take_scheduler_lease(&self.config.data_dir)
            .map(|file| liveness::SchedulerLease { _held: Some(file) })
    }

    /// Run identifiers currently executing.
    ///
    /// Every live run, delegated runs among them: a run that has asked three
    /// others to work is four runs using a model at once, and anything that
    /// limits concurrency must see four.
    pub async fn running_runs(&self) -> Vec<TaskRunId> {
        self.running.lock().keys().copied().collect()
    }
}

/// The runs this process is driving, each with the token that stops it.
///
/// Behind a plain mutex, held only for a lookup and never across an await,
/// so that a run can be forgotten from a destructor.
#[derive(Debug, Default)]
pub(super) struct LiveRuns(
    std::sync::Mutex<std::collections::HashMap<TaskRunId, CancellationToken>>,
);

impl LiveRuns {
    pub(super) fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<TaskRunId, CancellationToken>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Forgets a run as live, and as able to delegate, when dropped.
///
/// Made as the run is claimed and carried by it until `drive` returns, so the
/// forgetting cannot be skipped: not by a run that fails to start, nor by a
/// driver dropped part way, as a tool call is when its run is torn down.
pub(super) struct LiveRun {
    pub(super) runs: Arc<LiveRuns>,
    pub(super) broker: Arc<DelegationBroker>,
    pub(super) run_id: TaskRunId,
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        self.runs.lock().remove(&self.run_id);
        // The tool does this as the run releases its tools, but a registry
        // swapped in without `agent.delegate` never tells it to.
        self.broker.forget(self.run_id);
    }
}
