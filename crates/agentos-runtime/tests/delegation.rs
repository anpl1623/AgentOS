//! End-to-end tests of delegation: one agent asking another to do work.
//!
//! The claim under test is that delegation grants nothing. A child run is
//! evaluated by its own policy and every policy above it, shares its parent's
//! taint and step budget, asks the same person for approval under a card that
//! names the whole chain, stops when its parent stops, and is recorded in the
//! same audit chain. Each test below holds one of those properties, and is
//! written to fail if that property alone is broken.
//!
//! Everything runs against scripted providers, one per agent, and an in-memory
//! database. Scripting per agent rather than through one shared script is what
//! keeps a test's meaning independent of the order in which concurrent runs
//! happen to reach their model.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentos_audit::AuditRecord;
use agentos_core::agent::{Agent, ModelConfig};
use agentos_core::approval::{ApprovalRequest, ApprovalStatus};
use agentos_core::delegation::{
    DEFAULT_MAX_CHILDREN_PER_RUN, DEFAULT_MAX_DELEGATION_DEPTH, Delegation, DelegationRefusal,
    DelegationState,
};
use agentos_core::ids::TaskRunId;
use agentos_core::task::{TaskFailure, TaskRun, TaskState};
use agentos_core::tool::ToolOutcome;
use agentos_core::trust::DataSource;
use agentos_persistence::ToolExecutionRecord;
use agentos_providers::{
    CompletionRequest, CompletionResponse, MockProvider, ModelProvider, ProviderCapabilities,
    ProviderError, ScriptedTurn, SharedProvider,
};
use agentos_runtime::{ProviderFactory, RunOutcome, Runtime, RuntimeError};
use agentos_secrets::InMemorySecretStore;
use agentos_tools::{ApprovalGate, ApprovalOutcome, RecordingGate};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// Every agent in these tests, so that one policy can name all of them.
const AGENTS: &[&str] = &["supervisor", "worker", "helper", "scout"];

/// The tools every agent is offered. What each may actually do is its policy's
/// business, and in a delegation, every policy above it too.
const TOOLS: &[&str] = &[
    "agent.delegate",
    "filesystem.read",
    "filesystem.write",
    "filesystem.list",
];

/// How long any one run may take before a test calls it hung. Generous,
/// because the failure it guards against is a child that never returns and
/// a parent waiting on it forever, not a slow machine.
const HUNG: Duration = Duration::from_secs(30);

/// A policy that does not escalate on taint, so that every ask in a test
/// comes from a rule the test wrote rather than from what a run has read.
fn quiet(permissions: &str) -> String {
    format!(
        "default: deny\ntaint_escalation:\n  enabled: false\n  escalate_at_or_above: medium\n\
         permissions:\n{permissions}"
    )
}

/// A policy that turns a tainted run's medium-risk allows into asks.
fn watchful(permissions: &str) -> String {
    format!(
        "default: deny\ntaint_escalation:\n  enabled: true\n  escalate_at_or_above: medium\n\
         permissions:\n{permissions}"
    )
}

/// Builds each agent's provider from the script registered under its name.
///
/// An agent nobody scripted answers at once, so that a run the test did not
/// expect ends rather than hangs and the assertions about what ran report it.
#[derive(Debug, Default)]
struct Models {
    by_agent: std::sync::Mutex<HashMap<String, SharedProvider>>,
}

impl ProviderFactory for Models {
    fn build(
        &self,
        agent_name: &str,
        _model: &ModelConfig,
    ) -> Result<SharedProvider, RuntimeError> {
        Ok(self
            .by_agent
            .lock()
            .unwrap()
            .get(agent_name)
            .cloned()
            .unwrap_or_else(|| Arc::new(MockProvider::answering("Nothing was scripted."))))
    }
}

/// A model that is unavailable, with an error no retry will cure.
///
/// The scripted mock's errors are all retryable, which makes a failing child
/// spend seconds in back-off; this one fails on its first turn.
#[derive(Debug)]
struct Unavailable;

/// What [`Unavailable`] says, so a test can look for it where the supervisor
/// reads its tool results.
const UNAVAILABLE: &str = "the worker's model is unavailable";

#[async_trait::async_trait]
impl ModelProvider for Unavailable {
    fn id(&self) -> &str {
        agentos_providers::provider_ids::MOCK
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            tools: true,
            usage_reporting: true,
            vision: false,
        }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        _cancel: CancellationToken,
    ) -> Result<CompletionResponse, ProviderError> {
        Err(ProviderError::Api {
            provider: agentos_providers::provider_ids::MOCK.to_owned(),
            status: 400,
            message: UNAVAILABLE.to_owned(),
        })
    }
}

/// A gate that records a request, says it was asked, and then answers nobody
/// until the run is cancelled.
///
/// The request is pending in the database for as long as this holds it, which
/// is what lets a test cancel a run with an approval outstanding.
#[derive(Debug, Default)]
struct HoldingGate {
    asked: tokio::sync::Notify,
    seen: tokio::sync::Mutex<Vec<ApprovalRequest>>,
}

#[async_trait::async_trait]
impl ApprovalGate for HoldingGate {
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        self.seen.lock().await.push(request.clone());
        self.asked.notify_one();
        cancel.cancelled().await;
        ApprovalOutcome::Cancelled
    }
}

struct Harness {
    runtime: Runtime,
    agents: HashMap<&'static str, Agent>,
    models: Arc<Models>,
    _guard: TempDir,
}

impl Harness {
    async fn new() -> Self {
        let guard = TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let mut runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .unwrap();
        let models = Arc::new(Models::default());
        runtime.set_provider_factory(models.clone());

        let mut agents = HashMap::new();
        for name in AGENTS {
            let agent = runtime
                .create_agent(
                    name,
                    "Do what you are asked.",
                    ModelConfig::new("mock", "scripted"),
                    TOOLS.iter().map(|tool| (*tool).to_owned()).collect(),
                )
                .await
                .unwrap();
            std::fs::create_dir_all(runtime.config().workspace_for(name)).unwrap();
            agents.insert(*name, agent);
        }

        Self {
            runtime,
            agents,
            models,
            _guard: guard,
        }
    }

    fn agent(&self, name: &str) -> &Agent {
        &self.agents[name]
    }

    /// An agent's workspace, canonical, as the filesystem tools resolve it.
    fn workspace(&self, name: &str) -> PathBuf {
        std::fs::canonicalize(self.runtime.config().workspace_for(name)).unwrap()
    }

    /// Install a policy, with `{name}` standing for that agent's workspace.
    async fn policy(&self, name: &str, document: &str) {
        let mut rendered = document.to_owned();
        for agent in AGENTS {
            rendered = rendered.replace(
                &format!("{{{agent}}}"),
                &agentos_permissions::quote_scalar(&self.workspace(agent).display().to_string()),
            );
        }
        self.runtime
            .database()
            .agents()
            .set_policy(self.agent(name).id, &rendered)
            .await
            .unwrap();
    }

