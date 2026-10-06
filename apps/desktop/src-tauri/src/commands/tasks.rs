//! Starting, stopping and tracing tasks.

use std::sync::Arc;

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_tools::ApprovalGate;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::{Answer, DesktopError, parse_id, task_summaries};
use crate::dto::{
    ApprovalView, ExecutionView, RunSummary, StartedTask, StepView, TaskSummary, TraceView,
};
use crate::state::{AppState, ApprovalBridge, DesktopApprovalGate};

/// The approval gate every desktop run is given.
///
/// The one place a desktop gate is built. Two call sites constructing one by
/// hand is how one of them comes to hand a run a different gate.
pub(crate) fn desktop_gate(
    app: AppHandle,
    approvals: ApprovalBridge,
    objective: String,
) -> Arc<dyn ApprovalGate> {
    Arc::new(DesktopApprovalGate::new(app, approvals, objective))
}

/// Recent tasks.
#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Answer<Vec<TaskSummary>> {
    task_summaries(&state.runtime, limit.unwrap_or(50)).await
}

/// Start a task and return immediately.
///
/// The run proceeds in the background; the interface follows it through the
/// activity stream and the trace.
#[tauri::command]
pub async fn start_task(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_id: String,
    objective: String,
) -> Answer<StartedTask> {
    let id: AgentId = parse_id("agent", &agent_id)?;
    if objective.trim().is_empty() {
        return Err(DesktopError::Rejected(
            "an objective is required".to_owned(),
        ));
    }

    let runtime = &state.runtime;
    let task = runtime.create_task(id, &objective).await?;

    let gate = desktop_gate(app, state.approvals.clone(), objective);
    let (run_id, _handle) = runtime
        .start_task(&task, gate, CancellationToken::new())
        .await?;

    Ok(StartedTask {
        task_id: task.id.to_string(),
        run_id: run_id.to_string(),
    })
}

/// Stop a run that is currently executing.
#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>, run_id: String) -> Answer<bool> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    Ok(state.runtime.cancel_run(id).await)
}

/// The full trace of one run.
#[tauri::command]
pub async fn get_trace(state: State<'_, AppState>, run_id: String) -> Answer<TraceView> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    let trace = state.runtime.trace(id).await?;
    Ok(TraceView {
        run: RunSummary::from(&trace.run),
        task_id: trace.run.task_id.to_string(),
        agent_name: trace.agent_name.clone(),
        objective: trace.objective.clone(),
        steps: trace.steps.iter().map(StepView::from).collect(),
        executions: trace.executions.iter().map(ExecutionView::from).collect(),
        approvals: trace
            .approvals
            .iter()
            .map(|request| ApprovalView::new(request, trace.objective.clone()))
            .collect(),
    })
}

/// The trace of a task's most recent run.
#[tauri::command]
pub async fn get_task_trace(state: State<'_, AppState>, task_id: String) -> Answer<TraceView> {
    let id: TaskId = parse_id("task", &task_id)?;
    let run = state
        .runtime
        .database()
        .runs()
        .latest_for_task(id)
        .await?
        .ok_or_else(|| DesktopError::Rejected("this task has never been run".to_owned()))?;
    get_trace(state, run.id.to_string()).await
}
