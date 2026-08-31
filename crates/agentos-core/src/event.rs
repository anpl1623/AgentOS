//! The observability event taxonomy.
//!
//! Every meaningful thing the runtime does emits a structured event. Text logs
//! are a rendering of these, never the source of truth: the audit log, the
//! desktop activity feed and the CLI trace all read the same stream.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, ApprovalId, EventId, TaskId, TaskRunId, ToolExecutionId};
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
    },

    /// A human declined.
    #[serde(rename = "approval.denied")]
    ApprovalDenied {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// Their note, if any.
        note: Option<String>,
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

    /// A memory was written.
    #[serde(rename = "agent.memory.recorded")]
    MemoryRecorded {
        /// Kind of memory.
        kind: String,
        /// Where the claim came from.
        source: DataSource,
    },

    /// A schedule came due and produced a task.
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
    /// indistinguishable from one nobody has got to yet.
    TaskAbandoned {
        /// The task.
        task_id: TaskId,
        /// The dependency that ended it.
        blocked_by: TaskId,
        /// What happened to that dependency.
        reason: String,
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
            Self::MemoryRecorded { .. } => "agent.memory.recorded",
            Self::ScheduleFired { .. } => "schedule.fired",
            Self::TaskAbandoned { .. } => "agent.task.abandoned",
        }
    }

    /// Whether this event records a security-relevant refusal or escalation.
    ///
    /// The dashboard surfaces these separately from routine activity.
    #[must_use]
    pub const fn is_security_relevant(&self) -> bool {
        matches!(
            self,
            Self::PermissionDenied { .. }
                | Self::PermissionEscalatedByTaint { .. }
                | Self::ApprovalDenied { .. }
                | Self::ToolArgumentsRejected { .. }
                | Self::UnknownToolRequested { .. }
                | Self::TaintRaised { .. }
        )
    }
}

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
            },
            AgentEvent::ApprovalDenied {
                approval_id: ApprovalId::new(),
                tool: "t".into(),
                note: None,
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
            AgentEvent::MemoryRecorded {
                kind: "fact".into(),
                source: DataSource::User,
            },
        ]
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
    fn events_round_trip_through_json() {
        for event in sample_events() {
            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event);
        }
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
