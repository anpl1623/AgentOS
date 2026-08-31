//! End-to-end tests of the scheduler and of task graphs.
//!
//! Everything runs against the scripted mock provider and an in-memory
//! database, so a whole graph executes in milliseconds and the tests assert on
//! what actually happened rather than on what was scheduled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use agentos_core::agent::{Agent, ModelConfig};
use agentos_core::schedule::{Cadence, ScheduleStatus};
use agentos_core::task::TaskStatus;
use agentos_providers::{MockProvider, ScriptedTurn};
use agentos_runtime::{FixedProviderFactory, Runtime, RuntimeError, Scheduler, SchedulerOptions};
use agentos_secrets::InMemorySecretStore;
use agentos_tools::{ApprovalGate, RecordingGate};
use tempfile::TempDir;

const POLICY: &str = "\
default: deny
taint_escalation:
  enabled: false
  escalate_at_or_above: medium
permissions: {}
";

struct Harness {
    runtime: Runtime,
    agent: Agent,
    _guard: TempDir,
}

impl Harness {
    async fn new() -> Self {
        let guard = TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let mut runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .unwrap();

        // Every turn answers immediately, so a scheduled run is a fast one.
        runtime.set_provider_factory(Arc::new(FixedProviderFactory::new(Arc::new(
            MockProvider::new(vec![ScriptedTurn::text("Done.")])
                .with_exhausted(ScriptedTurn::text("Done.")),
        ))));

        let agent = runtime
            .create_agent(
                "worker",
                "Do the work.",
                ModelConfig::new("mock", "scripted"),
                vec![],
            )
            .await
            .unwrap();
        runtime
            .database()
            .agents()
            .set_policy(agent.id, POLICY)
            .await
            .unwrap();

        Self {
            runtime,
            agent,
            _guard: guard,
        }
    }

    fn scheduler(&self, max_concurrent_runs: usize) -> Scheduler {
        Scheduler::new(
            self.runtime.clone(),
            SchedulerOptions::default()
                .with_max_concurrent_runs(max_concurrent_runs)
                .with_tick(Duration::from_millis(10)),
        )
        .with_approvals(Arc::new(RecordingGate::approving()) as Arc<dyn ApprovalGate>)
    }

    async fn status(&self, id: agentos_core::ids::TaskId) -> TaskStatus {
        self.runtime.task(id).await.unwrap().status
    }

    /// Wait until `count` tasks have succeeded, without draining the
    /// scheduler's handles — the point being to let it reap them itself on the
    /// next tick.
    ///
    /// Waiting on a count rather than on "nothing is running" because a task
    /// that has been started but has not yet reached `Running` is
    /// indistinguishable from one that finished, and settling on that would be
    /// a race that passes locally and fails in CI.
    async fn wait_for_successes(&self, count: usize) {
        for _ in 0..400 {
            let succeeded = self
                .runtime
                .database()
                .tasks()
                .list(100)
                .await
                .unwrap()
                .into_iter()
                .filter(|task| task.status == TaskStatus::Succeeded)
                .count();
            if succeeded >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("expected {count} runs to succeed");
    }
}

fn at(text: &str) -> agentos_core::Timestamp {
    chrono::DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&chrono::Utc)
}

#[tokio::test]
async fn a_due_schedule_produces_a_task_and_moves_on() {
    let harness = Harness::new().await;
    let schedule = harness
        .runtime
        .create_schedule(
            harness.agent.id,
            "hourly",
            "Check the queue.",
            Cadence::Every { seconds: 3600 },
            at("2020-01-01T00:00:00Z"),
        )
        .await
        .unwrap();

    let scheduler = harness.scheduler(1);
    let report = scheduler.tick().await.unwrap();
    assert_eq!(report.fired.len(), 1);
    assert_eq!(report.fired[0].0, schedule.id);
    scheduler.drain().await;

    // It fired once, not once per hour since 2020.
    let stored = harness
        .runtime
        .database()
        .schedules()
        .get(schedule.id)
        .await
        .unwrap();
    assert!(stored.next_run_at.unwrap() > agentos_core::now());
    assert_eq!(stored.last_task_id, Some(report.fired[0].1));

    // And a second tick does not fire it again.
    assert!(scheduler.tick().await.unwrap().fired.is_empty());
}