    /// Script an agent's model, returning it so a test can read what the
    /// agent was shown.
    fn script(&self, name: &str, turns: Vec<ScriptedTurn>) -> Arc<MockProvider> {
        let provider = Arc::new(MockProvider::new(turns));
        self.provide(name, provider.clone());
        provider
    }

    fn provide(&self, name: &str, provider: SharedProvider) {
        self.models
            .by_agent
            .lock()
            .unwrap()
            .insert(name.to_owned(), provider);
    }

    /// Run an agent to the end, as the operator would start it.
    async fn run(&self, name: &str, gate: Arc<dyn ApprovalGate>) -> RunOutcome {
        tokio::time::timeout(
            HUNG,
            self.runtime.run_objective(
                self.agent(name).id,
                "Do the work.",
                gate,
                CancellationToken::new(),
            ),
        )
        .await
        .expect("the run hung")
        .unwrap()
    }

    async fn stored_run(&self, run_id: TaskRunId) -> TaskRun {
        self.runtime.database().runs().get(run_id).await.unwrap()
    }

    /// The delegations a run made, in the order it made them.
    async fn children(&self, run_id: TaskRunId) -> Vec<Delegation> {
        self.runtime
            .database()
            .delegations()
            .list_for_parent_run(run_id)
            .await
            .unwrap()
    }

    /// The one delegation a run made, and the run that carried it out.
    async fn only_child(&self, run_id: TaskRunId) -> (Delegation, TaskRunId) {
        let mut children = self.children(run_id).await;
        assert_eq!(children.len(), 1, "expected one delegation: {children:?}");
        let child = children.remove(0);
        let run = child
            .child_run_id
            .expect("the child run was never attached");
        (child, run)
    }

    /// The one call a run made to a tool.
    async fn execution(&self, run_id: TaskRunId, tool: &str) -> ToolExecutionRecord {
        let mut calls: Vec<_> = self
            .runtime
            .database()
            .executions()
            .list_for_run(run_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|execution| execution.tool == tool)
            .collect();
        assert_eq!(calls.len(), 1, "expected one call to {tool}: {calls:?}");
        calls.remove(0)
    }

    /// Every record of the policy being consulted about one capability in a
    /// run: what was asked of it and what it answered.
    ///
    /// The tests that expect a refusal from planning use this to show the
    /// policy was never consulted: a refusal that the policy engine made
    /// would have left a record here, and would be passing for the wrong
    /// reason.
    async fn consultations(
        &self,
        run_id: TaskRunId,
        domain: &str,
        action: &str,
    ) -> Vec<AuditRecord> {
        self.runtime
            .database()
            .audit_sink()
            .for_run(run_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.kind.starts_with("permission."))
            .filter(|record| {
                record.payload["capability"]["domain"] == domain
                    && record.payload["capability"]["action"] == action
            })
            .collect()
    }

    /// The policy's one answer about a capability in a run.
    async fn verdict(&self, run_id: TaskRunId, domain: &str, action: &str) -> AuditRecord {
        let mut answers: Vec<_> = self
            .consultations(run_id, domain, action)
            .await
            .into_iter()
            .filter(|record| record.kind != "permission.requested")
            .collect();
        assert_eq!(answers.len(), 1, "expected one answer: {answers:?}");
        answers.remove(0)
    }

    /// Assert that one delegation call was refused while it was planned, for
    /// a reason containing `because`.
    ///
    /// Planning records its refusals as `agent.delegation.refused` in the
    /// asking run; the policy engine does not. So the record is what tells a
    /// refusal from the limits apart from one the policy happened to make.
    async fn assert_refused_in_planning(&self, run_id: TaskRunId, call_id: &str, because: &str) {
        let call = self
            .runtime
            .database()
            .executions()
            .list_for_run(run_id)
            .await
            .unwrap()
            .into_iter()
            .find(|execution| execution.call_id == call_id)
            .unwrap_or_else(|| panic!("call {call_id} was not recorded"));
        assert_ne!(call.outcome, ToolOutcome::Success, "{call:?}");
        let error = call.error.unwrap_or_default();
        assert!(error.contains(because), "expected `{because}` in: {error}");

        let refusals: Vec<_> = self
            .runtime
            .database()
            .audit_sink()
            .for_run(run_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.kind == "agent.delegation.refused")
            .filter(|record| {
                record.payload["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains(because))
            })
            .collect();
        assert_eq!(refusals.len(), 1, "expected the refusal on record");
    }

    /// How many tasks an agent has been given, delegated ones included.
    async fn tasks_for(&self, name: &str) -> usize {
        self.runtime
            .database()
            .tasks()
            .list_for_agent(self.agent(name).id, 100)
            .await
            .unwrap()
            .len()
    }

    /// Every record of one kind, oldest first.
    async fn records_of(&self, kind: &str) -> Vec<AuditRecord> {
        self.runtime
            .database()
            .audit_sink()
            .all()
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.kind == kind)
            .collect()
    }
}

/// A turn asking for `agent.delegate` with one request per `(agent, objective)`.
fn delegate(id: &str, requests: &[(&str, &str)]) -> ScriptedTurn {
    let requests: Vec<_> = requests
        .iter()
        .map(|(agent, objective)| serde_json::json!({ "agent": agent, "objective": objective }))
        .collect();
    ScriptedTurn::call(
        id,
        "agent.delegate",
        serde_json::json!({ "requests": requests }),
    )
}

fn write(id: &str, path: &str) -> ScriptedTurn {
    ScriptedTurn::call(
        id,
        "filesystem.write",
        serde_json::json!({ "path": path, "content": "written" }),
    )
}

fn read(id: &str, path: &str) -> ScriptedTurn {
    ScriptedTurn::call(id, "filesystem.read", serde_json::json!({ "path": path }))
}

/// The matched rule a permission record names.
fn rule_of(record: &AuditRecord) -> Option<&str> {
    record.payload["matched_rule"].as_str()
}

// -- Attenuation -------------------------------------------------------------

#[tokio::test]
async fn a_child_cannot_do_what_the_parent_was_denied() {
    // The worker's own policy would let it write. The supervisor's forbids
    // every filesystem action under a rule only the supervisor has, so the
    // rule a denial names says which policy refused.
    let harness = Harness::new().await;
    harness
        .policy(
            "supervisor",
            &quiet("  runtime:\n    delegate: [worker]\n  filesystem:\n    \"*\": deny\n"),
        )
        .await;
    harness
        .policy("worker", &quiet("  filesystem:\n    write: [{worker}]\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Write the report.")]),
            ScriptedTurn::text("Asked."),
        ],
    );
    harness.script(
        "worker",
        vec![write("w1", "report.md"), ScriptedTurn::text("Refused.")],
    );

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");

    let (_, child) = harness.only_child(outcome.run_id).await;
    assert_eq!(
        harness.execution(child, "filesystem.write").await.outcome,
        ToolOutcome::Denied
    );
    assert!(!harness.workspace("worker").join("report.md").exists());

    let denied = harness.verdict(child, "filesystem", "write").await;
    assert_eq!(denied.kind, "permission.denied");
    assert_eq!(
        rule_of(&denied),
        Some("filesystem.*"),
        "the denial must name the supervisor's rule, not the worker's allow"
    );
}

