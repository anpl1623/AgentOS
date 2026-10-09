//! Every shipped tool's manifest, pinned, and checked against what it plans.
//!
//! A tool's `required_capabilities` is the list a policy author writes rules
//! against. Nothing enforces it at run time — the policy engine evaluates the
//! plan, and the pipeline only records drift — so this is where a manifest that
//! has fallen behind its tool is caught before it ships. The table fails in both
//! directions: a new tool is not covered until it is listed here, and a manifest
//! that silently widens changes a row someone has to review.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::ResourceRef;
use agentos_integrations::account::{Account, InMemoryDirectory};
use agentos_runtime::RuntimeConfig;
use agentos_tools::{ToolContext, plan_exceeds_manifest};
use tempfile::TempDir;

/// Each registered tool and its manifest as sorted `domain.action` names.
const EXPECTED: &[(&str, &[&str])] = &[
    ("browser.back", &["browser.navigate"]),
    ("browser.click", &["browser.interact"]),
    ("browser.extract", &["browser.read"]),
    ("browser.forward", &["browser.navigate"]),
    ("browser.inspect", &["browser.read"]),
    ("browser.navigate", &["browser.navigate"]),
    (
        "browser.screenshot",
        &["browser.read", "browser.vision", "filesystem.write"],
    ),
    ("browser.type", &["browser.interact"]),
    ("browser.wait", &["browser.read"]),
    ("computer.click", &["computer.click"]),
    ("computer.drag", &["computer.drag"]),
    ("computer.inspect", &["computer.read"]),
    ("computer.key", &["computer.key"]),
    ("computer.move", &["computer.move"]),
    (
        "computer.screenshot",
        &["computer.screenshot", "computer.vision", "filesystem.write"],
    ),
    ("computer.scroll", &["computer.scroll"]),
    ("computer.type", &["computer.type"]),
    ("filesystem.copy", &["filesystem.read", "filesystem.write"]),
    ("filesystem.delete", &["filesystem.delete"]),
    ("filesystem.list", &["filesystem.list"]),
    (
        "filesystem.move",
        &["filesystem.delete", "filesystem.write"],
    ),
    ("filesystem.read", &["filesystem.read"]),
    // `filesystem.search` names what it finds and, given `contains`, reads it.
    ("filesystem.search", &["filesystem.list", "filesystem.read"]),
    ("filesystem.write", &["filesystem.write"]),
    // Each GitHub tool needs exactly one grant, scoped to the repository by
    // name, and none of them also needs `network.credential`: the grant is to
    // act as the bound account there, and the token is how that is done.
    ("github.checks.list", &["github.checks.read"]),
    ("github.issues.comment", &["github.issues.write"]),
    ("github.issues.create", &["github.issues.write"]),
    ("github.issues.get", &["github.issues.read"]),
    ("github.issues.list", &["github.issues.read"]),
    ("github.issues.update", &["github.issues.write"]),
    ("github.pulls.comment", &["github.pulls.write"]),
    ("github.pulls.create", &["github.pulls.write"]),
    ("github.pulls.get", &["github.pulls.read"]),
    ("github.pulls.list", &["github.pulls.read"]),
    ("github.pulls.merge", &["github.pulls.merge"]),
    ("github.repos.get", &["github.repos.read"]),
    // Reading an origin, writing to one and spending a stored credential are
    // three grants; the plan names the one or two a call needs.
    (
        "network.request",
        &["network.credential", "network.fetch", "network.send"],
    ),
    // `terminal.exec` authorises where a program runs as well as which one, so
    // its working directory is a `filesystem.read` it must declare.
    ("terminal.exec", &["filesystem.read", "terminal.exec"]),
];

fn registry(dir: &TempDir) -> Arc<agentos_tools::ToolRegistry> {
    agentos_runtime::build_registry(&RuntimeConfig::rooted_at(dir.path()))
}

/// The same registry with one GitHub account bound, which an integration
/// tool needs to plan: the card names the account it would act as.
fn registry_with_an_account(dir: &TempDir) -> Arc<agentos_tools::ToolRegistry> {
    let directory = InMemoryDirectory::new(vec![Account {
        id: "account".into(),
        integration: "github".into(),
        label: "work".into(),
        host: "https://api.github.com".into(),
        private_network: false,
    }]);
    agentos_runtime::build_registry_for(&RuntimeConfig::rooted_at(dir.path()), Arc::new(directory))
}

