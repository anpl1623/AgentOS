//! Schedules, and the scheduler that fires them: standing permission for work
//! to start with nobody present, and the record of when it could.

use agentos_core::Timestamp;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, ScheduleId};
use agentos_core::schedule::{Cadence, Schedule, ScheduleStatus};

use crate::scheduler::{SchedulerOptions, SchedulerTransition};
use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Create a schedule.
    ///
    /// Nothing fires until a scheduler is running; the schedule is standing
    /// permission for it to start work when one is.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::InvalidSchedule`] if the cadence cannot be evaluated or
    /// the name or objective is empty, [`RuntimeError::Database`] if the agent
    /// does not exist, the name is taken or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_schedule(
        &self,
        agent_id: AgentId,
        name: &str,
        objective: &str,
        cadence: Cadence,
        first_run_at: Timestamp,
    ) -> Result<Schedule, RuntimeError> {
        let agent = self.database.agents().get(agent_id).await?;
        let schedule = Schedule::new(agent.id, name, objective, cadence, first_run_at)
            .map_err(|error| RuntimeError::InvalidSchedule(error.to_string()))?;
        self.database.schedules().insert(&schedule).await?;
        self.record_operator(
            agent.id,
            AgentEvent::ScheduleCreated {
                schedule_id: schedule.id,
                name: schedule.name.clone(),
                objective: schedule.objective.clone(),
                cadence: schedule.cadence.clone(),
                next_run_at: schedule.next_run_at,
            },
        )
        .await?;
        Ok(schedule)
    }

    /// Stop a schedule firing without deleting it, or start it again.
    ///
    /// Pausing keeps its next occurrence. Resuming computes the next occurrence
    /// forward from now, so a schedule paused over a weekend does not wake up
    /// owing a backlog; a one-shot whose moment passed while it was paused has
    /// nothing left to do and is finished rather than left active and inert.
    ///
    /// Recorded even when the schedule was already in the state asked for, as
    /// an agent being switched on or off is.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the schedule does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_schedule_paused(
        &self,
        id: ScheduleId,
        paused: bool,
    ) -> Result<Schedule, RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        let (status, next) = if paused {
            (ScheduleStatus::Paused, schedule.next_run_at)
        } else {
            let now = agentos_core::now();
            let next = match schedule.next_run_at {
                // Its slot is still ahead; nothing was missed.
                Some(next) if next > now => Some(next),
                _ => schedule.cadence.next_after(now),
            };
            let status = if next.is_some() {
                ScheduleStatus::Active
            } else {
                ScheduleStatus::Finished
            };
            (status, next)
        };
        self.database
            .schedules()
            .set_status(id, status, next)
            .await?;

        let payload = if paused {
            AgentEvent::SchedulePaused {
                schedule_id: id,
                name: schedule.name.clone(),
            }
        } else {
            AgentEvent::ScheduleResumed {
                schedule_id: id,
                name: schedule.name.clone(),
                next_run_at: next,
            }
        };
        self.record_operator(schedule.agent_id, payload).await?;
        Ok(self.database.schedules().get(id).await?)
    }

    /// Delete a schedule. The tasks it already created are kept: they are
    /// history, and removing the schedule does not mean the work never
    /// happened.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the schedule does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn delete_schedule(&self, id: ScheduleId) -> Result<(), RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        self.database.schedules().delete(id).await?;
        self.record_operator(
            schedule.agent_id,
            AgentEvent::ScheduleDeleted {
                schedule_id: id,
                name: schedule.name,
            },
        )
        .await
    }

    /// Record that a scheduler started or stopped, and how it was paced.
    ///
    /// Called by the client that owns the scheduler, after it has taken the
    /// lease and before it ticks, and after it has stopped and drained. Between
    /// the two records, work in this installation started with nobody present.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn record_scheduler_state(
        &self,
        transition: SchedulerTransition,
        options: &SchedulerOptions,
    ) -> Result<(), RuntimeError> {
        let tick_seconds = options.tick.as_secs();
        let max_concurrent_runs = u32::try_from(options.max_concurrent_runs).unwrap_or(u32::MAX);
        let payload = match transition {
            SchedulerTransition::Started { on_launch } => AgentEvent::SchedulerStarted {
                tick_seconds,
                max_concurrent_runs,
                on_launch,
            },
            SchedulerTransition::Stopped => AgentEvent::SchedulerStopped {
                tick_seconds,
                max_concurrent_runs,
            },
        };
        self.audit.record(Event::new(payload)).await?;
        Ok(())
    }
}
