//! What an operator changes reaches the audit chain, a run's approval budget
//! comes from its policy, and requests a dead run left behind are closed.
//!
//! These are the runtime-level halves of guarantees the clients rely on: the
//! CLI and the desktop both go through these methods, so a test here is a test
//! of both.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use agentos_audit::AuditRecord;
use agentos_core::agent::ModelConfig;
use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::ids::ApprovalId;
use agentos_core::permission::{Capability, Effect};
use agentos_core::risk::RiskLevel;
use agentos_core::task::{TaskRun, TaskState};
use agentos_core::tool::ToolOutcome;
use agentos_providers::{MockProvider, ScriptedTurn};
use agentos_runtime::{FixedProviderFactory, Runtime, RuntimeError};
use agentos_secrets::{InMemorySecretStore, SecretStore, provider_key};
use agentos_tools::RecordingGate;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

async fn runtime() -> (Runtime, Arc<InMemorySecretStore>, TempDir) {
    let guard = TempDir::new().unwrap();
    let root = std::fs::canonicalize(guard.path()).unwrap();
    let secrets = Arc::new(InMemorySecretStore::new());
    let runtime = Runtime::in_memory(root, secrets.clone()).await.unwrap();
    (runtime, secrets, guard)
}

async fn records(runtime: &Runtime) -> Vec<AuditRecord> {
    runtime.database().audit_sink().all().await.unwrap()
}

fn of_kind<'a>(records: &'a [AuditRecord], kind: &str) -> Vec<&'a AuditRecord> {
    records
        .iter()
        .filter(|record| record.kind == kind)
        .collect()
}

/// How many records of `kind` one change added.
async fn added(runtime: &Runtime, before: usize, kind: &str) -> usize {
    let all = records(runtime).await;
    of_kind(&all[before..], kind).len()
}

#[tokio::test]
async fn each_operator_change_is_recorded_exactly_once() {
    let (runtime, secrets, _guard) = runtime().await;

    let before = records(&runtime).await.len();
    let agent = runtime
        .create_agent(
            "sales",
            "Follow up.",
            ModelConfig::new("mock", "scripted"),
            vec!["filesystem.read".into()],
        )
        .await
        .unwrap();
    assert_eq!(added(&runtime, before, "operator.agent.created").await, 1);
    // The starter policy is the agent's first policy, and is recorded as one.
    assert_eq!(added(&runtime, before, "operator.policy.changed").await, 1);

    let before = records(&runtime).await.len();
    let version = runtime
        .set_policy(agent.id, "default: deny\npermissions: {}\n")
        .await
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(added(&runtime, before, "operator.policy.changed").await, 1);

    let before = records(&runtime).await.len();
    let disabled = runtime.set_agent_enabled(agent.id, false).await.unwrap();
    assert!(!disabled.is_enabled());
    assert_eq!(
        added(&runtime, before, "operator.agent.enabled_changed").await,
        1
    );

    let before = records(&runtime).await.len();
    runtime
        .set_provider_key("anthropic", "  sk-ant-secret-value  ")
        .await
        .unwrap();
    assert_eq!(
        added(&runtime, before, "operator.provider_key.set").await,
        1
    );
    assert_eq!(
        secrets.get(&provider_key("anthropic")).unwrap().expose(),
        "sk-ant-secret-value"
    );

    let before = records(&runtime).await.len();
    runtime.remove_provider_key("anthropic").await.unwrap();
    assert_eq!(
        added(&runtime, before, "operator.provider_key.removed").await,
        1
    );
    assert!(secrets.get(&provider_key("anthropic")).is_err());

    let all = records(&runtime).await;

    // Agent changes belong to the agent and to no run; credential changes
    // belong to nothing but the installation.
    for kind in [
        "operator.agent.created",
        "operator.policy.changed",
        "operator.agent.enabled_changed",
    ] {
        for record in of_kind(&all, kind) {
            assert_eq!(record.agent_id, Some(agent.id), "{kind}");
            assert!(
                record.task_id.is_none() && record.run_id.is_none(),
                "{kind}"
            );
        }
    }
    for kind in ["operator.provider_key.set", "operator.provider_key.removed"] {
        let record = of_kind(&all, kind)[0];
        assert!(record.agent_id.is_none() && record.run_id.is_none());
        assert_eq!(record.payload["provider"], "anthropic");
    }

    // The log carries what the policy said, and never the key.
    let installed = of_kind(&all, "operator.policy.changed")[1];
    assert_eq!(installed.payload["version"], 2);
    assert_eq!(
        installed.payload["document"],
        "default: deny\npermissions: {}\n"
    );
    for record in &all {
        assert!(
            !record.payload.to_string().contains("sk-ant-secret-value"),
            "{} carries key material",
            record.kind
        );
    }

    // And every one of them is security-relevant and on an intact chain.
    for record in all.iter().filter(|r| r.kind.starts_with("operator.")) {
        assert!(agentos_core::event::is_security_kind(&record.kind));
    }
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}