#[tokio::test]
async fn a_one_shot_schedule_finishes_after_firing() {
    let harness = Harness::new().await;
    let schedule = harness
        .runtime
        .create_schedule(
            harness.agent.id,
            "once",
            "Do it once.",
            Cadence::Once,
            at("2020-01-01T00:00:00Z"),
        )
        .await
        .unwrap();

    let scheduler = harness.scheduler(1);
    scheduler.tick().await.unwrap();
    scheduler.drain().await;

    let stored = harness
        .runtime
        .database()
        .schedules()
        .get(schedule.id)
        .await
        .unwrap();
    assert_eq!(stored.status, ScheduleStatus::Finished);
    assert_eq!(stored.next_run_at, None);
}

#[tokio::test]
async fn a_scheduled_task_actually_runs() {
    let harness = Harness::new().await;
    harness
        .runtime
        .create_schedule(
            harness.agent.id,
            "now",
            "Do the work.",
            Cadence::Once,
            at("2020-01-01T00:00:00Z"),
        )
        .await
        .unwrap();

    let scheduler = harness.scheduler(1);
    let fired = scheduler.tick().await.unwrap();
    let task_id = fired.fired[0].1;

    // The task exists before it has been started.
    assert_eq!(harness.status(task_id).await, TaskStatus::Pending);

    scheduler.tick().await.unwrap();
    scheduler.drain().await;
    assert_eq!(harness.status(task_id).await, TaskStatus::Succeeded);
}

#[tokio::test]
async fn a_graph_runs_in_dependency_order() {
    let harness = Harness::new().await;
    let gather = harness
        .runtime
        .create_task(harness.agent.id, "Gather.")
        .await
        .unwrap();
    let summarise = harness
        .runtime
        .create_task_after(harness.agent.id, "Summarise.", &[gather.id])
        .await
        .unwrap();

    assert_eq!(harness.status(summarise.id).await, TaskStatus::Blocked);

    let scheduler = harness.scheduler(4);

    // Only the unblocked half is startable.
    let first = scheduler.tick().await.unwrap();
    assert_eq!(first.started, vec![gather.id]);
    scheduler.drain().await;

    let second = scheduler.tick().await.unwrap();
    assert_eq!(second.started, vec![summarise.id]);
    scheduler.drain().await;

    assert_eq!(harness.status(gather.id).await, TaskStatus::Succeeded);
    assert_eq!(harness.status(summarise.id).await, TaskStatus::Succeeded);
}

#[tokio::test]
async fn a_fan_in_waits_for_every_branch() {
    let harness = Harness::new().await;
    let a = harness
        .runtime
        .create_task(harness.agent.id, "A.")
        .await
        .unwrap();
    let b = harness
        .runtime
        .create_task(harness.agent.id, "B.")
        .await
        .unwrap();
    let join = harness
        .runtime
        .create_task_after(harness.agent.id, "Both.", &[a.id, b.id])
        .await
        .unwrap();

    let scheduler = harness.scheduler(4);
    let first = scheduler.tick().await.unwrap();
    assert_eq!(first.started.len(), 2, "both branches start together");
    assert!(!first.started.contains(&join.id));
    scheduler.drain().await;

    let second = scheduler.tick().await.unwrap();
    assert_eq!(second.started, vec![join.id]);
    scheduler.drain().await;
    assert_eq!(harness.status(join.id).await, TaskStatus::Succeeded);
}

#[tokio::test]
async fn a_cycle_is_refused_and_names_the_path() {
    let harness = Harness::new().await;
    let a = harness
        .runtime
        .create_task(harness.agent.id, "A.")
        .await
        .unwrap();
    let b = harness
        .runtime
        .create_task_after(harness.agent.id, "B.", &[a.id])
        .await
        .unwrap();
    let c = harness
        .runtime
        .create_task_after(harness.agent.id, "C.", &[b.id])
        .await
        .unwrap();

    // A waiting for C would close A -> C -> B -> A.
    let error = harness
        .runtime
        .add_dependency(a.id, c.id)
        .await
        .expect_err("a cycle is refused");

    match error {
        RuntimeError::DependencyCycle { path } => {
            assert_eq!(path.first(), Some(&a.id));
            assert_eq!(path.last(), Some(&a.id));
            assert!(path.contains(&b.id));
            assert!(path.contains(&c.id));
        }
        other => panic!("expected a cycle, got {other:?}"),
    }

    // And nothing was written: the graph still runs.
    let scheduler = harness.scheduler(4);
    assert_eq!(scheduler.tick().await.unwrap().started, vec![a.id]);
    scheduler.drain().await;
}

