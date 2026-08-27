//! What a policy written against computer control actually does.
//!
//! These run the real pipeline — validation, the policy engine, the approval
//! gate, the audit log — against a desktop that records instead of acting. No
//! screen is involved, so they run identically on a laptop and on a headless CI
//! runner, which is the point: the part worth testing is the authorisation, and
//! it should not be testable on only one platform.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use agentos_audit::{AuditLog, InMemorySink};
use agentos_computer::{Desktop, RecordingDesktop};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::Effect;
use agentos_core::tool::{ToolCall, ToolOutcome};
use agentos_permissions::{PolicyDocument, PolicyEngine};
use agentos_tools::{
    ExecutionReport, RecordingGate, TaintTracker, ToolContext, ToolPipeline, ToolRegistry,
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// A pipeline over the computer tools and one policy.
struct Harness {
    pipeline: ToolPipeline,
    context: ToolContext,
    gate: Arc<RecordingGate>,
    desktop: Arc<RecordingDesktop>,
    enabled: Vec<String>,
    _workspace: TempDir,
}

impl Harness {
    async fn with_policy(yaml: &str, in_front: &str) -> Self {
        Self::build(yaml, Arc::new(RecordingDesktop::in_front(in_front))).await
    }

    async fn build(yaml: &str, desktop: Arc<RecordingDesktop>) -> Self {
        // A real workspace, so a policy can grant writes into it and a test can
        // then ask whether a file was actually written.
        let guard = TempDir::new().unwrap();
        let workspace = std::fs::canonicalize(guard.path()).unwrap();
        let yaml = yaml.replace(
            "{workspace}",
            &agentos_permissions::quote_scalar(&workspace.display().to_string()),
        );
        let policy = PolicyDocument::from_yaml(&yaml).unwrap().compile().unwrap();
        let mut registry = ToolRegistry::new();
        for tool in agentos_computer::tools::all(Arc::clone(&desktop) as Arc<dyn Desktop>) {
            registry.register(tool);
        }
        let gate = Arc::new(RecordingGate::approving());
        let pipeline = ToolPipeline::new(
            Arc::new(registry),
            Arc::new(PolicyEngine::new(policy)),
            Arc::clone(&gate) as Arc<dyn agentos_tools::ApprovalGate>,
            Arc::new(AuditLog::open(Arc::new(InMemorySink::new())).await.unwrap()),
        );
        Self {
            pipeline,
            context: ToolContext::new(AgentId::new(), TaskId::new(), TaskRunId::new(), workspace),
            gate,
            desktop,
            enabled: agentos_computer::TOOL_NAMES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            _workspace: guard,
        }
    }

    async fn call(&self, tool: &str, arguments: serde_json::Value) -> ExecutionReport {
        self.run(tool, arguments, &TaintTracker::new()).await
    }

    async fn run(
        &self,
        tool: &str,
        arguments: serde_json::Value,
        taint: &TaintTracker,
    ) -> ExecutionReport {
        self.pipeline
            .execute(
                &ToolCall::new("call", tool, arguments),
                &self.context,
                taint,
                "test-agent",
                &self.enabled,
                &CancellationToken::new(),
            )
            .await
    }
}

const MAIL_ONLY: &str = "
permissions:
  computer:
    type:
      effect: allow
      applications: [Mail]
    click:
      effect: allow
      applications: [Mail]
";

#[tokio::test]
async fn a_policy_scoped_to_one_application_allows_it() {
    let harness = Harness::with_policy(MAIL_ONLY, "Mail").await;
    let report = harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Mail", "text": "a draft"}),
        )
        .await;

    assert_eq!(report.outcome, ToolOutcome::Success, "{:?}", report.error);
    assert_eq!(report.effect, Effect::Allow);
    assert_eq!(harness.desktop.actions().len(), 1);
}

#[tokio::test]
async fn the_same_policy_refuses_every_other_application() {
    // Slack is in front, so a call naming Slack is the only one the tool will
    // plan — and the policy has nothing to say about Slack, so it is denied.
    let harness = Harness::with_policy(MAIL_ONLY, "Slack").await;
    let report = harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Slack", "text": "a draft"}),
        )
        .await;

    assert_eq!(report.outcome, ToolOutcome::Denied);
    assert!(harness.desktop.actions().is_empty());
}

#[tokio::test]
async fn naming_the_allowed_application_while_another_is_in_front_gets_nowhere() {
    // The interesting case: the agent asks for the application it is allowed to
    // use, but that application does not have focus. Authorising this on the
    // name alone would send the keystrokes to Slack.
    let harness = Harness::with_policy(MAIL_ONLY, "Slack").await;
    let report = harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Mail", "text": "a draft"}),
        )
        .await;

    assert_eq!(report.outcome, ToolOutcome::InvalidArguments);
    assert!(
        report
            .error
            .as_deref()
            .is_some_and(|error| error.contains("not in front")),
        "{:?}",
        report.error
    );
    assert!(harness.desktop.actions().is_empty());
}

#[tokio::test]
async fn a_committing_keystroke_exceeds_a_risk_ceiling_that_ordinary_typing_does_not() {
    // `max_risk` is how an operator says "you may work in Mail, but you may not
    // send". Without the commit distinction, Return would be indistinguishable
    // from the letter `a`.
    let harness = Harness::with_policy(
        "
permissions:
  computer:
    type:
      effect: allow
      applications: [Mail]
      max_risk: high
",
        "Mail",
    )
    .await;

    let draft = harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Mail", "text": "a draft"}),
        )
        .await;
    assert_eq!(draft.outcome, ToolOutcome::Success);

    let send = harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Mail", "text": "a draft\n"}),
        )
        .await;
    assert_eq!(send.outcome, ToolOutcome::Denied);
    assert_eq!(harness.desktop.actions().len(), 1, "the send went through");
}

