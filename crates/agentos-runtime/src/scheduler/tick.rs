//! One tick: the three steps the module documentation lists, in that order,
//! and the waiting for the runs it starts.
//!
//! Each step reads what is due and acts on it only if it is still due when
//! the write lands, so that two schedulers reading the same row produce one
//! firing and one run.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use agentos_core::event::AgentEvent;
use agentos_core::ids::{ScheduleId, TaskId};
use agentos_core::task::{Task, TaskStatus};

use super::Scheduler;
use crate::{RunOutcome, Runtime, RuntimeError};

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

impl Scheduler {
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
        // Live runs, delegated ones among them; see the module documentation.
        let capacity = self
            .options
            .max_concurrent_runs
            .saturating_sub(self.runtime.running_runs().await.len());
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
