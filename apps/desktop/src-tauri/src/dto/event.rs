//! Audit events as the feed shows them, and the dashboard.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{AgentSummary, ApprovalView, ExecutionView, TaskSummary};

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
///
/// Polled every few seconds while the window is visible, so it holds only what
/// is cheap to read and changes on that timescale. The audit chain's health is
/// deliberately not here: it has its own command on its own slower schedule.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DashboardView {
    /// Every configured agent.
    pub agents: Vec<AgentSummary>,
    /// Tasks whose latest run is still going.
    pub running_tasks: Vec<TaskSummary>,
    /// Approvals waiting on a human.
    pub pending_approvals: Vec<ApprovalView>,
    /// Recent tool calls that were refused.
    pub recent_refusals: Vec<ExecutionView>,
    /// Recent tasks whose latest attempt failed, or that were abandoned.
    ///
    /// The run's failure text and identity travel on `latest_run`; a task the
    /// scheduler abandoned before it ever ran has none.
    pub recent_failures: Vec<TaskSummary>,
}
