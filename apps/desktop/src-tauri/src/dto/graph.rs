//! Queued work: tasks, what each waits for, and whether it ever can start.

use std::collections::HashMap;

use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::{Task, TaskStatus};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{at, maybe_at};

/// One task in the dependency graph.
///
/// Its own type rather than more fields on [`super::TaskSummary`]: the graph
/// is a different question ("what is this waiting for?") from the task list's
/// ("how did its latest attempt go?"), and answering both everywhere would make
/// every list pay for the graph.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TaskNodeView {
    /// Identity.
    pub id: String,
    /// What was asked for.
    pub objective: String,
    /// Aggregate status.
    pub status: String,
    /// The agent responsible.
    pub agent_id: String,
    /// Its name, or `(deleted)`.
    pub agent_name: String,
    /// Not before this moment, if the task was queued for one.
    pub scheduled_for: Option<String>,
    /// The schedule whose firing created it, if any.
    pub schedule_id: Option<String>,
    /// When it was created.
    pub created_at: String,
    /// The tasks it waits for.
    pub blocked_by: Vec<String>,
    /// The tasks that wait for it.
    pub blocks: Vec<String>,
    /// Whether a scheduler tick would start it now.
    pub runnable: bool,
    /// Whether it can never start, because something it waits for failed or
    /// was cancelled.
    pub unreachable: bool,
    /// The dependency that failed or was cancelled, when it is unreachable.
    ///
    /// The same culprit the scheduler names when it abandons the task, so the
    /// two describe one dead branch the same way. A task that waits forever
    /// must never look like one nobody has got to yet.
    pub blocked_by_failure: Option<String>,
}

/// What the interface sends to queue a task.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CreateTaskInput {
    /// The agent that will do it.
    pub agent_id: String,
    /// What to do.
    pub objective: String,
    /// Tasks that must succeed first.
    pub depends_on: Vec<String>,
    /// Not before this moment, as RFC 3339.
    pub scheduled_for: Option<String>,
}

/// Whether a task in this status is still waiting to start.
const fn waiting(status: TaskStatus) -> bool {
    matches!(status, TaskStatus::Pending | TaskStatus::Blocked)
}

