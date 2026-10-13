//! Delegation: one agent asking another to do part of its work.
//!
//! An agent delegates by calling `agent.delegate`, a tool like any other: it
//! is planned, authorised against the policy and, where the policy says so,
//! put to a person before anything happens. The capability is
//! `runtime.delegate`, scoped to the name of the agent asked, so a policy
//! names which agents may be asked. What the agent asked may then do is not
//! that grant's to say.
//!
//! # What a child is given
//!
//! Nothing its delegator did not have. A child run is evaluated by the
//! intersection of its own policy and every policy above it, so it can do only
//! what all of them permit and the strictest answer governs each call. It
//! shares its delegator's taint tracker, so a child of a run that has read a
//! hostile page starts tainted and a page the child reads taints the parent.
//! It spends from the one step budget the root run was granted. Its approvals
//! go to the same operator gate its delegator's do, wrapped for the child's
//! own run and naming the chain of agents that asked, so under the
//! scheduler's deny-all gate a child is refused exactly what its parent would
//! be. Its cancellation token is a child of its delegator's, so stopping a run
//! stops everything it started.
//!
//! # Why it lives in the runtime
//!
//! The tool has to prepare and drive a run, which only [`Runtime`] can do, and
//! the runtime depends on the tools crate rather than the reverse. It is
//! registered with the other tools all the same, and like the browser tools it
//! holds a pool keyed by run, the [`DelegationBroker`], which the runtime fills
//! as each run is prepared and empties as each run ends.
//!
//! # Order of events
//!
//! Every refusal is decided while the call is planned, before authorisation
//! and before any side effect. Once authorised, each child is recorded before
//! it starts: its task, then its `delegations` row, then its run, and only
//! then the audit record that seals the edge between the two runs. A
//! delegation that cannot be recorded is not performed. A child that fails is
//! a result its delegator reads and re-plans around, exactly as it reads a
//! denied approval, never an error that ends the delegator's run.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use agentos_core::agent::Agent;
use agentos_core::delegation::{Delegation, DelegationRefusal, DelegationState};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, DelegationId, TaskId, TaskRunId};
use agentos_core::permission::{Capability, ResourceRef, permission_domains};
use agentos_core::risk::RiskLevel;
use agentos_core::task::{Task, TaskState, TaskStatus};
use agentos_core::tool::ToolMetadata;
use agentos_core::trust::{DataSource, UntrustedContent};
use agentos_permissions::PermissionEngine;
use agentos_tools::{
    ApprovalGate, TaintTracker, Tool, ToolContext, ToolError, ToolOutput, ToolPlan, metadata_for,
    parse_arguments,
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::budget::RunBudget;
use crate::config::DelegationLimits;
use crate::{PreparedRun, RunOutcome, Runtime, RuntimeError};

/// The tool's name.
pub const TOOL_NAME: &str = "agent.delegate";

/// The capability action a delegation needs, in the `runtime` domain.
pub const DELEGATE: &str = "delegate";

/// How much of an objective the one-line summary on an approval card quotes.
const SUMMARY_OBJECTIVE_CHARS: usize = 80;

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
    async fn record(&self, payload: AgentEvent) -> Result<(), agentos_audit::AuditError> {
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
    async fn record_refusal(&self, child_agent: &str, refusal: &DelegationRefusal) {
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

/// Arguments to `agent.delegate`.
///
/// A list rather than one request, so that fanning out and gathering the
/// results back in happen inside one parent run, where taint, approvals and
/// cancellation already reach. Work handed to the scheduler instead would run
/// later, unattended, and clean of everything the parent had read.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// The agents to ask, and what to ask each. They work at the same time,
    /// and each one's report comes back in this call's result.
    pub requests: Vec<DelegateRequest>,
}

/// One agent asked to carry out one objective.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateRequest {
    /// The name of the agent to ask.
    pub agent: String,
    /// What it should do, stated completely: it sees nothing of this
    /// conversation but this.
    pub objective: String,
}

/// Asks other agents to carry out objectives, and reports what they did.
#[derive(Debug)]
pub struct DelegateTool {
    broker: Arc<DelegationBroker>,
    metadata: ToolMetadata,
}

impl DelegateTool {
    /// The tool, performing delegations for the runs `broker` holds.
    #[must_use]
    pub fn new(broker: Arc<DelegationBroker>) -> Self {
        Self {
            broker,
            metadata: metadata_for::<DelegateArgs>(
                TOOL_NAME,
                "Ask other agents to carry out objectives and return their reports. Each agent \
                 works under its own policy and yours together, shares your step budget, and \
                 reports back when it finishes or fails.",
                // It causes another agent to act in the world.
                RiskLevel::High,
                vec![Capability::new(permission_domains::RUNTIME, DELEGATE)],
                true,
            ),
        }
    }

    /// The handle for the run a call belongs to.
    fn handle_for(&self, context: &ToolContext) -> Result<Arc<ParentRunHandle>, ToolError> {
        // Every run the runtime prepares is registered, so a call from one
        // that is not is a fault in the runtime, not a permitted call.
        self.broker.handle(context.run_id).ok_or_else(|| {
            ToolError::Failed(
                "this run cannot delegate: the runtime did not register it with this tool"
                    .to_owned(),
            )
        })
    }

    /// Whether a run may make these requests at all, before looking at whom
    /// they ask.
    fn refuse_shape(
        &self,
        handle: &ParentRunHandle,
        requests: &[DelegateRequest],
    ) -> Option<DelegationRefusal> {
        let limits = self.broker.limits();
        let fan_out = DelegationRefusal::FanOutExceeded {
            limit: limits.max_children_per_run,
        };
        let wanted = u16::try_from(requests.len()).unwrap_or(u16::MAX);
        if requests.is_empty() || wanted > limits.max_children_per_run {
            return Some(fan_out);
        }
        let started = handle.children_started.load(Ordering::SeqCst);
        if started.saturating_add(wanted) > limits.max_children_per_run {
            return Some(fan_out);
        }
        if handle.depth >= limits.max_depth {
            return Some(DelegationRefusal::DepthExceeded {
                limit: limits.max_depth,
            });
        }
        let remaining = handle.budget.remaining();
        if remaining == 0 {
            return Some(DelegationRefusal::StepBudgetExhausted { remaining });
        }
        None
    }

    /// Plan the call, recording any refusal in the parent's trail.
    async fn plan_for(
        &self,
        handle: &ParentRunHandle,
        requests: &[DelegateRequest],
    ) -> Result<ToolPlan, ToolError> {
        if let Some(refusal) = self.refuse_shape(handle, requests) {
            let asked = requests
                .iter()
                .map(|request| request.agent.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            handle.record_refusal(&asked, &refusal).await;
            return Err(denied(&refusal));
        }

        let mut summaries = Vec::with_capacity(requests.len());
        let mut capabilities = Vec::with_capacity(requests.len());
        for request in requests {
            let child = match vet(handle, &request.agent).await {
                Ok(child) => child,
                Err(Vetted::Refused(refusal)) => {
                    handle.record_refusal(&request.agent, &refusal).await;
                    return Err(denied(&refusal));
                }
                Err(Vetted::Unreadable(error)) => {
                    return Err(ToolError::Failed(format!(
                        "the agents could not be read: {error}"
                    )));
                }
            };
            summaries.push(format!(
                "Ask {} to: {}",
                child.name,
                shorten(&request.objective, SUMMARY_OBJECTIVE_CHARS)
            ));
            capabilities.push(
                Capability::new(permission_domains::RUNTIME, DELEGATE)
                    .with_resource(ResourceRef::Named { name: child.name }),
            );
        }

        // One capability per child, so the policy is asked about each, and
        // the strictest answer governs the whole call.
        Ok(capabilities.into_iter().fold(
            ToolPlan::new(RiskLevel::High, summaries.join("; ")),
            ToolPlan::requiring,
        ))
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: DelegateArgs = parse_arguments(TOOL_NAME, arguments)?;
        Ok(arguments.clone())
    }

    /// Decide every refusal, before authorisation and before any side effect.
    ///
    /// Reading the agents table is looking at the world, which planning may
    /// do; the only thing written is the audit record of a refusal.
    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: DelegateArgs = parse_arguments(TOOL_NAME, arguments)?;
        let handle = self.handle_for(context)?;
        self.plan_for(&handle, &args.requests).await
    }

    /// None. A delegation lasts as long as its children's runs, each of
    /// whose model requests and tool calls is limited on its own and any of
    /// which may wait on a person; the shared step budget bounds how much
    /// work there is, and the parent's token stops it.
    fn time_limit(&self, _context: &ToolContext) -> Option<std::time::Duration> {
        None
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: DelegateArgs = parse_arguments(TOOL_NAME, &arguments)?;
        let handle = self.handle_for(context)?;

        // The plan checked the count; this takes the slots, atomically, so
        // that nothing between the plan and here can spend them twice.
        let limit = self.broker.limits().max_children_per_run;
        let wanted = u16::try_from(args.requests.len()).unwrap_or(u16::MAX);
        if handle
            .children_started
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |started| {
                started.checked_add(wanted).filter(|total| *total <= limit)
            })
            .is_err()
        {
            return Err(denied(&DelegationRefusal::FanOutExceeded { limit }));
        }

        let mut children = Vec::with_capacity(args.requests.len());
        let mut guards = Vec::with_capacity(args.requests.len());
        for request in args.requests {
            if cancel.is_cancelled() {
                break;
            }
            let child = start(&handle, request).await;
            if let Child::Started { cancel, .. } = &child {
                // This call has no time limit, but it can still be dropped
                // before it returns, as the parent's run is torn down. Each
                // child is driven on a task of its own and records its own
                // ending, so dropping the call cannot orphan its bookkeeping;
                // these make sure it stops the children too, rather than
                // leaving them working for a parent that has gone.
                guards.push(cancel.clone().drop_guard());
            }
            children.push(child);
        }

        let mut reports = Vec::with_capacity(children.len());
        for child in children {
            reports.push(child.finish().await);
        }
        for guard in guards {
            guard.disarm();
        }

        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(output(&reports))
    }

    async fn end_run(&self, run_id: TaskRunId) {
        self.broker.forget(run_id);
    }
}

