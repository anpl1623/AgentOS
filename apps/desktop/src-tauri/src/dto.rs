//! The typed contract between the runtime and the user interface.
//!
//! These are view models, not domain types. They exist for three reasons:
//!
//! * The interface wants denormalised, display-ready data — a task carries its
//!   agent's name, an execution carries the decision that let it run — and
//!   reshaping in the browser means every screen reinventing the same joins.
//! * `agentos-core` should not grow a TypeScript-binding dependency. A frontend
//!   concern has no business in the domain crate.
//! * A view model can change to suit a screen without touching anything the
//!   permission engine or the audit chain depends on.
//!
//! Every type here derives [`ts_rs::TS`] and exports into `src/bindings`, so the
//! TypeScript definitions are generated from these declarations rather than
//! written by hand. `cargo test -p agentos-desktop` regenerates them; a shape
//! that drifts fails to compile on the other side.
//!
//! Identifiers and timestamps cross as strings. The interface formats them
//! anyway, and it keeps the wire format explicit rather than dependent on how
//! two serialisation libraries happen to agree.

use agentos_core::agent::Agent;
use agentos_core::approval::ApprovalRequest;
use agentos_core::task::{Task, TaskRun, TaskStep};
use agentos_core::tool::ToolMetadata;
use agentos_persistence::ToolExecutionRecord;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Render a timestamp for the interface.
fn at(value: &agentos_core::Timestamp) -> String {
    agentos_core::format_timestamp(value)
}

/// Render an optional timestamp.
fn maybe_at(value: Option<&agentos_core::Timestamp>) -> Option<String> {
    value.map(at)
}

/// An agent, as the list and dashboard show it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AgentSummary {
    /// Identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Provider identifier.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// `enabled` or `disabled`.
    pub status: String,
    /// Tools this agent has been given.
    pub tools: Vec<String>,
    /// Model turns allowed per run.
    pub max_steps: u32,
    /// When it was created.
    pub created_at: String,
}

impl From<&Agent> for AgentSummary {
    fn from(agent: &Agent) -> Self {
        Self {
            id: agent.id.to_string(),
            name: agent.name.clone(),
            provider: agent.model.provider.clone(),
            model: agent.model.model.clone(),
            status: agent.status.as_str().to_owned(),
            tools: agent.enabled_tools.clone(),
            max_steps: agent.max_steps,
            created_at: at(&agent.created_at),
        }
    }
}

/// An agent with everything the detail screen needs.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AgentDetail {
    /// The summary fields.
    pub summary: AgentSummary,
    /// The agent's system instructions.
    pub instructions: String,
    /// Its policy, if one is stored.
    pub policy: Option<PolicyView>,
    /// Recent tasks, newest first.
    pub recent_tasks: Vec<TaskSummary>,
    /// Where relative paths resolve for this agent.
    pub workspace: String,
}

/// A policy, summarised so the interface can show what it grants without
/// reimplementing the policy engine.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct PolicyView {
    /// The YAML source, exactly as the operator wrote it.
    pub document: String,
    /// Incremented on every save.
    #[ts(type = "number")]
    pub version: i64,
    /// What happens when no rule matches.
    pub default_effect: String,
    /// Global risk ceiling, if set.
    pub max_risk: Option<String>,
    /// Whether reading untrusted data raises the approval bar.
    pub taint_enabled: bool,
    /// The risk level at which that escalation begins.
    pub taint_threshold: String,
    /// One line per rule, in the engine's own words.
    pub rules: Vec<String>,
}

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

/// An approval request, carrying everything the card needs to show.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ApprovalView {
    /// Identity.
    pub id: String,
    /// The agent asking.
    pub agent_name: String,
    /// The task it is working on.
    pub task_id: String,
    /// The run.
    pub run_id: String,
    /// The objective, for context.
    pub objective: String,
    /// The tool it wants to invoke.
    pub tool: String,
    /// The validated arguments, as JSON text.
    pub arguments: String,
    /// Assessed risk.
    pub risk: String,
    /// Why the runtime is asking.
    pub reason: String,
    /// Plain-language description of what will happen.
    pub explanation: String,
    /// Resources the action touches.
    pub affected_resources: Vec<String>,
    /// Whether the agent has read untrusted data during this run.
    pub tainted: bool,
    /// Where that data came from.
    pub taint_sources: Vec<String>,
    /// Current status.
    pub status: String,
    /// When it was raised.
    pub requested_at: String,
    /// When it was answered.
    pub decided_at: Option<String>,
    /// The human's note.
    pub note: Option<String>,
}

