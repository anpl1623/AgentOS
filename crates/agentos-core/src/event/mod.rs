//! The observability event taxonomy.
//!
//! Every meaningful thing the runtime does emits a structured event. Text logs
//! are a rendering of these, never the source of truth: the audit log, the
//! desktop activity feed and the CLI trace all read the same stream.

mod kind;
#[cfg(test)]
mod samples;
mod summary;
mod taxonomy;

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, EventId, TaskId, TaskRunId};

pub use kind::{SECURITY_KINDS, is_security_kind};
pub use summary::operator_summary_of;
pub use taxonomy::AgentEvent;

/// An event with its context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Identity.
    pub id: EventId,
    /// When it happened.
    pub at: Timestamp,
    /// The agent involved, if any.
    pub agent_id: Option<AgentId>,
    /// The task involved, if any.
    pub task_id: Option<TaskId>,
    /// The run involved, if any.
    pub run_id: Option<TaskRunId>,
    /// What happened.
    pub payload: AgentEvent,
}

impl Event {
    /// Build an event with no context attached.
    #[must_use]
    pub fn new(payload: AgentEvent) -> Self {
        Self {
            id: EventId::new(),
            at: crate::now(),
            agent_id: None,
            task_id: None,
            run_id: None,
            payload,
        }
    }

    /// Attach the agent.
    #[must_use]
    pub const fn for_agent(mut self, agent_id: AgentId) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    /// Attach the task.
    #[must_use]
    pub const fn for_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Attach the run.
    #[must_use]
    pub const fn for_run(mut self, run_id: TaskRunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    /// The dotted event name.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.payload.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_builders_attach_ids() {
        let agent = AgentId::new();
        let task = TaskId::new();
        let run = TaskRunId::new();
        let event = Event::new(AgentEvent::TaskCancelled { steps: 0 })
            .for_agent(agent)
            .for_task(task)
            .for_run(run);
        assert_eq!(event.agent_id, Some(agent));
        assert_eq!(event.task_id, Some(task));
        assert_eq!(event.run_id, Some(run));
        assert_eq!(event.kind(), "agent.task.cancelled");
    }
}