#[tokio::test]
async fn a_child_cannot_do_what_its_own_policy_denies() {
    // The mirror: a permissive supervisor lends the worker nothing its own
    // policy withholds.
    let harness = Harness::new().await;
    harness
        .policy(
            "supervisor",
            &quiet("  runtime:\n    delegate: [worker]\n  filesystem:\n    write: [{worker}]\n"),
        )
        .await;
    harness
        .policy("worker", &quiet("  filesystem:\n    \"*\": deny\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Write the report.")]),
            ScriptedTurn::text("Asked."),
        ],
    );
    harness.script(
        "worker",
        vec![write("w1", "report.md"), ScriptedTurn::text("Refused.")],
    );

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");

    let (_, child) = harness.only_child(outcome.run_id).await;
    assert_eq!(
        harness.execution(child, "filesystem.write").await.outcome,
        ToolOutcome::Denied
    );
    assert!(!harness.workspace("worker").join("report.md").exists());

    let denied = harness.verdict(child, "filesystem", "write").await;
    assert_eq!(denied.kind, "permission.denied");
    assert_eq!(rule_of(&denied), Some("filesystem.*"));
}

#[tokio::test]
async fn an_ask_on_either_side_asks() {
    // Both directions, because an intersection that consulted only one side
    // would pass whichever case happened to put the ask on that side.
    let allow = "  filesystem:\n    write: [{worker}]\n";
    let ask = "  filesystem:\n    write:\n      effect: ask\n      paths: [{worker}]\n";

    for (parent, child) in [(allow, ask), (ask, allow)] {
        let harness = Harness::new().await;
        harness
            .policy(
                "supervisor",
                &quiet(&format!("  runtime:\n    delegate: [worker]\n{parent}")),
            )
            .await;
        harness.policy("worker", &quiet(child)).await;
        harness.script(
            "supervisor",
            vec![
                delegate("d1", &[("worker", "Write the report.")]),
                ScriptedTurn::text("Asked."),
            ],
        );
        harness.script(
            "worker",
            vec![write("w1", "report.md"), ScriptedTurn::text("Written.")],
        );

        let gate = Arc::new(RecordingGate::approving());
        let outcome = harness.run("supervisor", gate.clone()).await;
        assert!(outcome.succeeded(), "{outcome:?}");

        let (_, child_run) = harness.only_child(outcome.run_id).await;
        assert_eq!(
            gate.count().await,
            1,
            "parent `{}` with child `{}` must reach the operator exactly once",
            parent.trim(),
            child.trim()
        );
        let request = gate.requests().await.remove(0);
        assert_eq!(request.tool, "filesystem.write");
        assert_eq!(request.run_id, child_run);
        // Approved, so it ran: the ask was the whole of the restriction.
        assert!(harness.workspace("worker").join("report.md").exists());
    }
}

// -- Shared taint and budget -------------------------------------------------

#[tokio::test]
async fn taint_a_child_picks_up_reaches_the_parent() {
    let harness = Harness::new().await;
    std::fs::write(
        harness.workspace("worker").join("page.txt"),
        "external text",
    )
    .unwrap();
    harness
        .policy(
            "supervisor",
            &quiet(
                "  runtime:\n    delegate: [worker]\n  filesystem:\n    read: [{worker}]\n    \
                 write:\n      effect: ask\n      paths: [{supervisor}]\n",
            ),
        )
        .await;
    harness
        .policy("worker", &quiet("  filesystem:\n    read: [{worker}]\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Read the page.")]),
            write("s1", "summary.md"),
            ScriptedTurn::text("Summarised."),
        ],
    );
    harness.script(
        "worker",
        vec![read("w1", "page.txt"), ScriptedTurn::text("Read it.")],
    );

    let gate = Arc::new(RecordingGate::approving());
    let outcome = harness.run("supervisor", gate.clone()).await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (_, child) = harness.only_child(outcome.run_id).await;

    // The supervisor's own write is the only ask, and it names the file the
    // worker read. The report coming back as agent output taints the
    // supervisor anyway; the file's source is what proves the tracker is
    // shared rather than the report merely being labelled.
    assert_eq!(gate.count().await, 1);
    let request = gate.requests().await.remove(0);
    assert_eq!(request.run_id, outcome.run_id);
    assert!(request.tainted);
    let read_from_page = |label: &String| label.starts_with("file:") && label.ends_with("page.txt");
    assert!(
        request.taint_sources.iter().any(read_from_page),
        "the parent's approval must name what the child read: {:?}",
        request.taint_sources
    );

    for run_id in [outcome.run_id, child] {
        let run = harness.stored_run(run_id).await;
        assert!(run.tainted, "run {run_id} should be recorded tainted");
        assert!(
            run.taint_sources.iter().any(
                |source| matches!(source, DataSource::File { path } if path.ends_with("page.txt"))
            ),
            "run {run_id} should record the page as a source: {:?}",
            run.taint_sources
        );
    }
}

#[tokio::test]
async fn a_delegate_of_a_tainted_run_starts_tainted() {
    // Read a file, then ask a clean agent to act on it: delegation must not
    // launder what the supervisor read. The worker's very first action is a
    // write its own policy allows only while the run is clean, so the only way
    // it can be asked about is if the worker begins with the supervisor's
    // taint.
    let harness = Harness::new().await;
    std::fs::write(
        harness.workspace("supervisor").join("brief.txt"),
        "external brief",
    )
    .unwrap();
    harness
        .policy(
            "supervisor",
            &quiet(
                "  runtime:\n    delegate: [worker]\n  filesystem:\n    read: [{supervisor}]\n    \
                 write: [{worker}]\n",
            ),
        )
        .await;
    harness
        .policy(
            "worker",
            &watchful("  filesystem:\n    write: [{worker}]\n"),
        )
        .await;
    harness.script(
        "supervisor",
        vec![
            read("s1", "brief.txt"),
            delegate("d1", &[("worker", "Act on the brief.")]),
            ScriptedTurn::text("Delegated."),
        ],
    );
    harness.script(
        "worker",
        vec![write("w1", "out.txt"), ScriptedTurn::text("Wrote it.")],
    );

    let gate = Arc::new(RecordingGate::approving());
    let outcome = harness.run("supervisor", gate.clone()).await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (_, child) = harness.only_child(outcome.run_id).await;

    assert_eq!(
        gate.count().await,
        1,
        "the worker's first write must be escalated by the supervisor's taint"
    );
    let request = gate.requests().await.remove(0);
    assert_eq!(request.run_id, child);
    assert!(request.tainted);
    assert!(
        request
            .taint_sources
            .iter()
            .any(|label| label.starts_with("file:") && label.ends_with("brief.txt")),
        "the escalation must name what the supervisor read: {:?}",
        request.taint_sources
    );
    assert!(harness.stored_run(child).await.tainted);
}

