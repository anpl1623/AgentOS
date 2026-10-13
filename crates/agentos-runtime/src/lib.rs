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
pub mod budget;
pub mod config;
pub mod credentials;
pub mod delegation;
pub mod error;
pub mod gate;
pub mod grants;
pub mod integrations;
mod liveness;
pub mod operator;
pub mod prompt;
pub mod scheduler;
pub mod state;

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::agent::Agent;
use agentos_core::delegation::{Delegation, DelegationRefusal, DelegationState};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::task::{Task, TaskRun, TaskState, TaskStatus, TaskTrigger};
use agentos_integrations::account::{AccountDirectory, InMemoryDirectory};
use agentos_permissions::{
    ApprovalPolicy, DenyAllEngine, IntersectionEngine, PermissionEngine, Policy, PolicyDocument,
    PolicyEngine,
};
use agentos_persistence::Database;
use agentos_secrets::{ChainSecretStore, SecretStore};
use agentos_tools::egress::{AddressPolicy, Egress};
use agentos_tools::{ApprovalGate, TaintTracker, ToolContext, ToolPipeline, ToolRegistry};
use tokio_util::sync::CancellationToken;

pub use agent_loop::{AgentLoop, RunOutcome};
pub use agentos_permissions::Reach;
pub use agentos_tools::ToolRegistry as Registry;
pub use budget::RunBudget;
pub use config::{
    DelegationLimits, FixedProviderFactory, ProviderFactory, RuntimeConfig,
    SecretBackedProviderFactory, build_provider,
};
pub use credentials::SecretStoreResolver;
pub use delegation::{DelegateTool, DelegationBroker, ParentRunHandle};
pub use error::RuntimeError;
pub use gate::RunApprovalGate;
pub use grants::{CapabilityGrant, ToolGrant};
pub use integrations::SqliteAccountDirectory;
pub use scheduler::{
    MIN_TICK_SECONDS, Scheduler, SchedulerOptions, SchedulerPreference, SchedulerTransition,
    TickReport,
};
pub use state::RunStateMachine;

/// Why a task created by a delegation cannot be started on its own.
pub const DELEGATED_TASK: &str = "this task was delegated by another agent's run and is carried \
     out only as part of that run, under its policy as well as this agent's own, so it cannot be \
     started on its own; to try it again, start the work from the agent that delegated it";

/// Everything a running AgentOS installation needs.
#[derive(Debug, Clone)]
pub struct Runtime {
    config: RuntimeConfig,
    database: Database,
    audit: Arc<AuditLog>,
    registry: Arc<ToolRegistry>,
    /// The bound integration accounts, over this runtime's own database. The
    /// registry's integration tools hold the same directory.
    accounts: Arc<SqliteAccountDirectory>,
    secrets: Arc<dyn SecretStore>,
    providers: Arc<dyn ProviderFactory>,
    /// Cancellation tokens for runs currently in flight, so the operator can
    /// stop an agent that is already working.
    running: Arc<LiveRuns>,
    /// Tells other processes on this data directory that this one drives
    /// runs, so none of them reaps a run that is still alive here. Absent for
    /// an in-memory runtime, whose database no other process can see.
    runs_lock: Option<Arc<liveness::RunsLock>>,
    /// The runs that may delegate. The registry's `agent.delegate` holds the
    /// same broker, so every clone of this runtime registers runs where the
    /// tool will look for them.
    broker: Arc<DelegationBroker>,
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
        let accounts = Arc::new(SqliteAccountDirectory::new(database.clone()));
        let broker = Arc::new(DelegationBroker::new(config.delegation));

