//! The scheduler: what starts work when nobody is at the keyboard.
//!
//! It does three things on a tick, in this order, and nothing else:
//!
//! 1. **Fire due schedules.** Each firing creates a task, so every occurrence
//!    keeps its own runs, traces, approvals and audit trail.
//! 2. **Abandon what can no longer happen.** A task whose dependency failed is
//!    cancelled and recorded, because a task that waits forever looks exactly
//!    like one nobody has got to yet.
//! 3. **Start what is runnable.** Tasks whose clock has arrived and whose
//!    dependencies have all succeeded, up to a concurrency limit.
//!
//! # Nobody is watching
//!
//! This is the point that matters. A scheduled run happens unattended, so it is
//! driven behind [`DenyAllGate`]: everything the policy permits outright
//! proceeds, and everything that would have asked a human is refused with a note
//! the model can read and re-plan around. There is deliberately no configuration
//! that makes a schedule able to approve on your behalf. An agent that needs a
//! person to say yes needs a person, and a scheduler that could say yes for them
//! would make the approval gate decorative.
//!
//! [`Scheduler::with_approvals`] exists for tests and for a client that genuinely
//! does have somebody attached. It is not a way around the paragraph above.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use agentos_core::event::AgentEvent;
use agentos_core::ids::{ScheduleId, TaskId};
use agentos_core::task::{Task, TaskStatus};
use agentos_tools::{ApprovalGate, DenyAllGate};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{Runtime, RuntimeError};

/// How the scheduler paces itself.
#[derive(Debug, Clone, Copy)]
pub struct SchedulerOptions {
    /// How long to wait between ticks.
    pub tick: Duration,
    /// How many runs may be in flight at once.
    pub max_concurrent_runs: usize,
    /// How many due schedules, and how many runnable tasks, one tick considers.
    pub batch: i64,
}

/// Default gap between ticks.
///
/// Thirty seconds. The finest cadence a schedule can express is a minute, so
/// anything faster is a busy loop, and anything much slower would make a
/// once-a-minute schedule visibly late.
pub const DEFAULT_TICK: Duration = Duration::from_secs(30);

impl Default for SchedulerOptions {
    fn default() -> Self {
        Self {
            tick: DEFAULT_TICK,
            // One. Unattended runs cost money and touch the world, and an
            // operator who wants more of that at once should have to say so.
            max_concurrent_runs: 1,
            batch: 32,
        }
    }
}

impl SchedulerOptions {
    /// Set the gap between ticks.
    #[must_use]
    pub const fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Set how many runs may be in flight at once.
    #[must_use]
    pub const fn with_max_concurrent_runs(mut self, max: usize) -> Self {
        self.max_concurrent_runs = max;
        self
    }
}

/// What one tick did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    /// Schedules that fired, and the task each produced.
    pub fired: Vec<(ScheduleId, TaskId)>,
    /// Tasks that were started.
    pub started: Vec<TaskId>,
    /// Tasks abandoned because a dependency will not succeed.
    pub abandoned: Vec<TaskId>,
    /// Runs that finished since the previous tick.
    pub finished: Vec<TaskId>,
}

impl TickReport {
    /// Whether the tick did anything at all.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.fired.is_empty()
            && self.started.is_empty()
            && self.abandoned.is_empty()
            && self.finished.is_empty()
    }
}

/// Starts scheduled and unblocked work.
#[derive(Debug)]
pub struct Scheduler {
    runtime: Runtime,
    options: SchedulerOptions,
    approvals: Arc<dyn ApprovalGate>,
    cancel: CancellationToken,
    in_flight: Mutex<HashMap<TaskId, tokio::task::JoinHandle<()>>>,
}

