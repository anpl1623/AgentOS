//! What a run's delegations inherit from it, and the broker that holds that
//! for every run able to delegate.

use std::collections::HashMap;
use std::sync::atomic::AtomicU16;
use std::sync::{Arc, Mutex, PoisonError};

use agentos_core::agent::Agent;
use agentos_core::delegation::DelegationRefusal;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_permissions::PermissionEngine;
use agentos_tools::{ApprovalGate, TaintTracker};
use tokio_util::sync::CancellationToken;

use crate::Runtime;
use crate::budget::RunBudget;
use crate::config::DelegationLimits;

/// Everything a run's delegations inherit from it.
///
/// Registered with the [`DelegationBroker`] for every run as it is prepared,
/// root and delegated alike, so that any run whose policy permits it can
/// delegate under the same limits.
pub struct ParentRunHandle {
    /// The runtime that prepared the run, with the registry and providers it
    /// was prepared with; its children are prepared by the same one.
    pub runtime: Runtime,
    /// The agent performing the run.
    pub agent: Agent,
    /// The run.
    pub run_id: TaskRunId,
    /// The run's task, which a child's task names as its parent.
    pub task_id: TaskId,
    /// The engine the run is evaluated by: its own policy for a root run, and
    /// the intersection of every policy on its chain for a delegated one.
    pub engine: Arc<dyn PermissionEngine>,
    /// The operator's gate, as the run was given it, before the run wrapped it.
    ///
    /// Not the run's own `RunApprovalGate`: wrapping that again would drive
    /// the parent's state machine from a child's approval.
    pub approvals: Arc<dyn ApprovalGate>,
    /// The approval budget the run is held to, which bounds its children's.
    pub approval_budget: Option<u32>,
    /// The taint tracker, shared with every run in the tree.
    pub taint: Arc<TaintTracker>,
    /// The step budget, shared with every run in the tree.
    pub budget: Arc<RunBudget>,
    /// The token that stops the run; a child's token descends from it.
    pub cancel: CancellationToken,
    /// How many levels below its root the run sits; zero for a root run.
    pub depth: u8,
    /// The agents from the root run to this one, this one last.
    pub ancestry: Vec<AgentId>,
    /// The same agents by name, for refusals and approval cards.
    pub chain: Vec<String>,
    /// How many children the run has started, over its whole life.
    pub children_started: AtomicU16,
}

impl std::fmt::Debug for ParentRunHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Not the runtime: it holds the broker that holds this handle.
        f.debug_struct("ParentRunHandle")
            .field("agent", &self.agent.name)
            .field("run", &self.run_id)
            .field("depth", &self.depth)
            .field("chain", &self.chain)
            .field("children_started", &self.children_started)
            .finish_non_exhaustive()
    }
}

impl ParentRunHandle {
    /// Record an event in this run's trail.
    pub(super) async fn record(
        &self,
        payload: AgentEvent,
    ) -> Result<(), agentos_audit::AuditError> {
        self.runtime
            .audit()
            .record(
                Event::new(payload)
                    .for_agent(self.agent.id)
                    .for_task(self.task_id)
                    .for_run(self.run_id),
            )
            .await
            .map(|_| ())
    }

    /// Record a refusal, which is security-relevant, without letting a log
    /// that cannot take it turn the refusal into anything else.
    pub(super) async fn record_refusal(&self, child_agent: &str, refusal: &DelegationRefusal) {
        if let Err(error) = self
            .record(AgentEvent::DelegationRefused {
                child_agent: child_agent.to_owned(),
                reason: refusal.to_string(),
            })
            .await
        {
            tracing::error!(%error, "failed to record a refused delegation");
        }
    }
}

/// The runs that may delegate, by run.
///
/// The runtime registers every run as it is prepared and forgets it as it
/// ends; [`DelegateTool`] looks up the run a call belongs to here. A call from
/// a run that is not registered is refused, so a registry composed without a
/// runtime's broker offers the tool and can perform no delegation with it.
///
/// [`DelegateTool`]: super::DelegateTool
pub struct DelegationBroker {
    runs: Mutex<HashMap<TaskRunId, Arc<ParentRunHandle>>>,
    limits: DelegationLimits,
}

impl std::fmt::Debug for DelegationBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Not the handles: each holds a runtime, which holds this broker.
        f.debug_struct("DelegationBroker")
            .field("limits", &self.limits)
            .field("runs", &self.live())
            .finish()
    }
}

impl DelegationBroker {
    /// An empty broker enforcing `limits`.
    #[must_use]
    pub fn new(limits: DelegationLimits) -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            limits,
        }
    }

    /// The limits it enforces.
    #[must_use]
    pub const fn limits(&self) -> DelegationLimits {
        self.limits
    }

    /// Make a run able to delegate.
    pub fn register(&self, run_id: TaskRunId, handle: Arc<ParentRunHandle>) {
        self.lock().insert(run_id, handle);
    }

    /// The handle of a registered run.
    #[must_use]
    pub fn handle(&self, run_id: TaskRunId) -> Option<Arc<ParentRunHandle>> {
        self.lock().get(&run_id).cloned()
    }

    /// Forget a run that has ended.
    ///
    /// A handle holds a runtime, so one never forgotten would keep that
    /// runtime, and everything it holds, alive for the life of the process.
    pub fn forget(&self, run_id: TaskRunId) {
        self.lock().remove(&run_id);
    }

    /// How many runs are registered.
    #[must_use]
    pub fn live(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<TaskRunId, Arc<ParentRunHandle>>> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
