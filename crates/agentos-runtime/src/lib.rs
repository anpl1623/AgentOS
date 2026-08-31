//! The AgentOS runtime.
//!
//! This is the composition root and the only place the pieces meet: the
//! database, the audit log, the tool registry, the policy engine, the provider
//! and the agent loop.
//!
//! It is also the API. The CLI in `agentos-cli` and, later, the desktop
//! application are both clients of this type — there is no second
//! implementation of any of this behind either of them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent_loop;
pub mod config;
pub mod error;
pub mod gate;
pub mod prompt;
pub mod scheduler;
pub mod state;

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::agent::{Agent, ModelConfig};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::task::{Task, TaskRun, TaskState, TaskStatus, TaskTrigger};
use agentos_permissions::{DenyAllEngine, PermissionEngine, PolicyDocument, PolicyEngine};
use agentos_persistence::Database;
use agentos_secrets::{ChainSecretStore, SecretStore};
use agentos_tools::{ApprovalGate, TaintTracker, ToolContext, ToolPipeline, ToolRegistry};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub use agent_loop::{AgentLoop, RunOutcome};
pub use agentos_tools::ToolRegistry as Registry;
pub use config::{
    FixedProviderFactory, ProviderFactory, RuntimeConfig, SecretBackedProviderFactory,
    build_provider,
};
pub use error::RuntimeError;
pub use gate::RunApprovalGate;
pub use scheduler::{Scheduler, SchedulerOptions, TickReport};
pub use state::RunStateMachine;

/// Everything a running AgentOS installation needs.
#[derive(Debug, Clone)]
pub struct Runtime {
    config: RuntimeConfig,
    database: Database,
    audit: Arc<AuditLog>,
    registry: Arc<ToolRegistry>,
    secrets: Arc<dyn SecretStore>,
    providers: Arc<dyn ProviderFactory>,
    /// Cancellation tokens for runs currently in flight, so the operator can
    /// stop an agent that is already working.
    running: Arc<Mutex<std::collections::HashMap<TaskRunId, CancellationToken>>>,
}

impl Runtime {
    /// Open a runtime against a configuration, creating and migrating storage.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] if directories cannot be created or the database cannot
    /// be opened.
    pub async fn open(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        // The keychain when there is one, the environment when there is not.
        // A machine with no Secret Service — a server, a container, CI — must
        // still be able to run an agent.
        Self::open_with_secrets(config, Arc::new(ChainSecretStore::standard())).await
    }