/// Why an agent could not be asked.
enum Vetted {
    /// It may not be asked.
    Refused(DelegationRefusal),
    /// The agents could not be read.
    Unreadable(agentos_persistence::DbError),
}

/// Resolve an agent a run asked for, refusing one it may not ask.
async fn vet(handle: &ParentRunHandle, name: &str) -> Result<Agent, Vetted> {
    let child = handle
        .runtime
        .database()
        .agents()
        .find_by_name(name)
        .await
        .map_err(Vetted::Unreadable)?
        .ok_or_else(|| {
            Vetted::Refused(DelegationRefusal::UnknownAgent {
                name: name.to_owned(),
            })
        })?;
    if !child.is_enabled() {
        return Err(Vetted::Refused(DelegationRefusal::DisabledAgent {
            name: child.name,
        }));
    }
    // The run's own agent is the last of its ancestry, so asking oneself is
    // the shortest cycle and is caught here with the rest.
    if handle.ancestry.contains(&child.id) {
        let mut path = handle.chain.clone();
        path.push(child.name);
        return Err(Vetted::Refused(DelegationRefusal::AgencyCycle { path }));
    }
    Ok(child)
}

/// One child of a call: started, or not.
enum Child {
    /// Running on a task of its own, which records how it ends.
    Started {
        handle: tokio::task::JoinHandle<Report>,
        cancel: CancellationToken,
        /// What to report if the task itself is lost.
        fallback: Report,
    },
    /// Never started; already recorded as far as it got.
    NotStarted(Report),
}