#[tokio::test]
async fn the_step_budget_is_shared() {
    // Four steps for the whole tree. The supervisor spends one asking, the
    // worker three, and the supervisor's next turn finds nothing left.
    let harness = Harness::new().await;
    let mut supervisor = harness.agent("supervisor").clone();
    supervisor.max_steps = 4;
    harness
        .runtime
        .database()
        .agents()
        .update(&supervisor)
        .await
        .unwrap();
    harness
        .policy(
            "supervisor",
            &quiet("  runtime:\n    delegate: [worker]\n  filesystem:\n    list: [{worker}]\n"),
        )
        .await;
    harness
        .policy("worker", &quiet("  filesystem:\n    list: [{worker}]\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Look around.")]),
            ScriptedTurn::text("Should never be reached."),
        ],
    );
    let list =
        |id: &str| ScriptedTurn::call(id, "filesystem.list", serde_json::json!({"path": "."}));
    harness.script(
        "worker",
        vec![list("w1"), list("w2"), ScriptedTurn::text("Looked.")],
    );

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert_eq!(outcome.state, TaskState::Failed, "{outcome:?}");
    assert_eq!(
        outcome.failure,
        Some(TaskFailure::StepBudgetExhausted { limit: 4 })
    );

    let (_, child) = harness.only_child(outcome.run_id).await;
    let parent_steps = harness.stored_run(outcome.run_id).await.steps_taken;
    let child_run = harness.stored_run(child).await;
    assert_eq!(child_run.state, TaskState::Completed);
    assert_eq!(child_run.steps_taken, 3);
    assert!(
        parent_steps + child_run.steps_taken <= 4,
        "the tree took {parent_steps} + {} turns on a budget of four",
        child_run.steps_taken
    );
}

// -- Cancellation ------------------------------------------------------------

#[tokio::test]
async fn cancelling_the_parent_cancels_the_child() {
    let harness = Harness::new().await;
    harness
        .policy(
            "supervisor",
            &quiet("  runtime:\n    delegate: [worker]\n  filesystem:\n    write: [{worker}]\n"),
        )
        .await;
    harness
        .policy(
            "worker",
            &quiet("  filesystem:\n    write:\n      effect: ask\n      paths: [{worker}]\n"),
        )
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Write the report.")]),
            ScriptedTurn::text("Asked."),
        ],
    );
    harness.script(
        "worker",
        vec![write("w1", "report.md"), ScriptedTurn::text("Written.")],
    );

    let gate = Arc::new(HoldingGate::default());
    let task = harness
        .runtime
        .create_task(harness.agent("supervisor").id, "Supervise.", &[], None)
        .await
        .unwrap();
    let (parent, handle) = harness
        .runtime
        .start_task(&task, gate.clone(), CancellationToken::new())
        .await
        .unwrap();

    // The worker is now waiting on a person, with its request on file.
    tokio::time::timeout(HUNG, gate.asked.notified())
        .await
        .expect("the worker never asked");
    let (_, child) = harness.only_child(parent).await;
    assert_eq!(gate.seen.lock().await[0].run_id, child);
    let pending = harness
        .runtime
        .database()
        .approvals()
        .list_for_run(child)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, ApprovalStatus::Pending);

    // The operator stops the run they started, knowing nothing of the child.
    assert!(harness.runtime.cancel_run(parent).await);
    let outcome = tokio::time::timeout(HUNG, handle)
        .await
        .expect("the parent did not stop")
        .unwrap()
        .unwrap();
    assert_eq!(outcome.state, TaskState::Cancelled);

    assert_eq!(harness.stored_run(child).await.state, TaskState::Cancelled);
    let approvals = harness
        .runtime
        .database()
        .approvals()
        .list_for_run(child)
        .await
        .unwrap();
    assert!(
        approvals
            .iter()
            .all(|approval| approval.status == ApprovalStatus::Cancelled),
        "a cancelled child must not leave a card a person can still answer: {approvals:?}"
    );
    assert_eq!(
        harness.children(parent).await[0].state,
        DelegationState::Cancelled
    );
    assert!(
        harness.runtime.running_runs().await.is_empty(),
        "no run of the tree may still be registered as live"
    );
    assert!(!harness.workspace("worker").join("report.md").exists());
}

// -- Refusals ----------------------------------------------------------------

#[tokio::test]
async fn an_agent_cannot_delegate_to_itself_or_to_an_ancestor() {
    // Each policy grants the delegation the planner must refuse, so a refusal
    // can only come from the cycle check and never from the policy engine.
    let grant = "  runtime:\n    delegate: [supervisor, worker]\n";

    // Itself.
    let harness = Harness::new().await;
    harness.policy("supervisor", &quiet(grant)).await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("supervisor", "Do it again.")]),
            ScriptedTurn::text("Refused."),
        ],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    harness
        .assert_refused_in_planning(outcome.run_id, "d1", "supervisor -> supervisor")
        .await;
    assert!(harness.children(outcome.run_id).await.is_empty());
    assert!(
        harness
            .consultations(outcome.run_id, "runtime", "delegate")
            .await
            .is_empty()
    );
    assert_eq!(harness.tasks_for("supervisor").await, 1);

    // An ancestor.
    let harness = Harness::new().await;
    harness.policy("supervisor", &quiet(grant)).await;
    harness.policy("worker", &quiet(grant)).await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Find someone.")]),
            ScriptedTurn::text("Asked."),
        ],
    );
    harness.script(
        "worker",
        vec![
            delegate("d2", &[("supervisor", "You do it.")]),
            ScriptedTurn::text("Refused."),
        ],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (_, worker) = harness.only_child(outcome.run_id).await;
    harness
        .assert_refused_in_planning(worker, "d2", "supervisor -> worker -> supervisor")
        .await;
    assert!(harness.children(worker).await.is_empty());
    assert!(
        harness
            .consultations(worker, "runtime", "delegate")
            .await
            .is_empty()
    );
    assert_eq!(harness.tasks_for("supervisor").await, 1);
}

