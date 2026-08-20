//! Tasks, runs, and the execution state machine.
//!
//! A **task** is an objective. A **run** is one attempt at it. Separating them
//! means a task can be retried, scheduled, or replayed without losing the
//! history of previous attempts.
//!
//! The state machine is a pure function ([`transition`]). It performs no I/O and
//! knows nothing about tools or models, which is what makes its transition table
//! exhaustively testable. The driver that calls it lives in `agentos-runtime`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::error::CoreError;
use crate::ids::{AgentId, TaskId, TaskRunId, TaskStepId, ToolExecutionId};

/// Lifecycle state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Created, not yet started.
    #[default]
    Idle,
    /// Asking the model what to do next.
    Planning,
    /// Running tools the model asked for.
    Executing,
    /// Feeding tool results back to the model.
    Observing,
    /// Checking whether the objective has actually been met.
    Verifying,
    /// Blocked on a human decision.
    WaitingForApproval,
    /// Handling a failure before retrying.
    Recovering,
    /// Finished successfully.
    Completed,
    /// Finished unsuccessfully.
    Failed,
    /// Stopped by the operator.
    Cancelled,
}

impl TaskState {
    /// Whether no further transitions are possible.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Whether the run is actively consuming resources.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Planning | Self::Executing | Self::Observing | Self::Verifying | Self::Recovering
        )
    }

    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Planning => "planning",
            Self::Executing => "executing",
            Self::Observing => "observing",
            Self::Verifying => "verifying",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::Recovering => "recovering",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Every state, for exhaustive testing.
    pub const ALL: [Self; 10] = [
        Self::Idle,
        Self::Planning,
        Self::Executing,
        Self::Observing,
        Self::Verifying,
        Self::WaitingForApproval,
        Self::Recovering,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskState {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| CoreError::UnknownVariant {
                kind: "task state",
                value: s.to_owned(),
            })
    }
}

/// Something that happened, which may move the run to a new state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTrigger {
    /// The operator started the run.
    Start,
    /// The model produced a plan containing tool calls.
    PlanProducedToolCalls,
    /// The model produced a plan with no tool calls (it believes it is done).
    PlanProducedNoToolCalls,
    /// All requested tools finished.
    ToolsCompleted,
    /// A tool requires human approval before it can run.
    ApprovalRequired,
    /// A human approved.
    ApprovalGranted,
    /// A human declined.
    ApprovalDenied,
    /// Tool results were incorporated into the conversation.
    ObservationRecorded,
    /// Verification found the objective is met.
    VerificationPassed,
    /// Verification found more work is needed.
    VerificationNeedsMoreWork,
    /// A recoverable error occurred.
    RecoverableError,
    /// Recovery succeeded; resume.
    RecoverySucceeded,
    /// The error is not recoverable, or the retry budget is exhausted.
    UnrecoverableError,
    /// The operator cancelled the run.
    Cancel,
}

impl TaskTrigger {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::PlanProducedToolCalls => "plan_produced_tool_calls",
            Self::PlanProducedNoToolCalls => "plan_produced_no_tool_calls",
            Self::ToolsCompleted => "tools_completed",
            Self::ApprovalRequired => "approval_required",
            Self::ApprovalGranted => "approval_granted",
            Self::ApprovalDenied => "approval_denied",
            Self::ObservationRecorded => "observation_recorded",
            Self::VerificationPassed => "verification_passed",
            Self::VerificationNeedsMoreWork => "verification_needs_more_work",
            Self::RecoverableError => "recoverable_error",
            Self::RecoverySucceeded => "recovery_succeeded",
            Self::UnrecoverableError => "unrecoverable_error",
            Self::Cancel => "cancel",
        }
    }
}

impl fmt::Display for TaskTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A transition the state machine refuses to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid transition: cannot apply `{trigger}` while `{state}`")]
pub struct InvalidTransition {
    /// The state the run was in.
    pub state: TaskState,
    /// The trigger that was rejected.
    pub trigger: TaskTrigger,
}

