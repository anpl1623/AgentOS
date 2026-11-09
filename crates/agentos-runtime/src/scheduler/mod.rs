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
//!    dependencies have all succeeded, up to a limit on live runs.
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
//! [`SchedulerOptions::max_concurrent_runs`] limits live runs, not the tasks
//! this scheduler started. A run can delegate, and a scheduled run that asks
//! three other agents to work is four runs calling a model and touching the
//! world at once; counting it as one slot would let a limit of one mean four.
//! So the scheduler starts nothing while [`Runtime::running_runs`], which
//! counts every live run including delegated ones, is at the limit. The same
//! count includes runs other clients of this runtime started — a person's run
//! in the desktop application takes a slot as well — which is the point: the
//! limit is on how much is happening, not on who started it.
//!
//! The scheduler's own handles are kept for reaping and draining. They track
//! what it must wait for, not how much is running.
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

mod pacing;
mod tick;

use std::collections::HashMap;
use std::sync::{Arc, PoisonError};

use agentos_core::ids::TaskId;
use agentos_tools::{ApprovalGate, DenyAllGate};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{Runtime, RuntimeError};

pub use pacing::{
    DEFAULT_TICK, MIN_TICK_SECONDS, SchedulerOptions, SchedulerPreference, SchedulerTransition,
};
pub use tick::TickReport;

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
}