#[tokio::test]
async fn depth_and_fan_out_are_capped() {
    // Every agent may ask every other, so only the limits can refuse.
    let grant = "  runtime:\n    delegate: [supervisor, worker, helper, scout]\n";

    // Depth: supervisor -> worker -> helper is two levels; the helper asking
    // the scout would be a third.
    let harness = Harness::new().await;
    let limits = &harness.runtime.config().delegation;
    assert_eq!(
        (limits.max_depth, limits.max_children_per_run),
        (DEFAULT_MAX_DELEGATION_DEPTH, DEFAULT_MAX_CHILDREN_PER_RUN),
        "the cases below are built for the default limits"
    );
    for agent in AGENTS {
        harness.policy(agent, &quiet(grant)).await;
    }
    for (asker, asked) in [
        ("supervisor", "worker"),
        ("worker", "helper"),
        ("helper", "scout"),
    ] {
        harness.script(
            asker,
            vec![
                delegate("d", &[(asked, "Pass it on.")]),
                ScriptedTurn::text("Done."),
            ],
        );
    }
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (first, worker) = harness.only_child(outcome.run_id).await;
    let (second, helper) = harness.only_child(worker).await;
    assert_eq!((first.depth, second.depth), (1, 2));

    let expected = DelegationRefusal::DepthExceeded {
        limit: DEFAULT_MAX_DELEGATION_DEPTH,
    }
    .to_string();
    harness
        .assert_refused_in_planning(helper, "d", &expected)
        .await;
    assert!(harness.children(helper).await.is_empty());
    assert!(
        harness
            .consultations(helper, "runtime", "delegate")
            .await
            .is_empty()
    );
    assert_eq!(harness.tasks_for("scout").await, 0);

    // Fan-out in one call: one more request than a run may make.
    let harness = Harness::new().await;
    harness.policy("supervisor", &quiet(grant)).await;
    let too_many: Vec<(&str, &str)> = (0..=DEFAULT_MAX_CHILDREN_PER_RUN)
        .map(|_| ("worker", "Help."))
        .collect();
    harness.script(
        "supervisor",
        vec![delegate("d1", &too_many), ScriptedTurn::text("Refused.")],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let expected = DelegationRefusal::FanOutExceeded {
        limit: DEFAULT_MAX_CHILDREN_PER_RUN,
    }
    .to_string();
    harness
        .assert_refused_in_planning(outcome.run_id, "d1", &expected)
        .await;
    assert!(harness.children(outcome.run_id).await.is_empty());
    assert!(
        harness
            .consultations(outcome.run_id, "runtime", "delegate")
            .await
            .is_empty()
    );
    assert_eq!(harness.tasks_for("worker").await, 0);

    // Fan-out across calls: the limit is on the run, so a run that has used
    // it in one call cannot top it up in the next.
    let harness = Harness::new().await;
    harness.policy("supervisor", &quiet(grant)).await;
    harness.policy("worker", &quiet("  {}\n")).await;
    let allowed: Vec<(&str, &str)> = (0..DEFAULT_MAX_CHILDREN_PER_RUN)
        .map(|_| ("worker", "Help."))
        .collect();
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &allowed),
            delegate("d2", &[("worker", "And one more.")]),
            ScriptedTurn::text("Done."),
        ],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    assert_eq!(
        harness.children(outcome.run_id).await.len(),
        usize::from(DEFAULT_MAX_CHILDREN_PER_RUN)
    );
    harness
        .assert_refused_in_planning(outcome.run_id, "d2", &expected)
        .await;
    assert_eq!(
        harness.tasks_for("worker").await,
        usize::from(DEFAULT_MAX_CHILDREN_PER_RUN)
    );
}

#[tokio::test]
async fn delegation_without_the_capability_is_refused() {
    // The worker exists and is enabled, so planning succeeds and the policy
    // engine is what refuses. A refusal from anywhere else would leave no
    // decision on record and fail the check below.
    let harness = Harness::new().await;
    harness
        .policy(
            "supervisor",
            &quiet("  filesystem:\n    read: [{supervisor}]\n"),
        )
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Do it for me.")]),
            ScriptedTurn::text("Refused."),
        ],
    );

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");

    let call = harness.execution(outcome.run_id, "agent.delegate").await;
    assert_eq!(call.outcome, ToolOutcome::Denied);
    let denied = harness.verdict(outcome.run_id, "runtime", "delegate").await;
    assert_eq!(denied.kind, "permission.denied");
    assert_eq!(
        denied.payload["capability"]["resource"]["name"], "worker",
        "the policy must be asked about the agent named, not about delegation in general"
    );

    assert!(harness.children(outcome.run_id).await.is_empty());
    assert_eq!(harness.tasks_for("worker").await, 0);
    assert!(
        harness
            .records_of("agent.delegation.started")
            .await
            .is_empty()
    );
}

// -- Results -----------------------------------------------------------------

#[tokio::test]
async fn a_failed_child_is_a_result_the_supervisor_can_read() {
    let harness = Harness::new().await;
    harness
        .policy("supervisor", &quiet("  runtime:\n    delegate: [worker]\n"))
        .await;
    harness.policy("worker", &quiet("  {}\n")).await;
    let supervisor = harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Find the figures.")]),
            ScriptedTurn::text("The worker could not help, so I am reporting that."),
        ],
    );
    harness.provide("worker", Arc::new(Unavailable));

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert_eq!(outcome.state, TaskState::Completed, "{outcome:?}");

    let (delegation, child) = harness.only_child(outcome.run_id).await;
    assert_eq!(harness.stored_run(child).await.state, TaskState::Failed);
    assert_eq!(delegation.state, DelegationState::Failed);

    // A result, not a tool error: the call succeeded in reporting a failure.
    assert_eq!(
        harness
            .execution(outcome.run_id, "agent.delegate")
            .await
            .outcome,
        ToolOutcome::Success
    );
    let shown = supervisor.last_rendered_conversation();
    assert!(
        shown.contains(UNAVAILABLE),
        "the supervisor must be shown why the worker failed: {shown}"
    );
}

// -- Attribution and audit ---------------------------------------------------