impl Child {
    async fn finish(self) -> Report {
        match self {
            Self::NotStarted(report) => report,
            Self::Started {
                handle, fallback, ..
            } => match handle.await {
                Ok(report) => report,
                Err(error) => Report {
                    state: DelegationState::Failed,
                    failure: Some(format!("the run stopped without finishing: {error}")),
                    ..fallback
                },
            },
        }
    }
}

/// What a supervisor reads about one child.
#[derive(Debug, Clone)]
struct Report {
    delegation_id: Option<DelegationId>,
    agent: String,
    child_task_id: Option<TaskId>,
    child_run_id: Option<TaskRunId>,
    state: DelegationState,
    steps: u32,
    tainted: bool,
    result: Option<String>,
    failure: Option<String>,
}

impl Report {
    fn not_started(agent: &str, failure: impl Into<String>) -> Self {
        Self {
            delegation_id: None,
            agent: agent.to_owned(),
            child_task_id: None,
            child_run_id: None,
            state: DelegationState::Refused,
            steps: 0,
            tainted: false,
            result: None,
            failure: Some(failure.into()),
        }
    }

    /// The text the supervising model reads.
    fn text(&self) -> String {
        let why = self.failure.as_deref().unwrap_or("no reason was recorded");
        match self.state {
            DelegationState::Completed => self
                .result
                .clone()
                .unwrap_or_else(|| format!("{} finished without a report.", self.agent)),
            DelegationState::Cancelled => {
                format!("{} was cancelled before it finished.", self.agent)
            }
            DelegationState::Refused | DelegationState::Requested => {
                format!("{} was not started: {why}", self.agent)
            }
            DelegationState::Failed | DelegationState::Running => {
                format!("{} failed: {why}", self.agent)
            }
        }
    }

