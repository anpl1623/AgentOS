//! Every shipped tool's manifest, pinned, and checked against what it plans.
//!
//! A tool's `required_capabilities` is the list a policy author writes rules
//! against. Nothing enforces it at run time — the policy engine evaluates the
//! plan, and the pipeline only records drift — so this is where a manifest that
//! has fallen behind its tool is caught before it ships. The table fails in both
//! directions: a new tool is not covered until it is listed here, and a manifest
//! that silently widens changes a row someone has to review.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
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
    // `terminal.exec` authorises where a program runs as well as which one, so
    // its working directory is a `filesystem.read` it must declare.
    ("terminal.exec", &["filesystem.read", "terminal.exec"]),
];

fn registry(dir: &TempDir) -> std::sync::Arc<agentos_tools::ToolRegistry> {
    agentos_runtime::build_registry(&RuntimeConfig::rooted_at(dir.path()))
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
/// by this test.
#[tokio::test]
async fn tools_that_can_plan_offline_plan_within_their_manifest() {
    let dir = TempDir::new().unwrap();
    let registry = registry(&dir);
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
    }
}