#[tokio::test]
async fn an_approval_raised_inside_a_delegate_names_the_chain() {
    // Two levels, so a chain that only remembered the immediate delegator
    // would be caught, and an ask of the supervisor's own as the control.
    let harness = Harness::new().await;
    let grant = "  runtime:\n    delegate: [worker, helper]\n";
    harness
        .policy(
            "supervisor",
            &quiet(&format!(
                "{grant}  filesystem:\n    write:\n      effect: ask\n      paths: [{{supervisor}}, \
                 {{helper}}]\n"
            )),
        )
        .await;
    harness
        .policy(
            "worker",
            &quiet(&format!("{grant}  filesystem:\n    write: [{{helper}}]\n")),
        )
        .await;
    harness
        .policy("helper", &quiet("  filesystem:\n    write: [{helper}]\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Get it written.")]),
            write("s1", "summary.md"),
            ScriptedTurn::text("Done."),
        ],
    );
    harness.script(
        "worker",
        vec![
            delegate("d2", &[("helper", "Write it.")]),
            ScriptedTurn::text("Done."),
        ],
    );
    harness.script(
        "helper",
        vec![write("h1", "draft.md"), ScriptedTurn::text("Written.")],
    );

    let gate = Arc::new(RecordingGate::approving());
    let outcome = harness.run("supervisor", gate.clone()).await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (_, worker) = harness.only_child(outcome.run_id).await;
    let (_, helper) = harness.only_child(worker).await;

    let requests = gate.requests().await;
    assert_eq!(requests.len(), 2, "{requests:?}");
    let asked_by = |run: TaskRunId| {
        requests
            .iter()
            .find(|request| request.run_id == run)
            .unwrap_or_else(|| panic!("no request from run {run}"))
    };
    assert_eq!(
        asked_by(helper).delegation_chain,
        vec!["supervisor", "worker", "helper"],
        "the card must name every agent between the operator and the action"
    );
    assert!(
        asked_by(outcome.run_id).delegation_chain.is_empty(),
        "a run nobody delegated asks in its own name"
    );

    // And it is the chain on file, not only the one the gate was handed.
    let stored = harness
        .runtime
        .database()
        .approvals()
        .list_for_run(helper)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].delegation_chain,
        vec!["supervisor", "worker", "helper"]
    );
}

#[tokio::test]
async fn the_audit_chain_survives_a_delegation_tree() {
    let harness = Harness::new().await;
    let grant = quiet("  runtime:\n    delegate: [worker, helper]\n");
    for agent in ["supervisor", "worker"] {
        harness.policy(agent, &grant).await;
    }
    harness.policy("helper", &quiet("  {}\n")).await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Pass it on.")]),
            ScriptedTurn::text("Done."),
        ],
    );
    harness.script(
        "worker",
        vec![
            delegate("d2", &[("helper", "Do it.")]),
            ScriptedTurn::text("Done."),
        ],
    );
    harness.script("helper", vec![ScriptedTurn::text("Did it.")]);

    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (first, worker) = harness.only_child(outcome.run_id).await;
    let (second, _) = harness.only_child(worker).await;

    let verification = harness.runtime.verify_audit().await.unwrap();
    assert!(verification.is_intact(), "{verification:?}");

    // Every edge is sealed in the chain: started naming both runs, then
    // completed, each exactly once.
    let started = harness.records_of("agent.delegation.started").await;
    let completed = harness.records_of("agent.delegation.completed").await;
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(completed.len(), 2, "{completed:?}");
    for edge in [&first, &second] {
        let id = edge.id.to_string();
        let child = edge.child_run_id.unwrap().to_string();
        let opened: Vec<_> = started
            .iter()
            .filter(|record| record.payload["delegation_id"] == id)
            .collect();
        let closed: Vec<_> = completed
            .iter()
            .filter(|record| record.payload["delegation_id"] == id)
            .collect();
        assert_eq!((opened.len(), closed.len()), (1, 1), "edge {id}");
        assert_eq!(
            opened[0].payload["parent_run_id"],
            edge.parent_run_id.to_string()
        );
        assert_eq!(opened[0].payload["child_run_id"], child);
        assert_eq!(closed[0].payload["child_run_id"], child);
        assert!(opened[0].sequence < closed[0].sequence, "edge {id}");
    }

    // And nothing the tree did is missing from it.
    let records = harness.runtime.database().audit_sink().all().await.unwrap();
    let coverage = agentos_audit::verify_coverage(&records, outcome.run_id);
    assert!(coverage.gaps.is_empty(), "{coverage:?}");
}

// -- A delegated task runs only through its delegation -------------------------

#[tokio::test]
async fn a_delegated_task_cannot_be_started_on_its_own() {
    // Started on its own, the worker's task would be a root run under the
    // worker's policy alone, which allows the write the supervisor's forbids.
    // Retrying a failed child is exactly that start, so it is refused.
    let harness = Harness::new().await;
    harness
        .policy(
            "supervisor",
            &quiet("  runtime:\n    delegate: [worker]\n  filesystem:\n    \"*\": deny\n"),
        )
        .await;
    harness
        .policy("worker", &quiet("  filesystem:\n    write: [{worker}]\n"))
        .await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Write the report.")]),
            ScriptedTurn::text("Asked."),
        ],
    );
    harness.provide("worker", Arc::new(Unavailable));

    let gate = Arc::new(RecordingGate::approving());
    let outcome = harness.run("supervisor", gate.clone()).await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let (delegation, child) = harness.only_child(outcome.run_id).await;
    assert_eq!(harness.stored_run(child).await.state, TaskState::Failed);

    // The second attempt would write, if it were allowed to begin.
    harness.script(
        "worker",
        vec![write("w1", "report.md"), ScriptedTurn::text("Written.")],
    );
    let task = harness
        .runtime
        .task(delegation.child_task_id)
        .await
        .unwrap();
    let error = harness
        .runtime
        .start_task(&task, gate.clone(), CancellationToken::new())
        .await
        .map(|(run, _)| run)
        .unwrap_err();
    assert!(
        matches!(&error, RuntimeError::Rejected(why) if why == agentos_runtime::DELEGATED_TASK),
        "{error:?}"
    );
    let error = harness
        .runtime
        .run_task(&task, gate.clone(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Rejected(_)), "{error:?}");

    assert!(!harness.workspace("worker").join("report.md").exists());
    assert_eq!(gate.count().await, 0);
    let attempts = harness
        .runtime
        .database()
        .runs()
        .list_for_task(task.id)
        .await
        .unwrap();
    assert_eq!(
        attempts.len(),
        1,
        "a refused start writes no run: {attempts:?}"
    );

    // Nor in the moment after a delegated task is written and before its
    // delegation is: the parent link alone is enough to refuse it.
    let orphan = agentos_core::task::Task::new(harness.agent("worker").id, "Write it.")
        .with_parent(harness.stored_run(outcome.run_id).await.task_id);
    harness
        .runtime
        .database()
        .tasks()
        .insert(&orphan)
        .await
        .unwrap();
    let error = harness
        .runtime
        .start_task(&orphan, gate.clone(), CancellationToken::new())
        .await
        .map(|(run, _)| run)
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Rejected(_)), "{error:?}");
    assert!(!harness.workspace("worker").join("report.md").exists());
}

// -- Approval budget -----------------------------------------------------------