#[tokio::test]
async fn a_task_cannot_wait_for_itself_or_for_a_task_that_does_not_exist() {
    let harness = Harness::new().await;
    let a = harness
        .runtime
        .create_task(harness.agent.id, "A.")
        .await
        .unwrap();

    assert!(matches!(
        harness.runtime.add_dependency(a.id, a.id).await,
        Err(RuntimeError::InvalidGraph(_))
    ));
    assert!(matches!(
        harness
            .runtime
            .create_task_after(harness.agent.id, "B.", &[agentos_core::ids::TaskId::new()])
            .await,
        Err(RuntimeError::InvalidGraph(_))
    ));
}

#[tokio::test]
async fn a_task_held_until_later_is_not_started_yet() {
    let harness = Harness::new().await;
    let later = harness
        .runtime
        .create_task_at(harness.agent.id, "Later.", at("2999-01-01T00:00:00Z"))
        .await
        .unwrap();

    let scheduler = harness.scheduler(4);
    assert!(scheduler.tick().await.unwrap().started.is_empty());
    assert_eq!(harness.status(later.id).await, TaskStatus::Pending);
}

#[tokio::test]
async fn concurrency_is_bounded() {
    let harness = Harness::new().await;
    for index in 0..5 {
        harness
            .runtime
            .create_task(harness.agent.id, &format!("Task {index}."))
            .await
            .unwrap();
    }

    let scheduler = harness.scheduler(2);
    assert_eq!(
        scheduler.tick().await.unwrap().started.len(),
        2,
        "five runnable tasks, two slots"
    );

    // A tick while the first pair is still in flight starts nothing new.
    assert!(scheduler.tick().await.unwrap().started.is_empty());

    harness.wait_for_successes(2).await;
    let second = scheduler.tick().await.unwrap();
    assert_eq!(second.finished.len(), 2, "the first pair is reaped");
    assert_eq!(second.started.len(), 2, "and the slots are reused");
    scheduler.drain().await;
}

#[tokio::test]
async fn a_pause_stops_a_schedule_without_losing_it() {
    let harness = Harness::new().await;
    let schedule = harness
        .runtime
        .create_schedule(
            harness.agent.id,
            "hourly",
            "Check the queue.",
            Cadence::Every { seconds: 3600 },
            at("2020-01-01T00:00:00Z"),
        )
        .await
        .unwrap();

    harness.runtime.pause_schedule(schedule.id).await.unwrap();
    let scheduler = harness.scheduler(1);
    assert!(scheduler.tick().await.unwrap().fired.is_empty());

    harness.runtime.resume_schedule(schedule.id).await.unwrap();
    let resumed = harness
        .runtime
        .database()
        .schedules()
        .get(schedule.id)
        .await
        .unwrap();
    assert_eq!(resumed.status, ScheduleStatus::Active);
    assert!(
        resumed.next_run_at.unwrap() > agentos_core::now(),
        "resuming does not fire for every hour that passed while it was paused"
    );
}

#[tokio::test]
async fn an_unreachable_task_is_abandoned_rather_than_left_waiting() {
    let harness = Harness::new().await;
    let gather = harness
        .runtime
        .create_task(harness.agent.id, "Gather.")
        .await
        .unwrap();
    let summarise = harness
        .runtime
        .create_task_after(harness.agent.id, "Summarise.", &[gather.id])
        .await
        .unwrap();

    harness
        .runtime
        .database()
        .tasks()
        .set_status(gather.id, TaskStatus::Failed)
        .await
        .unwrap();

    let scheduler = harness.scheduler(4);
    let report = scheduler.tick().await.unwrap();
    assert_eq!(report.abandoned, vec![summarise.id]);
    assert!(report.started.is_empty());
    assert_eq!(harness.status(summarise.id).await, TaskStatus::Cancelled);

    // And it says so where somebody can find it afterwards.
    let abandoned: Vec<_> = harness
        .runtime
        .database()
        .audit_sink()
        .tail(100)
        .await
        .unwrap()
        .into_iter()
        .filter(|record| record.kind == "agent.task.abandoned")
        .collect();
    assert_eq!(abandoned.len(), 1);
    assert_eq!(abandoned[0].task_id, Some(summarise.id));
    assert_eq!(abandoned[0].payload["blocked_by"], gather.id.to_string());
}

#[tokio::test]
async fn a_scheduled_run_cannot_approve_on_a_persons_behalf() {
    // The default gate, not the test one: this is the property the whole
    // unattended story rests on.
    let harness = Harness::new().await;
    let scheduler = Scheduler::new(harness.runtime.clone(), SchedulerOptions::default());
    assert!(format!("{scheduler:?}").contains("DenyAllGate"));
}
