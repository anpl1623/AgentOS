//! Standing instructions: memory, schedules, queued work and the scheduler.
//!
//! Each of these acts on runs that have not started yet, often with nobody
//! present when they do. The tests here hold the runtime to what makes that
//! safe: every operator change reaches the chain exactly once, a memory the
//! operator writes can only ever claim the operator as its source, a report of
//! what an agent may do reads the policy as the engine does, and two
//! schedulers, two clients or two processes on one installation produce one
//! firing, one run and one unbroken chain.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use agentos_audit::AuditRecord;
use agentos_core::agent::{Agent, ModelConfig};
use agentos_core::ids::TaskId;
use agentos_core::memory::{Memory, MemoryKind};
use agentos_core::schedule::{Cadence, ScheduleStatus};
use agentos_core::task::{Task, TaskStatus};
use agentos_core::trust::DataSource;
use agentos_providers::{MockProvider, ScriptedTurn};
use agentos_runtime::{
    FixedProviderFactory, Reach, Runtime, RuntimeConfig, RuntimeError, Scheduler, SchedulerOptions,
    SchedulerPreference, SchedulerTransition,
};
use agentos_secrets::InMemorySecretStore;
use agentos_tools::{DenyAllGate, RecordingGate};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// A provider whose every turn ends the run at once.
fn quick() -> Arc<FixedProviderFactory> {
    Arc::new(FixedProviderFactory::new(Arc::new(
        MockProvider::new(vec![ScriptedTurn::text("Done.")])
            .with_exhausted(ScriptedTurn::text("Done.")),
    )))
}

async fn in_memory() -> (Runtime, TempDir) {
    let guard = TempDir::new().unwrap();
    let root = std::fs::canonicalize(guard.path()).unwrap();
    let mut runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
        .await
        .unwrap();
    runtime.set_provider_factory(quick());
    (runtime, guard)
}

/// A runtime over the installation in `directory`, as a second process
/// opening the same data directory would have.
async fn on_disk(directory: &std::path::Path) -> Runtime {
    let mut runtime = Runtime::open_with_secrets(
        RuntimeConfig::rooted_at(directory),
        Arc::new(InMemorySecretStore::new()),
    )
    .await
    .unwrap();
    runtime.set_provider_factory(quick());
    runtime
}

async fn agent(runtime: &Runtime, tools: &[&str]) -> Agent {
    runtime
        .create_agent(
            "worker",
            "Do the work.",
            ModelConfig::new("mock", "scripted"),
            tools.iter().map(|tool| (*tool).to_owned()).collect(),
        )
        .await
        .unwrap()
}

async fn records(runtime: &Runtime) -> Vec<AuditRecord> {
    runtime.database().audit_sink().all().await.unwrap()
}

/// The records one change added, by kind.
async fn added_since(runtime: &Runtime, before: usize) -> Vec<String> {
    records(runtime).await[before..]
        .iter()
        .map(|record| record.kind.clone())
        .collect()
}

