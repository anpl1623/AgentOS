//! Tasks, their runs, and everything a run's trace records.

use agentos_core::task::{TaskRun, TaskStep};
use agentos_persistence::ToolExecutionRecord;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ApprovalView, at, maybe_at};

/// A task and the state of its latest attempt.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TaskSummary {
    /// Identity.
    pub id: String,
    /// What was asked for.
    pub objective: String,
    /// Aggregate status.
    pub status: String,
    /// The agent responsible.
    pub agent_name: String,
    /// Its identity, for navigation.
    pub agent_id: String,
    /// When created.
    pub created_at: String,
    /// When the latest run finished.
    pub completed_at: Option<String>,
    /// The latest run, if the task has ever run.
    pub latest_run: Option<RunSummary>,
}

/// One attempt at a task.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RunSummary {
    /// Identity.
    pub id: String,
    /// 1-based attempt number.
    pub attempt: u32,
    /// Where it is in the state machine.
    pub state: String,
    /// Whether it has read untrusted data.
    pub tainted: bool,
    /// Model turns consumed.
    pub steps: u32,
    /// The agent's final report.
    pub result: Option<String>,
    /// Why it failed, if it did.
    pub failure: Option<String>,
    /// Input tokens, when the provider reports them.
    ///
    /// Bound as `number`, not `bigint`: serde emits a JSON number and the IPC
    /// layer hands JavaScript a `number`, so a `bigint` binding would describe a
    /// wire format that does not exist. None of the counts here — tokens,
    /// durations, sequence numbers — comes near 2^53.
    #[ts(type = "number")]
    pub input_tokens: u64,
    /// Output tokens, when the provider reports them.
    #[ts(type = "number")]
    pub output_tokens: u64,
    /// When it started.
    pub started_at: String,
    /// When it finished.
    pub completed_at: Option<String>,
}

impl From<&TaskRun> for RunSummary {
    fn from(run: &TaskRun) -> Self {
        Self {
            id: run.id.to_string(),
            attempt: run.attempt,
            state: run.state.as_str().to_owned(),
            tainted: run.tainted,
            steps: run.steps_taken,
            result: run.result.clone(),
            failure: run.failure.as_ref().map(ToString::to_string),
            input_tokens: run.input_tokens,
            output_tokens: run.output_tokens,
            started_at: at(&run.started_at),
            completed_at: maybe_at(run.completed_at.as_ref()),
        }
    }
}

/// One entry in a run's trace.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct StepView {
    /// Position within the run.
    pub ordinal: u32,
    /// What kind of thing happened.
    pub kind: String,
    /// The state the run was in.
    pub state: String,
    /// Human-readable summary.
    pub summary: String,
    /// The tool execution this refers to, if any.
    pub tool_execution_id: Option<String>,
    /// When it happened.
    pub at: String,
}

impl From<&TaskStep> for StepView {
    fn from(step: &TaskStep) -> Self {
        Self {
            ordinal: step.ordinal,
            kind: format!("{:?}", step.kind).to_lowercase(),
            state: step.state.as_str().to_owned(),
            summary: step.summary.clone(),
            tool_execution_id: step.tool_execution_id.map(|id| id.to_string()),
            at: at(&step.at),
        }
    }
}

/// A tool invocation, with the decision that governed it.
///
/// The permission effect and taint state are recorded as they were *at the time
/// of the call*, so this stays truthful after the policy changes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ExecutionView {
    /// Identity.
    pub id: String,
    /// The tool.
    pub tool: String,
    /// The model's call identifier.
    pub call_id: String,
    /// Validated arguments, as JSON text.
    pub arguments: String,
    /// How it ended.
    pub outcome: String,
    /// Whether it actually ran.
    pub executed: bool,
    /// What the policy decided.
    pub effect: String,
    /// Assessed risk.
    pub risk: String,
    /// Whether the run was tainted at the time.
    pub tainted: bool,
    /// The approval that gated it, if any.
    pub approval_id: Option<String>,
    /// How long it took.
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// Error text, when it failed or was refused.
    pub error: Option<String>,
    /// When it started.
    pub started_at: String,
}

impl From<&ToolExecutionRecord> for ExecutionView {
    fn from(record: &ToolExecutionRecord) -> Self {
        Self {
            id: record.id.to_string(),
            tool: record.tool.clone(),
            call_id: record.call_id.clone(),
            arguments: record.arguments.to_string(),
            outcome: record.outcome.as_str().to_owned(),
            executed: record.outcome.executed(),
            effect: record.effect.as_str().to_owned(),
            risk: record.risk.as_str().to_owned(),
            tainted: record.tainted,
            approval_id: record.approval_id.map(|id| id.to_string()),
            duration_ms: record.duration_ms,
            error: record.error.clone(),
            started_at: at(&record.started_at),
        }
    }
}

/// Everything recorded about one run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TraceView {
    /// The run.
    pub run: RunSummary,
    /// The task's identity.
    pub task_id: String,
    /// The agent that performed it.
    pub agent_name: String,
    /// What it was asked to do.
    pub objective: String,
    /// The ordered trace.
    pub steps: Vec<StepView>,
    /// Tool invocations and their decisions.
    pub executions: Vec<ExecutionView>,
    /// Approvals raised during the run.
    pub approvals: Vec<ApprovalView>,
}

/// The result of starting a task.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct StartedTask {
    /// The task that was created.
    pub task_id: String,
    /// The run that is executing it.
    pub run_id: String,
}
