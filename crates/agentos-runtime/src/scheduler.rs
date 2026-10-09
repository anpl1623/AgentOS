//! The scheduler: what starts work when nobody is at the keyboard.
//!
//! It does three things on a tick, in this order, and nothing else:
//!
//! 1. **Fire due schedules.** Each firing creates a task, so every occurrence
//!    keeps its own runs, traces, approvals and audit trail.
//! 2. **Abandon what can no longer happen.** A task whose dependency failed is
//!    cancelled and recorded, because a task that waits forever looks exactly
//!    like one nobody has got to yet. So is everything downstream of it, in the
//!    same tick: a dead chain resolves at once rather than one layer per tick.
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
//!
//! # What the concurrency limit counts
//!
//! [`SchedulerOptions::max_concurrent_runs`] counts the tasks this scheduler
//! started, not every run that is live. Today those are the same thing, because
//! a run cannot start another run. Wave 6 (delegation) makes them different — a
//! scheduled run that spawns three more would occupy one slot and consume four
//! — and must change this calculation to count live runs instead. It is left as
//! it is deliberately, and this note is the reminder.
//!
//! # One scheduler, and one start per task
//!
//! [`Scheduler::run`] holds the data directory's scheduler lease for as long as
//! it runs, and refuses to start while another process holds it. Underneath
//! that, nothing relies on there being one scheduler: firing a schedule
//! advances it only if it is still due at the occurrence that was read, and
//! starting a task claims it only if it is still runnable. Two schedulers that
//! somehow both read the same due schedule or the same runnable task — one
//! started by a process that predates the lease, or a person pressing retry as
//! the tick lands — produce one firing and one run.
//!
//! # Whether it runs
//!
//! The desktop application keeps a [`SchedulerPreference`] so that a scheduler
//! somebody turned on is running again the next time the application opens.
//! The preference is only a preference; nothing here reads it, and `agentos
//! schedule run` ignores it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use agentos_core::event::AgentEvent;
use agentos_core::ids::{ScheduleId, TaskId};
use agentos_core::task::{Task, TaskStatus};
use agentos_persistence::settings::keys;
use agentos_tools::{ApprovalGate, DenyAllGate};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{RunOutcome, Runtime, RuntimeError};

/// How the scheduler paces itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The shortest gap between ticks an operator may ask for.
///
/// Five seconds. A tick is a handful of queries, so this is not about cost; it
/// is the point below which a scheduler is a busy loop that happens to sleep.
pub const MIN_TICK_SECONDS: u64 = 5;

/// Whether the desktop application should run a scheduler, and how.
///
/// Persisted through the settings repository, so that turning the scheduler on
/// is a decision that outlives the window it was made in. The default is off:
/// unattended work starts only once somebody has asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerPreference {
    /// Whether a scheduler should be running.
    pub enabled: bool,
    /// Seconds between ticks.
    pub tick_seconds: u64,
    /// How many runs it may have in flight at once.
    pub max_concurrent_runs: u32,
}

impl Default for SchedulerPreference {
    fn default() -> Self {
        let options = SchedulerOptions::default();
        Self {
            enabled: false,
            tick_seconds: options.tick.as_secs(),
            max_concurrent_runs: u32::try_from(options.max_concurrent_runs).unwrap_or(1),
        }
    }
}

impl SchedulerPreference {
    /// The options a scheduler started from this preference runs with.
    #[must_use]
    pub fn options(&self) -> SchedulerOptions {
        SchedulerOptions::default()
            .with_tick(Duration::from_secs(self.tick_seconds))
            .with_max_concurrent_runs(usize::try_from(self.max_concurrent_runs).unwrap_or(1))
    }