/// The transition table.
///
/// Pure: no I/O, no clock, no allocation. Every rejected transition is an error
/// rather than a silent no-op, so a driver bug surfaces immediately instead of
/// leaving a run wedged in a state nobody expected.
///
/// `Cancel` is accepted from every non-terminal state — the operator must always
/// be able to stop an agent.
///
/// # Errors
///
/// Returns [`InvalidTransition`] when the trigger is not legal in `state`.
pub const fn transition(
    state: TaskState,
    trigger: TaskTrigger,
) -> Result<TaskState, InvalidTransition> {
    use TaskState as S;
    use TaskTrigger as T;

    // Cancellation is universal, but a finished run stays finished.
    if matches!(trigger, T::Cancel) {
        return if state.is_terminal() {
            Err(InvalidTransition { state, trigger })
        } else {
            Ok(S::Cancelled)
        };
    }

    let next = match (state, trigger) {
        (S::Idle, T::Start) => S::Planning,

        (S::Planning, T::PlanProducedToolCalls) => S::Executing,
        (S::Planning, T::PlanProducedNoToolCalls) => S::Verifying,

        (S::Executing, T::ApprovalRequired) => S::WaitingForApproval,
        (S::Executing, T::ToolsCompleted) => S::Observing,

        (S::WaitingForApproval, T::ApprovalGranted) => S::Executing,
        // A denial is not a failure. The remaining tool calls in the batch still
        // run, and the refusal is handed back to the model as a tool result so
        // it can re-plan around the refusal on its next turn.
        (S::WaitingForApproval, T::ApprovalDenied) => S::Executing,

        (S::Observing, T::ObservationRecorded) => S::Verifying,

        (S::Verifying, T::VerificationPassed) => S::Completed,
        (S::Verifying, T::VerificationNeedsMoreWork) => S::Planning,

        (S::Recovering, T::RecoverySucceeded) => S::Planning,

        // Any active state can hit a recoverable error.
        (
            S::Planning | S::Executing | S::Observing | S::Verifying | S::WaitingForApproval,
            T::RecoverableError,
        ) => S::Recovering,

        // Any non-terminal state can fail outright.
        (
            S::Idle
            | S::Planning
            | S::Executing
            | S::Observing
            | S::Verifying
            | S::WaitingForApproval
            | S::Recovering,
            T::UnrecoverableError,
        ) => S::Failed,

        _ => return Err(InvalidTransition { state, trigger }),
    };

    Ok(next)
}

/// Why a run finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskFailure {
    /// The model provider errored.
    Provider {
        /// Detail.
        message: String,
    },
    /// A tool failed in a way the agent could not work around.
    Tool {
        /// The tool.
        tool: String,
        /// Detail.
        message: String,
    },
    /// The run exceeded its step budget.
    StepBudgetExhausted {
        /// The budget that was hit.
        limit: u32,
    },
    /// The run exceeded its wall-clock budget.
    Timeout {
        /// The budget that was hit, in seconds.
        limit_secs: u64,
    },
    /// The runtime itself failed.
    Runtime {
        /// Detail.
        message: String,
    },
}

impl fmt::Display for TaskFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Provider { message } => write!(f, "provider error: {message}"),
            Self::Tool { tool, message } => write!(f, "tool `{tool}` failed: {message}"),
            Self::StepBudgetExhausted { limit } => {
                write!(f, "step budget of {limit} exhausted")
            }
            Self::Timeout { limit_secs } => write!(f, "timed out after {limit_secs}s"),
            Self::Runtime { message } => write!(f, "runtime error: {message}"),
        }
    }
}

/// Status of a task, aggregated across its runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Never run.
    #[default]
    Pending,
    /// A run is in progress.
    Running,
    /// The most recent run succeeded.
    Succeeded,
    /// The most recent run failed.
    Failed,
    /// The most recent run was cancelled.
    Cancelled,
}

impl TaskStatus {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Derive a task status from the state of its latest run.
    #[must_use]
    pub const fn from_run_state(state: TaskState) -> Self {
        match state {
            TaskState::Idle => Self::Pending,
            TaskState::Completed => Self::Succeeded,
            TaskState::Failed => Self::Failed,
            TaskState::Cancelled => Self::Cancelled,
            _ => Self::Running,
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(CoreError::UnknownVariant {
                kind: "task status",
                value: other.to_owned(),
            }),
        }
    }
}

