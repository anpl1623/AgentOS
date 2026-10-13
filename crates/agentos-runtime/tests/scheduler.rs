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

    /// Mark a task failed without running it.
    async fn fail(&self, id: agentos_core::ids::TaskId) {
        self.runtime
            .database()
            .tasks()
            .set_status(id, TaskStatus::Failed)
            .await
            .unwrap();
    }

    /// Every `agent.task.abandoned` record, newest first.
    async fn abandonments(&self) -> Vec<agentos_audit::AuditRecord> {
        self.runtime
            .database()
            .audit_sink()
            .tail(100)
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.kind == "agent.task.abandoned")
            .collect()
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

    // Fired and started in one tick, and claimed before the tick returned: a
    // task a scheduler has taken never still looks runnable to another.
    assert_eq!(fired.started, vec![task_id]);
    assert_eq!(harness.status(task_id).await, TaskStatus::Running);
    assert!(
        harness
            .runtime
            .database()
            .tasks()
            .list_runnable(10)
            .await
            .unwrap()
            .is_empty()
    );

    scheduler.drain().await;
    assert_eq!(harness.status(task_id).await, TaskStatus::Succeeded);
}

#[tokio::test]
async fn a_graph_runs_in_dependency_order() {
    let harness = Harness::new().await;
    let gather = harness
        .runtime
        .create_task(harness.agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    let summarise = harness
        .runtime
        .create_task(harness.agent.id, "Summarise.", &[gather.id], None)
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
        .create_task(harness.agent.id, "A.", &[], None)
        .await
        .unwrap();
    let b = harness
        .runtime
        .create_task(harness.agent.id, "B.", &[], None)
        .await
        .unwrap();
    let join = harness
        .runtime
        .create_task(harness.agent.id, "Both.", &[a.id, b.id], None)
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
        .create_task(harness.agent.id, "A.", &[], None)
        .await
        .unwrap();
    let b = harness
        .runtime
        .create_task(harness.agent.id, "B.", &[a.id], None)
        .await
        .unwrap();
    let c = harness
        .runtime
        .create_task(harness.agent.id, "C.", &[b.id], None)
        .await
        .unwrap();

    // A waiting for C would close A -> C -> B -> A.
    let error = harness
        .runtime
        .add_task_dependency(a.id, c.id)
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
        .create_task(harness.agent.id, "A.", &[], None)
        .await
        .unwrap();

    assert!(matches!(
        harness.runtime.add_task_dependency(a.id, a.id).await,
        Err(RuntimeError::InvalidGraph(_))
    ));
    assert!(matches!(
        harness
            .runtime
            .create_task(
                harness.agent.id,
                "B.",
                &[agentos_core::ids::TaskId::new()],
                None
            )
            .await,
        Err(RuntimeError::InvalidGraph(_))
    ));
}

#[tokio::test]
async fn a_task_held_until_later_is_not_started_yet() {
    let harness = Harness::new().await;
    let later = harness
        .runtime
        .create_task(
            harness.agent.id,
            "Later.",
            &[],
            Some(at("2999-01-01T00:00:00Z")),
        )
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
            .create_task(harness.agent.id, &format!("Task {index}."), &[], None)
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

    harness
        .runtime
        .set_schedule_paused(schedule.id, true)
        .await
        .unwrap();
    let scheduler = harness.scheduler(1);
    assert!(scheduler.tick().await.unwrap().fired.is_empty());

    harness
        .runtime
        .set_schedule_paused(schedule.id, false)
        .await
        .unwrap();
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
        .create_task(harness.agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    let summarise = harness
        .runtime
        .create_task(harness.agent.id, "Summarise.", &[gather.id], None)
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
async fn a_dead_chain_is_abandoned_whole_in_one_tick() {
    let harness = Harness::new().await;
    let gather = harness
        .runtime
        .create_task(harness.agent.id, "Gather.", &[], None)
        .await
        .unwrap();
    let summarise = harness
        .runtime
        .create_task(harness.agent.id, "Summarise.", &[gather.id], None)
        .await
        .unwrap();
    let publish = harness
        .runtime
        .create_task(harness.agent.id, "Publish.", &[summarise.id], None)
        .await
        .unwrap();
    harness.fail(gather.id).await;

    // One tick, not one per layer: the last task must not spend a tick looking
    // as though it were merely waiting its turn.
    let scheduler = harness.scheduler(4);
    let report = scheduler.tick().await.unwrap();
    assert_eq!(report.abandoned.len(), 2);
    assert_eq!(harness.status(summarise.id).await, TaskStatus::Cancelled);
    assert_eq!(harness.status(publish.id).await, TaskStatus::Cancelled);

    // Each names the task it was waiting on, so the chain reads link by link
    // rather than every event blaming the root.
    let records = harness.abandonments().await;
    assert_eq!(records.len(), 2);
    let blocker_of = |id: agentos_core::ids::TaskId| {
        records
            .iter()
            .find(|record| record.task_id == Some(id))
            .map(|record| record.payload["blocked_by"].clone())
            .unwrap()
    };
    assert_eq!(blocker_of(summarise.id), gather.id.to_string());
    assert_eq!(blocker_of(publish.id), summarise.id.to_string());

    // Nothing is left for the next tick to find.
    assert!(scheduler.tick().await.unwrap().abandoned.is_empty());
}

#[tokio::test]
async fn a_dead_diamond_abandons_its_join_once() {
    let harness = Harness::new().await;
    let root = harness
        .runtime
        .create_task(harness.agent.id, "Root.", &[], None)
        .await
        .unwrap();
    let left = harness
        .runtime
        .create_task(harness.agent.id, "Left.", &[root.id], None)
        .await
        .unwrap();
    let right = harness
        .runtime
        .create_task(harness.agent.id, "Right.", &[root.id], None)
        .await
        .unwrap();
    let join = harness
        .runtime
        .create_task(harness.agent.id, "Join.", &[left.id, right.id], None)
        .await
        .unwrap();
    harness.fail(root.id).await;

    let scheduler = harness.scheduler(4);
    let mut abandoned = scheduler.tick().await.unwrap().abandoned;
    abandoned.sort_unstable();
    let mut expected = vec![left.id, right.id, join.id];
    expected.sort_unstable();
    assert_eq!(abandoned, expected);

    // The join is reachable down both sides, and is recorded once.
    let records = harness.abandonments().await;
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.task_id == Some(join.id))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_task_found_both_directly_and_downstream_is_abandoned_once() {
    // `after` waits on the failed root and on `middle`, so the query lists it
    // as stuck in its own right and the walk from `middle` reaches it too. The
    // list was read before the walk ran, and must not be trusted after it.
    let harness = Harness::new().await;
    let root = harness
        .runtime
        .create_task(harness.agent.id, "Root.", &[], None)
        .await
        .unwrap();
    let middle = harness
        .runtime
        .create_task(harness.agent.id, "Middle.", &[root.id], None)
        .await
        .unwrap();
    let after = harness
        .runtime
        .create_task(harness.agent.id, "After.", &[root.id, middle.id], None)
        .await
        .unwrap();
    harness.fail(root.id).await;

    let scheduler = harness.scheduler(4);
    assert_eq!(
        scheduler.tick().await.unwrap().abandoned,
        vec![middle.id, after.id]
    );
    assert_eq!(harness.abandonments().await.len(), 2);
}

#[tokio::test]
async fn a_dead_chain_longer_than_a_batch_finishes_on_the_next_tick() {
    // The cap on one tick's walk is a fairness bound. What it leaves behind
    // must still be found, because the tasks it did cancel are themselves now
    // cancelled dependencies.
    let harness = Harness::new().await;
    let root = harness
        .runtime
        .create_task(harness.agent.id, "Root.", &[], None)
        .await
        .unwrap();
    let mut chain = Vec::new();
    let mut previous = root.id;
    for index in 0..4 {
        let task = harness
            .runtime
            .create_task(
                harness.agent.id,
                &format!("Step {index}."),
                &[previous],
                None,
            )
            .await
            .unwrap();
        previous = task.id;
        chain.push(task.id);
    }
    harness.fail(root.id).await;

    let scheduler = Scheduler::new(
        harness.runtime.clone(),
        SchedulerOptions {
            batch: 2,
            ..SchedulerOptions::default()
        },
    );
    assert_eq!(scheduler.tick().await.unwrap().abandoned, chain[..2]);
    assert_eq!(scheduler.tick().await.unwrap().abandoned, chain[2..]);
    for id in chain {
        assert_eq!(harness.status(id).await, TaskStatus::Cancelled);
    }
    assert_eq!(harness.abandonments().await.len(), 4);
}

#[tokio::test]
async fn a_scheduled_run_cannot_approve_on_a_persons_behalf() {
    // The default gate, not the test one: this is the property the whole
    // unattended story rests on.
    let harness = Harness::new().await;
    let scheduler = Scheduler::new(harness.runtime.clone(), SchedulerOptions::default());
    assert!(format!("{scheduler:?}").contains("DenyAllGate"));
}

/// A scheduled run that delegates.
///
/// Kept apart from [`Harness`], whose one agent may do nothing: here a
/// supervisor may ask a worker, and the worker's model holds its turn until
/// the test releases it or the run is cancelled, so the child is live for
/// exactly as long as the test needs it to be.
mod delegating {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use agentos_core::agent::{Agent, ModelConfig};
    use agentos_core::ids::{TaskId, TaskRunId};
    use agentos_core::task::{TaskState, TaskStatus};
    use agentos_providers::{
        CompletionRequest, CompletionResponse, MockProvider, ModelProvider, ProviderCapabilities,
        ProviderError, ScriptedTurn, SharedProvider,
    };
    use agentos_runtime::{ProviderFactory, Runtime, RuntimeError, Scheduler, SchedulerOptions};
    use agentos_secrets::InMemorySecretStore;
    use agentos_tools::{ApprovalGate, RecordingGate};
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use super::POLICY;

    /// How long a test waits on something that should happen at once before
    /// calling it hung.
    const HUNG: Duration = Duration::from_secs(10);

    /// A model that takes its turn only when released, and gives it up
    /// promptly when its run is cancelled.
    #[derive(Debug)]
    struct Held {
        entered: Notify,
        release: Notify,
        then: MockProvider,
    }

    #[async_trait::async_trait]
    impl ModelProvider for Held {
        fn id(&self) -> &str {
            self.then.id()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.then.capabilities()
        }

        async fn complete(
            &self,
            request: CompletionRequest,
            cancel: CancellationToken,
        ) -> Result<CompletionResponse, ProviderError> {
            self.entered.notify_one();
            tokio::select! {
                () = cancel.cancelled() => Err(ProviderError::Cancelled),
                () = self.release.notified() => self.then.complete(request, cancel).await,
            }
        }
    }

    #[derive(Debug)]
    struct Models(HashMap<&'static str, SharedProvider>);

    impl ProviderFactory for Models {
        fn build(
            &self,
            agent_name: &str,
            _model: &ModelConfig,
        ) -> Result<SharedProvider, RuntimeError> {
            Ok(self
                .0
                .get(agent_name)
                .cloned()
                .unwrap_or_else(|| Arc::new(MockProvider::answering("Done."))))
        }
    }

    struct Delegating {
        runtime: Runtime,
        supervisor: Agent,
        bystander: Agent,
        worker: Arc<Held>,
        _guard: TempDir,
    }

    impl Delegating {
        async fn new() -> Self {
            let guard = TempDir::new().unwrap();
            let root = std::fs::canonicalize(guard.path()).unwrap();
            let mut runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
                .await
                .unwrap();

            let worker = Arc::new(Held {
                entered: Notify::new(),
                release: Notify::new(),
                then: MockProvider::answering("Done."),
            });
            let supervisor_model = Arc::new(MockProvider::new(vec![
                ScriptedTurn::call(
                    "d1",
                    "agent.delegate",
                    serde_json::json!({
                        "requests": [{ "agent": "worker", "objective": "Do the slow part." }]
                    }),
                ),
                ScriptedTurn::text("Done."),
            ]));
            runtime.set_provider_factory(Arc::new(Models(HashMap::from([
                ("supervisor", supervisor_model as SharedProvider),
                ("worker", worker.clone() as SharedProvider),
            ]))));

            let mut agents = Vec::new();
            for (name, tools, policy) in [
                (
                    "supervisor",
                    vec!["agent.delegate".to_owned()],
                    "default: deny\npermissions:\n  runtime:\n    delegate: [worker]\n",
                ),
                ("worker", vec![], POLICY),
                ("bystander", vec![], POLICY),
            ] {
                let agent = runtime
                    .create_agent(
                        name,
                        "Do the work.",
                        ModelConfig::new("mock", "scripted"),
                        tools,
                    )
                    .await
                    .unwrap();
                runtime
                    .database()
                    .agents()
                    .set_policy(agent.id, policy)
                    .await
                    .unwrap();
                agents.push(agent);
            }
            let [supervisor, _worker, bystander] = <[Agent; 3]>::try_from(agents).unwrap();

            Self {
                runtime,
                supervisor,
                bystander,
                worker,
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

        /// Start the supervisor's task through the scheduler and wait until
        /// the worker it delegated to is live and holding its turn.
        async fn start_supervisor(&self, scheduler: &Scheduler) -> TaskId {
            let task = self
                .runtime
                .create_task(self.supervisor.id, "Supervise.", &[], None)
                .await
                .unwrap();
            assert_eq!(scheduler.tick().await.unwrap().started, vec![task.id]);
            tokio::time::timeout(HUNG, self.worker.entered.notified())
                .await
                .expect("the supervisor never delegated");
            task.id
        }

        /// The run that carried out the supervisor's one delegation.
        async fn child_of(&self, task: TaskId) -> TaskRunId {
            let parent = self
                .runtime
                .database()
                .runs()
                .list_for_task(task)
                .await
                .unwrap()[0]
                .id;
            self.runtime
                .database()
                .delegations()
                .list_for_parent_run(parent)
                .await
                .unwrap()[0]
                .child_run_id
                .expect("the child run was never attached")
        }

        async fn status(&self, id: TaskId) -> TaskStatus {
            self.runtime.task(id).await.unwrap().status
        }
    }

    #[tokio::test]
    async fn a_delegated_child_takes_a_concurrency_slot_of_its_own() {
        // Two slots, not one. With one, the supervisor alone fills the limit
        // whether or not its child is counted, and the test would pass
        // against a scheduler that counts only the tasks it started.
        let harness = Delegating::new().await;
        let scheduler = harness.scheduler(2);
        let supervisor = harness.start_supervisor(&scheduler).await;
        assert_eq!(
            harness.runtime.running_runs().await.len(),
            2,
            "the supervisor and its child are both live"
        );

        let waiting = harness
            .runtime
            .create_task(harness.bystander.id, "Wait your turn.", &[], None)
            .await
            .unwrap();
        assert!(
            scheduler.tick().await.unwrap().started.is_empty(),
            "one task started, two runs live: there is no slot left"
        );
        assert_eq!(harness.status(waiting.id).await, TaskStatus::Pending);

        // Once the tree has finished, the slots come back. Draining waits on
        // the supervisor's run itself, which cannot end before the child it
        // is waiting on, rather than on a guess at how long both take.
        harness.worker.release.notify_one();
        tokio::time::timeout(HUNG, scheduler.drain())
            .await
            .expect("the tree never finished");
        assert_eq!(harness.status(supervisor).await, TaskStatus::Succeeded);
        assert!(harness.runtime.running_runs().await.is_empty());
        assert_eq!(scheduler.tick().await.unwrap().started, vec![waiting.id]);
        scheduler.drain().await;
    }

    #[tokio::test]
    async fn shutdown_drains_a_run_whose_delegate_is_still_working() {
        // The worker never takes its turn unless it is cancelled. A child
        // whose token did not descend from the scheduler's would hold the
        // supervisor, and the shutdown with it, forever.
        let harness = Delegating::new().await;
        let scheduler = harness.scheduler(1);
        let supervisor = harness.start_supervisor(&scheduler).await;
        let child = harness.child_of(supervisor).await;

        tokio::time::timeout(HUNG, scheduler.shutdown())
            .await
            .expect("shutdown did not drain a run with a live child");

        let child_run = harness.runtime.database().runs().get(child).await.unwrap();
        assert_eq!(child_run.state, TaskState::Cancelled);
        assert_eq!(harness.status(supervisor).await, TaskStatus::Cancelled);
        assert_eq!(
            harness.status(child_run.task_id).await,
            TaskStatus::Cancelled
        );
        assert!(harness.runtime.running_runs().await.is_empty());
    }
}
