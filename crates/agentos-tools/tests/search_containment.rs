//! `filesystem.search` against a real policy, through the real pipeline.
//!
//! A search is authorised on its root, and policy path rules match by prefix,
//! so the plan alone cannot see a deny rule on a directory inside that root.
//! What keeps the walk out is the policy probe the pipeline hands the tool.
//! These tests fail if the probe is missing, if it answers from anything but
//! the engine that authorised the call, or if it treats an `ask` as a yes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentos_audit::{AuditLog, InMemorySink};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::tool::{ToolCall, ToolOutcome};
use agentos_permissions::pattern::ResourcePattern;
use agentos_permissions::policy::{PolicyRule, TaintPolicy};
use agentos_permissions::{Policy, PolicyEngine};
use agentos_tools::{
    ExecutionReport, RecordingGate, TaintTracker, ToolContext, ToolPipeline, standard_registry,
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// A workspace with a secret directory inside it, and a directory outside it.
struct Workspace {
    root: PathBuf,
    outside: PathBuf,
    _root_guard: TempDir,
    _outside_guard: TempDir,
}

impl Workspace {
    fn new() -> Self {
        let root_guard = TempDir::new().unwrap();
        let root = std::fs::canonicalize(root_guard.path()).unwrap();
        let outside_guard = TempDir::new().unwrap();
        let outside = std::fs::canonicalize(outside_guard.path()).unwrap();

        std::fs::write(root.join("plan.md"), "needle in the open\n").unwrap();
        std::fs::create_dir_all(root.join("secrets/nested")).unwrap();
        std::fs::write(root.join("secrets/keys.md"), "needle secret-token\n").unwrap();
        std::fs::write(root.join("secrets/nested/more.md"), "needle nested-token\n").unwrap();
        std::fs::write(outside.join("elsewhere.md"), "needle outside-token\n").unwrap();

        Self {
            root,
            outside,
            _root_guard: root_guard,
            _outside_guard: outside_guard,
        }
    }

    fn secrets(&self) -> PathBuf {
        self.root.join("secrets")
    }
}

/// `effect` on listing and reading `allowed`, and a deny on each of `denied`.
fn policy(effect: Effect, allowed: &Path, denied: &[PathBuf]) -> Policy {
    let mut policy = Policy::deny_all("search-containment")
        // Taint escalation off, so these tests isolate path scoping.
        .with_taint_policy(TaintPolicy {
            enabled: false,
            escalate_at_or_above: RiskLevel::Medium,
        });
    for action in ["list", "read"] {
        policy = policy.with_rule(
            PolicyRule::new(format!("{action}-root"), "filesystem", action, effect)
                .with_resources(vec![ResourcePattern::path_prefix(allowed.to_path_buf())]),
        );
        for (index, path) in denied.iter().enumerate() {
            policy = policy.with_rule(
                PolicyRule::new(
                    format!("{action}-deny-{index}"),
                    "filesystem",
                    action,
                    Effect::Deny,
                )
                .with_resources(vec![ResourcePattern::path_prefix(path.clone())]),
            );
        }
    }
    policy
}

async fn search(
    workspace: &Workspace,
    policy: Policy,
    arguments: serde_json::Value,
) -> ExecutionReport {
    let audit = Arc::new(AuditLog::open(Arc::new(InMemorySink::new())).await.unwrap());
    let pipeline = ToolPipeline::new(
        Arc::new(standard_registry()),
        Arc::new(PolicyEngine::new(policy)),
        Arc::new(RecordingGate::approving()),
        audit,
    );
    // A context exactly as the runtime builds one: no probe of its own. The
    // pipeline is the only thing that may give the tool one.
    let context = ToolContext::new(
        AgentId::new(),
        TaskId::new(),
        TaskRunId::new(),
        workspace.root.clone(),
    );
    pipeline
        .execute(
            &ToolCall::new("call-1", "filesystem.search", arguments),
            &context,
            &TaintTracker::new(),
            "test-agent",
            &["filesystem.search".to_owned()],
            &CancellationToken::new(),
        )
        .await
}

fn matched_paths(report: &ExecutionReport) -> Vec<String> {
    report.result.structured.as_ref().unwrap()["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found["path"].as_str().unwrap().to_owned())
        .collect()
}

fn not_permitted(report: &ExecutionReport) -> u64 {
    report.result.structured.as_ref().unwrap()["skipped"]["not_permitted"]
        .as_u64()
        .unwrap()
}

#[tokio::test]
async fn a_deny_inside_an_allowed_root_binds_the_walk() {
    let workspace = Workspace::new();
    let deny_secrets = || policy(Effect::Allow, &workspace.root, &[workspace.secrets()]);

    for arguments in [
        serde_json::json!({"path": ".", "name": "*"}),
        serde_json::json!({"path": ".", "contains": "needle"}),
        serde_json::json!({"path": ".", "name": "*.md", "contains": "token"}),
    ] {
        let report = search(&workspace, deny_secrets(), arguments.clone()).await;

        // The root is allowed, so the call is: the deny binds inside it.
        assert_eq!(report.effect, Effect::Allow, "{arguments}");
        assert_eq!(report.outcome, ToolOutcome::Success, "{report:?}");
        let body = &report.result.content.body;
        for leaked in ["secrets", "keys.md", "nested", "more.md", "-token"] {
            assert!(
                !body.contains(leaked),
                "`{leaked}` reached the model for {arguments}:\n{body}"
            );
        }
        assert!(
            matched_paths(&report)
                .iter()
                .all(|path| !Path::new(path).starts_with(workspace.secrets())),
            "a denied path is in the structured result for {arguments}"
        );
        assert_eq!(not_permitted(&report), 1, "{arguments}");
    }

    // The control: without the deny, the same search does reach `secrets`, so
    // the absence above is the policy's doing and not the fixture's.
    let report = search(
        &workspace,
        policy(Effect::Allow, &workspace.root, &[]),
        serde_json::json!({"path": ".", "contains": "token"}),
    )
    .await;
    assert_eq!(matched_paths(&report).len(), 2, "{report:?}");
}

#[tokio::test]
async fn an_approved_search_reaches_only_what_the_policy_allows_outright() {
    // A person approved a search of the root as planned. They did not see,
    // and so did not approve, each path it would find beneath it.
    let workspace = Workspace::new();
    let report = search(
        &workspace,
        policy(Effect::Ask, &workspace.root, &[]),
        serde_json::json!({"path": ".", "contains": "needle"}),
    )
    .await;

    assert_eq!(report.effect, Effect::Ask);
    assert_eq!(report.outcome, ToolOutcome::Success, "{report:?}");
    assert!(matched_paths(&report).is_empty(), "{report:?}");
    assert!(!report.result.content.body.contains("needle"));
    assert_eq!(not_permitted(&report), 2, "plan.md and secrets/");
}

#[tokio::test]
async fn a_search_of_a_denied_root_never_runs() {
    let workspace = Workspace::new();
    let report = search(
        &workspace,
        policy(Effect::Allow, &workspace.root, &[workspace.secrets()]),
        serde_json::json!({"path": "secrets", "name": "*"}),
    )
    .await;

    assert_eq!(report.effect, Effect::Deny);
    assert_eq!(report.outcome, ToolOutcome::Denied);
    assert!(!report.result.content.body.contains("keys.md"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_out_of_an_allowed_root_is_not_followed() {
    let workspace = Workspace::new();
    std::os::unix::fs::symlink(&workspace.outside, workspace.root.join("door")).unwrap();
    std::os::unix::fs::symlink(
        workspace.outside.join("elsewhere.md"),
        workspace.root.join("window.md"),
    )
    .unwrap();

    // The policy allows the outside directory too: containment in the root is
    // the tool's own rule, and it must hold even where the policy is wider.
    let mut wide = policy(Effect::Allow, &workspace.root, &[]);
    for action in ["list", "read"] {
        wide = wide.with_rule(
            PolicyRule::new(
                format!("{action}-outside"),
                "filesystem",
                action,
                Effect::Allow,
            )
            .with_resources(vec![ResourcePattern::path_prefix(
                workspace.outside.clone(),
            )]),
        );
    }
    let report = search(
        &workspace,
        wide,
        serde_json::json!({"path": ".", "contains": "needle"}),
    )
    .await;

    assert_eq!(report.outcome, ToolOutcome::Success, "{report:?}");
    let body = &report.result.content.body;
    assert!(!body.contains("outside-token"), "{body}");
    assert!(
        matched_paths(&report)
            .iter()
            .all(|path| Path::new(path).starts_with(&workspace.root)),
        "{body}"
    );
    assert_eq!(
        report.result.structured.as_ref().unwrap()["skipped"]["outside_root"],
        2
    );
}
