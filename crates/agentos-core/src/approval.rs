//! Human-in-the-loop approvals.
//!
//! An approval request is a first-class runtime object, not a UI concern: it is
//! persisted, audited and survives a restart. The desktop app and the CLI are
//! both just renderers of the same request.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
use crate::permission::Capability;
use crate::risk::RiskLevel;

/// Where an approval request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// Waiting on a human.
    #[default]
    Pending,
    /// A human said yes.
    Approved,
    /// A human said no.
    Denied,
    /// Nobody answered in time.
    Expired,
    /// The run was cancelled while waiting.
    Cancelled,
}

impl ApprovalStatus {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the action may proceed.
    #[must_use]
    pub const fn is_approved(self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Everything a human needs in order to decide.
///
/// Spec §9's approval card renders directly from this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Identity.
    pub id: ApprovalId,
    /// The agent asking.
    pub agent_id: AgentId,
    /// Its name, denormalised so the UI need not join.
    pub agent_name: String,
    /// The task.
    pub task_id: TaskId,
    /// The run.
    pub run_id: TaskRunId,
    /// The tool it wants to invoke.
    pub tool: String,
    /// The validated arguments it wants to invoke it with.
    ///
    /// Already schema-checked, so the UI can render them structurally.
    pub arguments: serde_json::Value,
    /// The capability being exercised.
    pub capability: Capability,
    /// Assessed risk, after taint escalation.
    pub risk: RiskLevel,
    /// Why the runtime is asking — the policy rule or escalation that triggered it.
    pub reason: String,
    /// Plain-language explanation of what will happen if approved.
    pub explanation: String,
    /// Resources the action will touch, for display.
    pub affected_resources: Vec<String>,
    /// Whether the run had ingested untrusted data before this request.
    ///
    /// Surfaced prominently: "this agent has read a webpage" changes how a human
    /// should read the request.
    pub tainted: bool,
    /// Current status.
    pub status: ApprovalStatus,
    /// When it was raised.
    pub requested_at: Timestamp,
    /// When it was answered.
    pub decided_at: Option<Timestamp>,
    /// Free-text note from the human.
    pub decision_note: Option<String>,
}

/// A human's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// The request being answered.
    pub approval_id: ApprovalId,
    /// Yes or no.
    pub approved: bool,
    /// Optional note recorded in the audit log.
    pub note: Option<String>,
}

impl ApprovalDecision {
    /// Approve.
    #[must_use]
    pub const fn approve(approval_id: ApprovalId) -> Self {
        Self {
            approval_id,
            approved: true,
            note: None,
        }
    }

    /// Deny.
    #[must_use]
    pub const fn deny(approval_id: ApprovalId) -> Self {
        Self {
            approval_id,
            approved: false,
            note: None,
        }
    }

    /// Attach a note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_approved_status_permits_action() {
        assert!(ApprovalStatus::Approved.is_approved());
        for status in [
            ApprovalStatus::Pending,
            ApprovalStatus::Denied,
            ApprovalStatus::Expired,
            ApprovalStatus::Cancelled,
        ] {
            assert!(!status.is_approved(), "{} must not permit", status.as_str());
        }
    }

    #[test]
    fn decisions_carry_notes() {
        let decision = ApprovalDecision::deny(ApprovalId::new()).with_note("wrong recipient");
        assert!(!decision.approved);
        assert_eq!(decision.note.as_deref(), Some("wrong recipient"));
    }
}
