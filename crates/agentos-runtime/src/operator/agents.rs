//! Agents and their policies: creating one, switching it on or off, and
//! replacing what it is allowed to do.

use agentos_core::agent::{Agent, AgentStatus, ModelConfig};
use agentos_core::event::AgentEvent;
use agentos_core::ids::AgentId;
use agentos_permissions::PolicyDocument;

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
}