    /// Open a runtime with an explicit secret store.
    ///
    /// Tests use this to stay off the real keychain.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub async fn open_with_secrets(
        config: RuntimeConfig,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self, RuntimeError> {
        config.ensure_directories()?;
        let database = Database::open(&config.database_path).await?;
        let audit = Arc::new(AuditLog::open(Arc::new(database.audit_sink())).await?);

        Ok(Self {
            database,
            audit,
            registry: build_registry(&config),
            providers: Arc::new(SecretBackedProviderFactory::new(secrets.clone())),
            secrets,
            config,
            running: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Open an entirely in-memory runtime. Tests only.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] if the database cannot be created.
    pub async fn in_memory(
        workspace: std::path::PathBuf,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self, RuntimeError> {
        let database = Database::in_memory().await?;
        let audit = Arc::new(AuditLog::open(Arc::new(database.audit_sink())).await?);
        let mut config = RuntimeConfig::rooted_at(workspace.clone());
        config.workspace = workspace;

        Ok(Self {
            database,
            audit,
            registry: build_registry(&config),
            providers: Arc::new(SecretBackedProviderFactory::new(secrets.clone())),
            secrets,
            config,
            running: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// The configuration in use.
    #[must_use]
    pub const fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// The database, for read-only queries by clients.
    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// The audit log.
    #[must_use]
    pub fn audit(&self) -> &Arc<AuditLog> {
        &self.audit
    }

    /// The tool registry.
    #[must_use]
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// The secret store.
    #[must_use]
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    /// Replace the tool registry. Used by tests and, later, by plugin loading.
    pub fn set_registry(&mut self, registry: Arc<ToolRegistry>) {
        self.registry = registry;
    }

    /// Replace the provider factory.
    ///
    /// The seam tests use to substitute a scripted provider, and the one a
    /// provider plugin would register through.
    pub fn set_provider_factory(&mut self, providers: Arc<dyn ProviderFactory>) {
        self.providers = providers;
    }

    // -- Agents -------------------------------------------------------------

    /// Create an agent with a starter policy scoped to its own workspace.
    ///
    /// The starter policy is deliberately close to useless: read-only inside one
    /// directory. Widening it is an explicit act by the operator.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the name is taken or the write fails.
    pub async fn create_agent(
        &self,
        name: &str,
        instructions: &str,
        model: ModelConfig,
        tools: Vec<String>,
    ) -> Result<Agent, RuntimeError> {
        let agent = Agent::new(name, instructions, model).with_tools(tools);
        self.database.agents().insert(&agent).await?;

        let workspace = self.config.workspace_for(name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let policy = agentos_permissions::starter_policy_yaml(&workspace);
        self.database.agents().set_policy(agent.id, &policy).await?;
        Ok(agent)
    }

    /// Look up an agent by name.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::UnknownAgent`] if there is no such agent.
    pub async fn agent_by_name(&self, name: &str) -> Result<Agent, RuntimeError> {
        self.database
            .agents()
            .find_by_name(name)
            .await?
            .ok_or_else(|| RuntimeError::UnknownAgent(name.to_owned()))
    }

    /// Build the policy engine for an agent.
    ///
    /// An agent with no stored policy gets [`DenyAllEngine`]. Absence of a
    /// policy must never mean absence of restriction.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Policy`] if the stored document does not compile.
    pub async fn engine_for(
        &self,
        agent_id: AgentId,
    ) -> Result<Arc<dyn PermissionEngine>, RuntimeError> {
        match self.database.agents().policy(agent_id).await? {
            None => Ok(Arc::new(DenyAllEngine)),
            Some(stored) => {
                let policy = PolicyDocument::from_yaml(&stored.document)?.compile()?;
                Ok(Arc::new(PolicyEngine::new(policy)))
            }
        }
    }

    // -- Tasks --------------------------------------------------------------

    /// Create a task for an agent.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn create_task(
        &self,
        agent_id: AgentId,
        objective: &str,
    ) -> Result<Task, RuntimeError> {
        let task = Task::new(agent_id, objective);
        self.database.tasks().insert(&task).await?;
        Ok(task)
    }

    /// Create a task that waits for others to succeed first.
    ///
    /// The task is stored `Blocked` and each edge is checked for cycles before
    /// it is written, so a graph that cannot make progress is refused at the
    /// moment somebody describes it rather than discovered by a scheduler that
    /// never starts anything.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::InvalidGraph`] if a named dependency does not exist,
    /// [`RuntimeError::DependencyCycle`] if an edge would close a cycle, or
    /// [`RuntimeError::Database`] on failure.
    pub async fn create_task_after(
        &self,
        agent_id: AgentId,
        objective: &str,
        dependencies: &[TaskId],
    ) -> Result<Task, RuntimeError> {
        // Validate before writing anything, so a graph with one bad edge does
        // not leave a half-built one behind.
        for dependency in dependencies {
            if self.database.tasks().find(*dependency).await?.is_none() {
                return Err(RuntimeError::InvalidGraph(format!(
                    "task {dependency} does not exist, so nothing can wait for it"
                )));
            }
        }

        let task = if dependencies.is_empty() {
            Task::new(agent_id, objective)
        } else {
            Task::new(agent_id, objective).blocked()
        };
        self.database.tasks().insert(&task).await?;

        for dependency in dependencies {
            self.add_dependency(task.id, *dependency).await?;
        }
        Ok(task)
    }

    /// Hold a task until a given moment.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn create_task_at(
        &self,
        agent_id: AgentId,
        objective: &str,
        when: agentos_core::Timestamp,
    ) -> Result<Task, RuntimeError> {
        let task = Task::new(agent_id, objective).scheduled_for(when);
        self.database.tasks().insert(&task).await?;
        Ok(task)
    }

    /// Record that `task` waits for `depends_on`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DependencyCycle`] if the edge would close a cycle,
    /// [`RuntimeError::InvalidGraph`] if a task is missing or the edge is a
    /// self-loop, or [`RuntimeError::Database`] on failure.
    pub async fn add_dependency(
        &self,
        task: TaskId,
        depends_on: TaskId,
    ) -> Result<(), RuntimeError> {
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
        let stored = self.database.tasks().get(task).await?;
        if matches!(stored.status, agentos_core::TaskStatus::Pending) {
            self.database
                .tasks()
                .set_status(task, agentos_core::TaskStatus::Blocked)
                .await?;
        }
        Ok(())
    }

    // -- Schedules ------------------------------------------------------------

    /// Create a schedule.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::UnknownAgent`] if the agent does not exist,
    /// [`RuntimeError::InvalidGraph`] never, and [`RuntimeError::Database`] on
    /// failure. An unevaluable cadence surfaces as
    /// [`RuntimeError::InvalidSchedule`].
    pub async fn create_schedule(
        &self,
        agent_id: AgentId,
        name: &str,
        objective: &str,
        cadence: agentos_core::schedule::Cadence,
        first_run_at: agentos_core::Timestamp,
    ) -> Result<agentos_core::schedule::Schedule, RuntimeError> {
        let schedule =
            agentos_core::schedule::Schedule::new(agent_id, name, objective, cadence, first_run_at)
                .map_err(|error| RuntimeError::InvalidSchedule(error.to_string()))?;
        self.database.schedules().insert(&schedule).await?;
        Ok(schedule)
    }

    /// Every schedule, newest first.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn schedules(&self) -> Result<Vec<agentos_core::schedule::Schedule>, RuntimeError> {
        Ok(self.database.schedules().list().await?)
    }

    /// Stop a schedule firing without deleting it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn pause_schedule(
        &self,
        id: agentos_core::ids::ScheduleId,
    ) -> Result<(), RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        self.database
            .schedules()
            .set_status(
                id,
                agentos_core::schedule::ScheduleStatus::Paused,
                schedule.next_run_at,
            )
            .await?;
        Ok(())
    }

    /// Start a paused schedule firing again.
    ///
    /// The next occurrence is computed forward from now, so a schedule that was
    /// paused over a weekend does not wake up owing a backlog.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn resume_schedule(
        &self,
        id: agentos_core::ids::ScheduleId,
    ) -> Result<(), RuntimeError> {
        let schedule = self.database.schedules().get(id).await?;
        let now = agentos_core::now();
        let next = match schedule.next_run_at {
            // Its slot is still ahead; nothing was missed.
            Some(next) if next > now => Some(next),
            _ => schedule.cadence.next_after(now),
        };
        let status = if next.is_some() {
            agentos_core::schedule::ScheduleStatus::Active
        } else {
            // A one-shot whose moment passed while it was paused has nothing
            // left to do, and saying so beats leaving it active and inert.
            agentos_core::schedule::ScheduleStatus::Finished
        };
        self.database
            .schedules()
            .set_status(id, status, next)
            .await?;
        Ok(())
    }

    /// Delete a schedule. The tasks it created are left alone.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn delete_schedule(
        &self,
        id: agentos_core::ids::ScheduleId,
    ) -> Result<(), RuntimeError> {
        self.database.schedules().delete(id).await?;
        Ok(())
    }