/// An objective, independent of any attempt to achieve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Identity.
    pub id: TaskId,
    /// The agent responsible.
    pub agent_id: AgentId,
    /// What the operator asked for. Trusted control-plane text.
    pub objective: String,
    /// Aggregate status.
    pub status: TaskStatus,
    /// The task this was spawned from, for orchestrated task graphs.
    pub parent_task_id: Option<TaskId>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When its first run started.
    pub started_at: Option<Timestamp>,
    /// When its latest run finished.
    pub completed_at: Option<Timestamp>,
}

impl Task {
    /// Create a pending task.
    #[must_use]
    pub fn new(agent_id: AgentId, objective: impl Into<String>) -> Self {
        Self {
            id: TaskId::new(),
            agent_id,
            objective: objective.into(),
            status: TaskStatus::Pending,
            parent_task_id: None,
            created_at: crate::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Mark this task as a child of another.
    #[must_use]
    pub const fn with_parent(mut self, parent: TaskId) -> Self {
        self.parent_task_id = Some(parent);
        self
    }
}

/// One attempt at a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRun {
    /// Identity.
    pub id: TaskRunId,
    /// The task being attempted.
    pub task_id: TaskId,
    /// 1-based attempt number.
    pub attempt: u32,
    /// Current state.
    pub state: TaskState,
    /// Whether this run has ingested untrusted data.
    pub tainted: bool,
    /// Number of model turns taken so far.
    pub steps_taken: u32,
    /// Final answer, when completed.
    pub result: Option<String>,
    /// Failure detail, when failed.
    pub failure: Option<TaskFailure>,
    /// Cumulative input tokens, when the provider reports them.
    pub input_tokens: u64,
    /// Cumulative output tokens, when the provider reports them.
    pub output_tokens: u64,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished.
    pub completed_at: Option<Timestamp>,
}

impl TaskRun {
    /// Begin a run.
    #[must_use]
    pub fn new(task_id: TaskId, attempt: u32) -> Self {
        Self {
            id: TaskRunId::new(),
            task_id,
            attempt,
            state: TaskState::Idle,
            tainted: false,
            steps_taken: 0,
            result: None,
            failure: None,
            input_tokens: 0,
            output_tokens: 0,
            started_at: crate::now(),
            completed_at: None,
        }
    }
}

/// What kind of thing a step recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStepKind {
    /// A model turn.
    Planning,
    /// A tool invocation.
    ToolCall,
    /// An approval request and its outcome.
    Approval,
    /// A verification turn.
    Verification,
    /// A recovery attempt.
    Recovery,
}