#[tokio::test]
async fn an_agent_whose_workspace_cannot_be_made_still_has_its_creation_recorded() {
    let (runtime, _secrets, _guard) = runtime().await;
    // A file where the workspace directory should go.
    let workspace = runtime.config().workspace_for("blocked");
    std::fs::create_dir_all(workspace.parent().unwrap()).unwrap();
    std::fs::write(&workspace, "in the way").unwrap();

    let error = runtime
        .create_agent(
            "blocked",
            "Nothing.",
            ModelConfig::new("mock", "scripted"),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Io { .. }), "{error}");

    // The row exists, so the record of its creation must too.
    let agent = runtime.agent_by_name("blocked").await.unwrap();
    let all = records(&runtime).await;
    let created = of_kind(&all, "operator.agent.created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].agent_id, Some(agent.id));
}

#[tokio::test]
async fn a_change_that_is_refused_is_not_recorded() {
    let (runtime, _secrets, _guard) = runtime().await;
    let agent = runtime
        .create_agent(
            "sales",
            "Follow up.",
            ModelConfig::new("mock", "scripted"),
            vec![],
        )
        .await
        .unwrap();
    let before = records(&runtime).await.len();

    let error = runtime
        .set_policy(agent.id, "permisions: {}\n")
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Policy(_)), "{error}");
    assert_eq!(
        runtime
            .database()
            .agents()
            .policy(agent.id)
            .await
            .unwrap()
            .unwrap()
            .version,
        1
    );

    let error = runtime
        .set_provider_key("anthropc", "sk-x")
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Rejected(_)), "{error}");
    let error = runtime
        .set_provider_key("anthropic", "   ")
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Rejected(_)), "{error}");

    assert_eq!(records(&runtime).await.len(), before);
}

