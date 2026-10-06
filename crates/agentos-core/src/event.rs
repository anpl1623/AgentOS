//! The observability event taxonomy.
//!
//! Every meaningful thing the runtime does emits a structured event. Text logs
//! are a rendering of these, never the source of truth: the audit log, the
//! desktop activity feed and the CLI trace all read the same stream.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, ApprovalId, EventId, MemoryId, TaskId, TaskRunId, ToolExecutionId};
use crate::permission::{Capability, Effect};
use crate::risk::RiskLevel;
use crate::task::{TaskState, TaskTrigger};
use crate::trust::DataSource;

/// What happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A run began.
    #[serde(rename = "agent.task.started")]
    TaskStarted {
        /// The objective being attempted.
        objective: String,
        /// Attempt number.
        attempt: u32,
    },

    /// A run finished successfully.
    #[serde(rename = "agent.task.completed")]
    TaskCompleted {
        /// Model turns consumed.
        steps: u32,
        /// Wall-clock duration.
        duration_ms: u64,
    },

    /// A run failed.
    #[serde(rename = "agent.task.failed")]
    TaskFailed {
        /// Why.
        reason: String,
        /// Model turns consumed.
        steps: u32,
    },

    /// A run was stopped by the operator.
    #[serde(rename = "agent.task.cancelled")]
    TaskCancelled {
        /// Model turns consumed before stopping.
        steps: u32,
    },

    /// The state machine moved.
    #[serde(rename = "agent.state.transitioned")]
    StateTransitioned {
        /// Previous state.
        from: TaskState,
        /// New state.
        to: TaskState,
        /// What caused it.
        trigger: TaskTrigger,
    },

    /// A model turn began.
    #[serde(rename = "agent.model.request.started")]
    ModelRequestStarted {
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Messages sent.
        message_count: usize,
        /// Tools advertised.
        tool_count: usize,
    },

    /// A model turn finished.
    #[serde(rename = "agent.model.request.completed")]
    ModelRequestCompleted {
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Latency.
        duration_ms: u64,
        /// Input tokens, when reported.
        input_tokens: Option<u64>,
        /// Output tokens, when reported.
        output_tokens: Option<u64>,
        /// Tool calls the model requested.
        tool_calls: usize,
    },

    /// A model turn errored.
    #[serde(rename = "agent.model.request.failed")]
    ModelRequestFailed {
        /// Provider identifier.
        provider: String,
        /// Detail.
        error: String,
    },

    /// The model finished reasoning about what to do next.
    #[serde(rename = "agent.reasoning.completed")]
    ReasoningCompleted {
        /// The model's prose, truncated for the log.
        summary: String,
        /// Tool calls it decided on.
        tool_calls: usize,
    },

    /// The policy engine was consulted.
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        /// The tool.
        tool: String,
        /// The capability requested.
        capability: Capability,
        /// Assessed risk.
        risk: RiskLevel,
        /// Whether the run was tainted at the time.
        tainted: bool,
    },

    /// The policy engine allowed an action.
    #[serde(rename = "permission.granted")]
    PermissionGranted {
        /// The tool.
        tool: String,
        /// The capability.
        capability: Capability,
        /// The rule that matched, if any.
        matched_rule: Option<String>,
    },

    /// The policy engine refused an action.
    #[serde(rename = "permission.denied")]
    PermissionDenied {
        /// The tool.
        tool: String,
        /// The capability.
        capability: Capability,
        /// Why.
        reason: String,
        /// The rule that matched, if any.
        matched_rule: Option<String>,
    },

    /// Taint escalation changed a decision.
    #[serde(rename = "permission.escalated_by_taint")]
    PermissionEscalatedByTaint {
        /// The tool.
        tool: String,
        /// What the policy alone would have said.
        original: Effect,
        /// What the runtime decided instead.
        escalated: Effect,
    },

    /// The run ingested data from outside the trust boundary.
    #[serde(rename = "agent.taint.raised")]
    TaintRaised {
        /// Where the data came from.
        source: DataSource,
        /// The tool that brought it in.
        tool: String,
    },

    /// A human was asked to decide.
    #[serde(rename = "approval.requested")]
    ApprovalRequested {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// Assessed risk.
        risk: RiskLevel,
    },

    /// A human approved.
    #[serde(rename = "approval.granted")]
    ApprovalGranted {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// How long the human took.
        waited_ms: u64,
        /// Why they said yes, if they wrote it down.
        ///
        /// The chain can already prove that a person allowed a high-risk call
        /// on a tainted run; this is what lets it say why. Defaulted so that
        /// records written before the field existed still read.
        #[serde(default)]
        note: Option<String>,
    },

    /// A human declined, or the run's approval budget refused without asking.
    #[serde(rename = "approval.denied")]
    ApprovalDenied {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// Their note, if any.
        note: Option<String>,
        /// Set when nobody was asked because the run had spent its approval
        /// budget, so the chain says who refused without anyone parsing the
        /// note. Omitted when false, which keeps a person's denial recorded in
        /// the shape it always had, and defaulted so that records written
        /// before the field existed still read.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        over_budget: bool,
    },

    /// A tool invocation began.
    #[serde(rename = "tool.execution.started")]
    ToolExecutionStarted {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Validated arguments.
        arguments: serde_json::Value,
    },

    /// A tool invocation finished.
    #[serde(rename = "tool.execution.completed")]
    ToolExecutionCompleted {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Latency.
        duration_ms: u64,
        /// Whether it succeeded.
        success: bool,
        /// Bytes of output produced.
        output_bytes: usize,
    },

    /// A tool invocation failed.
    #[serde(rename = "tool.execution.failed")]
    ToolExecutionFailed {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Latency before failing.
        duration_ms: u64,
        /// Detail.
        error: String,
    },

    /// The model asked for a tool that failed schema validation.
    #[serde(rename = "tool.arguments.rejected")]
    ToolArgumentsRejected {
        /// The tool.
        tool: String,
        /// Why the arguments were rejected.
        error: String,
    },

    /// The model asked for a tool that does not exist or is not enabled.
    #[serde(rename = "tool.unknown")]
    UnknownToolRequested {
        /// What it asked for.
        tool: String,
    },

    /// A tool planned a capability its manifest does not declare.
    ///
    /// The call was not failed for it: the policy engine evaluated the plan the
    /// tool actually produced, so the decision stands on what would really
    /// happen rather than on what the tool said it would do. The discrepancy is
    /// recorded because a manifest that understates a tool's reach is exactly
    /// how an operator comes to grant more than they meant to.
    #[serde(rename = "tool.manifest_exceeded")]
    ToolManifestExceeded {
        /// The tool.
        tool: String,
        /// The planned capabilities absent from its manifest.
        undeclared: Vec<String>,
    },

    /// A memory was written.
    #[serde(rename = "agent.memory.recorded")]
    MemoryRecorded {
        /// Kind of memory.
        kind: String,
        /// Where the claim came from.
        source: DataSource,
    },

    /// A schedule came due and produced a task.
    ///
    /// The alias reads records written before the tag was brought into line
    /// with [`AgentEvent::kind`]; the log is append-only, so those records are
    /// never rewritten and must go on deserialising.
    #[serde(rename = "schedule.fired", alias = "schedule_fired")]
    ScheduleFired {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name, so a deleted schedule's history still reads.
        name: String,
        /// What it created.
        task_id: TaskId,
    },

    /// A task was given up on because something it depended on will not
    /// succeed.
    ///
    /// Recorded rather than left implicit: a task that silently waits forever is
    /// indistinguishable from one nobody has got to yet. The alias reads records
    /// written under the tag this variant carried before it matched its kind.
    #[serde(rename = "agent.task.abandoned", alias = "task_abandoned")]
    TaskAbandoned {
        /// The task.
        task_id: TaskId,
        /// The dependency that ended it.
        blocked_by: TaskId,
        /// What happened to that dependency.
        reason: String,
    },

    /// An operator installed a policy for an agent.
    ///
    /// The document is carried whole. The policies table keeps only the
    /// current version, so without it a policy widened shortly before a bad
    /// action and narrowed again afterwards would leave no trace of what it
    /// said while the action ran.
    #[serde(rename = "operator.policy.changed")]
    PolicyChanged {
        /// The agent's name, so the record reads after the agent is deleted.
        agent: String,
        /// The version the policy now has.
        version: i64,
        /// The YAML as installed.
        document: String,
    },

    /// An operator created an agent.
    #[serde(rename = "operator.agent.created")]
    AgentCreated {
        /// Its name.
        agent: String,
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// The tools it was given.
        tools: Vec<String>,
    },

    /// An operator switched an agent on or off.
    #[serde(rename = "operator.agent.enabled_changed")]
    AgentEnabledChanged {
        /// Its name.
        agent: String,
        /// Whether it may now run.
        enabled: bool,
    },

    /// An operator stored a credential for a model provider.
    ///
    /// Names the provider and nothing else. Neither the key nor any part of it
    /// is recorded: the log is evidence, and evidence that holds a credential
    /// is one more place to steal it from.
    #[serde(rename = "operator.provider_key.set")]
    ProviderKeySet {
        /// Provider identifier.
        provider: String,
    },

    /// An operator removed a stored provider credential.
    #[serde(rename = "operator.provider_key.removed")]
    ProviderKeyRemoved {
        /// Provider identifier.
        provider: String,
    },

    /// An operator wrote a memory for an agent.
    ///
    /// The content is carried whole, as a policy's document is. Memories are
    /// retrieved into the conversation before planning, so what one said while a
    /// run planned is part of the explanation of that run, and the row may since
    /// have been revised or forgotten.
    #[serde(rename = "operator.memory.recorded")]
    MemoryRemembered {
        /// The agent's name, so the record reads after the agent is deleted.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// Kind of memory.
        kind: String,
        /// What it says.
        content: String,
    },

    /// An operator rewrote a memory.
    #[serde(rename = "operator.memory.revised")]
    MemoryRevised {
        /// The agent's name.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// What it says now.
        content: String,
    },

    /// An operator deleted a memory.
    ///
    /// Carries what was forgotten: the row is gone, and a record that says only
    /// that something was removed cannot explain a run that read it earlier.
    #[serde(rename = "operator.memory.forgotten")]
    MemoryForgotten {
        /// The agent's name.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// Kind of memory.
        kind: String,
        /// What it said.
        content: String,
    },

    /// An operator created a schedule.
    ///
    /// A schedule is standing permission for work to start with nobody present,
    /// so its objective and cadence are kept as they were when it was created.
    #[serde(rename = "operator.schedule.created")]
    ScheduleCreated {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
        /// The objective each firing gets.
        objective: String,
        /// How often it fires.
        cadence: crate::schedule::Cadence,
        /// Its first occurrence.
        next_run_at: Option<Timestamp>,
    },

    /// An operator stopped a schedule firing.
    #[serde(rename = "operator.schedule.paused")]
    SchedulePaused {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
    },

    /// An operator started a paused schedule firing again.
    #[serde(rename = "operator.schedule.resumed")]
    ScheduleResumed {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
        /// When it fires next, computed forward from the moment of resuming;
        /// `None` for a one-shot whose moment passed while it was paused.
        next_run_at: Option<Timestamp>,
    },

    /// An operator deleted a schedule. The tasks it created are kept.
    #[serde(rename = "operator.schedule.deleted")]
    ScheduleDeleted {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name, which is all a reader has once the row is gone.
        name: String,
    },

    /// An operator created a task.
    ///
    /// Whether it runs at once or waits, the objective is trusted control-plane
    /// text, and the record is what ties it to the person who typed it.
    #[serde(rename = "operator.task.created")]
    TaskCreated {
        /// What the agent is asked to do.
        objective: String,
        /// The tasks it waits for.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        depends_on: Vec<TaskId>,
        /// The earliest moment it may start.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheduled_for: Option<Timestamp>,
    },

    /// An operator made a task wait for another.
    #[serde(rename = "operator.task.dependency_added")]
    TaskDependencyAdded {
        /// The task that now waits.
        task_id: TaskId,
        /// The task it waits for.
        depends_on: TaskId,
    },

    /// A scheduler began acting on schedules and queued tasks.
    ///
    /// From here until the matching stop, work starts with nobody present.
    /// The options are recorded because they bound how much of it.
    #[serde(rename = "operator.scheduler.started")]
    SchedulerStarted {
        /// Seconds between ticks.
        tick_seconds: u64,
        /// How many runs it may have in flight at once.
        max_concurrent_runs: u32,
        /// Started by a preference saved earlier, as the application opened,
        /// rather than by somebody asking for it then.
        on_launch: bool,
    },

    /// A scheduler stopped.
    #[serde(rename = "operator.scheduler.stopped")]
    SchedulerStopped {
        /// Seconds between ticks.
        tick_seconds: u64,
        /// How many runs it could have in flight at once.
        max_concurrent_runs: u32,
    },
}

