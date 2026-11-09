//! Tasks and the graph between them: queuing one, and making one wait for
//! another without ever closing a cycle.

use agentos_core::Timestamp;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::{Task, TaskStatus};

use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Create a task, waiting for others to succeed first, not starting before
    /// a given moment, both or neither.
    ///
    /// A task with dependencies is stored `Blocked`. Every dependency is
    /// checked to exist before anything is written, so a graph with one bad
    /// edge does not leave a half-built one behind; a new task cannot close a
    /// cycle, since nothing waits for it yet.
    ///
    /// The task is queued, not run. A client that runs it at once passes it to
    /// [`Runtime::start_task`] or [`Runtime::run_task`]; otherwise it starts
    /// only when a scheduler finds it runnable.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for an empty objective,
    /// [`RuntimeError::InvalidGraph`] if a named dependency does not exist,
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_task(
        &self,
        agent_id: AgentId,
        objective: &str,
        depends_on: &[TaskId],
        scheduled_for: Option<Timestamp>,
    ) -> Result<Task, RuntimeError> {
        if objective.trim().is_empty() {
            return Err(RuntimeError::Rejected(
                "a task needs an objective".to_owned(),
            ));
        }
        let agent = self.database.agents().get(agent_id).await?;
        let mut unique: Vec<TaskId> = Vec::with_capacity(depends_on.len());
        for dependency in depends_on {
            if !unique.contains(dependency) {
                unique.push(*dependency);
            }
        }
        let depends_on = unique;
        for dependency in &depends_on {
            if self.database.tasks().find(*dependency).await?.is_none() {
                return Err(RuntimeError::InvalidGraph(format!(
                    "task {dependency} does not exist, so nothing can wait for it"
                )));
            }
        }

        let mut task = Task::new(agent.id, objective);
        if !depends_on.is_empty() {
            task = task.blocked();
        }
        if let Some(when) = scheduled_for {
            task = task.scheduled_for(when);
        }
        // The row and its edges in one write. A new task cannot close a cycle,
        // since nothing waits for it yet, so `link`'s walk has nothing to find;
        // what matters is that no scheduler sees the task before its edges.
        self.database
            .tasks()
            .insert_waiting(&task, &depends_on)
            .await?;

        self.audit
            .record(
                Event::new(AgentEvent::TaskCreated {
                    objective: task.objective.clone(),
                    depends_on,
                    scheduled_for: task.scheduled_for,
                })
                .for_agent(agent.id)
                .for_task(task.id),
            )
            .await?;
        Ok(task)
    }

    /// Make `task` wait for `depends_on`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DependencyCycle`] if the edge would close a cycle,
    /// [`RuntimeError::InvalidGraph`] if a task is missing or the edge is a
    /// self-loop, [`RuntimeError::Database`] on failure, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn add_task_dependency(
        &self,
        task: TaskId,
        depends_on: TaskId,
    ) -> Result<(), RuntimeError> {
        let waiting = self.link(task, depends_on).await?;
        self.audit
            .record(
                Event::new(AgentEvent::TaskDependencyAdded {
                    task_id: task,
                    depends_on,
                })
                .for_agent(waiting.agent_id)
                .for_task(task),
            )
            .await?;
        Ok(())
    }

    /// Write one edge of a task graph, refusing one that cannot be satisfied.
    ///
    /// Returns the waiting task as it now is. Not recorded here: the operator
    /// act that wrote the edge is, once, by its caller.
    async fn link(&self, task: TaskId, depends_on: TaskId) -> Result<Task, RuntimeError> {
        if task == depends_on {
            return Err(RuntimeError::InvalidGraph(
                "a task cannot wait for itself".to_owned(),
            ));
        }
        for id in [task, depends_on] {
            if self.database.tasks().find(id).await?.is_none() {
                return Err(RuntimeError::InvalidGraph(format!(
                    "task {id} does not exist"
                )));
            }
        }

        // Walk the existing graph from `depends_on`. If `task` is reachable,
        // then `task` is already upstream of `depends_on` and this edge would
        // close the loop.
        let edges = self.database.dependencies().all().await?;
        if let Some(path) = path_between(&edges, depends_on, task) {
            let mut cycle = vec![task];
            cycle.extend(path);
            return Err(RuntimeError::DependencyCycle { path: cycle });
        }

        self.database.dependencies().add(task, depends_on).await?;

        // A task somebody has just made wait is not pending any more.
        let mut stored = self.database.tasks().get(task).await?;
        if matches!(stored.status, TaskStatus::Pending) {
            self.database
                .tasks()
                .set_status(task, TaskStatus::Blocked)
                .await?;
            stored.status = TaskStatus::Blocked;
        }
        Ok(stored)
    }
}

/// The path from `from` to `to` following dependency edges, if one exists.
///
/// An edge `(task, depends_on)` means `task` waits for `depends_on`, so this
/// walks in the direction of "what am I waiting for". Breadth-first, so the path
/// reported to somebody who has just written a cycle is the shortest one rather
/// than whichever the recursion happened to find.
pub(crate) fn path_between(
    edges: &[(TaskId, TaskId)],
    from: TaskId,
    to: TaskId,
) -> Option<Vec<TaskId>> {
    use std::collections::{HashMap, HashSet, VecDeque};

    let mut queue = VecDeque::from([from]);
    let mut seen = HashSet::from([from]);
    let mut came_from: HashMap<TaskId, TaskId> = HashMap::new();

    while let Some(current) = queue.pop_front() {
        if current == to {
            let mut path = vec![current];
            let mut cursor = current;
            while let Some(previous) = came_from.get(&cursor) {
                path.push(*previous);
                cursor = *previous;
            }
            path.reverse();
            return Some(path);
        }
        for (task, depends_on) in edges {
            if *task == current && seen.insert(*depends_on) {
                came_from.insert(*depends_on, current);
                queue.push_back(*depends_on);
            }
        }
    }
    None
}