#[tokio::test]
async fn a_delegated_run_is_held_to_the_stricter_approval_budget() {
    // One request allowed, written on each side in turn, the other side
    // unlimited. The worker asks twice; the second ask must be refused by the
    // budget whichever policy set it.
    let limited = "approval_budget:\n  max_per_run: 1\n";
    let unlimited = "approval_budget:\n  max_per_run: ~\n";
    for (supervisor_budget, worker_budget) in [(limited, unlimited), (unlimited, limited)] {
        let harness = Harness::new().await;
        harness
            .policy(
                "supervisor",
                &format!(
                    "{supervisor_budget}{}",
                    quiet(
                        "  runtime:\n    delegate: [worker]\n  filesystem:\n    write: [{worker}]\n"
                    )
                ),
            )
            .await;
        harness
            .policy(
                "worker",
                &format!(
                    "{worker_budget}{}",
                    quiet(
                        "  filesystem:\n    write:\n      effect: ask\n      paths: [{worker}]\n"
                    )
                ),
            )
            .await;
        harness.script(
            "supervisor",
            vec![
                delegate("d1", &[("worker", "Write both.")]),
                ScriptedTurn::text("Asked."),
            ],
        );
        harness.script(
            "worker",
            vec![
                write("w1", "first.md"),
                write("w2", "second.md"),
                ScriptedTurn::text("Wrote what I could."),
            ],
        );

        let gate = Arc::new(RecordingGate::approving());
        let outcome = harness.run("supervisor", gate.clone()).await;
        assert!(outcome.succeeded(), "{outcome:?}");
        let (_, child) = harness.only_child(outcome.run_id).await;

        let case = format!("supervisor {supervisor_budget:?}, worker {worker_budget:?}");
        assert_eq!(
            gate.count().await,
            1,
            "{case}: only the first ask reaches a person"
        );
        let asked = gate.requests().await.remove(0);
        assert_eq!(asked.run_id, child);
        assert_eq!(asked.approval_budget, Some(1), "{case}");
        assert!(harness.workspace("worker").join("first.md").exists());
        assert!(
            !harness.workspace("worker").join("second.md").exists(),
            "{case}: the ask over budget must not be acted on"
        );

        let stored = harness
            .runtime
            .database()
            .approvals()
            .list_for_run(child)
            .await
            .unwrap();
        assert_eq!(stored.len(), 2, "{case}: {stored:?}");
        let refused = stored
            .iter()
            .find(|request| request.status == ApprovalStatus::Denied)
            .unwrap_or_else(|| panic!("{case}: no request refused: {stored:?}"));
        assert!(
            refused
                .decision_note
                .as_deref()
                .is_some_and(|note| note.contains("budget is 1")),
            "{case}: {refused:?}"
        );
    }
}

// -- Refusals in planning ------------------------------------------------------