impl AgentEvent {
    /// The dotted event name, matching the `serde` rename.
    ///
    /// Used as the `kind` column in the audit table so events can be filtered
    /// without deserialising the payload.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::TaskStarted { .. } => "agent.task.started",
            Self::TaskCompleted { .. } => "agent.task.completed",
            Self::TaskFailed { .. } => "agent.task.failed",
            Self::TaskCancelled { .. } => "agent.task.cancelled",
            Self::StateTransitioned { .. } => "agent.state.transitioned",
            Self::ModelRequestStarted { .. } => "agent.model.request.started",
            Self::ModelRequestCompleted { .. } => "agent.model.request.completed",
            Self::ModelRequestFailed { .. } => "agent.model.request.failed",
            Self::ReasoningCompleted { .. } => "agent.reasoning.completed",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionGranted { .. } => "permission.granted",
            Self::PermissionDenied { .. } => "permission.denied",
            Self::PermissionEscalatedByTaint { .. } => "permission.escalated_by_taint",
            Self::TaintRaised { .. } => "agent.taint.raised",
            Self::ApprovalRequested { .. } => "approval.requested",
            Self::ApprovalGranted { .. } => "approval.granted",
            Self::ApprovalDenied { .. } => "approval.denied",
            Self::ToolExecutionStarted { .. } => "tool.execution.started",
            Self::ToolExecutionCompleted { .. } => "tool.execution.completed",
            Self::ToolExecutionFailed { .. } => "tool.execution.failed",
            Self::ToolArgumentsRejected { .. } => "tool.arguments.rejected",
            Self::UnknownToolRequested { .. } => "tool.unknown",
            Self::ToolManifestExceeded { .. } => "tool.manifest_exceeded",
            Self::MemoryRecorded { .. } => "agent.memory.recorded",
            Self::ScheduleFired { .. } => "schedule.fired",
            Self::TaskAbandoned { .. } => "agent.task.abandoned",
            Self::PolicyChanged { .. } => "operator.policy.changed",
            Self::AgentCreated { .. } => "operator.agent.created",
            Self::AgentEnabledChanged { .. } => "operator.agent.enabled_changed",
            Self::ProviderKeySet { .. } => "operator.provider_key.set",
            Self::ProviderKeyRemoved { .. } => "operator.provider_key.removed",
            Self::MemoryRemembered { .. } => "operator.memory.recorded",
            Self::MemoryRevised { .. } => "operator.memory.revised",
            Self::MemoryForgotten { .. } => "operator.memory.forgotten",
            Self::ScheduleCreated { .. } => "operator.schedule.created",
            Self::SchedulePaused { .. } => "operator.schedule.paused",
            Self::ScheduleResumed { .. } => "operator.schedule.resumed",
            Self::ScheduleDeleted { .. } => "operator.schedule.deleted",
            Self::TaskCreated { .. } => "operator.task.created",
            Self::TaskDependencyAdded { .. } => "operator.task.dependency_added",
            Self::SchedulerStarted { .. } => "operator.scheduler.started",
            Self::SchedulerStopped { .. } => "operator.scheduler.stopped",
        }
    }

    /// Whether this event records a security-relevant refusal or escalation.
    ///
    /// The dashboard surfaces these separately from routine activity.
    ///
    /// A `match` rather than a `matches!` so that an arm can compute its answer
    /// from the event rather than return a constant: an egress event, for one,
    /// matters only when the run was tainted. It is exhaustive so that a new
    /// variant cannot be added without somebody deciding which side it is on.
    /// A stored record is classified by [`is_security_kind`] instead, which must
    /// agree with this for every variant whose answer does not depend on its
    /// fields.
    #[must_use]
    pub fn is_security_relevant(&self) -> bool {
        match self {
            Self::PermissionDenied { .. } => true,
            Self::PermissionEscalatedByTaint { .. } => true,
            Self::ApprovalDenied { .. } => true,
            Self::ToolArgumentsRejected { .. } => true,
            Self::UnknownToolRequested { .. } => true,
            Self::TaintRaised { .. } => true,
            Self::ToolManifestExceeded { .. } => true,
            // What an agent may do is decided by these. A grant widened before
            // a bad action is part of the explanation of that action.
            Self::PolicyChanged { .. }
            | Self::AgentCreated { .. }
            | Self::AgentEnabledChanged { .. }
            | Self::ProviderKeySet { .. }
            | Self::ProviderKeyRemoved { .. } => true,
            // What an agent is told before it plans, and what starts work with
            // nobody present, shape what it does as much as its grants do.
            Self::MemoryRemembered { .. }
            | Self::MemoryRevised { .. }
            | Self::MemoryForgotten { .. }
            | Self::ScheduleCreated { .. }
            | Self::SchedulePaused { .. }
            | Self::ScheduleResumed { .. }
            | Self::ScheduleDeleted { .. }
            | Self::TaskCreated { .. }
            | Self::TaskDependencyAdded { .. }
            | Self::SchedulerStarted { .. }
            | Self::SchedulerStopped { .. } => true,
            Self::TaskStarted { .. }
            | Self::TaskCompleted { .. }
            | Self::TaskFailed { .. }
            | Self::TaskCancelled { .. }
            | Self::StateTransitioned { .. }
            | Self::ModelRequestStarted { .. }
            | Self::ModelRequestCompleted { .. }
            | Self::ModelRequestFailed { .. }
            | Self::ReasoningCompleted { .. }
            | Self::PermissionRequested { .. }
            | Self::PermissionGranted { .. }
            | Self::ApprovalRequested { .. }
            | Self::ApprovalGranted { .. }
            | Self::ToolExecutionStarted { .. }
            | Self::ToolExecutionCompleted { .. }
            | Self::ToolExecutionFailed { .. }
            | Self::MemoryRecorded { .. }
            | Self::ScheduleFired { .. }
            | Self::TaskAbandoned { .. } => false,
        }
    }

    /// A one-line description of an operator change, or `None` for any other
    /// event.
    ///
    /// The generic summaries read a tool, an objective or a reason, and an
    /// operator change carries none of them: a feed would show a policy
    /// installed as a blank line, or an agent created as its model. Both
    /// clients summarise from here so they describe the change the same way.
    /// A provider change names the provider and nothing else, as its record does.
    #[must_use]
    pub fn operator_summary(&self) -> Option<String> {
        let line = match self {
            Self::PolicyChanged { agent, version, .. } => {
                format!("{agent}: policy version {version} installed")
            }
            Self::AgentCreated {
                agent,
                provider,
                model,
                ..
            } => format!("{agent} created on {provider}/{model}"),
            Self::AgentEnabledChanged { agent, enabled } => {
                format!("{agent} {}", if *enabled { "enabled" } else { "disabled" })
            }
            Self::ProviderKeySet { provider } => format!("{provider} key stored"),
            Self::ProviderKeyRemoved { provider } => format!("{provider} key removed"),
            Self::MemoryRemembered {
                agent,
                kind,
                content,
                ..
            } => format!("{agent}: {kind} remembered: {}", first_line(content)),
            Self::MemoryRevised { agent, content, .. } => {
                format!("{agent}: memory revised: {}", first_line(content))
            }
            Self::MemoryForgotten {
                agent,
                kind,
                content,
                ..
            } => format!("{agent}: {kind} forgotten: {}", first_line(content)),
            Self::ScheduleCreated { name, cadence, .. } => {
                format!("schedule {name} created, {}", cadence.describe())
            }
            Self::SchedulePaused { name, .. } => format!("schedule {name} paused"),
            Self::ScheduleResumed { name, .. } => format!("schedule {name} resumed"),
            Self::ScheduleDeleted { name, .. } => format!("schedule {name} deleted"),
            Self::TaskCreated {
                objective,
                depends_on,
                scheduled_for,
            } => {
                let mut line = format!("task queued: {}", first_line(objective));
                if !depends_on.is_empty() {
                    line.push_str(&format!(", after {} task(s)", depends_on.len()));
                }
                if let Some(when) = scheduled_for {
                    line.push_str(&format!(", not before {}", crate::format_timestamp(when)));
                }
                line
            }
            Self::TaskDependencyAdded { .. } => "task made to wait for another".to_owned(),
            Self::SchedulerStarted {
                tick_seconds,
                max_concurrent_runs,
                on_launch,
            } => format!(
                "scheduler started{}, every {tick_seconds}s, at most {max_concurrent_runs} run(s) at once",
                if *on_launch { " on launch" } else { "" }
            ),
            Self::SchedulerStopped { .. } => "scheduler stopped".to_owned(),
            _ => return None,
        };
        Some(line)
    }
}

