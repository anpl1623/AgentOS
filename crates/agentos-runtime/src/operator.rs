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

use agentos_core::agent::{Agent, AgentStatus, ModelConfig};
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::AgentId;
use agentos_permissions::PolicyDocument;
use agentos_providers::provider_ids;
use agentos_secrets::provider_key;

use crate::{Runtime, RuntimeError};

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
