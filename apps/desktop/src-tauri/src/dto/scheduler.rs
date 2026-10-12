//! The in-process scheduler, and what stands between the window and closing.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Whether the scheduler is running, and the work it would find if it were.
///
/// The counts are read from the database on every request, not from the
/// scheduler, so they are true whether it is running or not: a stopped
/// scheduler with overdue schedules is exactly the state this view exists to
/// make visible.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SchedulerView {
    /// Whether a scheduler is ticking in this process.
    pub running: bool,
    /// Seconds between ticks, running or not: what a start would use.
    #[ts(type = "number")]
    pub tick_seconds: u64,
    /// How many runs it may have in flight at once.
    #[ts(type = "number")]
    pub max_concurrent_runs: u32,
    /// When the running scheduler started.
    pub started_at: Option<String>,
    /// Why the scheduler stopped without being asked to, if it did.
    ///
    /// A scheduler that could not take its lease, or lost its database, ends on
    /// its own; without this the switch would simply read "off" again with no
    /// account of why.
    pub error: Option<String>,
    /// Schedules that are active.
    #[ts(type = "number")]
    pub active_schedules: u32,
    /// The soonest moment an active schedule fires.
    pub next_fire_at: Option<String>,
    /// Active schedules whose moment has already passed.
    #[ts(type = "number")]
    pub overdue: u32,
    /// Queued tasks a tick would start now.
    #[ts(type = "number")]
    pub runnable_tasks: u32,
    /// Queued tasks that can never start, because something they wait for
    /// failed or was cancelled.
    #[ts(type = "number")]
    pub unreachable_tasks: u32,
}

/// What is still going on when the window is asked to close.
///
/// Sent with the close-requested event, so the question the shell asks names
/// what closing will stop rather than asking in general terms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CloseGuard {
    /// Runs this process is driving.
    #[ts(type = "number")]
    pub live_runs: u32,
    /// Whether the scheduler is running.
    pub scheduler_running: bool,
    /// Approval requests in this process still waiting on a person.
    #[ts(type = "number")]
    pub pending_approvals: u32,
}

impl CloseGuard {
    /// Whether closing would stop nothing, so the window may close unasked.
    #[must_use]
    pub const fn is_quiet(&self) -> bool {
        self.live_runs == 0 && !self.scheduler_running && self.pending_approvals == 0
    }
}
