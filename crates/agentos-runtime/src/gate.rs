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
//! they were shown. Recording the answer matters as much: a yes that cannot be
//! written back is not honoured, for the same reason a request that cannot be
//! written is not asked.
//!
//! It is also where a run's approval budget is kept. Every request the run
//! raises passes through here, so the count stamped on the card a person reads
//! is the count the budget is enforced against. The budget bounds how often a
//! person is asked, so a request the inner gate settles by its own rule, such
//! as the CLI's auto-approval ceiling, does not spend it. Once the budget is
//! spent the gate stops asking: the request is recorded as refused and nobody
//! is shown it. There is no bulk approval, no select-all and no "always allow" anywhere
//! behind this gate, and there must not be one. A run that wants more than its
//! budget is a policy that asks too often, and the fix is an operator editing
//! the policy in Settings — deliberately slower than clicking a card.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use agentos_core::approval::{ApprovalRequest, ApprovalStatus, clean_decision_note};
use agentos_core::task::TaskTrigger;
use agentos_permissions::ApprovalPolicy;
use agentos_persistence::Database;
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::state::RunStateMachine;

/// Adds persistence, state transitions and the approval budget around an
/// inner gate.
#[derive(Debug)]
pub struct RunApprovalGate {
    inner: Arc<dyn ApprovalGate>,
    database: Database,
    machine: Arc<RunStateMachine>,
    budget: Option<u32>,
    asked: AtomicU32,
}

impl RunApprovalGate {
    /// Wrap a gate for one run, with the default approval budget.
    ///
    /// The default rather than none, so a gate somebody builds without
    /// consulting a policy is held to the same ceiling a policy that names no
    /// budget gets.
    #[must_use]
    pub fn new(
        inner: Arc<dyn ApprovalGate>,
        database: Database,
        machine: Arc<RunStateMachine>,
    ) -> Self {
        Self {
            inner,
            database,
            machine,
            budget: ApprovalPolicy::default().max_per_run,
            asked: AtomicU32::new(0),
        }
    }

    /// Set the budget from the run's policy. `None` lifts the limit.
    #[must_use]
    pub const fn with_budget(mut self, budget: Option<u32>) -> Self {
        self.budget = budget;
        self
    }

    /// Record a request the budget refuses, without putting it to anyone.
    ///
    /// Written already decided, in one statement, so it never appears in the
    /// pending queue even for the moment between an insert and a decision.
    async fn refuse_over_budget(
        &self,
        mut request: ApprovalRequest,
        budget: u32,
    ) -> ApprovalOutcome {
        let note = format!("this run has asked {budget} times; the policy budget is {budget}");
        request.status = ApprovalStatus::Denied;
        request.decided_at = Some(agentos_core::now());
        request.decision_note = Some(note.clone());
        if let Err(error) = self.database.approvals().insert(&request).await {
            tracing::error!(%error, "failed to record an approval refused by the budget");
        }
        tracing::warn!(
            run = %request.run_id,
            tool = %request.tool,
            budget,
            "approval budget spent; refusing without asking"
        );
        ApprovalOutcome::OverBudget { note }
    }
}

/// The outcome with its note made fit to keep, by every client alike.
fn with_clean_note(outcome: ApprovalOutcome) -> ApprovalOutcome {
    let clean = |note: Option<String>| note.as_deref().and_then(clean_decision_note);
    match outcome {
        ApprovalOutcome::Approved { note } => ApprovalOutcome::Approved { note: clean(note) },
        ApprovalOutcome::Denied { note } => ApprovalOutcome::Denied { note: clean(note) },
        other => other,
    }
}

#[async_trait]
impl ApprovalGate for RunApprovalGate {
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        // Only a request that will reach a person counts against the budget,
        // and only such a request can be refused by it. One settled by rule
        // carries the count so far, so the record says where the run stood.
        let asks = self.inner.will_ask(request);
        let ordinal = if asks {
            self.asked.fetch_add(1, Ordering::SeqCst).saturating_add(1)
        } else {
            self.asked.load(Ordering::SeqCst)
        };
        let mut request = request.clone();
        request.asked_this_run = ordinal;
        request.approval_budget = self.budget;

        if asks
            && let Some(budget) = self.budget
            && ordinal > budget
        {
            return self.refuse_over_budget(request, budget).await;
        }

