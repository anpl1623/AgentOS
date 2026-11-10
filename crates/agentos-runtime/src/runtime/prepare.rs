//! Assembling a run: claimed, written and ready, but not yet started.
//!
//! Every check that decides whether a run may start, and under what policy,
//! budget and taint, is made here, for a root run and a delegated one alike,
//! so that what a delegated run inherits is decided in one place. Kept apart
//! from driving the run, which is what lets a caller learn a run's identity
//! before it finishes.

use std::sync::Arc;

use agentos_core::agent::Agent;
use agentos_core::delegation::{Delegation, DelegationRefusal};
use agentos_core::ids::{TaskId, TaskRunId};
use agentos_core::task::{Task, TaskRun, TaskState};
use agentos_permissions::{ApprovalPolicy, IntersectionEngine, PermissionEngine};
use agentos_tools::{ApprovalGate, TaintTracker, ToolContext, ToolPipeline};
use tokio_util::sync::CancellationToken;

use super::run::LiveRun;
use super::{Runtime, engine_from};
use crate::budget::RunBudget;
use crate::credentials::SecretStoreResolver;
use crate::delegation::ParentRunHandle;
use crate::error::RuntimeError;
use crate::gate::RunApprovalGate;
use crate::state::RunStateMachine;

/// Why a task created by a delegation cannot be started on its own.
pub const DELEGATED_TASK: &str = "this task was delegated by another agent's run and is carried \
     out only as part of that run, under its policy as well as this agent's own, so it cannot be \
     started on its own; to try it again, start the work from the agent that delegated it";

