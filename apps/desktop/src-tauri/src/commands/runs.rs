//! A task's attempts, and starting another.

use std::sync::Arc;

use agentos_core::ids::TaskId;
use agentos_core::task::{Task, TaskRun, TaskState};
use agentos_runtime::{Runtime, RuntimeError};
use agentos_tools::ApprovalGate;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

use super::tasks::{begin, desktop_gate};
use super::{Answer, DesktopError, parse_id};
use crate::dto::{RunSummary, StartedTask};
use crate::state::AppState;

/// Every attempt at a task, oldest first.
#[tauri::command]
pub async fn list_runs(state: State<'_, AppState>, task_id: String) -> Answer<Vec<RunSummary>> {
    let id: TaskId = parse_id("task", &task_id)?;
    Ok(state
        .runtime
        .database()
        .runs()
        .list_for_task(id)
        .await?
        .iter()
        .map(RunSummary::from)
        .collect())
}

/// Start another attempt at a task whose latest attempt failed or was
/// cancelled.
///
/// The attempt number is assigned by the runtime when the run is prepared, as
/// it is for every run; nothing here computes one.
#[tauri::command]
pub async fn retry_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Answer<StartedTask> {
    let gate = desktop_gate(app, state.approvals.clone(), state.runtime.clone());
    retry(&state.runtime, &state.retrying, &task_id, gate).await
}

/// Start another attempt at a task, behind `gate`.
pub(crate) async fn retry(
    runtime: &Runtime,
    retrying: &Mutex<()>,
    task_id: &str,
    gate: Arc<dyn ApprovalGate>,
) -> Answer<StartedTask> {
    let id: TaskId = parse_id("task", task_id)?;

    // Held until the new run is recorded, which `start_task` does before it
    // returns, so a second retry waiting here sees that run and is refused.
    let _retrying = retrying.lock().await;

    let task = runtime.task(id).await?;
    let latest = runtime.database().runs().latest_for_task(id).await?;
    may_retry(&task, latest.as_ref())?;

    begin(runtime, &task, gate).await
}

/// Whether `task`, whose latest attempt is `latest`, may be attempted again.
///
/// Only an attempt that failed or was cancelled. One still going would leave
/// two attempts interleaving their traces under a single objective. One that
/// succeeded would carry out a finished objective a second time, side effects
/// and all; that is a new task, and asking for it should be as deliberate as
/// writing one.
///
/// Never a task another agent's run delegated. It is carried out only as part
/// of that run, under that run's policy as well as its own; the runtime
/// refuses to start it alone, and saying so here gives the reason before
/// anything is attempted.
pub(crate) fn may_retry(task: &Task, latest: Option<&TaskRun>) -> Answer<()> {
    let refuse = |why: &str| Err(DesktopError::Rejected(why.to_owned()));
    if task.parent_task_id.is_some() {
        return refuse(agentos_runtime::DELEGATED_TASK);
    }
    match latest.map(|run| run.state) {
        None => refuse("this task has never been run, so there is nothing to retry"),
        Some(TaskState::Failed | TaskState::Cancelled) => Ok(()),
        Some(TaskState::Completed) => refuse(
            "this task succeeded; running it again would repeat what it did, so start a new task \
             instead",
        ),
        Some(_) => refuse("this task is still running"),
    }
}

/// The refusal shown when another client started a task first.
pub const STARTED_ELSEWHERE: &str = "this task was started elsewhere first, most likely by a \
     scheduler in this window or a terminal, so this attempt was not made. That run is in the \
     task's trace. A scheduler runs unattended: anything in its run that would have asked you \
     is refused instead.";

/// Turn a lost claim into the words an operator is shown.
///
/// Between reading a task as startable and claiming it, a scheduler can claim
/// it instead. That is not a failure of the task, and a message that read as
/// one would invite another retry of something already running. Every other
/// failure passes through as the runtime wrote it.
#[must_use]
pub fn started_elsewhere(error: RuntimeError) -> DesktopError {
    match error {
        RuntimeError::TaskAlreadyClaimed(_) => DesktopError::Rejected(STARTED_ELSEWHERE.to_owned()),
        other => DesktopError::Runtime(other),
    }
}
