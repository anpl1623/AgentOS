//! A task's attempts, and starting another.

use agentos_core::ids::TaskId;
use agentos_core::task::{TaskRun, TaskState};
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::tasks::desktop_gate;
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
    let id: TaskId = parse_id("task", &task_id)?;
    let runtime = &state.runtime;

    // Held until the new run is recorded, which `start_task` does before it
    // returns, so a second retry waiting here sees that run and is refused.
    let _retrying = state.retrying.lock().await;

    let task = runtime.task(id).await?;
    let latest = runtime.database().runs().latest_for_task(id).await?;
    may_retry(latest.as_ref())?;

    let gate = desktop_gate(app, state.approvals.clone(), task.objective.clone());
    let (run_id, _handle) = runtime
        .start_task(&task, gate, CancellationToken::new())
        .await?;

    Ok(StartedTask {
        task_id: task.id.to_string(),
        run_id: run_id.to_string(),
    })
}

/// Whether a task whose latest attempt is `latest` may be attempted again.
///
/// Only an attempt that failed or was cancelled. One still going would leave
/// two attempts interleaving their traces under a single objective. One that
/// succeeded would carry out a finished objective a second time, side effects
/// and all; that is a new task, and asking for it should be as deliberate as
/// writing one.
pub(crate) fn may_retry(latest: Option<&TaskRun>) -> Answer<()> {
    let refuse = |why: &str| Err(DesktopError::Rejected(why.to_owned()));
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