        Ok(Self {
            database,
            audit,
            registry: runtime_registry(&config, accounts.clone(), broker.clone()),
            accounts,
            providers: Arc::new(SecretBackedProviderFactory::new(secrets.clone())),
            secrets,
            runs_lock: Some(Arc::new(liveness::RunsLock::in_directory(&config.data_dir))),
            config,
            running: Arc::new(LiveRuns::default()),
            broker,
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
        // Over the database just opened: an in-memory one cannot be found
        // again by a path.
        let accounts = Arc::new(SqliteAccountDirectory::new(database.clone()));
        let broker = Arc::new(DelegationBroker::new(config.delegation));

        Ok(Self {
            database,
            audit,
            registry: runtime_registry(&config, accounts.clone(), broker.clone()),
            accounts,
            providers: Arc::new(SecretBackedProviderFactory::new(secrets.clone())),
            secrets,
            config,
            running: Arc::new(LiveRuns::default()),
            runs_lock: None,
            broker,
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

    /// The bound integration accounts, as the registry's integration tools
    /// read them.
    ///
    /// For composing a registry around this runtime's accounts, as
    /// [`compose_registry`] does. Binding and unbinding go through
    /// [`Self::bind_integration`] and [`Self::unbind_integration`], which
    /// record them; this is for reading.
    #[must_use]
    pub fn integration_accounts(&self) -> Arc<dyn AccountDirectory> {
        self.accounts.clone()
    }

    /// The secret store.
    #[must_use]
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    /// The broker this runtime registers its runs with.
    ///
    /// For composing a registry whose `agent.delegate` can perform this
    /// runtime's delegations, as [`compose_registry_with`] does. A registry
    /// composed around any other broker offers the tool and refuses every
    /// call to it.
    #[must_use]
    pub fn delegation_broker(&self) -> Arc<DelegationBroker> {
        self.broker.clone()
    }

    /// Replace the tool registry. Used by tests and, later, by plugin loading.
    ///
    /// Delegation keeps working only if the new registry's `agent.delegate`
    /// was built around [`Self::delegation_broker`].
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
    /// having written nothing: no run row, no audit record, no model request.
    /// Everything that can refuse for a reason of its own is checked first, so
    /// a refusal leaves the task as it was rather than claimed by nobody.
    ///
    /// A task created by a delegation is refused. It runs only through
    /// [`Self::prepare_delegated_run`], under every policy above it, on the
    /// tree's step budget and within its depth; started here it would be a
    /// root run held to its own policy alone, so retrying a failed child would
    /// grant what the delegation withheld. The parent link is checked as well
    /// as the delegation's row because the task is written first, and a task
    /// is given a parent by nothing but delegation.
    async fn prepare_run(
        &self,
        task: &Task,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
    ) -> Result<PreparedRun, RuntimeError> {
        if task.parent_task_id.is_some()
            || self
                .database
                .delegations()
                .find_by_child_task(task.id)
                .await?
                .is_some()
        {
            return Err(RuntimeError::Rejected(DELEGATED_TASK.to_owned()));
        }
        let agent = self.database.agents().get(task.agent_id).await?;
        self.prepare(task, agent, approvals, cancel, None).await
    }

    /// Assemble the run that carries out a delegation, as [`Self::start_task`]
    /// assembles any other, under everything its delegator is held to.
    ///
    /// The differences are exactly what delegation must not widen. The run is
    /// evaluated by an [`IntersectionEngine`] over the delegator's engine and
    /// the child's own policy, so it may do only what both permit, and since
    /// the delegator's engine is itself an intersection for a delegated run,
    /// a grandchild answers to every policy up to the root. It shares the
    /// delegator's taint tracker and step budget rather than starting its own,
    /// and its token is a child of the delegator's. Its approvals go to the
    /// operator's gate the delegator was given, wrapped afresh around the
    /// child's own state machine, naming the chain of agents that asked, and
    /// held to the stricter of the two policies' approval budgets. Its run is
    /// registered with the broker one level deeper, so that it can delegate in
    /// turn under the same limits.
    ///
    /// The delegation's task must already be written, as a child of the
    /// delegator's task and for `child_agent`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Delegation`] if the delegator may not delegate any
    /// deeper, [`RuntimeError::Rejected`] if the task is not the delegation's,
    /// and otherwise as [`Self::start_task`].
    pub async fn prepare_delegated_run(
        &self,
        parent: &ParentRunHandle,
        delegation: &Delegation,
        child_agent: Agent,
    ) -> Result<PreparedRun, RuntimeError> {
        let limit = self.config.delegation.max_depth;
        if parent.depth >= limit {
            return Err(DelegationRefusal::DepthExceeded { limit }.into());
        }
        let task = self.database.tasks().get(delegation.child_task_id).await?;
        if task.agent_id != child_agent.id
            || delegation.child_agent_id != child_agent.id
            || task.parent_task_id != Some(parent.task_id)
            || delegation.parent_run_id != parent.run_id
        {
            return Err(RuntimeError::Rejected(
                "that task was not created for this delegation".to_owned(),
            ));
        }
        self.prepare(
            &task,
            child_agent,
            parent.approvals.clone(),
            parent.cancel.child_token(),
            Some(parent),
        )
        .await
    }

    /// What [`Self::prepare_run`] and [`Self::prepare_delegated_run`] share.
    ///
    /// `parent` is the delegator, for a delegated run, and `None` for a root
    /// run. Every difference between the two is decided here, in one place.
    async fn prepare(
        &self,
        task: &Task,
        agent: Agent,
        approvals: Arc<dyn ApprovalGate>,
        cancel: CancellationToken,
        parent: Option<&ParentRunHandle>,
    ) -> Result<PreparedRun, RuntimeError> {
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
        let own_budget = policy
            .as_ref()
            .map_or_else(ApprovalPolicy::default, |policy| policy.approvals)
            .max_per_run;
        let own_engine = engine_from(policy);
        let (engine, approval_budget): (Arc<dyn PermissionEngine>, _) = match parent {
            None => (own_engine, own_budget),
            Some(parent) => (
                Arc::new(IntersectionEngine::new(vec![
                    parent.engine.clone(),
                    own_engine,
                ])),
                stricter_budget(parent.approval_budget, own_budget),
            ),
        };

        let workspace = self.config.workspace_for(&agent.name);
        std::fs::create_dir_all(&workspace).map_err(|source| {
            RuntimeError::io(format!("creating {}", workspace.display()), source)
        })?;

        let mut run = self.next_run(task.id).await?;
        if let Some(parent) = parent {
            // Written into the row from the start, so a delegated run is never
            // on record as clean for even the moment before its first turn.
            run.tainted |= parent.taint.is_tainted();
            for source in parent.taint.sources() {
                if !run.taint_sources.contains(&source) {
                    run.taint_sources.push(source);
                }
            }
        }
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

        self.running.lock().insert(run.id, cancel.clone());
        // From here on, however the run ends or is abandoned, it is forgotten.
        let live = LiveRun {
            runs: self.running.clone(),
            broker: self.broker.clone(),
            run_id: run.id,
        };

        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Idle,
            self.database.clone(),
            self.audit.clone(),
        ));

        let chain = match parent {
            None => vec![agent.name.clone()],
            Some(parent) => {
                let mut chain = parent.chain.clone();
                chain.push(agent.name.clone());
                chain
            }
        };
        let gate: Arc<dyn ApprovalGate> = Arc::new(
            RunApprovalGate::new(approvals.clone(), self.database.clone(), machine.clone())
                .with_budget(approval_budget)
                // A run nobody delegated to names no chain: the card already
                // says which agent is asking.
                .with_delegation_chain(if parent.is_some() {
                    chain.clone()
                } else {
                    Vec::new()
                }),
        );

        let pipeline = ToolPipeline::new(
            self.registry.clone(),
            engine.clone(),
            gate,
            self.audit.clone(),
        );

        // The store's own resolver. The pipeline narrows it, call by call, to
        // the credentials each authorised plan named, and keeps the ledger
        // that redaction and the audit record are made from.
        let context = ToolContext::new(agent.id, task.id, run.id, workspace)
            .with_credentials(Arc::new(SecretStoreResolver::new(self.secrets.clone())));

        let (taint, budget) = match parent {
            None => (
                Arc::new(TaintTracker::seeded(run.tainted, run.taint_sources.clone())),
                RunBudget::of(agent.max_steps),
            ),
            Some(parent) => (parent.taint.clone(), parent.budget.clone()),
        };

        let mut ancestry = parent.map_or_else(Vec::new, |parent| parent.ancestry.clone());
        ancestry.push(agent.id);
        self.broker.register(
            run.id,
            Arc::new(ParentRunHandle {
                runtime: self.clone(),
                agent: agent.clone(),
                run_id: run.id,
                task_id: task.id,
                engine,
                approvals,
                approval_budget,
                taint: taint.clone(),
                budget: budget.clone(),
                cancel: cancel.clone(),
                depth: parent.map_or(0, |parent| parent.depth.saturating_add(1)),
                ancestry,
                chain,
                children_started: std::sync::atomic::AtomicU16::new(0),
            }),
        );

        Ok(PreparedRun {
            objective: task.objective.clone(),
            agent,
            run,
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            delegated: parent.is_some(),
            cancel,
            live,
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
    ///
    /// The run is forgotten as live, and as able to delegate, however it
    /// ends: by the guard the prepared run carries, which goes when this
    /// returns or when this future is dropped unfinished. One that ended in a
    /// runtime error, or whose driver was dropped, is not being driven by
    /// anyone, and left in the live set it would hold a scheduler slot, be
    /// spared by the reaper and keep its runtime alive in the broker, for the
    /// life of the process.
    async fn drive(&self, prepared: PreparedRun) -> Result<RunOutcome, RuntimeError> {
        let PreparedRun {
            objective,
            agent,
            run,
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            delegated,
            cancel,
            live,
        } = prepared;
        let mut agent_loop = AgentLoop::new(
            agent,
            run,
            self.database.clone(),
            provider,
            pipeline,
            machine,
            context,
            taint,
            budget,
            cancel,
        );
        if delegated {
            agent_loop = agent_loop.delegated();
        }

        let outcome = agent_loop.run(&objective).await;
        drop(live);
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
    /// inside it are watching. Every run it delegated to watches a token
    /// descended from that one, so they stop with it.
    ///
    /// The run stays live until it has actually stopped, so that what counts
    /// live runs, the scheduler among them, does not count a run that is
    /// still winding down as already gone.
    pub async fn cancel_run(&self, run_id: TaskRunId) -> bool {
        match self.running.lock().get(&run_id) {
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
    ///
    /// Every live run, delegated runs among them: a run that has asked three
    /// others to work is four runs using a model at once, and anything that
    /// limits concurrency must see four.
    pub async fn running_runs(&self) -> Vec<TaskRunId> {
        self.running.lock().keys().copied().collect()
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
            // A delegated run's edge would otherwise say it is still running
            // for as long as the record is kept.
            if let Some(edge) = self
                .database
                .delegations()
                .find_by_child_run(run.id)
                .await?
                && !edge.state.is_terminal()
            {
                self.database
                    .delegations()
                    .complete(edge.id, DelegationState::Failed, run.steps_taken)
                    .await?;
            }
            // And so would the edges it opened, which nothing is left to
            // conclude. One still `requested` names no run, so it is found
            // only from this end: the process died before the child's run was
            // attached, or before a refusal to prepare one was written.
            self.close_abandoned_delegations(run.id).await?;
            // Whatever the tools still hold for it is released, as the loop
            // would have released it had the run ended normally.
            self.registry.end_run(run.id).await;
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

    /// End every delegation an abandoned run left open.
    ///
    /// A child run that has itself ended keeps its ending: the work was done
    /// or stopped, whatever became of the run that asked for it. Every other
    /// edge failed, since no run is left to carry it out.
    async fn close_abandoned_delegations(&self, parent: TaskRunId) -> Result<(), RuntimeError> {
        let delegations = self.database.delegations();
        for edge in delegations.list_open_for_parent_run(parent).await? {
            let child = match edge.child_run_id {
                Some(id) => self.database.runs().find(id).await?,
                None => None,
            };
            let (state, steps) = match &child {
                Some(run) => (
                    match run.state {
                        TaskState::Completed => DelegationState::Completed,
                        TaskState::Cancelled => DelegationState::Cancelled,
                        _ => DelegationState::Failed,
                    },
                    run.steps_taken,
                ),
                None => (DelegationState::Failed, 0),
            };
            delegations.complete(edge.id, state, steps).await?;
        }
        Ok(())
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
            delegations: self
                .database
                .delegations()
                .list_for_parent_run(run_id)
                .await?,
            delegated_by: self
                .database
                .delegations()
                .find_by_child_run(run_id)
                .await?,
            run,
        })
    }

    /// The delegation tree below a run, depth first.
    ///
    /// Read to at most the configured depth plus one level below `run_id`,
    /// and never through the same run twice, so a corrupt row that pointed a
    /// run at its own ancestor cannot make the read go on forever. A
    /// delegation whose run was never prepared has no node of its own; it is
    /// in [`RunTrace::delegations`] for the run that asked.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure, or if `run_id` does not exist.
    pub async fn delegation_tree(&self, run_id: TaskRunId) -> Result<DelegationNode, RuntimeError> {
        let delegation = self
            .database
            .delegations()
            .find_by_child_run(run_id)
            .await?;
        let levels = usize::from(self.config.delegation.max_depth) + 1;
        let mut seen = std::collections::HashSet::from([run_id]);
        self.delegation_node(run_id, delegation, levels, &mut seen)
            .await
    }

    async fn delegation_node(
        &self,
        run_id: TaskRunId,
        delegation: Option<Delegation>,
        levels: usize,
        seen: &mut std::collections::HashSet<TaskRunId>,
    ) -> Result<DelegationNode, RuntimeError> {
        let run = self.database.runs().get(run_id).await?;
        let task = self.database.tasks().get(run.task_id).await?;
        let agent = self.database.agents().get(task.agent_id).await?;

        let mut children = Vec::new();
        if levels > 0 {
            for edge in self
                .database
                .delegations()
                .list_for_parent_run(run_id)
                .await?
            {
                let Some(child_run) = edge.child_run_id else {
                    continue;
                };
                if !seen.insert(child_run) {
                    continue;
                }
                children.push(
                    Box::pin(self.delegation_node(child_run, Some(edge), levels - 1, seen)).await?,
                );
            }
        }

        Ok(DelegationNode {
            run,
            agent_name: agent.name,
            objective: task.objective,
            delegation,
            children,
        })
    }

    /// Verify the audit chain, rehashing every record from genesis.
    ///
    /// A log whose oldest records were removed does not verify: the first
    /// record left must be the first record written.
    ///
    /// A delegation tree needs nothing more. The chain is one per
    /// installation, so it already orders every record of every run in a tree
    /// against every other, and each edge between two runs is sealed inside
    /// the hashed payload of its `agent.delegation.started` record. The record
    /// format is unchanged by delegation, deliberately: changing what is
    /// hashed would mean a migration over a log that cannot be rewritten.
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

/// Build the registry every client gets: the built-in tools, the browser,
/// computer control, the network, the integrations and delegation.
///
/// The composition root owns this so that the CLI and the desktop application
/// cannot end up offering different tools for the same installation — and so
/// that a command listing the catalogue lists the same catalogue an agent is
/// actually given.
///
/// Public and free of side effects: listing the tools should not create a
/// database, launch a browser, or ask macOS for the Accessibility permission.
/// The integrations' accounts are read from the configuration's database the
/// first time a tool asks for one, which listing never does. Its
/// `agent.delegate` belongs to no runtime, so it lists the tool and can
/// perform no delegation; a [`Runtime`] builds its own around its broker.
#[must_use]
pub fn build_registry(config: &RuntimeConfig) -> Arc<ToolRegistry> {
    build_registry_for(
        config,
        Arc::new(SqliteAccountDirectory::at(&config.database_path)),
    )
}

/// The same registry, with the integrations reading `accounts`.
#[must_use]
pub fn build_registry_for(
    config: &RuntimeConfig,
    accounts: Arc<dyn AccountDirectory>,
) -> Arc<ToolRegistry> {
    runtime_registry(config, accounts, unattached_broker(config.delegation))
}

/// What a [`Runtime`] builds for itself, around the database it already has
/// open and the broker it registers its runs with.
fn runtime_registry(
    config: &RuntimeConfig,
    accounts: Arc<dyn AccountDirectory>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    compose_registry_with(
        &Arc::new(agentos_browser::BrowserPool::new(
            agentos_browser::BrowserOptions::new(config.browser_profiles()),
        )),
        accounts,
        broker,
    )
}

/// The same registry again, around a browser pool the caller already holds.
///
/// Only a test needs this: to assert that a run released its browser it has to
/// be looking at the same pool the tools are using, and a second pool would make
/// the assertion pass by being empty. No accounts are bound in it, so an
/// integration tool called through it answers that none is, and its
/// `agent.delegate` refuses every call; [`build_registry_sharing_with`] gives
/// it a runtime's broker.
#[must_use]
pub fn build_registry_sharing(pool: &Arc<agentos_browser::BrowserPool>) -> Arc<ToolRegistry> {
    build_registry_sharing_with(pool, unattached_broker(DelegationLimits::default()))
}

/// [`build_registry_sharing`], with `agent.delegate` performing the
/// delegations of the runtime whose [`Runtime::delegation_broker`] this is.
#[must_use]
pub fn build_registry_sharing_with(
    pool: &Arc<agentos_browser::BrowserPool>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    compose_registry_with(pool, Arc::new(InMemoryDirectory::default()), broker)
}

/// The registry itself, around a browser pool and the integrations' accounts.
///
/// As [`compose_registry_with`], with an `agent.delegate` that belongs to no
/// runtime and so refuses every call. A caller composing a registry for a
/// runtime it will run agents on passes that runtime's broker to
/// [`compose_registry_with`] instead.
#[must_use]
pub fn compose_registry(
    pool: &Arc<agentos_browser::BrowserPool>,
    accounts: Arc<dyn AccountDirectory>,
) -> Arc<ToolRegistry> {
    compose_registry_with(
        pool,
        accounts,
        unattached_broker(DelegationLimits::default()),
    )
}

/// The registry itself, around a browser pool, the integrations' accounts and
/// the broker delegations are performed through.
///
/// This is where every tool is registered, the network and the integrations
/// among them, and the only place: a tool added anywhere else is how the CLI
/// and the desktop end up offering different catalogues. A caller composes one
/// directly only to change the browser — the demonstration runs it headed, so
/// that a human can watch — and passes its runtime's
/// [`Runtime::integration_accounts`] and [`Runtime::delegation_broker`], so
/// that nothing else differs.
/// `network.request` is built here with the strict address policy, public
/// addresses only, and nothing a caller passes can loosen it.
///
/// Each integration is registered once, whatever is bound, holding `accounts`
/// and resolving the account it acts as inside each call. So `agentos tools`
/// lists `github.issues.list` whether or not a token is bound; calling it with
/// none bound fails cleanly, naming `agentos integration add github`; and
/// binding a second account later changes nothing registered here. The
/// address policy a GitHub call connects under is the account's, which only
/// the operator sets when binding it; the transport handed over here is
/// strict, and nothing a tool argument says reaches either.
///
/// `agent.delegate` is registered last, and like every other tool is offered
/// to an agent only if the agent lists it and performs nothing its policy
/// does not grant.
#[must_use]
pub fn compose_registry_with(
    pool: &Arc<agentos_browser::BrowserPool>,
    accounts: Arc<dyn AccountDirectory>,
    broker: Arc<DelegationBroker>,
) -> Arc<ToolRegistry> {
    let mut registry = agentos_tools::standard_registry();
    for tool in agentos_browser::browser_tools(Arc::clone(pool)) {
        registry.register(tool);
    }
    for tool in agentos_computer::build() {
        registry.register(tool);
    }
    for tool in agentos_tools::network::all() {
        registry.register(tool);
    }
    for tool in agentos_integrations::github::build(Egress::new(AddressPolicy::Strict), accounts) {
        registry.register(tool);
    }
    registry.register(Arc::new(DelegateTool::new(broker)));
    Arc::new(registry)
}

/// A broker no runtime registers runs with, for a registry that lists
/// `agent.delegate` without being able to perform a delegation.
fn unattached_broker(limits: DelegationLimits) -> Arc<DelegationBroker> {
    Arc::new(DelegationBroker::new(limits))
}

/// The stricter of two approval budgets, where `None` is no limit.
const fn stricter_budget(first: Option<u32>, second: Option<u32>) -> Option<u32> {
    match (first, second) {
        (Some(first), Some(second)) => Some(if first < second { first } else { second }),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// A run that has been assembled but not started.
///
/// Splitting assembly from execution is what lets a caller learn a run's
/// identity before it finishes — the row exists and the state machine is ready,
/// but no model has been called yet.
pub struct PreparedRun {
    objective: String,
    agent: Agent,
    run: TaskRun,
    provider: agentos_providers::SharedProvider,
    pipeline: ToolPipeline,
    machine: Arc<RunStateMachine>,
    context: ToolContext,
    /// The tracker the loop observes into, shared with the rest of the tree
    /// for a delegated run, so the loop does not build one of its own.
    taint: Arc<TaintTracker>,
    budget: Arc<RunBudget>,
    delegated: bool,
    cancel: CancellationToken,
    /// Forgets the run as live when the run is done with, driven or not.
    live: LiveRun,
}

/// The runs this process is driving, each with the token that stops it.
///
/// Behind a plain mutex, held only for a lookup and never across an await,
/// so that a run can be forgotten from a destructor.
#[derive(Debug, Default)]
struct LiveRuns(std::sync::Mutex<std::collections::HashMap<TaskRunId, CancellationToken>>);

impl LiveRuns {
    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<TaskRunId, CancellationToken>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Forgets a run as live, and as able to delegate, when dropped.
///
/// Made as the run is claimed and carried by it until `drive` returns, so the
/// forgetting cannot be skipped: not by a run that fails to start, nor by a
/// driver dropped part way, as a tool call is when its run is torn down.
struct LiveRun {
    runs: Arc<LiveRuns>,
    broker: Arc<DelegationBroker>,
    run_id: TaskRunId,
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        self.runs.lock().remove(&self.run_id);
        // The tool does this as the run releases its tools, but a registry
        // swapped in without `agent.delegate` never tells it to.
        self.broker.forget(self.run_id);
    }
}

impl PreparedRun {
    /// The run that was written.
    #[must_use]
    pub const fn run_id(&self) -> TaskRunId {
        self.run.id
    }

    /// The task it carries out.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.run.task_id
    }
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
    /// The delegations this run made, in the order it asked.
    pub delegations: Vec<Delegation>,
    /// The delegation that started this run, if another run asked for it.
    pub delegated_by: Option<Delegation>,
}

/// One run in a delegation tree, and the runs it delegated to.
#[derive(Debug, Clone)]
pub struct DelegationNode {
    /// The run.
    pub run: TaskRun,
    /// The agent that performed it.
    pub agent_name: String,
    /// What it was asked to do.
    pub objective: String,
    /// The edge that started it; `None` for the run the tree was read from
    /// when nothing delegated to it.
    pub delegation: Option<Delegation>,
    /// The runs it delegated to, in the order it asked.
    pub children: Vec<DelegationNode>,
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