/// One entry in a run's execution trace.
///
/// The trace is what the Tasks UI renders and what `agentos task show` prints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskStep {
    /// Identity.
    pub id: TaskStepId,
    /// The run this belongs to.
    pub run_id: TaskRunId,
    /// 1-based ordinal within the run.
    pub ordinal: u32,
    /// What happened.
    pub kind: TaskStepKind,
    /// The state the run was in.
    pub state: TaskState,
    /// Human-readable summary.
    pub summary: String,
    /// The tool execution this step refers to, when applicable.
    pub tool_execution_id: Option<ToolExecutionId>,
    /// Structured detail for the UI.
    pub detail: Option<serde_json::Value>,
    /// When it happened.
    pub at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_reaches_completion() {
        let mut state = TaskState::Idle;
        for trigger in [
            TaskTrigger::Start,
            TaskTrigger::PlanProducedToolCalls,
            TaskTrigger::ToolsCompleted,
            TaskTrigger::ObservationRecorded,
            TaskTrigger::VerificationPassed,
        ] {
            state = transition(state, trigger).unwrap();
        }
        assert_eq!(state, TaskState::Completed);
    }

    #[test]
    fn approval_detour_returns_to_executing() {
        let state = transition(TaskState::Executing, TaskTrigger::ApprovalRequired).unwrap();
        assert_eq!(state, TaskState::WaitingForApproval);
        let state = transition(state, TaskTrigger::ApprovalGranted).unwrap();
        assert_eq!(state, TaskState::Executing);
    }

    #[test]
    fn denied_approval_resumes_execution_rather_than_failing() {
        let state = transition(TaskState::WaitingForApproval, TaskTrigger::ApprovalDenied).unwrap();
        assert_eq!(state, TaskState::Executing);
    }

    #[test]
    fn verification_can_loop_back_to_planning() {
        let state =
            transition(TaskState::Verifying, TaskTrigger::VerificationNeedsMoreWork).unwrap();
        assert_eq!(state, TaskState::Planning);
    }

    #[test]
    fn recovery_returns_to_planning() {
        let state = transition(TaskState::Executing, TaskTrigger::RecoverableError).unwrap();
        assert_eq!(state, TaskState::Recovering);
        let state = transition(state, TaskTrigger::RecoverySucceeded).unwrap();
        assert_eq!(state, TaskState::Planning);
    }

    #[test]
    fn cancel_works_from_every_non_terminal_state() {
        for state in TaskState::ALL {
            let result = transition(state, TaskTrigger::Cancel);
            if state.is_terminal() {
                assert!(result.is_err(), "{state} should reject cancel");
            } else {
                assert_eq!(
                    result.unwrap(),
                    TaskState::Cancelled,
                    "{state} should cancel"
                );
            }
        }
    }

    #[test]
    fn terminal_states_reject_every_trigger() {
        let triggers = [
            TaskTrigger::Start,
            TaskTrigger::PlanProducedToolCalls,
            TaskTrigger::PlanProducedNoToolCalls,
            TaskTrigger::ToolsCompleted,
            TaskTrigger::ApprovalRequired,
            TaskTrigger::ApprovalGranted,
            TaskTrigger::ApprovalDenied,
            TaskTrigger::ObservationRecorded,
            TaskTrigger::VerificationPassed,
            TaskTrigger::VerificationNeedsMoreWork,
            TaskTrigger::RecoverableError,
            TaskTrigger::RecoverySucceeded,
            TaskTrigger::UnrecoverableError,
            TaskTrigger::Cancel,
        ];
        for state in TaskState::ALL.into_iter().filter(|s| s.is_terminal()) {
            for trigger in triggers {
                assert!(
                    transition(state, trigger).is_err(),
                    "{state} must reject {trigger}"
                );
            }
        }
    }

    #[test]
    fn unrecoverable_error_fails_from_any_live_state() {
        for state in TaskState::ALL.into_iter().filter(|s| !s.is_terminal()) {
            assert_eq!(
                transition(state, TaskTrigger::UnrecoverableError).unwrap(),
                TaskState::Failed,
                "{state} should be able to fail"
            );
        }
    }

    #[test]
    fn idle_cannot_skip_planning() {
        assert!(transition(TaskState::Idle, TaskTrigger::ToolsCompleted).is_err());
        assert!(transition(TaskState::Idle, TaskTrigger::VerificationPassed).is_err());
    }

    #[test]
    fn executing_cannot_complete_directly() {
        assert!(transition(TaskState::Executing, TaskTrigger::VerificationPassed).is_err());
    }

    #[test]
    fn recovering_cannot_absorb_another_recoverable_error() {
        // Prevents an infinite recovery loop from being expressible in the table.
        assert!(transition(TaskState::Recovering, TaskTrigger::RecoverableError).is_err());
    }

    #[test]
    fn states_round_trip_through_strings() {
        for state in TaskState::ALL {
            assert_eq!(state.as_str().parse::<TaskState>().unwrap(), state);
        }
    }

    #[test]
    fn run_state_maps_to_task_status() {
        assert_eq!(
            TaskStatus::from_run_state(TaskState::Completed),
            TaskStatus::Succeeded
        );
        assert_eq!(
            TaskStatus::from_run_state(TaskState::Executing),
            TaskStatus::Running
        );
        assert_eq!(
            TaskStatus::from_run_state(TaskState::Idle),
            TaskStatus::Pending
        );
    }
}