        if let Err(error) = self.database.approvals().insert(&request).await {
            // A request we cannot record is a request we must not honour: the
            // approval would happen with no trace that it was ever asked for.
            tracing::error!(%error, "failed to persist approval request");
            return ApprovalOutcome::Denied {
                note: Some("the approval request could not be recorded".to_owned()),
            };
        }

        self.machine.try_apply(TaskTrigger::ApprovalRequired).await;

        let mut outcome = with_clean_note(self.inner.request(&request, cancel).await);

        let (status, note) = match &outcome {
            ApprovalOutcome::Approved { note } => (ApprovalStatus::Approved, note.clone()),
            ApprovalOutcome::Denied { note } => (ApprovalStatus::Denied, note.clone()),
            ApprovalOutcome::OverBudget { note } => (ApprovalStatus::Denied, Some(note.clone())),
            ApprovalOutcome::Cancelled => (ApprovalStatus::Cancelled, None),
        };

        if let Err(error) = self
            .database
            .approvals()
            .decide(request.id, status, note.as_deref())
            .await
        {
            // The row is no longer pending, or cannot be written. Either way
            // the record would say one thing and the run do another: most
            // often something else closed the request, believing the run that
            // asked had ended. A yes is therefore not honoured. A no or a
            // cancellation stands, since neither lets anything happen.
            tracing::error!(%error, "failed to record approval decision");
            if outcome.is_approved() {
                outcome = ApprovalOutcome::Denied {
                    note: Some(
                        "the approval could not be recorded, so it was not acted on".to_owned(),
                    ),
                };
            }
        }

        let trigger = match &outcome {
            ApprovalOutcome::Approved { .. } => TaskTrigger::ApprovalGranted,
            ApprovalOutcome::Denied { .. } | ApprovalOutcome::OverBudget { .. } => {
                TaskTrigger::ApprovalDenied
            }
            ApprovalOutcome::Cancelled => TaskTrigger::Cancel,
        };
        self.machine.try_apply(trigger).await;
        outcome
    }

    fn will_ask(&self, request: &ApprovalRequest) -> bool {
        self.inner.will_ask(request)
    }
}

#[cfg(test)]
mod tests {
    use agentos_audit::AuditLog;
    use agentos_core::agent::{Agent, ModelConfig};
    use agentos_core::ids::ApprovalId;
    use agentos_core::permission::{Capability, Effect};
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
            effect_before_taint: Effect::Ask,
            asked_this_run: 0,
            approval_budget: None,
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
                ApprovalOutcome::Approved { note: None }
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
    async fn an_approval_is_recorded_with_its_note() {
        #[derive(Debug)]
        struct NotingGate;

        #[async_trait]
        impl ApprovalGate for NotingGate {
            async fn request(
                &self,
                _request: &ApprovalRequest,
                _cancel: CancellationToken,
            ) -> ApprovalOutcome {
                ApprovalOutcome::Approved {
                    note: Some("the customer asked for this".into()),
                }
            }
        }

        let mut fixture = fixture(Arc::new(RecordingGate::approving())).await;
        fixture.gate = RunApprovalGate::new(
            Arc::new(NotingGate),
            fixture.database.clone(),
            fixture.machine.clone(),
        );
        let outcome = fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;
        assert!(outcome.is_approved());

        let stored = fixture
            .database
            .approvals()
            .get(fixture.request.id)
            .await
            .unwrap();
        assert_eq!(stored.status, ApprovalStatus::Approved);
        assert_eq!(
            stored.decision_note.as_deref(),
            Some("the customer asked for this")
        );
    }