#[tokio::test]
async fn a_call_asking_nobody_or_with_no_steps_left_is_refused_in_planning() {
    let grant = "  runtime:\n    delegate: [worker]\n";

    // Nobody: a call that names no agent is no delegation.
    let harness = Harness::new().await;
    harness.policy("supervisor", &quiet(grant)).await;
    harness.script(
        "supervisor",
        vec![delegate("d1", &[]), ScriptedTurn::text("Refused.")],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert!(outcome.succeeded(), "{outcome:?}");
    let expected = DelegationRefusal::FanOutExceeded {
        limit: DEFAULT_MAX_CHILDREN_PER_RUN,
    }
    .to_string();
    harness
        .assert_refused_in_planning(outcome.run_id, "d1", &expected)
        .await;
    assert!(harness.children(outcome.run_id).await.is_empty());
    assert!(
        harness
            .consultations(outcome.run_id, "runtime", "delegate")
            .await
            .is_empty()
    );

    // No steps left: the turn that asks is the tree's last, so a child could
    // not take a single one of its own.
    let harness = Harness::new().await;
    let mut supervisor = harness.agent("supervisor").clone();
    supervisor.max_steps = 1;
    harness
        .runtime
        .database()
        .agents()
        .update(&supervisor)
        .await
        .unwrap();
    harness.policy("supervisor", &quiet(grant)).await;
    harness.script(
        "supervisor",
        vec![
            delegate("d1", &[("worker", "Help.")]),
            ScriptedTurn::text("Never reached."),
        ],
    );
    let outcome = harness
        .run("supervisor", Arc::new(RecordingGate::approving()))
        .await;
    assert_eq!(
        outcome.failure,
        Some(TaskFailure::StepBudgetExhausted { limit: 1 }),
        "{outcome:?}"
    );
    let expected = DelegationRefusal::StepBudgetExhausted { remaining: 0 }.to_string();
    harness
        .assert_refused_in_planning(outcome.run_id, "d1", &expected)
        .await;
    assert!(harness.children(outcome.run_id).await.is_empty());
    assert_eq!(harness.tasks_for("worker").await, 0);
    assert!(
        harness
            .consultations(outcome.run_id, "runtime", "delegate")
            .await
            .is_empty()
    );
}

// -- What is left when a process dies ------------------------------------------

/// A tool that does what another does, and remembers every run it was told
/// had ended.
#[derive(Debug)]
struct Ending {
    inner: Arc<dyn agentos_tools::Tool>,
    ended: std::sync::Mutex<Vec<TaskRunId>>,
}

#[async_trait::async_trait]
impl agentos_tools::Tool for Ending {
    fn metadata(&self) -> &agentos_core::tool::ToolMetadata {
        self.inner.metadata()
    }

    fn validate(
        &self,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, agentos_tools::ToolError> {
        self.inner.validate(arguments)
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &agentos_tools::ToolContext,
    ) -> Result<agentos_tools::ToolPlan, agentos_tools::ToolError> {
        self.inner.plan(arguments, context).await
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &agentos_tools::ToolContext,
        cancel: CancellationToken,
    ) -> Result<agentos_tools::ToolOutput, agentos_tools::ToolError> {
        self.inner.execute(arguments, context, cancel).await
    }

    async fn end_run(&self, run_id: TaskRunId) {
        self.ended.lock().unwrap().push(run_id);
    }
}

impl Harness {
    /// A task for `agent`, delegated from `parent` when given, and a run of it
    /// left as a process that died mid-way leaves one.
    async fn abandoned_run(&self, agent: &str, parent: Option<&TaskRun>) -> TaskRun {
        let mut task =
            agentos_core::task::Task::new(self.agent(agent).id, format!("{agent}'s work"));
        if let Some(parent) = parent {
            task = task.with_parent(parent.task_id);
        }
        self.runtime.database().tasks().insert(&task).await.unwrap();
        let mut run = TaskRun::new(task.id, 1);
        run.state = TaskState::Executing;
        self.runtime.database().runs().insert(&run).await.unwrap();
        run
    }

    /// A delegation from `parent` to a new task of `child`, recorded as the
    /// tool records one, and attached to `run` when given.
    async fn edge(
        &self,
        parent: &TaskRun,
        parent_agent: &str,
        child: &str,
        run: Option<&TaskRun>,
        depth: u8,
    ) -> Delegation {
        let task_id = match run {
            Some(run) => run.task_id,
            None => {
                let task = agentos_core::task::Task::new(self.agent(child).id, "never started")
                    .with_parent(parent.task_id);
                self.runtime.database().tasks().insert(&task).await.unwrap();
                task.id
            }
        };
        let delegation = Delegation::requested(
            parent.id,
            self.agent(parent_agent).id,
            self.agent(child).id,
            task_id,
            format!("{child}, help {parent_agent}"),
            depth,
        );
        let delegations = self.runtime.database().delegations();
        delegations.insert(&delegation).await.unwrap();
        if let Some(run) = run {
            delegations.attach_run(delegation.id, run.id).await.unwrap();
        }
        delegations.get(delegation.id).await.unwrap()
    }
}

#[tokio::test]
async fn the_reaper_ends_every_delegation_a_dead_tree_left_open() {
    // A supervisor and its worker, both mid-way when the process died, and a
    // second delegation recorded but never given its run.
    let mut harness = Harness::new().await;
    let ending = Arc::new(Ending {
        inner: harness.runtime.registry().get("filesystem.read").unwrap(),
        ended: std::sync::Mutex::new(Vec::new()),
    });
    let mut registry = agentos_tools::ToolRegistry::new();
    registry.register(ending.clone());
    harness.runtime.set_registry(Arc::new(registry));

    let parent = harness.abandoned_run("supervisor", None).await;
    let child = harness.abandoned_run("worker", Some(&parent)).await;
    let running = harness
        .edge(&parent, "supervisor", "worker", Some(&child), 1)
        .await;
    let requested = harness.edge(&parent, "supervisor", "helper", None, 1).await;
    assert_eq!(
        (running.state, requested.state),
        (DelegationState::Running, DelegationState::Requested)
    );
    // And a worker left mid-way under a supervisor whose run did end, so its
    // edge can be found only from the child's end.
    let mut finished = harness.abandoned_run("supervisor", None).await;
    finished.state = TaskState::Completed;
    finished.completed_at = Some(agentos_core::now());
    harness
        .runtime
        .database()
        .runs()
        .update(&finished)
        .await
        .unwrap();
    let stranded = harness.abandoned_run("worker", Some(&finished)).await;
    let orphaned = harness
        .edge(&finished, "supervisor", "worker", Some(&stranded), 1)
        .await;

    assert_eq!(harness.runtime.reap_abandoned_runs().await.unwrap(), 3);

    for edge in [running, requested, orphaned] {
        let after = harness
            .runtime
            .database()
            .delegations()
            .get(edge.id)
            .await
            .unwrap();
        assert_eq!(after.state, DelegationState::Failed, "{after:?}");
        assert!(after.completed_at.is_some(), "{after:?}");
    }
    let mut ended = ending.ended.lock().unwrap().clone();
    ended.sort_by_key(ToString::to_string);
    let mut expected = vec![parent.id, child.id, stranded.id];
    expected.sort_by_key(ToString::to_string);
    assert_eq!(ended, expected, "every reaped run's tools must be released");
}

// -- Reading the tree back ------------------------------------------------------

#[tokio::test]
async fn the_tree_is_read_through_a_corrupt_cycle_once_and_no_deeper_than_the_limit() {
    let harness = Harness::new().await;

    // Two rows naming each other's runs, as only a hand-edited database holds.
    let first = harness.abandoned_run("supervisor", None).await;
    let second = harness.abandoned_run("worker", None).await;
    harness
        .edge(&first, "supervisor", "worker", Some(&second), 1)
        .await;
    harness
        .edge(&second, "worker", "supervisor", Some(&first), 1)
        .await;
    let tree = tokio::time::timeout(HUNG, harness.runtime.delegation_tree(first.id))
        .await
        .expect("a cycle held the reader")
        .unwrap();
    let [only] = tree.children.as_slice() else {
        panic!("expected one child, got {:?}", tree.children);
    };
    assert_eq!(only.run.id, second.id);
    assert!(
        only.children.is_empty(),
        "the run the tree was read from must not appear beneath itself"
    );

    // A chain longer than any the limits permit is cut where they would
    // have stopped it: the deepest permitted delegate, and one level more.
    let limit = usize::from(harness.runtime.config().delegation.max_depth);
    let names = ["supervisor", "worker", "helper", "scout"];
    let mut runs = vec![harness.abandoned_run(names[0], None).await];
    for level in 1..=limit + 3 {
        let parent = runs[level - 1].clone();
        let name = names[level % names.len()];
        let run = harness.abandoned_run(name, Some(&parent)).await;
        harness
            .edge(
                &parent,
                names[(level - 1) % names.len()],
                name,
                Some(&run),
                u8::try_from(level).unwrap(),
            )
            .await;
        runs.push(run);
    }
    let tree = tokio::time::timeout(HUNG, harness.runtime.delegation_tree(runs[0].id))
        .await
        .expect("the chain held the reader")
        .unwrap();
    let mut read = vec![tree.run.id];
    let mut node = &tree;
    while let Some(next) = node.children.first() {
        read.push(next.run.id);
        node = next;
    }
    let permitted: Vec<TaskRunId> = runs[..limit + 2].iter().map(|run| run.id).collect();
    assert_eq!(read, permitted);
}

// -- A run is forgotten however its driving ends --------------------------------

/// A model that says it has been asked, then answers nobody.
#[derive(Debug, Default)]
struct Parked {
    entered: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl ModelProvider for Parked {
    fn id(&self) -> &str {
        agentos_providers::provider_ids::MOCK
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            tools: true,
            usage_reporting: true,
            vision: false,
        }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, ProviderError> {
        self.entered.notify_one();
        cancel.cancelled().await;
        Err(ProviderError::Cancelled)
    }
}

#[tokio::test]
async fn a_run_whose_driver_is_dropped_is_no_longer_live() {
    // A run is driven inside a future that can be dropped part way, as a tool
    // call is when its run is torn down. Left live, it would hold a scheduler
    // slot that no cancellation could free, and keep its runtime in the
    // broker, for the life of the process.
    let harness = Harness::new().await;
    harness.policy("worker", &quiet("  {}\n")).await;
    let parked = Arc::new(Parked::default());
    harness.provide("worker", parked.clone());

    let gate: Arc<dyn ApprovalGate> = Arc::new(RecordingGate::approving());
    tokio::select! {
        outcome = harness.runtime.run_objective(
            harness.agent("worker").id,
            "Wait.",
            gate,
            CancellationToken::new(),
        ) => panic!("the parked run finished: {outcome:?}"),
        () = parked.entered.notified() => {}
    }

    assert!(
        harness.runtime.running_runs().await.is_empty(),
        "a dropped run must not stay live"
    );
    assert_eq!(harness.runtime.delegation_broker().live(), 0);
}
