//! The [`Runtime`] itself: what it holds, how it is opened, and what it
//! answers about the installation without starting anything.
//!
//! The rest of what a runtime does is in `impl Runtime` blocks of their own,
//! each beside what it concerns. Below this module: assembling a run
//! (`prepare`), driving and stopping it (`run`), clearing up after a process
//! that died mid-run (`reap`), reading back what a run did (`trace`) and
//! proving the record of it intact (`verify`). Elsewhere: the operator's
//! changes in [`crate::operator`], the scheduler's preference in
//! [`crate::scheduler`] and the grant report in [`crate::grants`]. The tool
//! catalogue a runtime is given is composed in `crate::registry`.

mod prepare;
mod reap;
mod run;
mod trace;
mod verify;

use std::sync::Arc;

use agentos_audit::AuditLog;
use agentos_core::agent::Agent;
use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::Task;
use agentos_integrations::account::AccountDirectory;
use agentos_permissions::{DenyAllEngine, PermissionEngine, Policy, PolicyDocument, PolicyEngine};
use agentos_persistence::Database;
use agentos_secrets::{ChainSecretStore, SecretStore};
use agentos_tools::ToolRegistry;

use crate::config::{ProviderFactory, RuntimeConfig, SecretBackedProviderFactory};
use crate::delegation::DelegationBroker;
use crate::error::RuntimeError;
use crate::integrations::SqliteAccountDirectory;
use crate::liveness;
use crate::registry::runtime_registry;

pub use prepare::{DELEGATED_TASK, PreparedRun};
pub use trace::{DelegationNode, RunTrace};
pub use verify::AuditCheckpoint;

use run::LiveRuns;

/// Everything a running AgentOS installation needs.
#[derive(Debug, Clone)]
pub struct Runtime {
    pub(crate) config: RuntimeConfig,
    pub(crate) database: Database,
    pub(crate) audit: Arc<AuditLog>,
    pub(crate) registry: Arc<ToolRegistry>,
    /// The bound integration accounts, over this runtime's own database. The
    /// registry's integration tools hold the same directory.
    accounts: Arc<SqliteAccountDirectory>,
    pub(crate) secrets: Arc<dyn SecretStore>,
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
    ///
    /// [`compose_registry`]: crate::compose_registry
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
    ///
    /// [`compose_registry_with`]: crate::compose_registry_with
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
    pub(crate) async fn policy_for(
        &self,
        agent_id: AgentId,
    ) -> Result<Option<Policy>, RuntimeError> {
        match self.database.agents().policy(agent_id).await? {
            None => Ok(None),
            Some(stored) => Ok(Some(
                PolicyDocument::from_yaml(&stored.document)?.compile()?,
            )),
        }
    }

    /// Every schedule, newest first.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] on failure.
    pub async fn schedules(&self) -> Result<Vec<agentos_core::schedule::Schedule>, RuntimeError> {
        Ok(self.database.schedules().list().await?)
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
