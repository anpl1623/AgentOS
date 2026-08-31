//! The approval gate.
//!
//! Asking a human is a runtime primitive, not a UI feature. The pipeline calls
//! an [`ApprovalGate`]; what that gate does — print a prompt, push a card into a
//! desktop queue, consult a fixed test policy — is somebody else's problem.

use std::fmt;

use agentos_core::approval::ApprovalRequest;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

/// How an approval request was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// A human said yes.
    Approved,
    /// A human said no.
    Denied {
        /// Their note, if any.
        note: Option<String>,
    },
    /// The run was cancelled while waiting.
    Cancelled,
}

impl ApprovalOutcome {
    /// Whether the action may proceed.
    #[must_use]
    pub const fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Somewhere approval requests go to be answered.
#[async_trait]
pub trait ApprovalGate: Send + Sync + fmt::Debug {
    /// Put a request to a human and wait.
    ///
    /// Must return [`ApprovalOutcome::Cancelled`] promptly when `cancel` fires,
    /// so cancelling a run does not leave it blocked on a prompt nobody is
    /// going to answer.
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome;
}

/// Denies everything.
///
/// The correct gate for an unattended context: if nobody is present to approve,
/// the answer to "may I do something consequential?" is no.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllGate;

#[async_trait]
impl ApprovalGate for DenyAllGate {
    async fn request(
        &self,
        _request: &ApprovalRequest,
        _cancel: CancellationToken,
    ) -> ApprovalOutcome {
        ApprovalOutcome::Denied {
            note: Some("no approver is available; running unattended".to_owned()),
        }
    }
}

/// Approves everything. **Tests only.**
///
/// Recording every request it saw, so a test can assert not just that something
/// was approved but that approval was asked for at all.
#[derive(Debug, Default)]
pub struct RecordingGate {
    approve: bool,
    seen: tokio::sync::Mutex<Vec<ApprovalRequest>>,
}

impl RecordingGate {
    /// A gate that approves everything.
    #[must_use]
    pub fn approving() -> Self {
        Self {
            approve: true,
            seen: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// A gate that denies everything but still records what it was asked.
    #[must_use]
    pub fn denying() -> Self {
        Self {
            approve: false,
            seen: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Every request this gate has been asked to resolve.
    pub async fn requests(&self) -> Vec<ApprovalRequest> {
        self.seen.lock().await.clone()
    }

    /// How many requests it has seen.
    pub async fn count(&self) -> usize {
        self.seen.lock().await.len()
    }
}

#[async_trait]
impl ApprovalGate for RecordingGate {
    async fn request(
        &self,
        request: &ApprovalRequest,
        _cancel: CancellationToken,
    ) -> ApprovalOutcome {
        self.seen.lock().await.push(request.clone());
        if self.approve {
            ApprovalOutcome::Approved
        } else {
            ApprovalOutcome::Denied {
                note: Some("denied by test gate".to_owned()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
    use agentos_core::permission::Capability;
    use agentos_core::risk::RiskLevel;

    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: AgentId::new(),
            agent_name: "test".into(),
            task_id: TaskId::new(),
            run_id: TaskRunId::new(),
            tool: "email.send".into(),
            arguments: serde_json::json!({}),
            capability: Capability::new("email", "send"),
            risk: RiskLevel::High,
            reason: "policy".into(),
            explanation: "sends an email".into(),
            affected_resources: vec![],
            tainted: false,
            taint_sources: vec![],
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        }
    }

    #[tokio::test]
    async fn unattended_runs_are_denied() {
        let outcome = DenyAllGate
            .request(&request(), CancellationToken::new())
            .await;
        assert!(!outcome.is_approved());
        assert!(matches!(outcome, ApprovalOutcome::Denied { .. }));
    }

    #[tokio::test]
    async fn recording_gate_captures_requests() {
        let gate = RecordingGate::approving();
        let sent = request();
        assert!(
            gate.request(&sent, CancellationToken::new())
                .await
                .is_approved()
        );

        let seen = gate.requests().await;
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].tool, "email.send");
        assert_eq!(gate.count().await, 1);
    }

    #[tokio::test]
    async fn denying_gate_still_records() {
        let gate = RecordingGate::denying();
        assert!(
            !gate
                .request(&request(), CancellationToken::new())
                .await
                .is_approved()
        );
        assert_eq!(gate.count().await, 1);
    }
}