    fn structured(&self) -> serde_json::Value {
        serde_json::json!({
            "delegation_id": self.delegation_id.map(|id| id.to_string()),
            "agent": self.agent,
            "child_task_id": self.child_task_id.map(|id| id.to_string()),
            "child_run_id": self.child_run_id.map(|id| id.to_string()),
            "state": self.state.as_str(),
            "steps": self.steps,
            "tainted": self.tainted,
            "result": self.result,
            "failure": self.failure,
        })
    }
}

/// Record and start one child, or record why it did not start.
///
/// Nothing a child does happens before its task, its `delegations` row and
/// its run exist, and the audit record naming both runs has been written.
async fn start(handle: &Arc<ParentRunHandle>, request: DelegateRequest) -> Child {
    let runtime = &handle.runtime;
    let database = runtime.database();

    // Again, because the agents table may have changed since the plan.
    let child_agent = match vet(handle, &request.agent).await {
        Ok(agent) => agent,
        Err(Vetted::Refused(refusal)) => {
            handle.record_refusal(&request.agent, &refusal).await;
            return Child::NotStarted(Report::not_started(&request.agent, refusal.to_string()));
        }
        Err(Vetted::Unreadable(error)) => {
            return Child::NotStarted(Report::not_started(
                &request.agent,
                format!("the agents could not be read: {error}"),
            ));
        }
    };

    // Born cancelled, and claimed from there with its run. A task nobody has
    // run is `pending`, and a scheduler starts pending tasks: one that read
    // this task in the moment before its run was written would carry it out
    // unattended, under the child's own policy alone, outside the
    // intersection. No scheduler starts a cancelled task, and a run may still
    // be claimed from one.
    let mut task = Task::new(child_agent.id, &request.objective).with_parent(handle.task_id);
    task.status = TaskStatus::Cancelled;
    if let Err(error) = database.tasks().insert(&task).await {
        tracing::error!(%error, "failed to record a delegated task");
        return Child::NotStarted(Report::not_started(
            &child_agent.name,
            "the delegation could not be recorded, so it was not performed",
        ));
    }

    let delegation = Delegation::requested(
        handle.run_id,
        handle.agent.id,
        child_agent.id,
        task.id,
        request.objective,
        handle.depth.saturating_add(1),
    );
    if let Err(error) = database.delegations().insert(&delegation).await {
        tracing::error!(%error, "failed to record a delegation");
        return Child::NotStarted(Report {
            child_task_id: Some(task.id),
            ..Report::not_started(
                &child_agent.name,
                "the delegation could not be recorded, so it was not performed",
            )
        });
    }

    let mut report = Report {
        delegation_id: Some(delegation.id),
        agent: child_agent.name.clone(),
        child_task_id: Some(task.id),
        child_run_id: None,
        state: DelegationState::Requested,
        steps: 0,
        tainted: false,
        result: None,
        failure: None,
    };

    let prepared = match runtime
        .prepare_delegated_run(handle, &delegation, child_agent)
        .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            report.state = match error {
                RuntimeError::Delegation(_)
                | RuntimeError::DisabledAgent(_)
                | RuntimeError::UnknownAgent(_) => DelegationState::Refused,
                _ => DelegationState::Failed,
            };
            report.failure = Some(format!("its run could not be started: {error}"));
            close(handle, delegation.id, report.state, 0).await;
            return Child::NotStarted(report);
        }
    };
    let child_run_id = prepared.run.id;
    report.child_run_id = Some(child_run_id);

    let sealed = match database
        .delegations()
        .attach_run(delegation.id, child_run_id)
        .await
    {
        Ok(()) => handle
            .record(AgentEvent::AgentDelegated {
                delegation_id: delegation.id,
                parent_run_id: handle.run_id,
                child_agent: report.agent.clone(),
                child_task_id: task.id,
                child_run_id,
                objective: delegation.objective.clone(),
                depth: delegation.depth,
            })
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    if let Err(error) = sealed {
        // The run exists and is claimed, so it is ended properly rather than
        // abandoned: driven under a cancelled token, which stops it before it
        // asks a model anything.
        tracing::error!(%error, "a delegation could not be sealed; stopping its run unstarted");
        prepared.cancel.cancel();
        let _ = runtime.drive(prepared).await;
        report.state = DelegationState::Failed;
        report.failure = Some(
            "the delegation could not be recorded, so its run was stopped before it began"
                .to_owned(),
        );
        close(handle, delegation.id, report.state, 0).await;
        return Child::NotStarted(report);
    }

    let cancel = prepared.cancel.clone();
    let fallback = report.clone();
    let parent = Arc::clone(handle);
    let delegation_id = delegation.id;
    let task =
        tokio::spawn(async move { conclude(&parent, delegation_id, prepared, report).await });
    Child::Started {
        handle: task,
        cancel,
        fallback,
    }
}