impl Runtime {
    /// Everything a run needs, assembled but not yet started.
    ///
    /// The task is claimed, and its run written, in one step. Two clients can
    /// both have read a task as runnable — a scheduler here and another in a
    /// terminal, or a scheduler and a person pressing retry — and only the one
    /// whose claim lands may start it. The claim compares with the status in
    /// `task`, which is what the caller read: a task cancelled, run or given
    /// something to wait for since then is not started on the strength of the
    /// old read. A caller that loses gets [`RuntimeError::TaskAlreadyClaimed`]
    /// having written nothing: no run row, no audit record, no model request.
    /// Everything that can refuse for a reason of its own is checked first, so
    /// a refusal leaves the task as it was rather than claimed by nobody.
    ///
    /// A task created by a delegation is refused. It runs only through
    /// [`Self::prepare_delegated_run`], under every policy above it, on the
    /// tree's step budget and within its depth; started here it would be a
    /// root run held to its own policy alone, so retrying a failed child would
    /// grant what the delegation withheld. The parent link is checked as well
    /// as the delegation's row because the task is written first, and a task
    /// is given a parent by nothing but delegation.
    pub(super) async fn prepare_run(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<PreparedRun, RuntimeError> {
        if task.parent_task_id.is_some()
            || self
                .database
                .delegations()
                .find_by_child_task(task.id)
                .await?
                .is_some()
        {
            return Err(RuntimeError::Rejected(DELEGATED_TASK.to_owned()));
        }
        let agent = self.database.agents().get(task.agent_id).await?;
        self.prepare(task, agent, approvals, cancel, None).await
    }

    /// Assemble the run that carries out a delegation, as [`Self::start_task`]
    /// assembles any other, under everything its delegator is held to.
    ///
    /// The differences are exactly what delegation must not widen. The run is
    /// evaluated by an [`IntersectionEngine`] over the delegator's engine and
    /// the child's own policy, so it may do only what both permit, and since
    /// the delegator's engine is itself an intersection for a delegated run,
    /// a grandchild answers to every policy up to the root. It shares the
    /// delegator's taint tracker and step budget rather than starting its own,
    /// and its token is a child of the delegator's. Its approvals go to the
    /// operator's gate the delegator was given, wrapped afresh around the
    /// child's own state machine, naming the chain of agents that asked, and
    /// held to the stricter of the two policies' approval budgets. Its run is
    /// registered with the broker one level deeper, so that it can delegate in
    /// turn under the same limits.
    ///
    /// The delegation's task must already be written, as a child of the
    /// delegator's task and for `child_agent`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Delegation`] if the delegator may not delegate any
    /// deeper, [`RuntimeError::Rejected`] if the task is not the delegation's,
    /// and otherwise as [`Self::start_task`].
    pub async fn prepare_delegated_run(
        &self,
        parent: &ParentRunHandle,
        delegation: &Delegation,
        child_agent: Agent,
    ) -> Result<PreparedRun, RuntimeError> {
        let limit = self.config.delegation.max_depth;
        if parent.depth >= limit {
            return Err(DelegationRefusal::DepthExceeded { limit }.into());
        }
        let task = self.database.tasks().get(delegation.child_task_id).await?;
        if task.agent_id != child_agent.id
            || delegation.child_agent_id != child_agent.id
            || task.parent_task_id != Some(parent.task_id)
            || delegation.parent_run_id != parent.run_id
        {
            return Err(RuntimeError::Rejected(
                "that task was not created for this delegation".to_owned(),
            ));
        }
        self.prepare(
            &task,
            child_agent,
            parent.approvals.clone(),
            parent.cancel.child_token(),
            Some(parent),
        )
        .await
    }

    /// What [`Self::prepare_run`] and [`Self::prepare_delegated_run`] share.
    ///
    /// `parent` is the delegator, for a delegated run, and `None` for a root
    /// run. Every difference between the two is decided here, in one place.
    async fn prepare(
        &self,
        task: &Task,
        agent: Agent,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
        parent: Option<&ParentRunHandle>,
    ) -> Result<PreparedRun, RuntimeError> {
        if !agent.is_enabled() {
            return Err(RuntimeError::DisabledAgent(agent.name));
        }

        let provider = self.providers.build(&agent.name, &agent.model)?;
        // Before the run exists, so no reaper elsewhere can see it without
        // also seeing that this process is alive to drive it.
        if let Some(lock) = &self.runs_lock {
            lock.hold().await?;
        }
        // One compilation feeds both the engine and the gate, so the budget
        // a run is held to comes from the same document as its rules. An
        // agent with no policy is denied everything and never asks, so the
        // budget it gets is moot; it gets the default all the same.
        let policy = self.policy_for(agent.id).await?;
        let own_budget = policy
            .as_ref()
            .map_or_else(ApprovalPolicy::default, |policy| policy.approvals)
            .max_per_run;
        let own_engine = engine_from(policy);
        let (engine, approval_budget): (Arc<dyn PermissionEngine>, _) = match parent {
            None => (own_engine, own_budget),
            Some(parent) => (
                Arc::new(IntersectionEngine::new(vec![
                    parent.engine.clone(),
                    own_engine,
                ])),
                stricter_budget(parent.approval_budget, own_budget),
            ),
        };

        let workspace = self.config.workspace_for(&agent.name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let mut run = self.next_run(task.id).await?;
        if let Some(parent) = parent {
            // Written into the row from the start, so a delegated run is never
            // on record as clean for even the moment before its first turn.
            run.tainted |= parent.taint.is_tainted();
            for source in parent.taint.sources() {
                if !run.taint_sources.contains(&source) {
                    run.taint_sources.push(source);
                }
            }
        }
        // The claim and the run row are one write, so the task is never
        // running without a run for the reaper to find.
        if !self
            .database
            .tasks()
            .claim_for_run(task.status, &run)
            .await?
        {
            return Err(RuntimeError::TaskAlreadyClaimed(task.id));
        }

        self.running.lock().insert(run.id, cancel.clone());
        // From here on, however the run ends or is abandoned, it is forgotten.
        let live = LiveRun {
            runs: self.running.clone(),
            broker: self.broker.clone(),
            run_id: run.id,
        };

        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Idle,
            self.database.clone(),
            self.audit.clone(),
        ));

        let chain = match parent {
            None => vec![agent.name.clone()],
            Some(parent) => {
                let mut chain = parent.chain.clone();
                chain.push(agent.name.clone());
                chain
            }
        };
        let gate: Arc<dyn ApprovalGate> = Arc::new(
            RunApprovalGate::new(approvals.clone(), self.database.clone(), machine.clone())
                .with_budget(approval_budget)
                // A run nobody delegated to names no chain: the card already
                // says which agent is asking.
                .with_delegation_chain(if parent.is_some() {
                    chain.clone()
                } else {
                    Vec::new()
                }),
        );

        let pipeline = ToolPipeline::new(
            self.registry.clone(),
            engine.clone(),
            gate,
            self.audit.clone(),
        );

        // The store's own resolver. The pipeline narrows it, call by call, to
        // the credentials each authorised plan named, and keeps the ledger
        // that redaction and the audit record are made from.
        let context = ToolContext::new(agent.id, task.id, run.id, workspace)
            .with_credentials(Arc::new(SecretStoreResolver::new(self.secrets.clone())));

        let (taint, budget) = match parent {
            None => (
                Arc::new(TaintTracker::seeded(run.tainted, run.taint_sources.clone())),
                RunBudget::of(agent.max_steps),
            ),
            Some(parent) => (parent.taint.clone(), parent.budget.clone()),
        };

        let mut ancestry = parent.map_or_else(Vec::new, |parent| parent.ancestry.clone());
        ancestry.push(agent.id);
        self.broker.register(
            run.id,
            Arc::new(ParentRunHandle {
                runtime: self.clone(),
                agent: agent.clone(),
                run_id: run.id,
                task_id: task.id,
                engine,
                approvals,
                approval_budget,
                taint: taint.clone(),
                budget: budget.clone(),
                cancel: cancel.clone(),
                depth: parent.map_or(0, |parent| parent.depth.saturating_add(1)),
                ancestry,
                chain,
                children_started: std::sync::atomic::AtomicU16::new(0),
            }),
        );

        Ok(PreparedRun {
            objective: task.objective.clone(),
            agent,
            run,
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            delegated: parent.is_some(),
            cancel,
            live,
        })
    }

