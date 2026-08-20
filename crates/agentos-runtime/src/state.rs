//! The run state machine, driven and observed.
//!
//! [`agentos_core::task::transition`] is the pure transition table. This is the
//! thing that owns a run's current state, applies transitions to it, refuses
//! illegal ones, persists the result and emits an event for each one.
//!
//! It is shared: the agent loop drives it, and the approval gate wrapper also
//! holds a handle so that a run genuinely enters `WaitingForApproval` while a
//! human is deciding, rather than that state existing only in a diagram.

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::task::{TaskState, TaskTrigger, transition};
use agentos_persistence::Database;
use tokio::sync::Mutex;

use crate::error::RuntimeError;

/// Owns and advances one run's state.
#[derive(Debug)]
pub struct RunStateMachine {
    agent_id: AgentId,
    task_id: TaskId,
    run_id: TaskRunId,
    state: Mutex<TaskState>,
    database: Database,
    audit: Arc<AuditLog>,
}

impl RunStateMachine {
    /// Start tracking a run.
    #[must_use]
    pub fn new(
        agent_id: AgentId,
        task_id: TaskId,
        run_id: TaskRunId,
        initial: TaskState,
        database: Database,
        audit: Arc<AuditLog>,
    ) -> Self {
        Self {
            agent_id,
            task_id,
            run_id,
            state: Mutex::new(initial),
            database,
            audit,
        }
    }

    /// The current state.
    pub async fn current(&self) -> TaskState {
        *self.state.lock().await
    }

    /// Apply a trigger.
    ///
    /// The lock is held across the persist and the audit write so that no
    /// observer can see a state the log does not also record.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::InvalidTransition`] if the trigger is illegal in the
    /// current state, or [`RuntimeError::Database`] if the write fails.
    pub async fn apply(&self, trigger: TaskTrigger) -> Result<TaskState, RuntimeError> {
        let mut state = self.state.lock().await;
        let from = *state;
        let to = transition(from, trigger)?;

        sqlx_update_state(&self.database, self.run_id, to).await?;
        *state = to;
        drop(state);

        self.emit(AgentEvent::StateTransitioned { from, to, trigger })
            .await;
        Ok(to)
    }

    /// Apply a trigger, tolerating an illegal one.
    ///
    /// Used on the cancellation path, where a race between the operator
    /// cancelling and the run finishing naturally is expected rather than a bug.
    pub async fn try_apply(&self, trigger: TaskTrigger) -> Option<TaskState> {
        self.apply(trigger).await.ok()
    }

    /// Record an event tagged with this run's context.
    pub async fn emit(&self, payload: AgentEvent) {
        let event = Event::new(payload)
            .for_agent(self.agent_id)
            .for_task(self.task_id)
            .for_run(self.run_id);
        if let Err(error) = self.audit.record(event).await {
            tracing::error!(%error, "failed to record audit event");
        }
    }

    /// The run being tracked.
    #[must_use]
    pub const fn run_id(&self) -> TaskRunId {
        self.run_id
    }

    /// The task being attempted.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// The agent responsible.
    #[must_use]
    pub const fn agent_id(&self) -> AgentId {
        self.agent_id
    }
}

async fn sqlx_update_state(
    database: &Database,
    run_id: TaskRunId,
    state: TaskState,
) -> Result<(), RuntimeError> {
    let mut run = database.runs().get(run_id).await?;
    run.state = state;
    if state.is_terminal() && run.completed_at.is_none() {
        run.completed_at = Some(agentos_core::now());
    }
    database.runs().update(&run).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use agentos_core::agent::{Agent, ModelConfig};
    use agentos_core::task::{Task, TaskRun};

    use super::*;

    async fn machine() -> (RunStateMachine, Arc<agentos_audit::InMemorySink>, Database) {
        let database = Database::in_memory().await.unwrap();
        let agent = Agent::new("tester", "instructions", ModelConfig::new("mock", "m"));
        database.agents().insert(&agent).await.unwrap();
        let task = Task::new(agent.id, "objective");
        database.tasks().insert(&task).await.unwrap();
        let run = TaskRun::new(task.id, 1);
        database.runs().insert(&run).await.unwrap();

        let sink = Arc::new(agentos_audit::InMemorySink::new());
        let audit = Arc::new(AuditLog::open(sink.clone()).await.unwrap());
        (
            RunStateMachine::new(
                agent.id,
                task.id,
                run.id,
                TaskState::Idle,
                database.clone(),
                audit,
            ),
            sink,
            database,
        )
    }

    #[tokio::test]
    async fn transitions_are_persisted_and_audited() {
        let (machine, sink, database) = machine().await;

        assert_eq!(
            machine.apply(TaskTrigger::Start).await.unwrap(),
            TaskState::Planning
        );
        assert_eq!(machine.current().await, TaskState::Planning);
        assert_eq!(
            database.runs().get(machine.run_id()).await.unwrap().state,
            TaskState::Planning
        );

        let kinds: Vec<String> = sink
            .records()
            .await
            .into_iter()
            .map(|record| record.kind)
            .collect();
        assert_eq!(kinds, vec!["agent.state.transitioned"]);
    }

    #[tokio::test]
    async fn illegal_transitions_are_refused_and_leave_state_untouched() {
        let (machine, _sink, _database) = machine().await;
        let error = machine
            .apply(TaskTrigger::ToolsCompleted)
            .await
            .unwrap_err();
        assert!(matches!(error, RuntimeError::InvalidTransition(_)));
        assert_eq!(machine.current().await, TaskState::Idle);
    }

    #[tokio::test]
    async fn terminal_states_stamp_a_completion_time() {
        let (machine, _sink, database) = machine().await;
        machine.apply(TaskTrigger::Start).await.unwrap();
        machine
            .apply(TaskTrigger::UnrecoverableError)
            .await
            .unwrap();

        let run = database.runs().get(machine.run_id()).await.unwrap();
        assert_eq!(run.state, TaskState::Failed);
        assert!(run.completed_at.is_some());
    }

    #[tokio::test]
    async fn try_apply_tolerates_a_lost_cancellation_race() {
        let (machine, _sink, _database) = machine().await;
        machine.apply(TaskTrigger::Start).await.unwrap();
        machine
            .apply(TaskTrigger::PlanProducedNoToolCalls)
            .await
            .unwrap();
        machine
            .apply(TaskTrigger::VerificationPassed)
            .await
            .unwrap();

        // The run finished first; cancelling now is a no-op, not a crash.
        assert_eq!(machine.try_apply(TaskTrigger::Cancel).await, None);
        assert_eq!(machine.current().await, TaskState::Completed);
    }
}
