//! Audit events and the dashboard that gathers them.

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