    /// A fresh request in the fixture's run.
    fn another(fixture: &Fixture) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            ..fixture.request.clone()
        }
    }

    #[tokio::test]
    async fn the_request_a_person_sees_says_where_the_run_is_in_its_budget() {
        let inner = Arc::new(RecordingGate::approving());
        let mut fixture = fixture(inner.clone()).await;
        fixture.gate = RunApprovalGate::new(
            inner.clone(),
            fixture.database.clone(),
            fixture.machine.clone(),
        )
        .with_budget(Some(10));

        for _ in 0..3 {
            let request = another(&fixture);
            fixture
                .gate
                .request(&request, CancellationToken::new())
                .await;
        }

        let seen = inner.requests().await;
        assert_eq!(
            seen.iter().map(|r| r.asked_this_run).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(seen.iter().all(|r| r.approval_budget == Some(10)));

        // And the row says the same as the card.
        let stored = fixture.database.approvals().get(seen[2].id).await.unwrap();
        assert_eq!(stored.asked_this_run, 3);
        assert_eq!(stored.approval_budget, Some(10));
    }

    #[tokio::test]
    async fn the_eleventh_request_in_a_run_is_refused_without_asking() {
        let inner = Arc::new(RecordingGate::approving());
        let mut fixture = fixture(inner.clone()).await;
        // The default budget, as a policy that names none gets.
        fixture.gate = RunApprovalGate::new(
            inner.clone(),
            fixture.database.clone(),
            fixture.machine.clone(),
        );

        for _ in 0..10 {
            let request = another(&fixture);
            assert!(
                fixture
                    .gate
                    .request(&request, CancellationToken::new())
                    .await
                    .is_approved()
            );
        }
        assert_eq!(inner.count().await, 10);

        let eleventh = another(&fixture);
        let outcome = fixture
            .gate
            .request(&eleventh, CancellationToken::new())
            .await;

        // Nobody was asked, and the outcome says so rather than passing for
        // a person's no.
        assert_eq!(inner.count().await, 10);
        let ApprovalOutcome::OverBudget { note } = outcome else {
            panic!("expected a refusal by the budget, got {outcome:?}");
        };
        assert!(note.contains("asked 10 times"), "{note}");
        assert!(note.contains("budget is 10"), "{note}");

        // It is on the record as refused, and was never pending.
        let stored = fixture.database.approvals().get(eleventh.id).await.unwrap();
        assert_eq!(stored.status, ApprovalStatus::Denied);
        assert_eq!(stored.decision_note.as_deref(), Some(note.as_str()));
        assert_eq!(stored.asked_this_run, 11);
        assert_eq!(stored.approval_budget, Some(10));
        assert!(
            fixture
                .database
                .approvals()
                .list_pending()
                .await
                .unwrap()
                .is_empty()
        );
        // The run never entered waiting for it.
        assert_eq!(fixture.machine.current().await, TaskState::Executing);

        // And the budget stays spent.
        let twelfth = another(&fixture);
        assert!(
            !fixture
                .gate
                .request(&twelfth, CancellationToken::new())
                .await
                .is_approved()
        );
        assert_eq!(inner.count().await, 10);
    }

    /// Approves everything, asking a person only above low risk, as the CLI
    /// does with `--auto-approve-up-to low`.
    #[derive(Debug, Default)]
    struct CeilingGate {
        asked: AtomicU32,
    }

    #[async_trait]
    impl ApprovalGate for CeilingGate {
        async fn request(
            &self,
            request: &ApprovalRequest,
            _cancel: CancellationToken,
        ) -> ApprovalOutcome {
            if self.will_ask(request) {
                self.asked.fetch_add(1, Ordering::SeqCst);
            }
            ApprovalOutcome::Approved { note: None }
        }

        fn will_ask(&self, request: &ApprovalRequest) -> bool {
            request.risk > RiskLevel::Low
        }
    }

    #[tokio::test]
    async fn a_request_settled_without_asking_does_not_spend_the_budget() {
        let inner = Arc::new(CeilingGate::default());
        let mut fixture = fixture(inner.clone()).await;
        fixture.gate = RunApprovalGate::new(
            inner.clone(),
            fixture.database.clone(),
            fixture.machine.clone(),
        )
        .with_budget(Some(2));

        let low = |fixture: &Fixture| ApprovalRequest {
            risk: RiskLevel::Low,
            ..another(fixture)
        };

        for _ in 0..2 {
            let asked = another(&fixture);
            let outcome = fixture.gate.request(&asked, CancellationToken::new()).await;
            assert!(outcome.is_approved());
        }
        // Far more settled by rule than the budget allows asked.
        for _ in 0..12 {
            let settled = low(&fixture);
            let outcome = fixture
                .gate
                .request(&settled, CancellationToken::new())
                .await;
            assert!(outcome.is_approved(), "{outcome:?}");
            let stored = fixture.database.approvals().get(settled.id).await.unwrap();
            assert_eq!(stored.status, ApprovalStatus::Approved);
            assert_eq!(stored.asked_this_run, 2);
        }
        assert_eq!(inner.asked.load(Ordering::SeqCst), 2);

        // The third request a person would see is the one the budget refuses.
        let third = another(&fixture);
        let outcome = fixture.gate.request(&third, CancellationToken::new()).await;
        assert!(
            matches!(outcome, ApprovalOutcome::OverBudget { .. }),
            "{outcome:?}"
        );
        assert_eq!(inner.asked.load(Ordering::SeqCst), 2);
        let stored = fixture.database.approvals().get(third.id).await.unwrap();
        assert_eq!(stored.asked_this_run, 3);
    }

    #[tokio::test]
    async fn a_yes_that_cannot_be_recorded_is_not_honoured() {
        // While the person decides, something else closes the request as
        // belonging to a dead run: what a reaper in another process did before
        // the run lock existed.
        #[derive(Debug)]
        struct ClosedMeanwhile(Database);

        #[async_trait]
        impl ApprovalGate for ClosedMeanwhile {
            async fn request(
                &self,
                request: &ApprovalRequest,
                _cancel: CancellationToken,
            ) -> ApprovalOutcome {
                let mut run = self.0.runs().get(request.run_id).await.unwrap();
                run.state = TaskState::Failed;
                self.0.runs().update(&run).await.unwrap();
                let closed = self
                    .0
                    .approvals()
                    .expire_for_finished_runs("the run that asked had ended")
                    .await
                    .unwrap();
                assert_eq!(closed, 1);
                ApprovalOutcome::Approved {
                    note: Some("go ahead".into()),
                }
            }
        }

        let mut fixture = fixture(Arc::new(RecordingGate::approving())).await;
        fixture.gate = RunApprovalGate::new(
            Arc::new(ClosedMeanwhile(fixture.database.clone())),
            fixture.database.clone(),
            fixture.machine.clone(),
        );
        let outcome = fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;

        assert!(!outcome.is_approved(), "{outcome:?}");
        let ApprovalOutcome::Denied { note: Some(note) } = outcome else {
            panic!("expected a denial with a reason, got {outcome:?}");
        };
        assert!(note.contains("could not be recorded"), "{note}");
        // The record is left as the other party closed it, and the run is
        // not moved on as though it had been approved.
        let stored = fixture
            .database
            .approvals()
            .get(fixture.request.id)
            .await
            .unwrap();
        assert_eq!(stored.status, ApprovalStatus::Expired);
    }

    #[tokio::test]
    async fn a_note_is_made_fit_to_keep_whichever_client_wrote_it() {
        #[derive(Debug)]
        struct Pasting;

        #[async_trait]
        impl ApprovalGate for Pasting {
            async fn request(
                &self,
                _request: &ApprovalRequest,
                _cancel: CancellationToken,
            ) -> ApprovalOutcome {
                ApprovalOutcome::Denied {
                    note: Some(format!(
                        "\u{1b}[31mwrong recipient\u{1b}[0m {}",
                        "x".repeat(agentos_core::approval::MAX_DECISION_NOTE_CHARS * 10)
                    )),
                }
            }
        }

        let mut fixture = fixture(Arc::new(RecordingGate::approving())).await;
        fixture.gate = RunApprovalGate::new(
            Arc::new(Pasting),
            fixture.database.clone(),
            fixture.machine.clone(),
        );
        let outcome = fixture
            .gate
            .request(&fixture.request, CancellationToken::new())
            .await;

        let ApprovalOutcome::Denied { note: Some(note) } = outcome else {
            panic!("expected a denial with a note, got {outcome:?}");
        };
        assert!(!note.contains('\u{1b}'), "{note:?}");
        assert!(note.starts_with("[31mwrong recipient"), "{note:?}");
        assert_eq!(
            note.chars().count(),
            agentos_core::approval::MAX_DECISION_NOTE_CHARS + 1
        );
        let stored = fixture
            .database
            .approvals()
            .get(fixture.request.id)
            .await
            .unwrap();
        assert_eq!(stored.decision_note.as_deref(), Some(note.as_str()));
    }

    #[tokio::test]
    async fn a_policy_that_lifts_the_budget_keeps_asking() {
        let inner = Arc::new(RecordingGate::approving());
        let mut fixture = fixture(inner.clone()).await;
        fixture.gate = RunApprovalGate::new(
            inner.clone(),
            fixture.database.clone(),
            fixture.machine.clone(),
        )
        .with_budget(None);

        for _ in 0..12 {
            let request = another(&fixture);
            assert!(
                fixture
                    .gate
                    .request(&request, CancellationToken::new())
                    .await
                    .is_approved()
            );
        }
        assert_eq!(inner.count().await, 12);
        assert!(
            inner
                .requests()
                .await
                .iter()
                .all(|r| r.approval_budget.is_none())
        );
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