#[test]
fn every_registered_tool_has_exactly_its_pinned_manifest() {
    let dir = TempDir::new().unwrap();
    let registry = registry(&dir);

    let actual = registry
        .all_metadata()
        .into_iter()
        .map(|metadata| (metadata.name.clone(), metadata.capability_names()))
        .collect::<Vec<_>>();
    let expected = EXPECTED
        .iter()
        .map(|(name, capabilities)| {
            (
                (*name).to_owned(),
                capabilities
                    .iter()
                    .map(|capability| (*capability).to_owned())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    for (name, capabilities) in &actual {
        let pinned = expected.iter().find(|(pinned, _)| pinned == name);
        assert!(
            pinned.is_some(),
            "`{name}` is registered but not pinned here; add it to EXPECTED with {capabilities:?}"
        );
        assert_eq!(
            Some(capabilities),
            pinned.map(|(_, pinned)| pinned),
            "`{name}` declares a different manifest from the one pinned here"
        );
    }
    for (name, _) in &expected {
        assert!(
            actual.iter().any(|(registered, _)| registered == name),
            "`{name}` is pinned here but no longer registered"
        );
    }
}

/// Drive `plan()` for every tool that can plan without live external state.
///
/// Browser and computer tools cannot: planning them needs a running browser
/// session or a desktop with an application in front, so they are covered by
/// the pinned table above and by the pipeline recording drift at run time, not
/// by this test. GitHub's can, given an account to name: planning one asks
/// GitHub nothing.
#[tokio::test]
async fn tools_that_can_plan_offline_plan_within_their_manifest() {
    let dir = TempDir::new().unwrap();
    let registry = registry_with_an_account(&dir);
    let workspace = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(workspace.join("a.txt"), "x").unwrap();
    let context = ToolContext::new(AgentId::new(), TaskId::new(), TaskRunId::new(), workspace);

    let calls = [
        ("filesystem.read", serde_json::json!({"path": "a.txt"})),
        (
            "filesystem.write",
            serde_json::json!({"path": "b.txt", "content": "y"}),
        ),
        ("filesystem.list", serde_json::json!({"path": "."})),
        (
            "filesystem.search",
            serde_json::json!({"path": ".", "name": "*.txt", "contains": "x"}),
        ),
        ("filesystem.delete", serde_json::json!({"path": "a.txt"})),
        (
            "filesystem.copy",
            serde_json::json!({"from": "a.txt", "to": "c.txt"}),
        ),
        (
            "filesystem.move",
            serde_json::json!({"from": "a.txt", "to": "d.txt"}),
        ),
        (
            "terminal.exec",
            serde_json::json!({"program": "git", "args": ["status"]}),
        ),
        // Planning a request resolves no name and opens no socket.
        (
            "network.request",
            serde_json::json!({"url": "https://api.example.com/v1/items"}),
        ),
        (
            "network.request",
            serde_json::json!({
                "url": "https://api.example.com/v1/items",
                "method": "POST",
                "body": "{}",
                "credential": "default",
            }),
        ),
        (
            "github.repos.get",
            serde_json::json!({"repo": "acme/widgets"}),
        ),
        (
            "github.issues.list",
            serde_json::json!({"repo": "acme/widgets", "state": "all", "limit": 5}),
        ),
        (
            "github.issues.get",
            serde_json::json!({"repo": "acme/widgets", "number": 412}),
        ),
        (
            "github.issues.create",
            serde_json::json!({"repo": "acme/widgets", "title": "t", "body": "b"}),
        ),
        (
            "github.issues.comment",
            serde_json::json!({"repo": "acme/widgets", "number": 412, "body": "b"}),
        ),
        (
            "github.issues.update",
            serde_json::json!({"repo": "acme/widgets", "number": 412, "state": "closed"}),
        ),
        (
            "github.pulls.list",
            serde_json::json!({"repo": "acme/widgets"}),
        ),
        (
            "github.pulls.get",
            serde_json::json!({"repo": "acme/widgets", "number": 88, "include_diff": true}),
        ),
        (
            "github.pulls.create",
            serde_json::json!({
                "repo": "acme/widgets",
                "head": "fix",
                "base": "main",
                "title": "t",
                "body": "b",
            }),
        ),
        (
            "github.pulls.comment",
            serde_json::json!({"repo": "acme/widgets", "number": 88, "body": "b"}),
        ),
        (
            "github.pulls.merge",
            serde_json::json!({
                "repo": "acme/widgets",
                "number": 88,
                "base": "main",
                "method": "squash",
            }),
        ),
        (
            "github.checks.list",
            serde_json::json!({"repo": "acme/widgets", "git_ref": "main"}),
        ),
    ];

    for (name, arguments) in calls {
        let tool = registry
            .get(name)
            .unwrap_or_else(|| panic!("`{name}` is not registered"));
        let arguments = tool
            .validate(&arguments)
            .unwrap_or_else(|error| panic!("`{name}` rejected its probe arguments: {error}"));
        let plan = tool
            .plan(&arguments, &context)
            .await
            .unwrap_or_else(|error| panic!("`{name}` could not plan: {error}"));
        assert!(
            !plan.capabilities.is_empty(),
            "`{name}` planned no capability"
        );
        assert_eq!(
            plan_exceeds_manifest(tool.metadata(), &plan),
            Vec::<String>::new(),
            "`{name}` plans capabilities its manifest does not declare"
        );
        // A GitHub call is one grant, to act as the account on one
        // repository. The repository is the resource a `github` rule's names
        // are matched against, so a plan that scoped it any other way would
        // be matched by no rule an operator wrote.
        if name.starts_with("github.") {
            assert_eq!(plan.capabilities.len(), 1, "`{name}`");
            assert_eq!(
                plan.capabilities[0].resource,
                Some(ResourceRef::Named {
                    name: "acme/widgets".to_owned()
                }),
                "`{name}`"
            );
        }
    }
}