    /// Execute a task and return what it produced.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] for runtime failures. A task that merely fails returns
    /// `Ok` with a failed [`RunOutcome`].
    pub async fn run_task(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<RunOutcome, RuntimeError> {
        let prepared = self.prepare_run(task, approvals, cancel).await?;
        self.drive(prepared).await
    }

    /// Begin a run and hand back its identity immediately.
    ///
    /// The run proceeds in the background. A user interface needs this: it has
    /// to show the trace of a run that may take minutes, so it cannot wait for
    /// the run to finish before learning what to show.
    ///
    /// The returned handle resolves to the same outcome [`Self::run_task`]
    /// would produce. Dropping it does not stop the run — use
    /// [`Self::cancel_run`], which is what the operator's stop button does.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] if the run cannot be started. Once started, failures
    /// are reported through the outcome rather than here.
    pub async fn start_task(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<
        (
            TaskRunId,
            tokio::task::JoinHandle<Result<RunOutcome, RuntimeError>>,
        ),
        RuntimeError,
    > {
        let prepared = self.prepare_run(task, approvals, cancel).await?;
        let run_id = prepared.run.id;
        let runtime = self.clone();
        let handle = tokio::spawn(async move { runtime.drive(prepared).await });
        Ok((run_id, handle))
    }

    /// Everything a run needs, assembled but not yet started.
    async fn prepare_run(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<PreparedRun, RuntimeError> {
        let agent = self.database.agents().get(task.agent_id).await?;
        if !agent.is_enabled() {
            return Err(RuntimeError::DisabledAgent(agent.name));
        }

        let provider = self.providers.build(&agent.name, &agent.model)?;
        let engine = self.engine_for(agent.id).await?;

        let attempt = self.database.runs().next_attempt(task.id).await?;
        let run = TaskRun::new(task.id, attempt);
        self.database.runs().insert(&run).await?;

        self.running.lock().await.insert(run.id, cancel.clone());

        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Idle,
            self.database.clone(),
            self.audit.clone(),
        ));

        let gate: Arc<dyn ApprovalGate> = Arc::new(RunApprovalGate::new(
            approvals,
            self.database.clone(),
            machine.clone(),
        ));

        let pipeline = ToolPipeline::new(self.registry.clone(), engine, gate, self.audit.clone());

        let workspace = self.config.workspace_for(&agent.name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let context = ToolContext::new(agent.id, task.id, run.id, workspace);

        Ok(PreparedRun {
            objective: task.objective.clone(),
            agent,
            run,
            provider,
            pipeline,
            machine,
            context,
            cancel,
        })
    }

    /// Drive a prepared run to a terminal state.
    async fn drive(&self, prepared: PreparedRun) -> Result<RunOutcome, RuntimeError> {
        let objective = prepared.objective;
        let agent_loop = AgentLoop::new(
            prepared.agent,
            prepared.run,
            self.database.clone(),
            prepared.provider,
            prepared.pipeline,
            prepared.machine,
            prepared.context,
            Arc::new(TaintTracker::new()),
            prepared.cancel,
        );

        let outcome = agent_loop.run(&objective).await;
        if let Ok(report) = &outcome {
            self.running.lock().await.remove(&report.run_id);
        }
        outcome
    }

    /// Create and immediately execute a task.
    ///
    /// # Errors
    ///
    /// As [`Self::run_task`].
    pub async fn run_objective(
        &self,
        agent_id: AgentId,
        objective: &str,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<RunOutcome, RuntimeError> {
        let task = self.create_task(agent_id, objective).await?;
        self.run_task(&task, approvals, cancel).await
    }

    /// Stop a run that is currently executing.
    ///
    /// Returns whether a live run was found. The operator must always be able
    /// to stop an agent, so this cancels the token the loop and every tool
    /// inside it are watching.
    pub async fn cancel_run(&self, run_id: TaskRunId) -> bool {
        match self.running.lock().await.remove(&run_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Run identifiers currently executing.
    pub async fn running_runs(&self) -> Vec<TaskRunId> {
        self.running.lock().await.keys().copied().collect()
    }

    /// Mark runs abandoned by a previous process as failed.
    ///
    /// Called at startup: a run that was executing when the process died is not
    /// executing now, and leaving it looking alive would misreport the system's
    /// state indefinitely.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn reap_abandoned_runs(&self) -> Result<usize, RuntimeError> {
        let abandoned = self.database.runs().list_unfinished().await?;
        let count = abandoned.len();

        for mut run in abandoned {
            let machine = RunStateMachine::new(
                AgentId::new(),
                run.task_id,
                run.id,
                run.state,
                self.database.clone(),
                self.audit.clone(),
            );
            machine.try_apply(TaskTrigger::UnrecoverableError).await;

            run.state = TaskState::Failed;
            run.failure = Some(agentos_core::task::TaskFailure::Runtime {
                message: "the process exited while this run was in progress".to_owned(),
            });
            run.completed_at = Some(agentos_core::now());
            self.database.runs().update(&run).await?;
            self.database
                .tasks()
                .set_status(run.task_id, TaskStatus::Failed)
                .await?;
            let _ = self
                .database
                .approvals()
                .cancel_pending_for_run(run.id)
                .await;
        }

        Ok(count)
    }

    /// Read a run's full execution trace.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn trace(&self, run_id: TaskRunId) -> Result<RunTrace, RuntimeError> {
        let run = self.database.runs().get(run_id).await?;
        let task = self.database.tasks().get(run.task_id).await?;
        let agent = self.database.agents().get(task.agent_id).await?;
        Ok(RunTrace {
            agent_name: agent.name,
            objective: task.objective,
            steps: self.database.steps().list_for_run(run_id).await?,
            executions: self.database.executions().list_for_run(run_id).await?,
            approvals: self.database.approvals().list_for_run(run_id).await?,
            run,
        })
    }

    /// Verify the audit chain.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn verify_audit(&self) -> Result<agentos_audit::ChainVerification, RuntimeError> {
        let records = self.database.audit_sink().all().await?;
        Ok(agentos_audit::verify_chain(&records))
    }

    /// The most recent task for an agent, if any.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn latest_task(&self, agent_id: AgentId) -> Result<Option<Task>, RuntimeError> {
        Ok(self
            .database
            .tasks()
            .list_for_agent(agent_id, 1)
            .await?
            .into_iter()
            .next())
    }

    /// Resolve a task by its identifier.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if it does not exist.
    pub async fn task(&self, task_id: TaskId) -> Result<Task, RuntimeError> {
        Ok(self.database.tasks().get(task_id).await?)
    }
}

/// Build the registry every client gets: the built-in tools, the browser, and
/// computer control.
///
/// The composition root owns this so that the CLI and the desktop application
/// cannot end up offering different tools for the same installation — and so
/// that a command listing the catalogue lists the same catalogue an agent is
/// actually given.
///
/// Public and free of side effects: listing the tools should not create a
/// database, launch a browser, or ask macOS for the Accessibility permission.
#[must_use]
pub fn build_registry(config: &RuntimeConfig) -> Arc<ToolRegistry> {
    build_registry_with(agentos_browser::BrowserOptions::new(
        config.browser_profiles(),
    ))
}

/// The same registry, with the browser configured differently.
///
/// The demonstration runs headed so that a human can watch it work. That is the
/// only reason this exists — a second registry composed by hand is how the
/// catalogue and the runtime drift apart, which has happened here before.
#[must_use]
pub fn build_registry_with(browser: agentos_browser::BrowserOptions) -> Arc<ToolRegistry> {
    build_registry_sharing(&Arc::new(agentos_browser::BrowserPool::new(browser)))
}

/// The same registry again, around a browser pool the caller already holds.
///
/// Only a test needs this: to assert that a run released its browser it has to
/// be looking at the same pool the tools are using, and a second pool would make
/// the assertion pass by being empty.
#[must_use]
pub fn build_registry_sharing(pool: &Arc<agentos_browser::BrowserPool>) -> Arc<ToolRegistry> {
    let mut registry = agentos_tools::standard_registry();
    for tool in agentos_browser::browser_tools(Arc::clone(pool)) {
        registry.register(tool);
    }
    for tool in agentos_computer::build() {
        registry.register(tool);
    }
    Arc::new(registry)
}

/// A run that has been assembled but not started.
///
/// Splitting assembly from execution is what lets a caller learn a run's
/// identity before it finishes — the row exists and the state machine is ready,
/// but no model has been called yet.
struct PreparedRun {
    objective: String,
    agent: Agent,
    run: TaskRun,
    provider: agentos_providers::SharedProvider,
    pipeline: ToolPipeline,
    machine: Arc<RunStateMachine>,
    context: ToolContext,
    cancel: CancellationToken,
}

impl std::fmt::Debug for PreparedRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRun")
            .field("agent", &self.agent.name)
            .field("run", &self.run.id)
            .finish_non_exhaustive()
    }
}

/// Everything recorded about one run.
#[derive(Debug, Clone)]
pub struct RunTrace {
    /// The run itself.
    pub run: TaskRun,
    /// The agent that performed it.
    pub agent_name: String,
    /// What it was asked to do.
    pub objective: String,
    /// The ordered trace.
    pub steps: Vec<agentos_core::task::TaskStep>,
    /// Tool invocations, with their permission decisions.
    pub executions: Vec<agentos_persistence::ToolExecutionRecord>,
    /// Approvals raised during the run.
    pub approvals: Vec<agentos_core::approval::ApprovalRequest>,
}

/// The path from `from` to `to` following dependency edges, if one exists.
///
/// An edge `(task, depends_on)` means `task` waits for `depends_on`, so this
/// walks in the direction of "what am I waiting for". Breadth-first, so the path
/// reported to somebody who has just written a cycle is the shortest one rather
/// than whichever the recursion happened to find.
fn path_between(edges: &[(TaskId, TaskId)], from: TaskId, to: TaskId) -> Option<Vec<TaskId>> {
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