/// Lay out tasks as graph nodes.
///
/// `edges` is every `(task, depends_on)` pair, read once; `statuses` holds the
/// status of every task an edge names, including tasks outside `tasks`, so a
/// node near the edge of the window still knows whether what it waits for has
/// failed. A dependency with no known status is treated as not yet succeeded:
/// the node is neither runnable nor unreachable, which is the claim that can
/// be made without the row.
///
/// Runnability and unreachability are the conditions
/// `TaskRepository::list_runnable` and `list_unreachable` evaluate in SQL, so a
/// node marked runnable is one the scheduler would start.
#[must_use]
pub fn task_nodes(
    tasks: &[Task],
    edges: &[(TaskId, TaskId)],
    statuses: &HashMap<TaskId, TaskStatus>,
    names: &HashMap<AgentId, String>,
    now: agentos_core::Timestamp,
) -> Vec<TaskNodeView> {
    let mut blocked_by: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    let mut blocks: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    for (task, depends_on) in edges {
        blocked_by.entry(*task).or_default().push(*depends_on);
        blocks.entry(*depends_on).or_default().push(*task);
    }

    tasks
        .iter()
        .map(|task| {
            let waits_for = blocked_by.get(&task.id).map_or(&[][..], Vec::as_slice);
            let failure = waits_for.iter().copied().find(|dependency| {
                matches!(
                    statuses.get(dependency),
                    Some(TaskStatus::Failed | TaskStatus::Cancelled)
                )
            });
            let satisfied = waits_for
                .iter()
                .all(|dependency| statuses.get(dependency) == Some(&TaskStatus::Succeeded));
            let due = task.scheduled_for.is_none_or(|when| when <= now);
            let still_waiting = waiting(task.status);

            TaskNodeView {
                id: task.id.to_string(),
                objective: task.objective.clone(),
                status: task.status.as_str().to_owned(),
                agent_id: task.agent_id.to_string(),
                agent_name: names
                    .get(&task.agent_id)
                    .map_or("(deleted)", String::as_str)
                    .to_owned(),
                scheduled_for: maybe_at(task.scheduled_for.as_ref()),
                schedule_id: task.schedule_id.map(|id| id.to_string()),
                created_at: at(&task.created_at),
                blocked_by: waits_for.iter().map(ToString::to_string).collect(),
                blocks: blocks
                    .get(&task.id)
                    .map(|ids| ids.iter().map(ToString::to_string).collect())
                    .unwrap_or_default(),
                runnable: still_waiting && due && satisfied,
                unreachable: still_waiting && failure.is_some(),
                blocked_by_failure: failure.filter(|_| still_waiting).map(|id| id.to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(agent: AgentId, objective: &str, status: TaskStatus) -> Task {
        let mut task = Task::new(agent, objective);
        task.status = status;
        task
    }

    #[test]
    fn a_fan_in_with_a_failed_branch_names_the_branch_and_never_reads_as_runnable() {
        let agent = AgentId::new();
        let gather_a = task(agent, "gather a", TaskStatus::Succeeded);
        let gather_b = task(agent, "gather b", TaskStatus::Failed);
        let summarise = task(agent, "summarise", TaskStatus::Blocked);
        let edges = vec![(summarise.id, gather_a.id), (summarise.id, gather_b.id)];
        let tasks = vec![gather_a.clone(), gather_b.clone(), summarise.clone()];
        let statuses = tasks.iter().map(|task| (task.id, task.status)).collect();
        let names = HashMap::from([(agent, "ops".to_owned())]);

        let nodes = task_nodes(&tasks, &edges, &statuses, &names, agentos_core::now());
        let node = |id: TaskId| nodes.iter().find(|node| node.id == id.to_string()).unwrap();

        let join = node(summarise.id);
        assert!(join.unreachable);
        assert!(!join.runnable);
        assert_eq!(join.blocked_by_failure, Some(gather_b.id.to_string()));
        assert_eq!(join.blocked_by.len(), 2);
        assert_eq!(node(gather_a.id).blocks, vec![summarise.id.to_string()]);

        // A finished task is history, not something waiting on its graph.
        assert!(!node(gather_b.id).unreachable);
        assert_eq!(node(gather_b.id).blocked_by_failure, None);
        assert_eq!(join.agent_name, "ops");
    }

    #[test]
    fn a_task_is_runnable_only_once_its_clock_and_every_dependency_allow() {
        let agent = AgentId::new();
        let done = task(agent, "done", TaskStatus::Succeeded);
        let pending = task(agent, "pending", TaskStatus::Pending);
        let ready = task(agent, "ready", TaskStatus::Blocked);
        let waiting_on_pending = task(agent, "waits", TaskStatus::Blocked);
        let mut later = task(agent, "later", TaskStatus::Pending);
        later.scheduled_for = Some(agentos_core::now() + std::time::Duration::from_secs(3600));
        let edges = vec![(ready.id, done.id), (waiting_on_pending.id, pending.id)];
        let tasks = vec![
            done.clone(),
            pending.clone(),
            ready.clone(),
            waiting_on_pending.clone(),
            later.clone(),
        ];
        let statuses = tasks.iter().map(|task| (task.id, task.status)).collect();

        let nodes = task_nodes(
            &tasks,
            &edges,
            &statuses,
            &HashMap::new(),
            agentos_core::now(),
        );
        let runnable = |id: TaskId| {
            nodes
                .iter()
                .find(|node| node.id == id.to_string())
                .unwrap()
                .runnable
        };

        assert!(runnable(pending.id));
        assert!(runnable(ready.id));
        assert!(!runnable(waiting_on_pending.id));
        assert!(!runnable(later.id), "its moment has not come");
        assert!(!runnable(done.id), "it has already run");
    }

    #[test]
    fn a_dependency_outside_the_window_is_not_assumed_to_have_succeeded() {
        let agent = AgentId::new();
        let waits = task(agent, "waits", TaskStatus::Blocked);
        let unseen = TaskId::new();
        let edges = vec![(waits.id, unseen)];

        let nodes = task_nodes(
            std::slice::from_ref(&waits),
            &edges,
            &HashMap::new(),
            &HashMap::new(),
            agentos_core::now(),
        );
        assert!(!nodes[0].runnable);
        assert!(!nodes[0].unreachable);
        assert_eq!(nodes[0].agent_name, "(deleted)");
    }
}
