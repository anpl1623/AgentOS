//! Smoke tests that run the real `agentos` binary.
//!
//! The unit tests cover rendering and the runtime tests cover behaviour; what
//! these check is that the assembled program actually starts, creates state on
//! disk, runs a task end to end and reports honestly about it. A binary that
//! compiles and immediately panics on startup would pass everything else.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// Run the binary with an isolated data directory and no colour.
fn agentos(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agentos"))
        .args(args)
        .env("AGENTOS_HOME", home)
        .env("NO_COLOR", "1")
        // Keep the test off the developer's real keychain and their shell's
        // environment; `doctor` reads provider keys.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("the agentos binary should be runnable")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_ok(home: &Path, args: &[&str]) -> String {
    let output = agentos(home, args);
    assert!(
        output.status.success(),
        "`agentos {}` failed:\n{}\n{}",
        args.join(" "),
        stdout(&output),
        String::from_utf8_lossy(&output.stderr)
    );
    stdout(&output)
}

#[test]
fn a_fresh_installation_runs_a_task_end_to_end() {
    let guard = TempDir::new().unwrap();
    let home = guard.path();

    // Doctor on an empty installation should succeed and say what is missing,
    // rather than failing because nothing is set up yet.
    let doctor = run_ok(home, &["doctor"]);
    assert!(doctor.contains("Everything checks out"), "{doctor}");
    assert!(doctor.contains("none configured"), "{doctor}");
    assert!(home.join("agentos.db").exists());

    // The tool catalogue marks which tools return attacker-controllable data.
    let tools = run_ok(home, &["tools"]);
    assert!(tools.contains("filesystem.read"));
    assert!(tools.contains("terminal.exec"));
    assert!(tools.contains("external"));

    // Creating an agent installs a deny-by-default policy.
    let created = run_ok(
        home,
        &[
            "agent",
            "create",
            "--name",
            "demo",
            "--provider",
            "mock",
            "--model",
            "scripted",
        ],
    );
    assert!(created.contains("Created agent demo"), "{created}");

    let policy = run_ok(home, &["policy", "show", "demo"]);
    assert!(policy.contains("default: deny"), "{policy}");
    assert!(policy.contains("default deny"), "{policy}");
    assert!(
        policy.contains("need approval"),
        "taint escalation should be described: {policy}"
    );

    // Run a task. The mock provider answers in one turn.
    let run = run_ok(home, &["task", "run", "Check in.", "--agent", "demo"]);
    assert!(run.contains("completed"), "{run}");
    assert!(run.contains("1 step(s)"), "{run}");

    let list = run_ok(home, &["task", "list"]);
    assert!(list.contains("succeeded"), "{list}");
    assert!(list.contains("Check in."), "{list}");

    // Everything that happened is on the record, and the record verifies.
    let tail = run_ok(home, &["audit", "tail"]);
    assert!(tail.contains("agent.task.started"), "{tail}");
    assert!(tail.contains("idle → planning"), "{tail}");
    assert!(tail.contains("agent.task.completed"), "{tail}");

    let verify = run_ok(home, &["audit", "verify"]);
    assert!(verify.contains("intact"), "{verify}");
}

#[test]
fn an_unknown_agent_fails_with_a_useful_message() {
    let guard = TempDir::new().unwrap();
    let output = agentos(guard.path(), &["agent", "show", "nobody"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no agent named `nobody`"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn creating_an_agent_with_an_unknown_tool_is_refused() {
    let guard = TempDir::new().unwrap();
    let output = agentos(
        guard.path(),
        &[
            "agent",
            "create",
            "--name",
            "bad",
            "--provider",
            "mock",
            "--model",
            "m",
            "--tool",
            "filesystem.chmod",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown tool"), "{stderr}");
}

#[test]
fn running_with_no_agents_explains_what_to_do() {
    let guard = TempDir::new().unwrap();
    let output = agentos(guard.path(), &["task", "run", "do something"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agentos agent create"), "{stderr}");
}

#[test]
fn an_invalid_policy_is_refused_rather_than_silently_stored() {
    let guard = TempDir::new().unwrap();
    let home = guard.path();
    run_ok(
        home,
        &[
            "agent",
            "create",
            "--name",
            "demo",
            "--provider",
            "mock",
            "--model",
            "m",
        ],
    );

    let bad = home.join("bad-policy.yaml");
    // `permisions` is misspelled. Accepting it would produce a policy that
    // grants nothing while appearing to grant plenty.
    std::fs::write(&bad, "permisions:\n  filesystem:\n    read: allow\n").unwrap();

    let output = agentos(home, &["policy", "set", "demo", bad.to_str().unwrap()]);
    assert!(!output.status.success());

    // The original policy is untouched.
    let policy = run_ok(home, &["policy", "show", "demo"]);
    assert!(policy.contains("policy version v1"), "{policy}");
}

#[test]
fn a_valid_policy_can_be_installed_and_is_summarised() {
    let guard = TempDir::new().unwrap();
    let home = guard.path();
    run_ok(
        home,
        &[
            "agent",
            "create",
            "--name",
            "demo",
            "--provider",
            "mock",
            "--model",
            "m",
        ],
    );

    let path = home.join("policy.yaml");
    std::fs::write(
        &path,
        format!(
            "default: deny\npermissions:\n  filesystem:\n    read: [\"{}\"]\n  terminal:\n    exec: [git]\n",
            home.display()
        ),
    )
    .unwrap();

    let validated = run_ok(home, &["policy", "validate", path.to_str().unwrap()]);
    assert!(validated.contains("Policy is valid"), "{validated}");

    let installed = run_ok(home, &["policy", "set", "demo", path.to_str().unwrap()]);
    assert!(installed.contains("version 2"), "{installed}");

    let shown = run_ok(home, &["policy", "show", "demo"]);
    assert!(shown.contains("terminal.exec => allow"), "{shown}");
}
