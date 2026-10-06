//! What an operator changes, and the record that they changed it.
//!
//! A policy, an agent's existence, whether it may run and the credentials its
//! model is reached with decide what every later run is able to do. A policy
//! widened five minutes before a bad action and narrowed again afterwards is
//! part of the explanation of that action, so each of these changes is written
//! to the audit chain beside the actions it made possible.
//!
//! This is the one layer the CLI and the desktop application both go through
//! for these changes, which is what makes the record unconditional: neither
//! client can make the change without it. The repositories underneath stay
//! thin and decide nothing; a client that writes to them directly is going
//! around the runtime, which no client in this repository does.
//!
//! Each method makes its change and then records it. A change that succeeds
//! and cannot be recorded returns the audit error rather than swallowing it:
//! the change stands, but the operator is told the log did not take it, which
//! is better than a log that silently has a gap.
//!
//! The same holds for what an agent is told before it plans and for what
//! starts work with nobody present: a memory, a schedule, a queued task and the
//! scheduler itself. Each method here makes exactly one record of its kind.

use agentos_core::Timestamp;
use agentos_core::agent::{Agent, AgentStatus, ModelConfig};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, MemoryId, ScheduleId, TaskId};
use agentos_core::memory::{Memory, MemoryKind};
use agentos_core::schedule::{Cadence, Schedule, ScheduleStatus};
use agentos_core::task::{Task, TaskStatus};
use agentos_core::trust::DataSource;
use agentos_permissions::PolicyDocument;
use agentos_providers::provider_ids;
use agentos_secrets::provider_key;

use crate::scheduler::{SchedulerOptions, SchedulerTransition};
use crate::{Runtime, RuntimeError, path_between};

