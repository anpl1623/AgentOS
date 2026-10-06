//! The scheduler switch, and what it would find.

use agentos_runtime::{Runtime, SchedulerPreference};
use tauri::State;

use super::Answer;
use crate::dto::SchedulerView;
use crate::state::{AppState, SchedulerSupervisor};

/// How many queued tasks the counts consider at most.
///
/// A count is a reason to look, not an inventory; past this the number shown
/// is a floor.
const COUNTED: i64 = 1_000;

/// Whether the scheduler is running, and the work it would find.
#[tauri::command]
pub async fn scheduler_status(state: State<'_, AppState>) -> Answer<SchedulerView> {
    scheduler_view(&state.runtime, &state.scheduler).await
}

/// Turn the scheduler on or off with the given pacing, and remember the
/// choice for the next launch.
///
/// Pacing the runtime would refuse to save, a tick under five seconds or no
/// room for a run, is refused before anything changes. So is new pacing for a
/// scheduler that has runs in progress, since applying it means a restart and
/// a restart would stop them; the same pacing as before changes nothing and
/// stops nothing. Otherwise the preference is saved before the scheduler is
/// started, so a start that fails because another process holds the lease
/// still leaves the operator's intent recorded; the failure is reported, and
/// the next launch tries again.
#[tauri::command]
pub async fn set_scheduler_running(
    state: State<'_, AppState>,
    enabled: bool,
    tick_seconds: u64,
    max_concurrent_runs: u32,
) -> Answer<SchedulerView> {
    switch(
        &state.runtime,
        &state.scheduler,
        enabled,
        tick_seconds,
        max_concurrent_runs,
    )
    .await
}

/// Save the preference, then start or stop the scheduler to match it.
pub(crate) async fn switch(
    runtime: &Runtime,
    supervisor: &SchedulerSupervisor,
    enabled: bool,
    tick_seconds: u64,
    max_concurrent_runs: u32,
) -> Answer<SchedulerView> {
    let preference = SchedulerPreference {
        enabled,
        tick_seconds,
        max_concurrent_runs,
    };
    preference.validate()?;
    let options = preference.options();

    // Stopping before taking the new pacing, so turning the scheduler off
    // never restarts it with new options on the way. Turning it on takes the
    // pacing first, so a refusal to disturb runs in progress leaves both the
    // scheduler and the saved preference as they were.
    if enabled {
        supervisor.repace(options).await?;
    } else {
        supervisor.stop().await;
        supervisor.repace(options).await?;
    }
    runtime
        .set_scheduler_preference(enabled, tick_seconds, max_concurrent_runs)
        .await?;
    if enabled {
        supervisor.start(false).await?;
    }
    scheduler_view(runtime, supervisor).await
}

/// The scheduler's state and the work waiting for it.
pub(crate) async fn scheduler_view(
    runtime: &Runtime,
    supervisor: &SchedulerSupervisor,
) -> Answer<SchedulerView> {
    let status = supervisor.status().await;
    let now = agentos_core::now();

    let schedules = runtime.schedules().await?;
    let active: Vec<_> = schedules
        .iter()
        .filter(|schedule| schedule.status.is_active())
        .collect();
    let next_fire_at = active
        .iter()
        .filter_map(|schedule| schedule.next_run_at)
        .min();
    let overdue = active
        .iter()
        .filter(|schedule| schedule.is_due(now))
        .count();

    let tasks = runtime.database().tasks();
    let runnable = tasks.list_runnable(COUNTED).await?.len();
    let unreachable = tasks.list_unreachable(COUNTED).await?.len();

    Ok(SchedulerView {
        running: status.running,
        tick_seconds: status.options.tick.as_secs(),
        max_concurrent_runs: count(status.options.max_concurrent_runs),
        started_at: status
            .started_at
            .as_ref()
            .map(agentos_core::format_timestamp),
        error: status.error,
        active_schedules: count(active.len()),
        next_fire_at: next_fire_at.as_ref().map(agentos_core::format_timestamp),
        overdue: count(overdue),
        runnable_tasks: count(runnable),
        unreachable_tasks: count(unreachable),
    })
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
