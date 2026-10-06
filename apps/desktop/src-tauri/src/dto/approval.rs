//! Approval requests and the answer a human sends back.

use agentos_core::approval::ApprovalRequest;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{at, maybe_at};

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