impl Runtime {
    /// Create an agent with a starter policy scoped to its own workspace.
    ///
    /// The starter policy is deliberately close to useless: read-only inside one
    /// directory. Widening it is an explicit act by the operator.
    ///
    /// Records the agent's creation and then its starter policy, so the history
    /// of what the agent was allowed to do starts at version one rather than at
    /// the first edit somebody made to it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the name is taken or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn create_agent(
        &self,
        name: &str,
        instructions: &str,
        model: ModelConfig,
        tools: Vec<String>,
    ) -> Result<Agent, RuntimeError> {
        let agent = Agent::new(name, instructions, model).with_tools(tools);
        self.database.agents().insert(&agent).await?;
        // Recorded before anything else can fail: once the row exists the
        // agent exists, and its history must not begin after its creation.
        self.record_operator(
            agent.id,
            AgentEvent::AgentCreated {
                agent: agent.name.clone(),
                provider: agent.model.provider.clone(),
                model: agent.model.model.clone(),
                tools: agent.enabled_tools.clone(),
            },
        )
        .await?;

        let workspace = self.config.workspace_for(name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let policy = agentos_permissions::starter_policy_yaml(&workspace);
        self.install_policy(&agent, &policy).await?;
        Ok(agent)
    }

    /// Replace an agent's policy.
    ///
    /// Refuses a document that does not compile. A stored policy that fails to
    /// load makes every run of the agent deny everything, which is safe but
    /// bewildering; better to say so at the moment of saving. Returns the new
    /// version.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Policy`] if the document does not parse or compile,
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_policy(&self, agent_id: AgentId, document: &str) -> Result<i64, RuntimeError> {
        PolicyDocument::from_yaml(document)?.compile()?;
        let agent = self.database.agents().get(agent_id).await?;
        self.install_policy(&agent, document).await
    }

    /// Allow an agent to be given work, or stop it being given any.
    ///
    /// Recorded even when the agent was already in the state asked for: the
    /// record is of what the operator did, and a reader should not have to
    /// infer an act from its absence.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the agent does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn set_agent_enabled(
        &self,
        agent_id: AgentId,
        enabled: bool,
    ) -> Result<Agent, RuntimeError> {
        let mut agent = self.database.agents().get(agent_id).await?;
        agent.status = if enabled {
            AgentStatus::Enabled
        } else {
            AgentStatus::Disabled
        };
        self.database.agents().update(&agent).await?;
        self.record_operator(
            agent.id,
            AgentEvent::AgentEnabledChanged {
                agent: agent.name.clone(),
                enabled,
            },
        )
        .await?;
        Ok(agent)
    }

    /// Store a credential for a model provider in the runtime's secret store.
    ///
    /// The record names the provider and nothing else.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know or
    /// an empty key, [`RuntimeError::Secrets`] if the store refuses — on a
    /// machine with no keychain it is read-only — and [`RuntimeError::Audit`]
    /// if the change could not be recorded.
    pub async fn set_provider_key(&self, provider: &str, key: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(RuntimeError::Rejected("no key was provided".to_owned()));
        }
        self.secrets.set(&provider_key(provider), key)?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeySet {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    /// Remove a stored provider credential.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a provider the runtime does not know,
    /// [`RuntimeError::Secrets`] if the store fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remove_provider_key(&self, provider: &str) -> Result<(), RuntimeError> {
        known_provider(provider)?;
        self.secrets.delete(&provider_key(provider))?;
        self.audit
            .record(Event::new(AgentEvent::ProviderKeyRemoved {
                provider: provider.to_owned(),
            }))
            .await?;
        Ok(())
    }

    // -- Memory -------------------------------------------------------------

    /// Write a memory for an agent, as the operator.
    ///
    /// The source is always [`DataSource::User`], and no caller can say
    /// otherwise. This method can attest only that a person typed the text. A
    /// caller able to name another source could claim `Web` for a note it
    /// wanted distrusted, or — the direction that matters — launder a webpage's
    /// claim into a trusted note by calling it the operator's. Untrusted
    /// memories exist; they are written by the runtime from what a run read,
    /// never through here.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the agent does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn remember(
        &self,
        agent_id: AgentId,
        kind: MemoryKind,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let agent = self.database.agents().get(agent_id).await?;
        let memory =
            Memory::new(agent.id, kind, content, DataSource::User).with_confidence(confidence);
        self.database.memories().insert(&memory).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRemembered {
                agent: agent.name,
                memory_id: memory.id,
                kind: kind.as_str().to_owned(),
                content: memory.content.clone(),
            },
        )
        .await?;
        Ok(memory)
    }

    /// Rewrite a memory's content and confidence.
    ///
    /// Its source is kept. An operator correcting the wording of something a
    /// webpage claimed has not thereby checked the claim, so a revised
    /// untrusted memory stays untrusted. To vouch for a statement, forget the
    /// old memory and remember the statement.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for empty content, [`RuntimeError::Database`]
    /// if the memory does not exist or the write fails, and
    /// [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn revise_memory(
        &self,
        id: MemoryId,
        content: &str,
        confidence: f32,
    ) -> Result<Memory, RuntimeError> {
        let content = non_empty(content, "a memory needs something to remember")?;
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database
            .memories()
            .update(id, content, confidence)
            .await?;
        let revised = self.database.memories().get(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryRevised {
                agent: agent.name,
                memory_id: id,
                content: revised.content.clone(),
            },
        )
        .await?;
        Ok(revised)
    }

    /// Delete a memory.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the memory does not exist or the write
    /// fails, and [`RuntimeError::Audit`] if the change could not be recorded.
    pub async fn forget_memory(&self, id: MemoryId) -> Result<(), RuntimeError> {
        let memory = self.database.memories().get(id).await?;
        let agent = self.database.agents().get(memory.agent_id).await?;
        self.database.memories().delete(id).await?;
        self.record_operator(
            agent.id,
            AgentEvent::MemoryForgotten {
                agent: agent.name,
                memory_id: id,
                kind: memory.kind.as_str().to_owned(),
                content: memory.content,
            },
        )
        .await
    }

    // -- Schedules ----------------------------------------------------------

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

    // -- Tasks --------------------------------------------------------------

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

    // -- Scheduler ----------------------------------------------------------

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

    /// Store a policy that has already been checked, and record it.
    async fn install_policy(&self, agent: &Agent, document: &str) -> Result<i64, RuntimeError> {
        let version = self
            .database
            .agents()
            .set_policy(agent.id, document)
            .await?;
        self.record_operator(
            agent.id,
            AgentEvent::PolicyChanged {
                agent: agent.name.clone(),
                version,
                document: document.to_owned(),
            },
        )
        .await?;
        Ok(version)
    }

    /// Record an operator change about one agent.
    ///
    /// Attached to the agent and to no task or run: nothing was running when
    /// the operator made it, and the record says so.
    async fn record_operator(
        &self,
        agent_id: AgentId,
        payload: AgentEvent,
    ) -> Result<(), RuntimeError> {
        self.audit
            .record(Event::new(payload).for_agent(agent_id))
            .await?;
        Ok(())
    }
}

/// Operator-typed text with its surrounding whitespace removed, or a refusal
/// saying what was missing.
fn non_empty<'a>(text: &'a str, missing: &str) -> Result<&'a str, RuntimeError> {
    let text = text.trim();
    if text.is_empty() {
        Err(RuntimeError::Rejected(missing.to_owned()))
    } else {
        Ok(text)
    }
}

/// Refuse a provider identifier the runtime cannot build.
///
/// A credential stored under a misspelt provider would never be read, and its
/// record would name a provider that does not exist.
fn known_provider(provider: &str) -> Result<(), RuntimeError> {
    if provider_ids::ALL.contains(&provider) {
        Ok(())
    } else {
        Err(RuntimeError::Rejected(format!(
            "unknown provider `{provider}`; expected one of {}",
            provider_ids::ALL.join(", ")
        )))
    }
}
