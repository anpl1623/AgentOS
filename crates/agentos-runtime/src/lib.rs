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
pub mod grants;
mod liveness;
pub mod operator;
pub mod prompt;
pub mod scheduler;
pub mod state;

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::agent::Agent;
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::task::{Task, TaskRun, TaskState, TaskStatus, TaskTrigger};
use agentos_permissions::{
    ApprovalPolicy, DenyAllEngine, PermissionEngine, Policy, PolicyDocument, PolicyEngine,
};
use agentos_persistence::Database;
use agentos_secrets::{ChainSecretStore, SecretStore};
use agentos_tools::{ApprovalGate, TaintTracker, ToolContext, ToolPipeline, ToolRegistry};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub use agent_loop::{AgentLoop, RunOutcome};
pub use agentos_permissions::Reach;
pub use agentos_tools::ToolRegistry as Registry;
pub use config::{
    FixedProviderFactory, ProviderFactory, RuntimeConfig, SecretBackedProviderFactory,
    build_provider,
};
pub use error::RuntimeError;
pub use gate::RunApprovalGate;
pub use grants::{CapabilityGrant, ToolGrant};
pub use scheduler::{
    MIN_TICK_SECONDS, Scheduler, SchedulerOptions, SchedulerPreference, SchedulerTransition,
    TickReport,
};
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
    /// Tells other processes on this data directory that this one drives
    /// runs, so none of them reaps a run that is still alive here. Absent for
    /// an in-memory runtime, whose database no other process can see.
    runs_lock: Option<Arc<liveness::RunsLock>>,
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
            runs_lock: Some(Arc::new(liveness::RunsLock::in_directory(&config.data_dir))),
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
            runs_lock: None,
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
        Ok(engine_from(self.policy_for(agent_id).await?))
    }

    /// The agent's compiled policy, or `None` when it has none stored.
    async fn policy_for(&self, agent_id: AgentId) -> Result<Option<Policy>, RuntimeError> {
        match self.database.agents().policy(agent_id).await? {
            None => Ok(None),
            Some(stored) => Ok(Some(
                PolicyDocument::from_yaml(&stored.document)?.compile()?,
            )),
        }
    }

    // -- Schedules ------------------------------------------------------------

    /// Every schedule, newest first.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn schedules(&self) -> Result<Vec<agentos_core::schedule::Schedule>, RuntimeError> {
        Ok(self.database.schedules().list().await?)
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
    ///
    /// The task is claimed, and its run written, in one step. Two clients can
    /// both have read a task as runnable — a scheduler here and another in a
    /// terminal, or a scheduler and a person pressing retry — and only the one
    /// whose claim lands may start it. The claim compares with the status in
    /// `task`, which is what the caller read: a task cancelled, run or given
    /// something to wait for since then is not started on the strength of the
    /// old read. A caller that loses gets [`RuntimeError::TaskAlreadyClaimed`]
    /// having written nothing: no run row, no audit record, no model request. Everything that can refuse for a
    /// reason of its own is checked first, so a refusal leaves the task as it
    /// was rather than claimed by nobody.
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
        // Before the run exists, so no reaper elsewhere can see it without
        // also seeing that this process is alive to drive it.
        if let Some(lock) = &self.runs_lock {
            lock.hold().await?;
        }
        // One compilation feeds both the engine and the gate, so the budget
        // a run is held to comes from the same document as its rules. An
        // agent with no policy is denied everything and never asks, so the
        // budget it gets is moot; it gets the default all the same.
        let policy = self.policy_for(agent.id).await?;
        let budget = policy
            .as_ref()
            .map_or_else(ApprovalPolicy::default, |policy| policy.approvals)
            .max_per_run;
        let engine = engine_from(policy);

        let workspace = self.config.workspace_for(&agent.name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let run = self.next_run(task.id).await?;
        // The claim and the run row are one write, so the task is never
        // running without a run for the reaper to find.
        if !self
            .database
            .tasks()
            .claim_for_run(task.status, &run)
            .await?
        {
            return Err(RuntimeError::TaskAlreadyClaimed(task.id));
        }

        self.running.lock().await.insert(run.id, cancel.clone());

        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Idle,
            self.database.clone(),
            self.audit.clone(),
        ));

        let gate: Arc<dyn ApprovalGate> = Arc::new(
            RunApprovalGate::new(approvals, self.database.clone(), machine.clone())
                .with_budget(budget),
        );

        let pipeline = ToolPipeline::new(self.registry.clone(), engine, gate, self.audit.clone());

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

    /// Assemble the next attempt at a task, to be written with its claim.
    ///
    /// Read before the claim, so a caller that loses it may have read for
    /// nothing; it has written nothing, which is what matters.
    async fn next_run(&self, task_id: TaskId) -> Result<TaskRun, RuntimeError> {
        let attempt = self.database.runs().next_attempt(task_id).await?;
        let mut run = TaskRun::new(task_id, attempt);
        self.inherit_taint(&mut run).await?;
        Ok(run)
    }

    /// Carry the taint of every earlier attempt at a task into a new one.
    ///
    /// A retry is the same task over the same conversation, memory and
    /// workspace. Whatever an earlier attempt read from outside is still in
    /// reach, so a new attempt starting clean would let a single failure
    /// launder it. Every earlier attempt is consulted rather than only the
    /// latest, so that one attempt recorded without its sources cannot break
    /// the chain for all the attempts after it.
    async fn inherit_taint(&self, run: &mut TaskRun) -> Result<(), RuntimeError> {
        for earlier in self.database.runs().list_for_task(run.task_id).await? {
            run.tainted |= earlier.tainted;
            for source in earlier.taint_sources {
                if !run.taint_sources.contains(&source) {
                    run.taint_sources.push(source);
                }
            }
        }
        Ok(())
    }

    /// Drive a prepared run to a terminal state.
    async fn drive(&self, prepared: PreparedRun) -> Result<RunOutcome, RuntimeError> {
        let objective = prepared.objective;
        let taint = Arc::new(TaintTracker::seeded(
            prepared.run.tainted,
            prepared.run.taint_sources.clone(),
        ));
        let agent_loop = AgentLoop::new(
            prepared.agent,
            prepared.run,
            self.database.clone(),
            prepared.provider,
            prepared.pipeline,
            prepared.machine,
            prepared.context,
            taint,
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
        let task = self.create_task(agent_id, objective, &[], None).await?;
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

    /// Take this data directory's scheduler lease, for as long as the
    /// returned handle is held.
    ///
    /// `None` for an in-memory runtime, whose database no other process can
    /// see and which therefore has no other scheduler to exclude.
    pub(crate) fn scheduler_lease(&self) -> Result<liveness::SchedulerLease, RuntimeError> {
        if self.runs_lock.is_none() {
            return Ok(liveness::SchedulerLease { _held: None });
        }
        liveness::take_scheduler_lease(&self.config.data_dir)
            .map(|file| liveness::SchedulerLease { _held: Some(file) })
    }

    /// Run identifiers currently executing.
    pub async fn running_runs(&self) -> Vec<TaskRunId> {
        self.running.lock().await.keys().copied().collect()
    }

    /// Mark runs abandoned by a previous process as failed, and close the
    /// approval requests they left behind.
    ///
    /// Called at startup: a run that was executing when the process died is not
    /// executing now, and leaving it looking alive would misreport the system's
    /// state indefinitely.
    ///
    /// Its pending requests are the same lie in the approvals queue, and the
    /// more dangerous one: a card a person can still answer, for a run that will
    /// never act on the answer. Every pending request whose run has ended —
    /// those reaped here, and any an earlier crash left on a run that had
    /// already finished — is marked expired, with a note saying why.
    ///
    /// An unfinished run is only abandoned if no process is driving it, and
    /// the desktop application, a terminal run and `agentos doctor` can all
    /// share one data directory. So nothing is reaped while any process,
    /// this one included, holds the data directory's run lock; the call
    /// returns zero and the runs are left to whoever is driving them. A run
    /// this process is driving is never reaped either way.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure, [`RuntimeError::Io`] if the run
    /// lock cannot be consulted.
    pub async fn reap_abandoned_runs(&self) -> Result<usize, RuntimeError> {
        // Held until the reap is done, so no run can start meanwhile.
        let _exclusive = match &self.runs_lock {
            Some(lock) => match lock.try_exclusive()? {
                Some(exclusive) => Some(exclusive),
                None => {
                    tracing::info!(
                        "another process is driving runs; leaving unfinished runs to it"
                    );
                    return Ok(0);
                }
            },
            None => None,
        };

        let live = self.running_runs().await;
        let abandoned: Vec<TaskRun> = self
            .database
            .runs()
            .list_unfinished()
            .await?
            .into_iter()
            .filter(|run| !live.contains(&run.id))
            .collect();
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
        }

        let expired = self
            .database
            .approvals()
            .expire_for_finished_runs(
                "the run that asked had ended, so nobody could act on an answer; most often \
                 the process exited while this request was waiting",
            )
            .await?;
        if expired > 0 {
            tracing::warn!(expired, "closed approval requests left by runs that ended");
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

    /// Verify the audit chain, rehashing every record from genesis.
    ///
    /// A log whose oldest records were removed does not verify: the first
    /// record left must be the first record written.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn verify_audit(&self) -> Result<agentos_audit::ChainVerification, RuntimeError> {
        let records = self.database.audit_sink().all().await?;
        Ok(agentos_audit::verify_chain(&records))
    }

    /// Verify only the records written since an earlier verification.
    ///
    /// Rehashing the whole chain is the most expensive thing the runtime can
    /// be asked to do, and a screen that polls cannot afford it. This checks
    /// each new record's own hash, that the records follow one another, and
    /// that the first of them points at the checkpoint — so the new part is
    /// proved to extend the part already proved.
    ///
    /// What it cannot see is a change to a record before the checkpoint made
    /// after that record was verified. That is what [`Self::verify_audit`] is
    /// for, and why it stays a deliberate act rather than a poll.
    ///
    /// The checkpoint returned is advanced only past an intact stretch, so a
    /// break goes on being reported until somebody looks at it.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn verify_audit_from(
        &self,
        checkpoint: Option<&AuditCheckpoint>,
    ) -> Result<(agentos_audit::ChainVerification, AuditCheckpoint), RuntimeError> {
        let start = checkpoint.cloned().unwrap_or_else(AuditCheckpoint::genesis);
        let records = self.database.audit_sink().after(start.sequence).await?;
        let verification = agentos_audit::verify_chain_from(&records, start.sequence, &start.hash);

        let next = match records.last() {
            Some(last) if verification.is_intact() => AuditCheckpoint {
                sequence: last.sequence,
                hash: last.hash.clone(),
            },
            _ => start,
        };
        Ok((verification, next))
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

/// Where a verification of the audit chain got to.
///
/// Held by a client between calls to [`Runtime::verify_audit_from`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditCheckpoint {
    /// The last record verified; zero before any.
    pub sequence: u64,
    /// Its hash, which the next record must name as its predecessor.
    pub hash: String,
}

impl AuditCheckpoint {
    /// Before the first record.
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            sequence: 0,
            hash: agentos_audit::GENESIS_HASH.to_owned(),
        }
    }
}

/// The engine for a compiled policy, or one that denies everything.
///
/// An agent with no stored policy gets [`DenyAllEngine`]. Absence of a policy
/// must never mean absence of restriction.
fn engine_from(policy: Option<Policy>) -> Arc<dyn PermissionEngine> {
    match policy {
        None => Arc::new(DenyAllEngine),
        Some(policy) => Arc::new(PolicyEngine::new(policy)),
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