/// The first line of operator-typed text, for a one-line summary.
///
/// The record keeps the whole text; a feed row that wrapped a pasted paragraph
/// would push everything after it off the screen.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// [`AgentEvent::operator_summary`] for a stored payload.
///
/// Only a payload tagged with an `operator.` kind is deserialised, so a feed of
/// routine records pays nothing for it. A payload that carries the tag and does
/// not read as its event gives `None`, and the caller's generic summary stands.
#[must_use]
pub fn operator_summary_of(payload: &serde_json::Value) -> Option<String> {
    let kind = payload.get("event")?.as_str()?;
    if !kind.starts_with("operator.") {
        return None;
    }
    serde_json::from_value::<AgentEvent>(payload.clone())
        .ok()?
        .operator_summary()
}

/// Whether a stored record of this kind is security-relevant.
///
/// The historical classification: what a reader of the audit table has when it
/// holds only the `kind` column and not a live [`AgentEvent`]. For every variant
/// it agrees with [`AgentEvent::is_security_relevant`], and a test holds it to
/// that.
///
/// It can only do so while the answer depends on the kind alone. An event that
/// is security-relevant only conditionally — egress, say, which matters when the
/// run was tainted and not otherwise — cannot be classified from its name after
/// the fact, and must be classified by the writer at record time and the answer
/// stored beside it. Adding such a variant means extending the record, not this
/// list.
#[must_use]
pub fn is_security_kind(kind: &str) -> bool {
    SECURITY_KINDS.contains(&kind)
}