#[tokio::test]
async fn an_unscoped_grant_is_what_it_says_it_is() {
    // Not a bug, but worth pinning: `computer: { click: allow }` really does
    // mean any application. An operator who writes it should not be able to
    // believe otherwise, and the test says so out loud.
    let harness = Harness::with_policy(
        "permissions:\n  computer:\n    click: allow\n",
        "Some Other App",
    )
    .await;
    let report = harness
        .call(
            "computer.click",
            serde_json::json!({"application": "Some Other App", "x": 10, "y": 10}),
        )
        .await;
    assert_eq!(report.outcome, ToolOutcome::Success);
}

#[tokio::test]
async fn agentos_refuses_to_be_the_target_whatever_the_policy_says() {
    // The chain this closes: read a hostile page, then click your own Approve
    // button. `runtime.disable_approvals` is immutably denied, but the immutable
    // denies have no resource dimension, so they cannot express this one.
    let harness = Harness::build(
        "permissions:\n  computer:\n    click: allow\n    type: allow\n",
        Arc::new(RecordingDesktop::in_front("AgentOS").owned_by_this_process()),
    )
    .await;

    for (tool, arguments) in [
        (
            "computer.click",
            serde_json::json!({"application": "AgentOS", "x": 10, "y": 10}),
        ),
        (
            "computer.type",
            serde_json::json!({"application": "AgentOS", "text": "y\n"}),
        ),
    ] {
        let report = harness.call(tool, arguments).await;
        assert_ne!(report.outcome, ToolOutcome::Success, "{tool} was permitted");
        assert!(
            report
                .error
                .as_deref()
                .is_some_and(|error| error.contains("may not send input")),
            "{tool}: {:?}",
            report.error
        );
    }
    assert!(harness.desktop.actions().is_empty());
}

#[tokio::test]
async fn reading_the_screen_raises_the_bar_for_what_follows() {
    // The property the whole design rests on: an agent that has looked at the
    // screen cannot then act silently. Reading is allowed outright; the click
    // afterwards is escalated to an approval it would not otherwise have needed.
    let harness = Harness::with_policy(
        "
taint_escalation:
  enabled: true
  escalate_at_or_above: medium

permissions:
  computer:
    read: allow
    click:
      effect: allow
      applications: [Mail]
",
        "Mail",
    )
    .await;

    let taint = TaintTracker::new();
    let before = harness
        .run(
            "computer.click",
            serde_json::json!({"application": "Mail", "x": 10, "y": 10}),
            &taint,
        )
        .await;
    assert_eq!(before.effect, Effect::Allow);
    assert_eq!(
        harness.gate.count().await,
        0,
        "nothing should have been asked yet"
    );

    let look = harness
        .run("computer.inspect", serde_json::json!({}), &taint)
        .await;
    assert_eq!(look.outcome, ToolOutcome::Success);
    assert!(taint.is_tainted(), "reading the desktop must taint the run");

    let after = harness
        .run(
            "computer.click",
            serde_json::json!({"application": "Mail", "x": 10, "y": 10}),
            &taint,
        )
        .await;
    assert_eq!(after.effect, Effect::Ask);
    assert_eq!(harness.gate.count().await, 1);

    // And the operator is told what the agent had been reading.
    let request = harness.gate.requests().await.into_iter().next().unwrap();
    assert!(request.tainted);
    assert!(
        request
            .taint_sources
            .iter()
            .any(|source| source.starts_with("screen:")),
        "{:?}",
        request.taint_sources
    );
}

#[tokio::test]
async fn the_approval_card_names_the_application_and_the_act() {
    let harness = Harness::with_policy(
        "
permissions:
  computer:
    type:
      effect: ask
      applications: [Mail]
",
        "Mail",
    )
    .await;

    harness
        .call(
            "computer.type",
            serde_json::json!({"application": "Mail", "text": "see you at three\n"}),
        )
        .await;

    let request = harness.gate.requests().await.into_iter().next().unwrap();
    assert_eq!(request.affected_resources, vec!["application:Mail"]);
    assert!(request.explanation.contains("In Mail"));
    assert!(
        request.explanation.contains("commits"),
        "an operator approving a send should be told it is a send: {}",
        request.explanation
    );
}

#[tokio::test]
async fn a_window_capture_refuses_when_the_window_changed_after_authorisation() {
    // An approval to capture the Mail window must not come back with a picture
    // of whatever took focus while the operator was reading the card.
    let harness = Harness::with_policy(
        "
permissions:
  filesystem:
    write: [{workspace}]
  computer:
    screenshot:
      effect: allow
      applications: [Mail]
",
        "Mail",
    )
    .await;

    // Mail is in front when the call is planned and authorised; something else
    // is in front by the time it runs.
    harness.desktop.switch_after(1, "1Password");

    let report = harness
        .call(
            "computer.screenshot",
            serde_json::json!({
                "filename": "shot.png", "target": "window", "application": "Mail"
            }),
        )
        .await;

    assert_ne!(report.outcome, ToolOutcome::Success);
    assert!(
        !harness.context.workspace.join("shot.png").exists(),
        "a capture of the wrong window was written to disk"
    );
}