    /// Refuse pacing a scheduler should not run with.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a tick under [`MIN_TICK_SECONDS`] or a
    /// concurrency of zero, which would be a scheduler that starts nothing.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.tick_seconds < MIN_TICK_SECONDS {
            return Err(RuntimeError::Rejected(format!(
                "a scheduler ticks at most every {MIN_TICK_SECONDS} seconds"
            )));
        }
        if self.max_concurrent_runs == 0 {
            return Err(RuntimeError::Rejected(
                "a scheduler that may run nothing at once would start nothing".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A scheduler starting or stopping, for [`Runtime::record_scheduler_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerTransition {
    /// It took the lease and is about to tick.
    Started {
        /// Started because a saved [`SchedulerPreference`] said so as the
        /// application opened, rather than because somebody asked just now.
        on_launch: bool,
    },
    /// It stopped ticking and its runs have drained.
    Stopped,
}

impl Runtime {
    /// The saved scheduler preference, or the default when none is saved.
    ///
    /// A stored value that does not read is treated as no preference, which is
    /// off: a damaged setting must not be what starts unattended work.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the setting cannot be read.
    pub async fn scheduler_preference(&self) -> Result<SchedulerPreference, RuntimeError> {
        let Some(stored) = self.database.settings().get(keys::SCHEDULER).await? else {
            return Ok(SchedulerPreference::default());
        };
        Ok(serde_json::from_str(&stored).unwrap_or_else(|error| {
            tracing::warn!(%error, "the saved scheduler preference does not read; treating it as off");
            SchedulerPreference::default()
        }))
    }

    /// Save the scheduler preference.
    ///
    /// Saving it starts and stops nothing; the client that owns the scheduler
    /// does that, and records it with [`Runtime::record_scheduler_state`].
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for pacing [`SchedulerPreference::validate`]
    /// refuses, and [`RuntimeError::Database`] if the setting cannot be
    /// written.
    pub async fn set_scheduler_preference(
        &self,
        enabled: bool,
        tick_seconds: u64,
        max_concurrent_runs: u32,
    ) -> Result<SchedulerPreference, RuntimeError> {
        let preference = SchedulerPreference {
            enabled,
            tick_seconds,
            max_concurrent_runs,
        };
        preference.validate()?;
        let stored = serde_json::to_string(&preference)
            .map_err(|error| RuntimeError::Rejected(error.to_string()))?;
        self.database
            .settings()
            .set(keys::SCHEDULER, &stored)
            .await?;
        Ok(preference)
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
    /// The lease, once [`Self::take_lease`] has taken it and until
    /// [`Self::run`] has finished with it.
    lease: std::sync::Mutex<Option<crate::liveness::SchedulerLease>>,
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
            lease: std::sync::Mutex::new(None),
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

    /// The options it runs with.
    #[must_use]
    pub const fn options(&self) -> &SchedulerOptions {
        &self.options
    }

    /// Take this installation's scheduler lease now, ahead of [`Self::run`].
    ///
    /// For a client that records the scheduler starting: taking the lease
    /// first means the record is written only once this scheduler is the one
    /// that will run, never for one about to be refused. [`Self::run`] uses the
    /// lease taken here, and takes its own if none was. Taking it twice is
    /// taking it once.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::SchedulerAlreadyRunning`] if another scheduler holds it.
    pub fn take_lease(&self) -> Result<(), RuntimeError> {
        let mut held = self.lease.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            *held = Some(self.runtime.scheduler_lease()?);
        }
        Ok(())
    }

    /// Tick until cancelled, holding the scheduler lease throughout.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::SchedulerAlreadyRunning`] at once, having done nothing,
    /// if another scheduler holds the lease for this installation.
    ///
    /// Never returns an error for a failed *task* — that is a normal outcome
    /// recorded on the task. Any other [`RuntimeError`] here means the
    /// scheduler itself could not read or write the database, at which point
    /// continuing would be guessing.
    pub async fn run(&self) -> Result<(), RuntimeError> {
        // Held until the runs it started have drained, so a successor cannot
        // start while this one's runs are still finishing.
        self.take_lease()?;
        let _lease = self
            .lease
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
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

    /// Stop ticking, as [`Self::shutdown`] does, but only if none of the runs
    /// this scheduler started is still going; `false`, having changed nothing,
    /// if one is.
    ///
    /// Stopping cancels every run the scheduler started, which is right when
    /// the operator turns unattended work off and wrong when they only want it
    /// paced differently. A client that restarts a scheduler to change its
    /// pacing uses this, so that the change cannot stop work in passing. The
    /// check and the stop are made under the lock a start is made under, so
    /// no run can begin between them.
    pub async fn shutdown_if_idle(&self) -> bool {
        {
            let in_flight = self.in_flight.lock().await;
            if in_flight.values().any(|handle| !handle.is_finished()) {
                return false;
            }
            self.cancel.cancel();
        }
        self.drain().await;
        true
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
            let due_at = schedule.next_run_at;
            let task = Task::new(schedule.agent_id, &schedule.objective).from_schedule(schedule.id);

            // Advance the schedule before the task is ever started. If the
            // process dies mid-run the work is recorded once and re-run zero
            // times; the alternative ordering re-fires on every restart. The
            // advance is conditional on the occurrence just read, so a firing
            // that another scheduler, or a pause, got to first writes nothing.
            schedule.record_firing(agentos_core::now(), task.id);
            if !self
                .runtime
                .database()
                .schedules()
                .fire(&schedule, due_at, &task)
                .await?
            {
                tracing::debug!(schedule = %schedule.id, "already fired, or paused, since it was read");
                continue;
            }

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

    /// Cancel every task that can no longer run, and everything waiting on it.
    ///
    /// The query finds only the first layer — tasks with a failed or cancelled
    /// dependency — so each one found is the root of a walk down its dependents.
    /// Without the walk, a dependent would become visible only on the next tick,
    /// once its own dependency had been cancelled, and a three-deep chain would
    /// spend two ticks looking merely unstarted.
    async fn abandon_unreachable(&self) -> Result<Vec<TaskId>, RuntimeError> {
        let stuck = self
            .runtime
            .database()
            .tasks()
            .list_unreachable(self.options.batch)
            .await?;

        // The cap is a fairness bound, not a correctness one: it stops one tick
        // on a vast dead graph from starving the start step behind it. Whatever
        // it leaves is found next tick, because every task cancelled here is
        // itself now a cancelled dependency for the query above.
        let budget = usize::try_from(self.options.batch).unwrap_or(0);

        // A task can be met more than once in a pass: a join node down several
        // branches, or a task the query listed that an earlier walk has since
        // reached — the list was read before any walk ran and is stale after
        // one. Either way it is abandoned, and recorded, once.
        let mut abandoned = Vec::new();
        let mut seen = HashSet::new();
        for task in stuck {
            if abandoned.len() >= budget {
                break;
            }
            if seen.contains(&task.id) {
                continue;
            }

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

            self.abandon(&task, blocked_by, reason).await?;
            seen.insert(task.id);
            abandoned.push(task.id);

            // Each dependent names the task it was waiting on, not the root
            // failure: the chain reads by following the events, and naming the
            // root would have every event down the chain claim one culprit.
            let mut frontier = VecDeque::from([task.id]);
            while let Some(cancelled) = frontier.pop_front() {
                let dependents = self
                    .runtime
                    .database()
                    .dependencies()
                    .dependents_of(cancelled)
                    .await?;
                for dependent in dependents {
                    if abandoned.len() >= budget {
                        return Ok(abandoned);
                    }
                    if seen.contains(&dependent) {
                        continue;
                    }
                    let dependent = self.runtime.database().tasks().get(dependent).await?;
                    // The same statuses the query considers: a dependent that
                    // has somehow already run, or already ended, is history and
                    // is left alone.
                    if !matches!(dependent.status, TaskStatus::Pending | TaskStatus::Blocked) {
                        continue;
                    }

                    self.abandon(&dependent, cancelled, TaskStatus::Cancelled)
                        .await?;
                    seen.insert(dependent.id);
                    abandoned.push(dependent.id);
                    frontier.push_back(dependent.id);
                }
            }
        }
        Ok(abandoned)
    }

    /// Cancel one task and record why.
    async fn abandon(
        &self,
        task: &Task,
        blocked_by: TaskId,
        reason: TaskStatus,
    ) -> Result<(), RuntimeError> {
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
        Ok(())
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
        for task in runnable {
            if started.len() >= capacity {
                break;
            }
            // Held from before the start until its handle is in the map, and
            // the start skipped once the scheduler has been told to stop. A
            // stop then either finds the run and drains it, or comes first and
            // no run begins: never a run started under a cancelled token,
            // which would be recorded as stopped before it had done anything.
            let mut in_flight = self.in_flight.lock().await;
            if self.cancel.is_cancelled() {
                break;
            }
            let (id, read) = (task.id, task.status);
            let begun = self
                .runtime
                .start_task(
                    &task,
                    Arc::clone(&self.approvals),
                    self.cancel.child_token(),
                )
                .await;
            let run = match begun {
                Ok((_, run)) => run,
                // Somebody else is running it. Not a failure of the task, and
                // not this scheduler's to report; the slot goes to the next.
                Err(RuntimeError::TaskAlreadyClaimed(_)) => continue,
                Err(error) => {
                    drop(in_flight);
                    self.fail_unstartable(id, read, &error).await;
                    continue;
                }
            };

            let runtime = self.runtime.clone();
            let handle = tokio::spawn(async move { settle(&runtime, id, run).await });
            in_flight.insert(id, handle);
            started.push(id);
        }
        Ok(started)
    }

    /// Fail a task whose run could not be assembled — a disabled or missing
    /// agent, an unbuildable provider — so the next tick does not try again.
    ///
    /// Claimed first, against the status the tick read, so that a task
    /// another scheduler has meanwhile started, or the operator has meanwhile
    /// cancelled, is not marked failed underneath them.
    async fn fail_unstartable(&self, id: TaskId, read: TaskStatus, error: &RuntimeError) {
        tracing::error!(task = %id, %error, "a scheduled run could not be started");
        let tasks = self.runtime.database().tasks();
        if matches!(tasks.claim(id, read).await, Ok(true)) {
            let _ = tasks.set_status(id, TaskStatus::Failed).await;
        }
    }
}

/// Wait for a scheduled run to end.
///
/// A task that merely fails is reported through its outcome. An error here is
/// the runtime failing mid-run, and the task must not be left as `running`.
async fn settle(
    runtime: &Runtime,
    id: TaskId,
    run: tokio::task::JoinHandle<Result<RunOutcome, RuntimeError>>,
) {
    let failed = match run.await {
        Ok(Ok(_)) => return,
        Ok(Err(error)) => error.to_string(),
        Err(error) => error.to_string(),
    };
    tracing::error!(task = %id, error = %failed, "a scheduled run stopped without finishing");
    let _ = runtime
        .database()
        .tasks()
        .set_status(id, TaskStatus::Failed)
        .await;
}