fn at(text: &str) -> agentos_core::Timestamp {
    chrono::DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[tokio::test]
async fn each_standing_change_is_recorded_exactly_once() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;

    // Each step names the one record it must add, and nothing else.
    let mut before = records(&runtime).await.len();
    let mut expect = async |kind: &str| {
        let added = added_since(&runtime, before).await;
        assert_eq!(added, vec![kind.to_owned()], "{kind}");
        before += added.len();
    };

    let memory = runtime
        .remember(agent.id, MemoryKind::Preference, "  Reply briefly.  ", 0.9)
        .await
        .unwrap();
    expect("operator.memory.recorded").await;
    runtime
        .revise_memory(memory.id, "Reply very briefly.", 1.0)
        .await
        .unwrap();
    expect("operator.memory.revised").await;
    runtime.forget_memory(memory.id).await.unwrap();
    expect("operator.memory.forgotten").await;

    let schedule = runtime
        .create_schedule(
            agent.id,
            "hourly",
            "Check the queue.",
            Cadence::Every { seconds: 3600 },
            at("2999-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
    expect("operator.schedule.created").await;
    let paused = runtime
        .set_schedule_paused(schedule.id, true)
        .await
        .unwrap();
    assert_eq!(paused.status, ScheduleStatus::Paused);
    expect("operator.schedule.paused").await;
    let resumed = runtime
        .set_schedule_paused(schedule.id, false)
        .await
        .unwrap();
    assert_eq!(resumed.status, ScheduleStatus::Active);
    expect("operator.schedule.resumed").await;
    runtime.delete_schedule(schedule.id).await.unwrap();
    expect("operator.schedule.deleted").await;

    let first = runtime
        .create_task(agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    expect("operator.task.created").await;
    // Edges written as part of creating a task are part of that one act.
    let second = runtime
        .create_task(agent.id, "Summarise.", &[first.id, first.id], None)
        .await
        .unwrap();
    expect("operator.task.created").await;
    let third = runtime
        .create_task(agent.id, "Publish.", &[], None)
        .await
        .unwrap();
    expect("operator.task.created").await;
    runtime
        .add_task_dependency(third.id, second.id)
        .await
        .unwrap();
    expect("operator.task.dependency_added").await;

    let options = SchedulerOptions::default();
    runtime
        .record_scheduler_state(SchedulerTransition::Started { on_launch: true }, &options)
        .await
        .unwrap();
    expect("operator.scheduler.started").await;
    runtime
        .record_scheduler_state(SchedulerTransition::Stopped, &options)
        .await
        .unwrap();
    expect("operator.scheduler.stopped").await;

    let all = records(&runtime).await;
    let of = |kind: &str| all.iter().find(|record| record.kind == kind).unwrap();

    // What a run will be told, as it was when it was written, survives the row.
    let remembered = of("operator.memory.recorded");
    assert_eq!(remembered.agent_id, Some(agent.id));
    assert_eq!(remembered.payload["content"], "Reply briefly.");
    assert_eq!(
        of("operator.memory.forgotten").payload["content"],
        "Reply very briefly."
    );

    // A queued task belongs to its agent and to itself, and names its edges
    // once each.
    let created = all
        .iter()
        .filter(|record| record.kind == "operator.task.created")
        .nth(1)
        .unwrap();
    assert_eq!(created.task_id, Some(second.id));
    assert_eq!(created.payload["depends_on"], serde_json::json!([first.id]));

    let started = of("operator.scheduler.started");
    assert_eq!(started.payload["on_launch"], true);
    assert_eq!(started.payload["tick_seconds"], 30);
    assert!(started.agent_id.is_none());

    for record in all.iter().filter(|r| r.kind.starts_with("operator.")) {
        assert!(
            agentos_core::event::is_security_kind(&record.kind),
            "{}",
            record.kind
        );
        assert!(
            agentos_core::event::operator_summary_of(&record.payload).is_some(),
            "{} has no summary",
            record.kind
        );
    }
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}

#[tokio::test]
async fn a_remembered_note_claims_the_operator_and_nothing_else() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;

    let memory = runtime
        .remember(agent.id, MemoryKind::Fact, "The office is in Leeds.", 5.0)
        .await
        .unwrap();
    let stored = runtime.database().memories().get(memory.id).await.unwrap();
    assert_eq!(stored.source, DataSource::User);
    assert!(!stored.is_from_untrusted_source());
    assert!((stored.confidence - 1.0).abs() < f32::EPSILON, "clamped");

    // A webpage's claim, recorded by the runtime from a run. Revising its
    // wording does not vouch for it.
    let claimed = Memory::new(
        agent.id,
        MemoryKind::Observation,
        "The invoice is overdue.",
        DataSource::Web {
            url: "https://example.com".into(),
        },
    );
    runtime
        .database()
        .memories()
        .insert(&claimed)
        .await
        .unwrap();
    let revised = runtime
        .revise_memory(claimed.id, "The invoice may be overdue.", 0.5)
        .await
        .unwrap();
    assert!(revised.is_from_untrusted_source());
    assert_eq!(revised.content, "The invoice may be overdue.");

    // Nothing to remember is refused, and refused without a record.
    let before = records(&runtime).await.len();
    assert!(matches!(
        runtime
            .remember(agent.id, MemoryKind::Fact, "   ", 1.0)
            .await,
        Err(RuntimeError::Rejected(_))
    ));
    assert_eq!(records(&runtime).await.len(), before);
}

#[tokio::test]
async fn a_queued_task_can_wait_for_others_and_for_a_time_at_once() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;
    let first = runtime
        .create_task(agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    let later = at("2999-01-01T00:00:00Z");

    let task = runtime
        .create_task(agent.id, "Summarise.", &[first.id], Some(later))
        .await
        .unwrap();
    let stored = runtime.task(task.id).await.unwrap();
    assert_eq!(stored.status, TaskStatus::Blocked);
    assert_eq!(stored.scheduled_for, Some(later));
    assert_eq!(
        runtime
            .database()
            .dependencies()
            .dependencies_of(task.id)
            .await
            .unwrap(),
        vec![first.id]
    );

    // One missing dependency refuses the whole task, before anything is
    // written: no task, no edges, no record.
    let before_records = records(&runtime).await.len();
    let before_tasks = runtime.database().tasks().list(100).await.unwrap().len();
    assert!(matches!(
        runtime
            .create_task(agent.id, "Never.", &[first.id, TaskId::new()], None)
            .await,
        Err(RuntimeError::InvalidGraph(_))
    ));
    assert_eq!(records(&runtime).await.len(), before_records);
    assert_eq!(
        runtime.database().tasks().list(100).await.unwrap().len(),
        before_tasks
    );
}

#[tokio::test]
async fn the_grant_report_reads_the_policy_as_the_engine_does() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(
        &runtime,
        &[
            "filesystem.read",
            "browser.screenshot",
            "terminal.exec",
            "nowhere.nothing",
        ],
    )
    .await;
    let workspace = runtime.config().workspace_for("worker");
    let policy = format!(
        "default: deny\n\
         permissions:\n\
         \x20 filesystem:\n\
         \x20   read: [\"{}\"]\n\
         \x20 browser:\n\
         \x20   screenshot: allow\n\
         \x20   read: allow\n\
         \x20 terminal:\n\
         \x20   exec: deny\n",
        workspace.display()
    );
    runtime.set_policy(agent.id, &policy).await.unwrap();

    let report = runtime.grant_report(agent.id).await.unwrap();
    let reach = |tool: &str, capability: &str| {
        report
            .iter()
            .find(|grant| grant.tool == tool)
            .unwrap()
            .capabilities
            .iter()
            .find(|grant| grant.capability == capability)
            .unwrap_or_else(|| panic!("{tool} does not report {capability}"))
            .reach
    };

    // One entry per granted tool, in the order it was granted.
    assert_eq!(
        report
            .iter()
            .map(|grant| grant.tool.as_str())
            .collect::<Vec<_>>(),
        vec![
            "filesystem.read",
            "browser.screenshot",
            "terminal.exec",
            "nowhere.nothing"
        ]
    );

    // A path-scoped allow is scoped.
    assert_eq!(reach("filesystem.read", "filesystem.read"), Reach::Scoped);

    // A rule named after the tool grants nothing the tool needs: a screenshot
    // shows a page to the model, which is `browser.vision`, and nothing allows
    // that.
    assert_eq!(reach("browser.screenshot", "browser.read"), Reach::Allowed);
    assert_eq!(reach("browser.screenshot", "browser.vision"), Reach::Denied);
    let screenshot = report
        .iter()
        .find(|grant| grant.tool == "browser.screenshot")
        .unwrap();
    assert!(
        screenshot
            .capabilities
            .iter()
            .all(|grant| grant.capability != "browser.screenshot")
    );
    assert_eq!(screenshot.weakest(), Reach::Denied);

    // A deny is not a grant.
    assert_eq!(reach("terminal.exec", "terminal.exec"), Reach::Denied);

    // Granted by name and missing from the installation.
    let missing = report
        .iter()
        .find(|grant| grant.tool == "nowhere.nothing")
        .unwrap();
    assert!(!missing.registered && missing.capabilities.is_empty());
}

#[tokio::test]
async fn an_agent_with_no_policy_is_reported_as_denied_everything() {
    let (runtime, _guard) = in_memory().await;
    let agent = Agent::new("bare", "x", ModelConfig::new("mock", "scripted"))
        .with_tools(["filesystem.read", "browser.navigate"]);
    runtime.database().agents().insert(&agent).await.unwrap();

    let report = runtime.grant_report(agent.id).await.unwrap();
    assert_eq!(report.len(), 2);
    for grant in &report {
        assert!(grant.registered);
        assert!(!grant.capabilities.is_empty());
        assert!(
            grant
                .capabilities
                .iter()
                .all(|capability| capability.reach == Reach::Denied),
            "{grant:?}"
        );
    }
}

#[tokio::test]
async fn the_scheduler_preference_is_off_until_somebody_turns_it_on() {
    let (runtime, _guard) = in_memory().await;
    let preference = runtime.scheduler_preference().await.unwrap();
    assert_eq!(preference, SchedulerPreference::default());
    assert!(!preference.enabled);

    let saved = runtime.set_scheduler_preference(true, 15, 2).await.unwrap();
    assert_eq!(runtime.scheduler_preference().await.unwrap(), saved);
    let options = saved.options();
    assert_eq!(options.tick, Duration::from_secs(15));
    assert_eq!(options.max_concurrent_runs, 2);

    // Pacing a scheduler should not run with is refused, and the saved
    // preference stands.
    for (tick, runs) in [(4, 1), (30, 0)] {
        assert!(matches!(
            runtime.set_scheduler_preference(true, tick, runs).await,
            Err(RuntimeError::Rejected(_))
        ));
    }
    assert_eq!(runtime.scheduler_preference().await.unwrap(), saved);

    // A damaged setting does not start unattended work.
    runtime
        .database()
        .settings()
        .set(agentos_persistence::settings::keys::SCHEDULER, "{not json")
        .await
        .unwrap();
    assert!(!runtime.scheduler_preference().await.unwrap().enabled);
}

#[tokio::test]
async fn a_second_scheduler_on_one_installation_refuses_to_start() {
    let directory = TempDir::new().unwrap();
    let desktop = on_disk(directory.path()).await;
    let terminal = on_disk(directory.path()).await;
    let options = SchedulerOptions::default().with_tick(Duration::from_secs(3600));

    let first = Arc::new(Scheduler::new(desktop.clone(), options));
    let running = tokio::spawn({
        let first = first.clone();
        async move { first.run().await }
    });
    wait_until_leased(directory.path()).await;

    // Refused at once, having fired and started nothing.
    let second = Scheduler::new(terminal.clone(), options);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), second.run())
            .await
            .expect("refused at once, not after a wait"),
        Err(RuntimeError::SchedulerAlreadyRunning)
    ));
    // Taken ahead of running, for a client that records the start, it is
    // refused the same way.
    assert!(matches!(
        second.take_lease(),
        Err(RuntimeError::SchedulerAlreadyRunning)
    ));

    // Once the first has stopped and drained, the lease is free. Taken ahead
    // of running, and twice, it is still one lease, and running uses it rather
    // than being refused by it.
    first.shutdown().await;
    running.await.unwrap().unwrap();
    let third = Arc::new(Scheduler::new(terminal, options));
    third.take_lease().unwrap();
    third.take_lease().unwrap();
    let again = tokio::spawn({
        let third = third.clone();
        async move { third.run().await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!again.is_finished(), "the lease was not released");
    third.shutdown().await;
    again.await.unwrap().unwrap();
}

/// Wait until some scheduler holds the installation's lease.
///
/// Read from the lock file itself, which is the lease: nothing else would show
/// that a spawned scheduler has got as far as taking it.
async fn wait_until_leased(directory: &std::path::Path) {
    let path = directory.join("scheduler.lock");
    for _ in 0..400 {
        if let Ok(file) = std::fs::File::open(&path)
            && matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no scheduler took the lease");
}

#[tokio::test]
async fn of_two_clients_starting_one_task_exactly_one_runs_it() {
    let directory = TempDir::new().unwrap();
    let desktop = on_disk(directory.path()).await;
    let terminal = on_disk(directory.path()).await;
    let agent = agent(&desktop, &[]).await;

    for round in 0..5 {
        let task = desktop
            .create_task(agent.id, &format!("Contended {round}."), &[], None)
            .await
            .unwrap();
        let gate = || Arc::new(RecordingGate::approving());
        let (a, b) = tokio::join!(
            desktop.start_task(&task, gate(), CancellationToken::new()),
            terminal.start_task(&task, gate(), CancellationToken::new()),
        );

        let (won, lost) = match (a, b) {
            (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
            (Ok(_), Ok(_)) => panic!("round {round}: both started the task"),
            (Err(a), Err(b)) => panic!("round {round}: neither started it: {a}; {b}"),
        };
        assert!(
            matches!(lost, RuntimeError::TaskAlreadyClaimed(id) if id == task.id),
            "{lost}"
        );
        won.1.await.unwrap().unwrap();

        // The loser wrote nothing: one run, and it finished.
        let runs = desktop
            .database()
            .runs()
            .list_for_task(task.id)
            .await
            .unwrap();
        assert_eq!(runs.len(), 1, "round {round}");
        assert_eq!(
            desktop.task(task.id).await.unwrap().status,
            TaskStatus::Succeeded
        );
    }

    // A task that has succeeded is not started again by anybody.
    let done = desktop.latest_task(agent.id).await.unwrap().unwrap();
    assert!(matches!(
        terminal
            .start_task(
                &done,
                Arc::new(RecordingGate::approving()),
                CancellationToken::new()
            )
            .await,
        Err(RuntimeError::TaskAlreadyClaimed(_))
    ));
}

#[tokio::test]
async fn two_schedulers_ticking_together_fire_a_schedule_once_and_run_its_task_once() {
    let directory = TempDir::new().unwrap();
    let desktop = on_disk(directory.path()).await;
    let terminal = on_disk(directory.path()).await;
    let agent = agent(&desktop, &[]).await;
    let options = SchedulerOptions::default().with_max_concurrent_runs(4);

    for round in 0..5 {
        let schedule = desktop
            .create_schedule(
                agent.id,
                &format!("due {round}"),
                "Check the queue.",
                Cadence::Every { seconds: 3600 },
                at("2020-01-01T00:00:00Z"),
            )
            .await
            .unwrap();

        // `tick` rather than `run`: these two get past the lease on purpose,
        // to show the claims underneath hold without it.
        let one = Scheduler::new(desktop.clone(), options);
        let two = Scheduler::new(terminal.clone(), options);
        let (a, b) = tokio::join!(one.tick(), two.tick());
        let (a, b) = (a.unwrap(), b.unwrap());
        one.drain().await;
        two.drain().await;

        assert_eq!(a.fired.len() + b.fired.len(), 1, "round {round}");
        assert_eq!(a.started.len() + b.started.len(), 1, "round {round}");
        let tasks: Vec<Task> = desktop
            .database()
            .tasks()
            .list(100)
            .await
            .unwrap()
            .into_iter()
            .filter(|task| task.schedule_id == Some(schedule.id))
            .collect();
        assert_eq!(tasks.len(), 1, "round {round}");
        assert_eq!(
            desktop
                .database()
                .runs()
                .list_for_task(tasks[0].id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    // Both processes wrote to the chain throughout, each from its own cached
    // tip, and nothing was lost or forked.
    let verification = desktop.verify_audit().await.unwrap();
    assert!(verification.is_intact(), "{:?}", verification.breaks);
    assert_eq!(
        records(&desktop)
            .await
            .iter()
            .filter(|record| record.kind == "schedule.fired")
            .count(),
        5
    );
}

#[tokio::test]
async fn a_queued_task_that_cannot_start_is_failed_once_and_not_counted_as_started() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;
    let task = runtime
        .create_task(agent.id, "Never.", &[], None)
        .await
        .unwrap();
    runtime.set_agent_enabled(agent.id, false).await.unwrap();

    let scheduler = Scheduler::new(runtime.clone(), SchedulerOptions::default());
    let report = scheduler.tick().await.unwrap();
    assert!(report.started.is_empty());
    assert_eq!(
        runtime.task(task.id).await.unwrap().status,
        TaskStatus::Failed
    );
    assert!(
        runtime
            .database()
            .runs()
            .list_for_task(task.id)
            .await
            .unwrap()
            .is_empty()
    );

    // Failed, it is no longer runnable, so the next tick does not try again.
    assert!(scheduler.tick().await.unwrap().is_idle());
}

#[tokio::test]
async fn a_stale_read_does_not_rerun_a_task_the_operator_has_since_stopped() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;
    let task = runtime
        .create_task(agent.id, "Queued.", &[], None)
        .await
        .unwrap();

    // A scheduler's tick reads the task as runnable...
    let stale = runtime.database().tasks().list_runnable(10).await.unwrap();
    assert_eq!(stale.len(), 1);

    // ...the operator starts it from the window and stops it at once...
    let cancel = CancellationToken::new();
    cancel.cancel();
    runtime
        .run_task(&task, Arc::new(DenyAllGate), cancel)
        .await
        .unwrap();
    assert_eq!(
        runtime.task(task.id).await.unwrap().status,
        TaskStatus::Cancelled
    );

    // ...and the tick's claim, made from the pending task it read, loses.
    assert!(matches!(
        runtime
            .start_task(&stale[0], Arc::new(DenyAllGate), CancellationToken::new())
            .await,
        Err(RuntimeError::TaskAlreadyClaimed(id)) if id == task.id
    ));
    assert_eq!(
        runtime.task(task.id).await.unwrap().status,
        TaskStatus::Cancelled
    );
    assert_eq!(
        runtime
            .database()
            .runs()
            .list_for_task(task.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_stale_read_does_not_start_a_task_before_a_dependency_added_since() {
    let (runtime, _guard) = in_memory().await;
    let agent = agent(&runtime, &[]).await;
    let first = runtime
        .create_task(agent.id, "Gather.", &[], Some(at("2999-01-01T00:00:00Z")))
        .await
        .unwrap();
    let second = runtime
        .create_task(agent.id, "Summarise.", &[], None)
        .await
        .unwrap();

    let stale = runtime.database().tasks().list_runnable(10).await.unwrap();
    let stale = stale.iter().find(|task| task.id == second.id).unwrap();
    runtime
        .add_task_dependency(second.id, first.id)
        .await
        .unwrap();

    assert!(matches!(
        runtime
            .start_task(stale, Arc::new(DenyAllGate), CancellationToken::new())
            .await,
        Err(RuntimeError::TaskAlreadyClaimed(_))
    ));
    assert_eq!(
        runtime.task(second.id).await.unwrap().status,
        TaskStatus::Blocked
    );
    assert!(
        runtime
            .database()
            .runs()
            .list_for_task(second.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_queued_task_and_its_edges_are_one_write() {
    // A task whose row is written before its edges is, for that moment, a
    // task that waits for nothing, and a scheduler in another process can
    // start it. The moment cannot be timed from a test, but its other face
    // can: if the edges cannot be written, a single write leaves no task,
    // while two writes leave one waiting for nothing. A trigger makes every
    // edge fail.
    let directory = TempDir::new().unwrap();
    let runtime = on_disk(directory.path()).await;
    let agent = agent(&runtime, &[]).await;
    let first = runtime
        .create_task(agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER refuse_edges BEFORE INSERT ON task_dependencies
         BEGIN SELECT RAISE(ABORT, 'edges refused'); END",
    )
    .execute(runtime.database().pool())
    .await
    .unwrap();

    let before = runtime.database().tasks().list(100).await.unwrap().len();
    let records_before = records(&runtime).await.len();
    assert!(
        runtime
            .create_task(agent.id, "Summarise.", &[first.id], None)
            .await
            .is_err()
    );
    assert_eq!(
        runtime.database().tasks().list(100).await.unwrap().len(),
        before
    );
    assert!(
        runtime
            .database()
            .tasks()
            .list_runnable(10)
            .await
            .unwrap()
            .iter()
            .all(|task| task.objective != "Summarise.")
    );
    assert_eq!(records(&runtime).await.len(), records_before);
}