    /// Assemble the next attempt at a task, to be written with its claim.
    ///
    /// Read before the claim, so a caller that loses it may have read for
    /// nothing; it has written nothing, which is what matters.
    async fn next_run(&self, task_id: TaskId) -> Result<TaskRun, RuntimeError> {
        let attempt = self.database.runs().next_attempt(task_id).await?;
        let mut run = TaskRun::new(task_id, attempt);
        self.inherit_taint(&mut run).await?;
        Ok(run)
    }

    /// Carry the taint of every earlier attempt at a task into a new one.
    ///
    /// A retry is the same task over the same conversation, memory and
    /// workspace. Whatever an earlier attempt read from outside is still in
    /// reach, so a new attempt starting clean would let a single failure
    /// launder it. Every earlier attempt is consulted rather than only the
    /// latest, so that one attempt recorded without its sources cannot break
    /// the chain for all the attempts after it.
    async fn inherit_taint(&self, run: &mut TaskRun) -> Result<(), RuntimeError> {
        for earlier in self.database.runs().list_for_task(run.task_id).await? {
            run.tainted |= earlier.tainted;
            for source in earlier.taint_sources {
                if !run.taint_sources.contains(&source) {
                    run.taint_sources.push(source);
                }
            }
        }
        Ok(())
    }
}

/// The stricter of two approval budgets, where `None` is no limit.
const fn stricter_budget(first: Option<u32>, second: Option<u32>) -> Option<u32> {
    match (first, second) {
        (Some(first), Some(second)) => Some(if first < second { first } else { second }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// A run that has been assembled but not started.
///
/// Splitting assembly from execution is what lets a caller learn a run's
/// identity before it finishes — the row exists and the state machine is ready,
/// but no model has been called yet.
pub struct PreparedRun {
    pub(super) objective: String,
    pub(super) agent: Agent,
    pub(crate) run: TaskRun,
    pub(super) provider: agentos_providers::SharedProvider,
    pub(super) pipeline: ToolPipeline,
    pub(super) machine: Arc<RunStateMachine>,
    pub(super) context: ToolContext,
    /// The tracker the loop observes into, shared with the rest of the tree
    /// for a delegated run, so the loop does not build one of its own.
    pub(super) taint: Arc<TaintTracker>,
    pub(super) budget: Arc<RunBudget>,
    pub(super) delegated: bool,
    pub(crate) cancel: CancellationToken,
    /// Forgets the run as live when the run is done with, driven or not.
    pub(super) live: LiveRun,
}

impl PreparedRun {
    /// The run that was written.
    #[must_use]
    pub const fn run_id(&self) -> TaskRunId {
        self.run.id
    }

    /// The task it carries out.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.run.task_id
    }
}

impl std::fmt::Debug for PreparedRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRun")
            .field("agent", &self.agent.name)
            .field("run", &self.run.id)
            .finish_non_exhaustive()
    }
}