impl Scheduler {
    /// Build a scheduler that refuses everything needing a human.
    #[must_use]
    pub fn new(runtime: Runtime, options: SchedulerOptions) -> Self {
        Self {
            runtime,
            options,
            approvals: Arc::new(DenyAllGate),
            cancel: CancellationToken::new(),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// Use a different approval gate.
    ///
    /// For tests, and for a client that genuinely has somebody attached. Read
    /// the module documentation before reaching for this: a gate that approves
    /// on nobody's behalf makes the approval gate decorative.
    #[must_use]
    pub fn with_approvals(mut self, approvals: Arc<dyn ApprovalGate>) -> Self {
        self.approvals = approvals;
        self
    }

    /// The token that stops every run this scheduler starts.
    #[must_use]
    pub fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Tick until cancelled.
    ///
    /// # Errors
    ///
    /// Never returns an error for a failed *task* — that is a normal outcome
    /// recorded on the task. A [`RuntimeError`] here means the scheduler itself
    /// could not read or write the database, at which point continuing would be
    /// guessing.
    pub async fn run(&self) -> Result<(), RuntimeError> {
        tracing::info!(
            tick_secs = self.options.tick.as_secs(),
            max_concurrent_runs = self.options.max_concurrent_runs,
            "scheduler started"
        );

        loop {
            let report = self.tick().await?;
            if !report.is_idle() {
                tracing::info!(
                    fired = report.fired.len(),
                    started = report.started.len(),
                    abandoned = report.abandoned.len(),
                    finished = report.finished.len(),
                    "scheduler tick"
                );
            }

            tokio::select! {
                () = self.cancel.cancelled() => break,
                () = tokio::time::sleep(self.options.tick) => {}
            }
        }

        // Runs already in flight are given the same cancellation the operator's
        // stop button uses, and then waited for, so shutting down does not leave
        // a half-finished run marked as running forever.
        self.drain().await;
        tracing::info!("scheduler stopped");
        Ok(())
    }

    /// Do one pass. Exposed so the behaviour is testable without a clock.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the scheduler cannot read or write.
    pub async fn tick(&self) -> Result<TickReport, RuntimeError> {
        let mut report = TickReport {
            finished: self.reap().await,
            ..TickReport::default()
        };
        report.fired = self.fire_due_schedules().await?;
        report.abandoned = self.abandon_unreachable().await?;
        report.started = self.start_runnable().await?;
        Ok(report)
    }

    /// Stop ticking, and wait for what is already running.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.drain().await;
    }

    /// Wait for every in-flight run to finish.
    pub async fn drain(&self) {
        let handles: Vec<_> = {
            let mut in_flight = self.in_flight.lock().await;
            in_flight.drain().map(|(_, handle)| handle).collect()
        };
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Forget handles whose runs have ended.
    async fn reap(&self) -> Vec<TaskId> {
        let mut in_flight = self.in_flight.lock().await;
        let finished: Vec<TaskId> = in_flight
            .iter()
            .filter(|(_, handle)| handle.is_finished())
            .map(|(id, _)| *id)
            .collect();
        for id in &finished {
            in_flight.remove(id);
        }
        finished
    }

    async fn fire_due_schedules(&self) -> Result<Vec<(ScheduleId, TaskId)>, RuntimeError> {
        let due = self
            .runtime
            .database()
            .schedules()
            .list_due(self.options.batch)
            .await?;

        let mut fired = Vec::new();
        for mut schedule in due {
            let task = Task::new(schedule.agent_id, &schedule.objective).from_schedule(schedule.id);
            self.runtime.database().tasks().insert(&task).await?;

            // Advance the schedule before the task is ever started. If the
            // process dies mid-run the work is recorded once and re-run zero
            // times; the alternative ordering re-fires on every restart.
            schedule.record_firing(agentos_core::now(), task.id);
            self.runtime
                .database()
                .schedules()
                .update(&schedule)
                .await?;

            let _ = self
                .runtime
                .audit()
                .record(
                    agentos_core::Event::new(AgentEvent::ScheduleFired {
                        schedule_id: schedule.id,
                        name: schedule.name.clone(),
                        task_id: task.id,
                    })
                    .for_agent(schedule.agent_id)
                    .for_task(task.id),
                )
                .await;

            fired.push((schedule.id, task.id));
        }
        Ok(fired)
    }

    async fn abandon_unreachable(&self) -> Result<Vec<TaskId>, RuntimeError> {
        let stuck = self
            .runtime
            .database()
            .tasks()
            .list_unreachable(self.options.batch)
            .await?;

        let mut abandoned = Vec::new();
        for task in stuck {
            let blockers = self
                .runtime
                .database()
                .dependencies()
                .dependencies_of(task.id)
                .await?;

            // Name the one that actually ended it, not the whole list.
            let mut culprit = None;
            for blocker in blockers {
                let dependency = self.runtime.database().tasks().get(blocker).await?;
                if matches!(
                    dependency.status,
                    TaskStatus::Failed | TaskStatus::Cancelled
                ) {
                    culprit = Some((dependency.id, dependency.status));
                    break;
                }
            }
            let Some((blocked_by, reason)) = culprit else {
                continue;
            };

            self.runtime
                .database()
                .tasks()
                .set_status(task.id, TaskStatus::Cancelled)
                .await?;

            let _ = self
                .runtime
                .audit()
                .record(
                    agentos_core::Event::new(AgentEvent::TaskAbandoned {
                        task_id: task.id,
                        blocked_by,
                        reason: reason.as_str().to_owned(),
                    })
                    .for_agent(task.agent_id)
                    .for_task(task.id),
                )
                .await;

            abandoned.push(task.id);
        }
        Ok(abandoned)
    }

    async fn start_runnable(&self) -> Result<Vec<TaskId>, RuntimeError> {
        let capacity = self
            .options
            .max_concurrent_runs
            .saturating_sub(self.in_flight.lock().await.len());
        if capacity == 0 {
            return Ok(Vec::new());
        }

        let runnable = self
            .runtime
            .database()
            .tasks()
            .list_runnable(self.options.batch)
            .await?;

        let mut started = Vec::new();
        for task in runnable.into_iter().take(capacity) {
            let runtime = self.runtime.clone();
            let approvals = Arc::clone(&self.approvals);
            let cancel = self.cancel.child_token();
            let id = task.id;

            let handle = tokio::spawn(async move {
                if let Err(error) = runtime.run_task(&task, approvals, cancel).await {
                    // A task that merely fails is reported through its outcome
                    // and is not this branch. Reaching here means the run could
                    // not be assembled — a missing agent, an unbuildable
                    // provider — and the task must not be left as `running`.
                    tracing::error!(task = %id, %error, "a scheduled run could not be started");
                    let _ = runtime
                        .database()
                        .tasks()
                        .set_status(id, TaskStatus::Failed)
                        .await;
                }
            });

            self.in_flight.lock().await.insert(id, handle);
            started.push(id);
        }
        Ok(started)
    }
}