impl ApprovalView {
    /// Build a view, given the objective the run is pursuing.
    #[must_use]
    pub fn new(request: &ApprovalRequest, objective: String) -> Self {
        Self {
            id: request.id.to_string(),
            agent_name: request.agent_name.clone(),
            task_id: request.task_id.to_string(),
            run_id: request.run_id.to_string(),
            objective,
            tool: request.tool.clone(),
            arguments: serde_json::to_string_pretty(&request.arguments)
                .unwrap_or_else(|_| request.arguments.to_string()),
            risk: request.risk.as_str().to_owned(),
            reason: request.reason.clone(),
            explanation: request.explanation.clone(),
            affected_resources: request.affected_resources.clone(),
            tainted: request.tainted,
            taint_sources: request.taint_sources.clone(),
            status: request.status.as_str().to_owned(),
            requested_at: at(&request.requested_at),
            decided_at: maybe_at(request.decided_at.as_ref()),
            note: request.decision_note.clone(),
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

/// A tool, as the catalogue and the agent editor show it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ToolView {
    /// Fully-qualified name.
    pub name: String,
    /// Domain half of the name.
    pub domain: String,
    /// What it does, in the words the model sees.
    pub description: String,
    /// Baseline risk.
    pub risk: String,
    /// Whether its output can be attacker-controlled.
    ///
    /// The single most useful thing to know when deciding what to grant, so it
    /// is surfaced rather than buried in the description.
    pub returns_untrusted_data: bool,
}

impl From<&ToolMetadata> for ToolView {
    fn from(metadata: &ToolMetadata) -> Self {
        Self {
            name: metadata.name.clone(),
            domain: metadata.domain().to_owned(),
            description: metadata.description.clone(),
            risk: metadata.risk.as_str().to_owned(),
            returns_untrusted_data: metadata.returns_untrusted_data,
        }
    }
}

/// One audit event, flattened for the activity feed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct EventView {
    /// Identity.
    pub id: String,
    /// Position in the audit chain, when it came from storage.
    #[ts(type = "number | null")]
    pub sequence: Option<u64>,
    /// When it happened.
    pub at: String,
    /// The dotted event name.
    pub kind: String,
    /// The run it belongs to.
    pub run_id: Option<String>,
    /// The task it belongs to.
    pub task_id: Option<String>,
    /// A one-line description.
    pub summary: String,
    /// Whether this records a refusal, an escalation or a rejection.
    pub security_relevant: bool,
}

/// The dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DashboardView {
    /// Every configured agent.
    pub agents: Vec<AgentSummary>,
    /// Tasks whose latest run is still going.
    pub running_tasks: Vec<TaskSummary>,
    /// Approvals waiting on a human.
    pub pending_approvals: Vec<ApprovalView>,
    /// The most recent activity.
    pub recent_events: Vec<EventView>,
    /// Recent tool calls that were refused.
    pub recent_refusals: Vec<ExecutionView>,
    /// How many events the audit log holds.
    #[ts(type = "number")]
    pub audit_events: i64,
    /// Whether the audit chain verifies.
    pub audit_intact: bool,
}

/// A model provider and whether it can be used.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ProviderView {
    /// Identifier.
    pub id: String,
    /// Whether a credential is available.
    pub configured: bool,
    /// A redacted hint, never the key.
    pub hint: Option<String>,
    /// Which store supplied it.
    pub source: Option<String>,
    /// Guidance when it is not configured.
    pub note: String,
}

/// The settings screen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SettingsView {
    /// Where AgentOS keeps its state.
    pub data_dir: String,
    /// Agent workspaces.
    pub workspace: String,
    /// The database file.
    pub database: String,
    /// Whether the OS keychain is usable here.
    pub keychain_available: bool,
    /// Why not, when it is not.
    pub keychain_reason: Option<String>,
    /// Providers and their credential status.
    pub providers: Vec<ProviderView>,
    /// The browser executable that will be driven, if one was found.
    pub browser_path: Option<String>,
    /// Guidance when no browser is installed.
    pub browser_hint: Option<String>,
    /// Every tool an agent can be granted.
    pub tools: Vec<ToolView>,
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