/// Drive a child to its end and record how it ended.
async fn conclude(
    parent: &ParentRunHandle,
    delegation_id: DelegationId,
    prepared: PreparedRun,
    mut report: Report,
) -> Report {
    let run_id = prepared.run.id;
    let (state, run_state) = match parent.runtime.drive(prepared).await {
        Ok(outcome) => {
            let state = settled(&outcome);
            report.steps = outcome.steps;
            report.tainted = outcome.tainted;
            report.result = outcome.result;
            report.failure = outcome.failure.map(|failure| failure.to_string());
            (state, outcome.state)
        }
        Err(error) => {
            report.failure = Some(format!("the runtime failed while it worked: {error}"));
            (DelegationState::Failed, TaskState::Failed)
        }
    };
    report.state = state;
    if !report.tainted {
        // The tracker is shared: whatever the child read, or the parent had
        // read before it, is on it.
        report.tainted = parent.taint.is_tainted();
    }

    close(parent, delegation_id, state, report.steps).await;
    if let Err(error) = parent
        .record(AgentEvent::DelegationCompleted {
            delegation_id,
            child_run_id: run_id,
            state: run_state,
            steps: report.steps,
            tainted: report.tainted,
        })
        .await
    {
        tracing::error!(%error, "failed to record a delegation ending");
    }
    report
}

/// The delegation state a child's outcome amounts to.
const fn settled(outcome: &RunOutcome) -> DelegationState {
    match outcome.state {
        TaskState::Completed => DelegationState::Completed,
        TaskState::Cancelled => DelegationState::Cancelled,
        _ => DelegationState::Failed,
    }
}

/// Mark a delegation finished, saying so in the log if the row will not take it.
async fn close(handle: &ParentRunHandle, id: DelegationId, state: DelegationState, steps: u32) {
    if let Err(error) = handle
        .runtime
        .database()
        .delegations()
        .complete(id, state, steps)
        .await
    {
        tracing::error!(%error, delegation = %id, "failed to record a delegation's end");
    }
}