/// Every kind [`is_security_kind`] accepts.
///
/// A list as well as a predicate so a store can select these kinds in its own
/// query. Filtering the newest records of every kind instead finds almost
/// none: security records are a small fraction of a busy log.
pub const SECURITY_KINDS: &[&str] = &[
    "permission.denied",
    "permission.escalated_by_taint",
    "approval.denied",
    "tool.arguments.rejected",
    "tool.unknown",
    "agent.taint.raised",
    "tool.manifest_exceeded",
    "operator.policy.changed",
    "operator.agent.created",
    "operator.agent.enabled_changed",
    "operator.provider_key.set",
    "operator.provider_key.removed",
    "operator.memory.recorded",
    "operator.memory.revised",
    "operator.memory.forgotten",
    "operator.schedule.created",
    "operator.schedule.paused",
    "operator.schedule.resumed",
    "operator.schedule.deleted",
    "operator.task.created",
    "operator.task.dependency_added",
    "operator.scheduler.started",
    "operator.scheduler.stopped",
];

/// An event with its context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Identity.
    pub id: EventId,
    /// When it happened.
    pub at: Timestamp,
    /// The agent involved, if any.
    pub agent_id: Option<AgentId>,
    /// The task involved, if any.
    pub task_id: Option<TaskId>,
    /// The run involved, if any.
    pub run_id: Option<TaskRunId>,
    /// What happened.
    pub payload: AgentEvent,
}