/// What the interface sends back when a human answers an approval.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ApprovalDecisionInput {
    /// The request being answered.
    pub approval_id: String,
    /// Yes or no.
    pub approved: bool,
    /// An optional note, recorded in the audit log.
    pub note: Option<String>,
}

/// What the interface sends to create an agent.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CreateAgentInput {
    /// Unique name.
    pub name: String,
    /// System instructions.
    pub instructions: String,
    /// Provider identifier.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Base URL override, for OpenAI-compatible endpoints.
    pub base_url: Option<String>,
    /// Tools to grant.
    pub tools: Vec<String>,
}

/// The outcome of checking a policy document without installing it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct PolicyCheck {
    /// Whether it compiles.
    pub valid: bool,
    /// Why not, when it does not.
    pub error: Option<String>,
    /// What it grants, when it does.
    pub summary: Option<PolicyView>,
}

/// Build a one-line summary of an audit event payload.
///
/// Mirrors what the CLI shows, so the two clients describe the same event the
/// same way.
#[must_use]
pub fn summarise_event(payload: &serde_json::Value) -> String {
    if let (Some(from), Some(to)) = (
        payload.get("from").and_then(serde_json::Value::as_str),
        payload.get("to").and_then(serde_json::Value::as_str),
    ) {
        return format!("{from} → {to}");
    }

    for name in ["tool", "objective", "reason", "error", "summary"] {
        if let Some(value) = payload.get(name).and_then(serde_json::Value::as_str)
            && !value.is_empty()
        {
            return value.to_owned();
        }
    }

    if let Some(model) = payload.get("model").and_then(serde_json::Value::as_str) {
        let provider = payload
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return format!("{provider}/{model}");
    }

    String::new()
}

/// Build a [`TaskSummary`] from its parts.
#[must_use]
pub fn task_summary(task: &Task, agent_name: &str, latest_run: Option<&TaskRun>) -> TaskSummary {
    TaskSummary {
        id: task.id.to_string(),
        objective: task.objective.clone(),
        status: task.status.as_str().to_owned(),
        agent_name: agent_name.to_owned(),
        agent_id: task.agent_id.to_string(),
        created_at: at(&task.created_at),
        completed_at: maybe_at(task.completed_at.as_ref()),
        latest_run: latest_run.map(RunSummary::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_summaries_prefer_the_most_specific_field() {
        let transition = serde_json::json!({"from": "idle", "to": "planning"});
        assert_eq!(summarise_event(&transition), "idle → planning");

        let tool = serde_json::json!({"tool": "browser.navigate", "risk": "medium"});
        assert_eq!(summarise_event(&tool), "browser.navigate");

        let model = serde_json::json!({"provider": "anthropic", "model": "claude-opus-5"});
        assert_eq!(summarise_event(&model), "anthropic/claude-opus-5");

        assert_eq!(summarise_event(&serde_json::json!({})), "");
    }

    #[test]
    fn an_execution_view_says_whether_it_actually_ran() {
        use agentos_core::ids::{TaskRunId, ToolExecutionId};
        use agentos_core::permission::Effect;
        use agentos_core::risk::RiskLevel;
        use agentos_core::tool::ToolOutcome;

        let record = ToolExecutionRecord {
            id: ToolExecutionId::new(),
            run_id: TaskRunId::new(),
            tool: "terminal.exec".into(),
            call_id: "c1".into(),
            arguments: serde_json::json!({"program": "curl"}),
            outcome: ToolOutcome::Denied,
            effect: Effect::Deny,
            risk: RiskLevel::High,
            tainted: true,
            approval_id: None,
            output_bytes: 0,
            error: Some("permission denied".into()),
            duration_ms: 0,
            started_at: agentos_core::now(),
            completed_at: None,
        };

        let view = ExecutionView::from(&record);
        assert!(!view.executed, "a denied call must not read as executed");
        assert_eq!(view.outcome, "denied");
        assert_eq!(view.effect, "deny");
        assert!(view.tainted);
    }
}