/// The call's result: each child's report, untrusted and labelled with the
/// agent that wrote it.
///
/// With several children each report is sealed in its own envelope, so that
/// one child's prose cannot pass itself off as the end of another's.
fn output(reports: &[Report]) -> ToolOutput {
    let structured = serde_json::Value::Array(reports.iter().map(Report::structured).collect());
    let output = match reports {
        [only] => ToolOutput::text(
            DataSource::Agent {
                agent: only.agent.clone(),
            },
            only.text(),
        ),
        _ => {
            let body = reports
                .iter()
                .map(|report| {
                    UntrustedContent::new(
                        DataSource::Agent {
                            agent: report.agent.clone(),
                        },
                        report.text(),
                    )
                    .render()
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let agents = reports
                .iter()
                .map(|report| report.agent.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ToolOutput::text(DataSource::Agent { agent: agents }, body)
        }
    };
    output.with_structured(structured)
}

/// The refusal as the model reads it.
fn denied(refusal: &DelegationRefusal) -> ToolError {
    ToolError::Denied {
        reason: refusal.to_string(),
    }
}

/// At most `max` characters of `text`, marked when cut.
fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        None => text.to_owned(),
        Some((cut, _)) => format!("{}…", &text[..cut]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(agent: &str, state: DelegationState) -> Report {
        Report {
            state,
            ..Report::not_started(agent, "step budget of 4 exhausted")
        }
    }

    #[test]
    fn a_failed_child_reads_as_a_result_naming_why() {
        let text = report("researcher", DelegationState::Failed).text();
        assert_eq!(text, "researcher failed: step budget of 4 exhausted");
    }

    #[test]
    fn one_report_is_labelled_with_its_agent() {
        let mut done = report("researcher", DelegationState::Completed);
        done.result = Some("Churn was 4%.".into());
        let output = output(&[done]);
        assert_eq!(
            output.content.source,
            DataSource::Agent {
                agent: "researcher".into()
            }
        );
        assert_eq!(output.content.body, "Churn was 4%.");
        let structured = output.structured.unwrap();
        assert_eq!(structured[0]["state"], "completed");
        assert_eq!(structured[0]["agent"], "researcher");
    }

    #[test]
    fn several_reports_are_sealed_apart() {
        let output = output(&[
            report("researcher", DelegationState::Failed),
            report("summariser", DelegationState::Cancelled),
        ]);
        assert!(output.content.source.is_externally_influenced());
        assert!(output.content.body.contains("source=\"agent:researcher\""));
        assert!(output.content.body.contains("source=\"agent:summariser\""));
        assert_eq!(output.structured.unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_summary_quotes_a_long_objective_only_in_part() {
        let long = "x".repeat(200);
        let short = shorten(&long, SUMMARY_OBJECTIVE_CHARS);
        assert_eq!(short.chars().count(), SUMMARY_OBJECTIVE_CHARS + 1);
        assert_eq!(shorten("  brief  ", SUMMARY_OBJECTIVE_CHARS), "brief");
    }

    #[test]
    fn the_tool_declares_one_capability_and_untrusted_output() {
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        let metadata = tool.metadata();
        assert_eq!(metadata.name, TOOL_NAME);
        assert_eq!(metadata.risk, RiskLevel::High);
        assert!(metadata.returns_untrusted_data);
        assert_eq!(metadata.capability_names(), vec!["runtime.delegate"]);
    }

    #[test]
    fn a_delegation_is_not_cut_off_at_the_per_call_timeout() {
        // A supervisor's call lasts as long as its workers do, which is
        // routinely longer than any one tool call is allowed.
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        let context = ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            std::env::temp_dir(),
        );
        assert_eq!(tool.time_limit(&context), None);
    }

    #[test]
    fn arguments_carry_nothing_the_tool_does_not_expect() {
        let tool = DelegateTool::new(Arc::new(DelegationBroker::new(DelegationLimits::default())));
        assert!(
            tool.validate(&serde_json::json!({
                "requests": [{"agent": "worker", "objective": "o"}]
            }))
            .is_ok()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "requests": [{"agent": "worker", "objective": "o", "policy": "allow"}]
            }))
            .is_err()
        );
    }
}