impl Event {
    /// Build an event with no context attached.
    #[must_use]
    pub fn new(payload: AgentEvent) -> Self {
        Self {
            id: EventId::new(),
            at: crate::now(),
            agent_id: None,
            task_id: None,
            run_id: None,
            payload,
        }
    }

    /// Attach the agent.
    #[must_use]
    pub const fn for_agent(mut self, agent_id: AgentId) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    /// Attach the task.
    #[must_use]
    pub const fn for_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Attach the run.
    #[must_use]
    pub const fn for_run(mut self, run_id: TaskRunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    /// The dotted event name.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.payload.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ScheduleId;

    /// How many variants [`AgentEvent`] has.
    ///
    /// Kept beside [`variant_index`] because the two change together.
    const VARIANT_COUNT: usize = 42;

    /// A dense index per variant, in declaration order.
    ///
    /// Exhaustive with no wildcard, so a new variant does not compile until it
    /// is given an index here — and once it has one, the sampling test below
    /// fails until it is also in [`sample_events`]. Rust offers no way to count
    /// an enum's variants directly; this is the next best thing.
    const fn variant_index(event: &AgentEvent) -> usize {
        match event {
            AgentEvent::TaskStarted { .. } => 0,
            AgentEvent::TaskCompleted { .. } => 1,
            AgentEvent::TaskFailed { .. } => 2,
            AgentEvent::TaskCancelled { .. } => 3,
            AgentEvent::StateTransitioned { .. } => 4,
            AgentEvent::ModelRequestStarted { .. } => 5,
            AgentEvent::ModelRequestCompleted { .. } => 6,
            AgentEvent::ModelRequestFailed { .. } => 7,
            AgentEvent::ReasoningCompleted { .. } => 8,
            AgentEvent::PermissionRequested { .. } => 9,
            AgentEvent::PermissionGranted { .. } => 10,
            AgentEvent::PermissionDenied { .. } => 11,
            AgentEvent::PermissionEscalatedByTaint { .. } => 12,
            AgentEvent::TaintRaised { .. } => 13,
            AgentEvent::ApprovalRequested { .. } => 14,
            AgentEvent::ApprovalGranted { .. } => 15,
            AgentEvent::ApprovalDenied { .. } => 16,
            AgentEvent::ToolExecutionStarted { .. } => 17,
            AgentEvent::ToolExecutionCompleted { .. } => 18,
            AgentEvent::ToolExecutionFailed { .. } => 19,
            AgentEvent::ToolArgumentsRejected { .. } => 20,
            AgentEvent::UnknownToolRequested { .. } => 21,
            AgentEvent::ToolManifestExceeded { .. } => 22,
            AgentEvent::MemoryRecorded { .. } => 23,
            AgentEvent::ScheduleFired { .. } => 24,
            AgentEvent::TaskAbandoned { .. } => 25,
            AgentEvent::PolicyChanged { .. } => 26,
            AgentEvent::AgentCreated { .. } => 27,
            AgentEvent::AgentEnabledChanged { .. } => 28,
            AgentEvent::ProviderKeySet { .. } => 29,
            AgentEvent::ProviderKeyRemoved { .. } => 30,
            AgentEvent::MemoryRemembered { .. } => 31,
            AgentEvent::MemoryRevised { .. } => 32,
            AgentEvent::MemoryForgotten { .. } => 33,
            AgentEvent::ScheduleCreated { .. } => 34,
            AgentEvent::SchedulePaused { .. } => 35,
            AgentEvent::ScheduleResumed { .. } => 36,
            AgentEvent::ScheduleDeleted { .. } => 37,
            AgentEvent::TaskCreated { .. } => 38,
            AgentEvent::TaskDependencyAdded { .. } => 39,
            AgentEvent::SchedulerStarted { .. } => 40,
            AgentEvent::SchedulerStopped { .. } => 41,
        }
    }

    /// One of every variant.
    ///
    /// Every test that claims to cover "every event" iterates this, so an
    /// omission here is an omission from all of them; the sampling test is what
    /// stops that happening quietly.
    fn sample_events() -> Vec<AgentEvent> {
        vec![
            AgentEvent::TaskStarted {
                objective: "o".into(),
                attempt: 1,
            },
            AgentEvent::TaskCompleted {
                steps: 3,
                duration_ms: 10,
            },
            AgentEvent::TaskFailed {
                reason: "r".into(),
                steps: 1,
            },
            AgentEvent::TaskCancelled { steps: 1 },
            AgentEvent::StateTransitioned {
                from: TaskState::Idle,
                to: TaskState::Planning,
                trigger: TaskTrigger::Start,
            },
            AgentEvent::ModelRequestStarted {
                provider: "mock".into(),
                model: "m".into(),
                message_count: 1,
                tool_count: 0,
            },
            AgentEvent::ModelRequestCompleted {
                provider: "mock".into(),
                model: "m".into(),
                duration_ms: 1,
                input_tokens: None,
                output_tokens: None,
                tool_calls: 0,
            },
            AgentEvent::ModelRequestFailed {
                provider: "mock".into(),
                error: "e".into(),
            },
            AgentEvent::ReasoningCompleted {
                summary: "s".into(),
                tool_calls: 0,
            },
            AgentEvent::PermissionRequested {
                tool: "t".into(),
                capability: Capability::new("filesystem", "read"),
                risk: RiskLevel::Low,
                tainted: false,
            },
            AgentEvent::PermissionGranted {
                tool: "t".into(),
                capability: Capability::new("filesystem", "read"),
                matched_rule: None,
            },
            AgentEvent::PermissionDenied {
                tool: "t".into(),
                capability: Capability::new("filesystem", "read"),
                reason: "r".into(),
                matched_rule: None,
            },
            AgentEvent::PermissionEscalatedByTaint {
                tool: "t".into(),
                original: Effect::Allow,
                escalated: Effect::Ask,
            },
            AgentEvent::TaintRaised {
                source: DataSource::User,
                tool: "t".into(),
            },
            AgentEvent::ApprovalRequested {
                approval_id: ApprovalId::new(),
                tool: "t".into(),
                risk: RiskLevel::High,
            },
            AgentEvent::ApprovalGranted {
                approval_id: ApprovalId::new(),
                tool: "t".into(),
                waited_ms: 5,
                note: Some("checked the recipient".into()),
            },
            AgentEvent::ApprovalDenied {
                approval_id: ApprovalId::new(),
                tool: "t".into(),
                note: None,
                over_budget: false,
            },
            AgentEvent::ToolExecutionStarted {
                execution_id: ToolExecutionId::new(),
                tool: "t".into(),
                arguments: serde_json::Value::Null,
            },
            AgentEvent::ToolExecutionCompleted {
                execution_id: ToolExecutionId::new(),
                tool: "t".into(),
                duration_ms: 1,
                success: true,
                output_bytes: 0,
            },
            AgentEvent::ToolExecutionFailed {
                execution_id: ToolExecutionId::new(),
                tool: "t".into(),
                duration_ms: 1,
                error: "e".into(),
            },
            AgentEvent::ToolArgumentsRejected {
                tool: "t".into(),
                error: "e".into(),
            },
            AgentEvent::UnknownToolRequested { tool: "t".into() },
            AgentEvent::ToolManifestExceeded {
                tool: "t".into(),
                undeclared: vec!["filesystem.write".into()],
            },
            AgentEvent::MemoryRecorded {
                kind: "fact".into(),
                source: DataSource::User,
            },
            AgentEvent::ScheduleFired {
                schedule_id: ScheduleId::new(),
                name: "n".into(),
                task_id: TaskId::new(),
            },
            AgentEvent::TaskAbandoned {
                task_id: TaskId::new(),
                blocked_by: TaskId::new(),
                reason: "failed".into(),
            },
            AgentEvent::PolicyChanged {
                agent: "a".into(),
                version: 2,
                document: "default: deny\n".into(),
            },
            AgentEvent::AgentCreated {
                agent: "a".into(),
                provider: "mock".into(),
                model: "m".into(),
                tools: vec!["filesystem.read".into()],
            },
            AgentEvent::AgentEnabledChanged {
                agent: "a".into(),
                enabled: false,
            },
            AgentEvent::ProviderKeySet {
                provider: "anthropic".into(),
            },
            AgentEvent::ProviderKeyRemoved {
                provider: "anthropic".into(),
            },
            AgentEvent::MemoryRemembered {
                agent: "a".into(),
                memory_id: MemoryId::new(),
                kind: "preference".into(),
                content: "Reply in British English.".into(),
            },
            AgentEvent::MemoryRevised {
                agent: "a".into(),
                memory_id: MemoryId::new(),
                content: "Reply in plain English.".into(),
            },
            AgentEvent::MemoryForgotten {
                agent: "a".into(),
                memory_id: MemoryId::new(),
                kind: "fact".into(),
                content: "The office is in Leeds.".into(),
            },
            AgentEvent::ScheduleCreated {
                schedule_id: ScheduleId::new(),
                name: "weekly".into(),
                objective: "o".into(),
                cadence: crate::schedule::Cadence::Every { seconds: 3600 },
                next_run_at: Some(crate::now()),
            },
            AgentEvent::SchedulePaused {
                schedule_id: ScheduleId::new(),
                name: "weekly".into(),
            },
            AgentEvent::ScheduleResumed {
                schedule_id: ScheduleId::new(),
                name: "weekly".into(),
                next_run_at: None,
            },
            AgentEvent::ScheduleDeleted {
                schedule_id: ScheduleId::new(),
                name: "weekly".into(),
            },
            AgentEvent::TaskCreated {
                objective: "o".into(),
                depends_on: vec![TaskId::new()],
                scheduled_for: None,
            },
            AgentEvent::TaskDependencyAdded {
                task_id: TaskId::new(),
                depends_on: TaskId::new(),
            },
            AgentEvent::SchedulerStarted {
                tick_seconds: 30,
                max_concurrent_runs: 1,
                on_launch: true,
            },
            AgentEvent::SchedulerStopped {
                tick_seconds: 30,
                max_concurrent_runs: 1,
            },
        ]
    }

    #[test]
    fn every_variant_is_sampled() {
        // Sorted indices equal to 0..VARIANT_COUNT means every variant appears
        // exactly once: a missing one leaves a gap, a duplicate repeats an
        // index, and a variant added without bumping the count overshoots.
        let mut indices: Vec<usize> = sample_events().iter().map(variant_index).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..VARIANT_COUNT).collect::<Vec<_>>());
    }

    #[test]
    fn a_refusal_by_budget_is_told_apart_from_a_person_saying_no() {
        let by_budget = AgentEvent::ApprovalDenied {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            note: Some("over budget".into()),
            over_budget: true,
        };
        let json = serde_json::to_value(&by_budget).unwrap();
        assert_eq!(json["over_budget"], serde_json::Value::Bool(true));
        assert_eq!(
            serde_json::from_value::<AgentEvent>(json).unwrap(),
            by_budget
        );

        // A person's denial keeps the shape it had before the field existed,
        // and a record written then still reads as a person's denial.
        let by_person = AgentEvent::ApprovalDenied {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            note: None,
            over_budget: false,
        };
        let json = serde_json::to_value(&by_person).unwrap();
        assert!(json.get("over_budget").is_none(), "{json}");
        assert_eq!(
            serde_json::from_value::<AgentEvent>(json).unwrap(),
            by_person
        );
    }

    #[test]
    fn every_kind_is_distinct() {
        // The `kind` column is the audit table's filter; two variants sharing a
        // name would make one of them unfindable.
        let mut kinds: Vec<&str> = sample_events().iter().map(AgentEvent::kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), VARIANT_COUNT);
    }

    #[test]
    fn kind_matches_the_serialised_tag() {
        // If these ever drift, consumers filtering on the `kind` column would
        // silently miss events. Assert they cannot.
        for event in sample_events() {
            let json = serde_json::to_value(&event).unwrap();
            let tag = json
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap();
            assert_eq!(tag, event.kind(), "tag/kind mismatch for {event:?}");
        }
    }

    #[test]
    fn every_operator_change_has_a_summary_and_nothing_else_does() {
        // Keyed on the kind's prefix rather than a list, so a sixth operator
        // event without a summary line fails here instead of reaching a feed
        // as a blank row.
        for event in sample_events() {
            let stored = serde_json::to_value(&event).unwrap();
            let summary = operator_summary_of(&stored);
            if event.kind().starts_with("operator.") {
                let line = summary.unwrap_or_else(|| panic!("no summary for {}", event.kind()));
                assert!(
                    !line.trim().is_empty(),
                    "blank summary for {}",
                    event.kind()
                );
            } else {
                assert_eq!(summary, None, "{} is not an operator change", event.kind());
            }
        }

        let disabled = AgentEvent::AgentEnabledChanged {
            agent: "ops".into(),
            enabled: false,
        };
        assert_eq!(disabled.operator_summary().as_deref(), Some("ops disabled"));
        let stored =
            serde_json::json!({"event": "operator.provider_key.set", "provider": "openai"});
        assert_eq!(
            operator_summary_of(&stored).as_deref(),
            Some("openai key stored")
        );
    }

    #[test]
    fn events_round_trip_through_json() {
        for event in sample_events() {
            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event);
        }
    }

    #[test]
    fn records_written_under_the_old_tags_still_deserialise() {
        // These two once serialised under snake_case tags that disagreed with
        // their kind. The log is append-only, so those records are still there.
        let schedule_id = ScheduleId::new();
        let task_id = TaskId::new();
        let fired: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "schedule_fired",
            "schedule_id": schedule_id,
            "name": "hourly",
            "task_id": task_id,
        }))
        .unwrap();
        assert_eq!(
            fired,
            AgentEvent::ScheduleFired {
                schedule_id,
                name: "hourly".into(),
                task_id,
            }
        );

        let blocked_by = TaskId::new();
        let abandoned: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "task_abandoned",
            "task_id": task_id,
            "blocked_by": blocked_by,
            "reason": "failed",
        }))
        .unwrap();
        assert_eq!(
            abandoned,
            AgentEvent::TaskAbandoned {
                task_id,
                blocked_by,
                reason: "failed".into(),
            }
        );

        // And they are written back under the tag that matches the kind.
        let tag = serde_json::to_value(&abandoned).unwrap()["event"].clone();
        assert_eq!(tag, "agent.task.abandoned");
    }

    #[test]
    fn an_approval_recorded_before_notes_existed_still_reads() {
        let approval_id = ApprovalId::new();
        let granted: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "approval.granted",
            "approval_id": approval_id,
            "tool": "email.send",
            "waited_ms": 1200,
        }))
        .unwrap();
        assert_eq!(
            granted,
            AgentEvent::ApprovalGranted {
                approval_id,
                tool: "email.send".into(),
                waited_ms: 1200,
                note: None,
            }
        );
    }

    #[test]
    fn operator_events_carry_no_key_material() {
        // The provider events are the only ones a credential passes near. Their
        // serialised form is pinned to the tag and the provider id, so a field
        // added later has to come through this test to get into the log.
        for event in [
            AgentEvent::ProviderKeySet {
                provider: "anthropic".into(),
            },
            AgentEvent::ProviderKeyRemoved {
                provider: "anthropic".into(),
            },
        ] {
            let json = serde_json::to_value(&event).unwrap();
            let mut fields: Vec<&str> = json
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            fields.sort_unstable();
            assert_eq!(fields, vec!["event", "provider"]);
        }
    }

    #[test]
    fn the_stored_classification_agrees_with_the_live_one() {
        // The desktop classifies stored records by kind and live events by
        // variant. If the two disagree, the same event is flagged in one view
        // and not the other.
        for event in sample_events() {
            assert_eq!(
                is_security_kind(event.kind()),
                event.is_security_relevant(),
                "classification mismatch for {}",
                event.kind()
            );
        }
    }

    #[test]
    fn the_security_relevant_set_is_exactly_this() {
        // Pinned, so that moving an event across the line is a decision that
        // shows up in review rather than a side effect.
        let mut flagged: Vec<&str> = sample_events()
            .iter()
            .filter(|event| event.is_security_relevant())
            .map(AgentEvent::kind)
            .collect();
        flagged.sort_unstable();
        assert_eq!(
            flagged,
            vec![
                "agent.taint.raised",
                "approval.denied",
                "operator.agent.created",
                "operator.agent.enabled_changed",
                "operator.memory.forgotten",
                "operator.memory.recorded",
                "operator.memory.revised",
                "operator.policy.changed",
                "operator.provider_key.removed",
                "operator.provider_key.set",
                "operator.schedule.created",
                "operator.schedule.deleted",
                "operator.schedule.paused",
                "operator.schedule.resumed",
                "operator.scheduler.started",
                "operator.scheduler.stopped",
                "operator.task.created",
                "operator.task.dependency_added",
                "permission.denied",
                "permission.escalated_by_taint",
                "tool.arguments.rejected",
                "tool.manifest_exceeded",
                "tool.unknown",
            ]
        );
    }

    #[test]
    fn security_events_are_flagged() {
        assert!(
            AgentEvent::PermissionDenied {
                tool: "t".into(),
                capability: Capability::new("filesystem", "write"),
                reason: "r".into(),
                matched_rule: None,
            }
            .is_security_relevant()
        );
        assert!(
            !AgentEvent::TaskCompleted {
                steps: 1,
                duration_ms: 1
            }
            .is_security_relevant()
        );
    }

    #[test]
    fn context_builders_attach_ids() {
        let agent = AgentId::new();
        let task = TaskId::new();
        let run = TaskRunId::new();
        let event = Event::new(AgentEvent::TaskCancelled { steps: 0 })
            .for_agent(agent)
            .for_task(task)
            .for_run(run);
        assert_eq!(event.agent_id, Some(agent));
        assert_eq!(event.task_id, Some(task));
        assert_eq!(event.run_id, Some(run));
        assert_eq!(event.kind(), "agent.task.cancelled");
    }
}