fn pending_request(run: &TaskRun, agent: &agentos_core::agent::Agent) -> ApprovalRequest {
    ApprovalRequest {
        id: ApprovalId::new(),
        agent_id: agent.id,
        agent_name: agent.name.clone(),
        task_id: run.task_id,
        run_id: run.id,
        tool: "filesystem.write".into(),
        arguments: serde_json::json!({"path": "x"}),
        capability: Capability::new("filesystem", "write"),
        risk: RiskLevel::High,
        effect_before_taint: Effect::Ask,
        asked_this_run: 1,
        approval_budget: Some(10),
        reason: "policy requires approval".into(),
        explanation: "writes a file".into(),
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
async fn requests_left_by_runs_that_died_are_closed_at_startup() {
    let (runtime, _secrets, _guard) = runtime().await;
    let agent = runtime
        .create_agent(
            "sales",
            "Follow up.",
            ModelConfig::new("mock", "scripted"),
            vec![],
        )
        .await
        .unwrap();
    let task = runtime.create_task(agent.id, "interrupted").await.unwrap();
    let database = runtime.database();

    // A run that was waiting on a person when the process died.
    let mut waiting = TaskRun::new(task.id, 1);
    waiting.state = TaskState::WaitingForApproval;
    database.runs().insert(&waiting).await.unwrap();
    let interrupted = pending_request(&waiting, &agent);
    database.approvals().insert(&interrupted).await.unwrap();

    // And one an older crash left on a run that had already been failed.
    let mut ended = TaskRun::new(task.id, 2);
    ended.state = TaskState::Failed;
    database.runs().insert(&ended).await.unwrap();
    let stranded = pending_request(&ended, &agent);
    database.approvals().insert(&stranded).await.unwrap();

    assert_eq!(database.approvals().list_pending().await.unwrap().len(), 2);
    assert_eq!(runtime.reap_abandoned_runs().await.unwrap(), 1);

    assert!(
        database
            .approvals()
            .list_pending()
            .await
            .unwrap()
            .is_empty()
    );
    for id in [interrupted.id, stranded.id] {
        let closed = database.approvals().get(id).await.unwrap();
        assert_eq!(closed.status, ApprovalStatus::Expired);
        assert!(closed.decided_at.is_some());
        let note = closed.decision_note.unwrap();
        assert!(note.contains("run that asked had ended"), "{note}");
    }
}

#[tokio::test]
async fn a_run_is_held_to_the_approval_budget_its_policy_names() {
    let (runtime, _secrets, _guard) = runtime().await;
    let agent = runtime
        .create_agent(
            "writer",
            "Write.",
            ModelConfig::new("mock", "scripted"),
            vec!["filesystem.write".into()],
        )
        .await
        .unwrap();
    let workspace = std::fs::canonicalize(runtime.config().workspace_for(&agent.name)).unwrap();
    let policy = format!(
        "default: deny\n\
         approval_budget:\n  max_per_run: 2\n\
         permissions:\n  filesystem:\n    write:\n      effect: ask\n      paths: [{}]\n",
        agentos_permissions::quote_scalar(&workspace.display().to_string())
    );
    runtime.set_policy(agent.id, &policy).await.unwrap();

    let mut script: Vec<ScriptedTurn> = (0..3)
        .map(|i| {
            ScriptedTurn::call(
                &format!("c{i}"),
                "filesystem.write",
                serde_json::json!({"path": format!("note-{i}.txt"), "content": "hello"}),
            )
        })
        .collect();
    script.push(ScriptedTurn::text("Done."));

    let mut runtime = runtime.clone();
    runtime.set_provider_factory(Arc::new(FixedProviderFactory::new(Arc::new(
        MockProvider::new(script),
    ))));
    let gate = Arc::new(RecordingGate::approving());
    let outcome = runtime
        .run_objective(
            agent.id,
            "Write notes.",
            gate.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    // Two were put to a person; the third never reached anyone.
    assert_eq!(gate.count().await, 2);
    let trace = runtime.trace(outcome.run_id).await.unwrap();
    let outcomes: Vec<ToolOutcome> = trace.executions.iter().map(|e| e.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            ToolOutcome::Success,
            ToolOutcome::Success,
            ToolOutcome::ApprovalDenied
        ]
    );
    assert!(!workspace.join("note-2.txt").exists());

    let refused = &trace.approvals[2];
    assert_eq!(refused.status, ApprovalStatus::Denied);
    assert_eq!(refused.asked_this_run, 3);
    assert_eq!(refused.approval_budget, Some(2));
    let note = refused.decision_note.as_deref().unwrap();
    assert!(note.contains("asked 2 times"), "{note}");
    assert!(note.contains("budget is 2"), "{note}");

    // The model was told why, in the same words.
    let error = trace.executions[2].error.as_deref().unwrap();
    assert!(error.contains("budget is 2"), "{error}");

    // And the chain records the refusal as the budget's, not a person's.
    let all = records(&runtime).await;
    let denied = of_kind(&all, "approval.denied");
    assert_eq!(denied.len(), 1, "{denied:?}");
    assert_eq!(
        denied[0].payload["approval_id"],
        refused.id.to_string().as_str()
    );
    assert_eq!(denied[0].payload["over_budget"], true);
    assert!(
        denied[0].payload["note"]
            .as_str()
            .unwrap()
            .contains("budget is 2")
    );
    assert_eq!(denied[0].run_id, Some(outcome.run_id));
    // The two a person answered are recorded as granted.
    assert_eq!(of_kind(&all, "approval.granted").len(), 2);
}

/// Approves after letting another process try to reap, while the run that
/// asked is still waiting on the answer.
#[derive(Debug)]
struct ReapedWhileWaiting {
    other: Runtime,
    reaped: std::sync::Mutex<Option<usize>>,
}

#[async_trait::async_trait]
impl agentos_tools::ApprovalGate for ReapedWhileWaiting {
    async fn request(
        &self,
        request: &ApprovalRequest,
        _cancel: CancellationToken,
    ) -> agentos_tools::ApprovalOutcome {
        let reaped = self.other.reap_abandoned_runs().await.unwrap();
        *self.reaped.lock().unwrap() = Some(reaped);
        let stored = self.other.database().approvals().get(request.id).await;
        assert_eq!(stored.unwrap().status, ApprovalStatus::Pending);
        agentos_tools::ApprovalOutcome::Approved {
            note: Some("checked".into()),
        }
    }
}

#[tokio::test]
async fn a_second_process_does_not_reap_a_run_that_is_waiting_on_a_person() {
    // Two runtimes on one data directory are the desktop and a terminal: the
    // run lock is held per open file, so it separates them as it would two
    // processes.
    let guard = TempDir::new().unwrap();
    let home = std::fs::canonicalize(guard.path()).unwrap();
    let config = agentos_runtime::RuntimeConfig::rooted_at(&home);
    let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::new());
    let mut terminal = Runtime::open_with_secrets(config.clone(), secrets.clone())
        .await
        .unwrap();
    let desktop = Runtime::open_with_secrets(config, secrets).await.unwrap();

    let agent = terminal
        .create_agent(
            "writer",
            "Write.",
            ModelConfig::new("mock", "scripted"),
            vec!["filesystem.write".into()],
        )
        .await
        .unwrap();
    let workspace = std::fs::canonicalize(terminal.config().workspace_for(&agent.name)).unwrap();
    let policy = format!(
        "default: deny\n\
         permissions:\n  filesystem:\n    write:\n      effect: ask\n      paths: [{}]\n",
        agentos_permissions::quote_scalar(&workspace.display().to_string())
    );
    terminal.set_policy(agent.id, &policy).await.unwrap();
    terminal.set_provider_factory(Arc::new(FixedProviderFactory::new(Arc::new(
        MockProvider::new(vec![
            ScriptedTurn::call(
                "c1",
                "filesystem.write",
                serde_json::json!({"path": "note.txt", "content": "hello"}),
            ),
            ScriptedTurn::text("Done."),
        ]),
    ))));

    let gate = Arc::new(ReapedWhileWaiting {
        other: desktop.clone(),
        reaped: std::sync::Mutex::new(None),
    });
    let outcome = terminal
        .run_objective(
            agent.id,
            "Write a note.",
            gate.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    // The other process left the live run alone, so the answer was acted on
    // and the record agrees with what happened.
    assert_eq!(*gate.reaped.lock().unwrap(), Some(0));
    assert!(outcome.succeeded(), "{outcome:?}");
    assert!(workspace.join("note.txt").exists());
    let trace = desktop.trace(outcome.run_id).await.unwrap();
    assert_eq!(trace.run.state, TaskState::Completed);
    assert_eq!(trace.approvals[0].status, ApprovalStatus::Approved);

    // Once the terminal is gone, however it went, its leftovers are reaped.
    drop(terminal);
    let task = desktop.create_task(agent.id, "interrupted").await.unwrap();
    let mut left = TaskRun::new(task.id, 1);
    left.state = TaskState::WaitingForApproval;
    desktop.database().runs().insert(&left).await.unwrap();
    assert_eq!(desktop.reap_abandoned_runs().await.unwrap(), 1);
}
