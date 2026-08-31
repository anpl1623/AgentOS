//! The run-scoped approval gate.
//!
//! Wraps whatever gate the caller supplied — an interactive CLI prompt, a
//! desktop queue, a test double — and adds the parts that must happen no matter
//! which one is in use: the request is persisted before anyone is asked, the run
//! genuinely enters `WaitingForApproval` while they think, and the decision is
//! written back.
//!
//! Persisting first matters. If the process dies while a human is deciding, the
//! request is still there when it comes back, and the audit trail shows what
//! they were shown.

use std::sync::Arc;

use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::task::TaskTrigger;
use agentos_persistence::Database;
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::state::RunStateMachine;

/// Adds persistence and state transitions around an inner gate.
#[derive(Debug)]
pub struct RunApprovalGate {
    inner: Arc<dyn ApprovalGate>,
    database: Database,
    machine: Arc<RunStateMachine>,
}

impl RunApprovalGate {
    /// Wrap a gate for one run.
    #[must_use]
    pub const fn new(
        inner: Arc<dyn ApprovalGate>,
        database: Database,
        machine: Arc<RunStateMachine>,
    ) -> Self {
        Self {
            inner,
            database,
            machine,
        }
    }
}

#[async_trait]
impl ApprovalGate for RunApprovalGate {
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        if let Err(error) = self.database.approvals().insert(request).await {
            // A request we cannot record is a request we must not honour: the
            // approval would happen with no trace that it was ever asked for.
            tracing::error!(%error, "failed to persist approval request");
            return ApprovalOutcome::Denied {
                note: Some("the approval request could not be recorded".to_owned()),
            };
        }

        self.machine.try_apply(TaskTrigger::ApprovalRequired).await;

        let outcome = self.inner.request(request, cancel).await;

        let (status, note, trigger) = match &outcome {
            ApprovalOutcome::Approved => {
                (ApprovalStatus::Approved, None, TaskTrigger::ApprovalGranted)
            }
            ApprovalOutcome::Denied { note } => (
                ApprovalStatus::Denied,
                note.clone(),
                TaskTrigger::ApprovalDenied,
            ),
            ApprovalOutcome::Cancelled => (ApprovalStatus::Cancelled, None, TaskTrigger::Cancel),
        };

        if let Err(error) = self
            .database
            .approvals()
            .decide(request.id, status, note.as_deref())
            .await
        {
            tracing::error!(%error, "failed to record approval decision");
        }

        self.machine.try_apply(trigger).await;
        outcome
    }
}

#[cfg(test)]
mod tests {
    use agentos_audit::AuditLog;
    use agentos_core::agent::{Agent, ModelConfig};
    use agentos_core::ids::ApprovalId;
    use agentos_core::permission::Capability;
    use agentos_core::risk::RiskLevel;
    use agentos_core::task::{Task, TaskRun, TaskState};
    use agentos_tools::RecordingGate;

    use super::*;

    struct Fixture {
        gate: RunApprovalGate,
        database: Database,
        machine: Arc<RunStateMachine>,
        request: ApprovalRequest,
    }

    async fn fixture(inner: Arc<dyn ApprovalGate>) -> Fixture {
        let database = Database::in_memory().await.unwrap();
        let agent = Agent::new("approver", "i", ModelConfig::new("mock", "m"));
        database.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "objective");
        database.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        database.runs().insert(&run).await.unwrap();

        let audit = Arc::new(
            AuditLog::open(Arc::new(agentos_audit::InMemorySink::new()))
                .await
                .unwrap(),
        );
        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Executing,
            database.clone(),
            audit,
        ));

        let request = ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: agent.id,
            agent_name: agent.name.clone(),
            task_id: task.id,
            run_id: run.id,
            tool: "filesystem.write".into(),
            arguments: serde_json::json!({"path": "x"}),
            capability: Capability::new("filesystem", "write"),
            risk: RiskLevel::High,
            reason: "policy requires approval".into(),
            explanation: "writes a file".into(),
            affected_resources: vec![],
            tainted: false,
            taint_sources: vec![],
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        };

        Fixture {
            gate: RunApprovalGate::new(inner, database.clone(), machine.clone()),
            database,
            machine,
            request,
        }
    }

    #[tokio::test]
    async fn an_approval_is_persisted_before_anyone_is_asked() {
        // The inner gate asserts that the row already exists at the moment it is
        // consulted, which is the property that survives a crash mid-decision.
        #[derive(Debug)]
        struct AssertingGate(Database);

        #[async_trait]
        impl ApprovalGate for AssertingGate {
            async fn request(
                &self,
                request: &ApprovalRequest,
                _cancel: CancellationToken,
            ) -> ApprovalOutcome {
                let stored = self.0.approvals().get(request.id).await;
                assert!(stored.is_ok(), "request was not persisted before asking");
                assert_eq!(stored.unwrap().status, ApprovalStatus::Pending);
                ApprovalOutcome::Approved
            }
        }

        let database = Database::in_memory().await.unwrap();
        let mut fixture = fixture(Arc::new(RecordingGate::approving())).await;
        let _ = database;
        fixture.gate = RunApprovalGate::new(
            Arc::new(AssertingGate(fixture.database.clone())),
            fixture.database.clone(),
            fixture.machine.clone(),
        );

        let outcome = fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;
        assert!(outcome.is_approved());
    }

    #[tokio::test]
    async fn approval_moves_the_run_through_waiting_and_back() {
        let fixture = fixture(Arc::new(RecordingGate::approving())).await;
        assert_eq!(fixture.machine.current().await, TaskState::Executing);

        fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;

        // Ends where it started, having genuinely passed through waiting.
        assert_eq!(fixture.machine.current().await, TaskState::Executing);
        let stored = fixture
            .database
            .approvals()
            .get(fixture.request.id)
            .await
            .unwrap();
        assert_eq!(stored.status, ApprovalStatus::Approved);
        assert!(stored.decided_at.is_some());
    }

    #[tokio::test]
    async fn denial_is_recorded_with_its_note() {
        let fixture = fixture(Arc::new(RecordingGate::denying())).await;
        let outcome = fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;

        assert!(!outcome.is_approved());
        let stored = fixture
            .database
            .approvals()
            .get(fixture.request.id)
            .await
            .unwrap();
        assert_eq!(stored.status, ApprovalStatus::Denied);
        assert_eq!(stored.decision_note.as_deref(), Some("denied by test gate"));
        assert_eq!(fixture.machine.current().await, TaskState::Executing);
    }

    #[tokio::test]
    async fn cancelling_while_waiting_cancels_the_run() {
        #[derive(Debug)]
        struct CancellingGate;

        #[async_trait]
        impl ApprovalGate for CancellingGate {
            async fn request(
                &self,
                _request: &ApprovalRequest,
                _cancel: CancellationToken,
            ) -> ApprovalOutcome {
                ApprovalOutcome::Cancelled
            }
        }

        let mut fixture = fixture(Arc::new(RecordingGate::approving())).await;
        fixture.gate = RunApprovalGate::new(
            Arc::new(CancellingGate),
            fixture.database.clone(),
            fixture.machine.clone(),
        );

        fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;

        assert_eq!(fixture.machine.current().await, TaskState::Cancelled);
        assert_eq!(
            fixture
                .database
                .approvals()
                .get(fixture.request.id)
                .await
                .unwrap()
                .status,
            ApprovalStatus::Cancelled
        );
    }
}
