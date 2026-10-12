//! Queued work: tasks that wait for other tasks, or for a moment.
//!
//! `start_task` creates one unconditioned task and runs it at once. This is the
//! other path: the task is stored and left for the scheduler, so a fan-in, such
//! as three gatherers and a summariser waiting on all of them, can be
//! described. Nothing here starts a run.

use std::collections::HashMap;

use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::TaskStatus;
use agentos_runtime::Runtime;
use tauri::State;

use super::schedules::parse_time;
use super::{Answer, DesktopError, agent_names, parse_id};
use crate::dto::{CreateTaskInput, TaskNodeView, task_nodes};
use crate::state::AppState;

/// Queue a task: after other tasks succeed, not before a moment, or both.
///
/// The task is stored and not run. With dependencies it is stored `Blocked`;
/// each edge is checked for a cycle before it is written, and a cycle is
/// refused with the path it would have closed. It starts on its own only while
/// a scheduler is running.
#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    input: CreateTaskInput,
) -> Answer<TaskNodeView> {
    queue(&state.runtime, input).await
}

/// Make a queued task wait for another.
///
/// A cycle is refused with the path it would have closed, and a missing task
/// is named; both are the runtime's own words.
#[tauri::command]
pub async fn add_task_dependency(
    state: State<'_, AppState>,
    task_id: String,
    depends_on: String,
) -> Answer<()> {
    let task: TaskId = parse_id("task", &task_id)?;
    let depends_on: TaskId = parse_id("task", &depends_on)?;
    state.runtime.add_task_dependency(task, depends_on).await?;
    Ok(())
}

/// Recent tasks as a graph, newest first.
#[tauri::command]
pub async fn task_graph(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Answer<Vec<TaskNodeView>> {
    graph(&state.runtime, limit.unwrap_or(100)).await
}

/// Queue a task from the interface's input.
pub(crate) async fn queue(runtime: &Runtime, input: CreateTaskInput) -> Answer<TaskNodeView> {
    let agent_id: AgentId = parse_id("agent", &input.agent_id)?;
    let depends_on = input
        .depends_on
        .iter()
        .map(|id| parse_id::<TaskId>("task", id))
        .collect::<Answer<Vec<_>>>()?;
    let scheduled_for = input
        .scheduled_for
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(parse_time)
        .transpose()?;

    let task = runtime
        .create_task(agent_id, &input.objective, &depends_on, scheduled_for)
        .await?;

    // Read back through the graph, so the answer carries the edges as stored
    // and the same runnable verdict a later `task_graph` would give.
    let nodes = nodes_for(runtime, std::slice::from_ref(&task)).await?;
    nodes
        .into_iter()
        .next()
        .ok_or_else(|| DesktopError::Rejected("the queued task could not be read back".to_owned()))
}

/// The most recent tasks, laid out as graph nodes.
pub(crate) async fn graph(runtime: &Runtime, limit: i64) -> Answer<Vec<TaskNodeView>> {
    let tasks = runtime.database().tasks().list(limit).await?;
    nodes_for(runtime, &tasks).await
}

/// Lay out the given tasks, reading every edge once.
///
/// The edge list is read in one query and indexed in memory, rather than two
/// queries per node. A dependency outside `tasks` is read individually, which
/// happens only at the edge of the window.
async fn nodes_for(
    runtime: &Runtime,
    tasks: &[agentos_core::task::Task],
) -> Answer<Vec<TaskNodeView>> {
    let edges = runtime.database().dependencies().all().await?;
    let mut statuses: HashMap<TaskId, TaskStatus> =
        tasks.iter().map(|task| (task.id, task.status)).collect();

    let listed: std::collections::HashSet<TaskId> = tasks.iter().map(|task| task.id).collect();
    for (task, depends_on) in &edges {
        if listed.contains(task)
            && !statuses.contains_key(depends_on)
            && let Some(found) = runtime.database().tasks().find(*depends_on).await?
        {
            statuses.insert(found.id, found.status);
        }
    }

    let names = agent_names(runtime).await?;
    Ok(task_nodes(
        tasks,
        &edges,
        &statuses,
        &names,
        agentos_core::now(),
    ))
}
