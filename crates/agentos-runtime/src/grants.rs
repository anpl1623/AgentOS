//! What each of an agent's tools may actually do under its policy.
//!
//! An agent's tool list is what it may ask for, and its policy is what asking
//! gets it. This puts the two side by side, per capability each tool declares,
//! using the policy the agent's next run would be held to. The reading itself
//! is [`agentos_permissions::Policy::reach`]; this module only supplies the
//! tools and the policy.

use agentos_core::ids::AgentId;
use agentos_core::permission::Capability;
use agentos_permissions::Reach;

use crate::{Runtime, RuntimeError};

/// One granted tool, and how far its policy lets each of its capabilities
/// reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGrant {
    /// The tool's name, as the agent was given it.
    pub tool: String,
    /// Whether this installation has the tool at all. A tool granted by name
    /// and missing from the registry can never be called, whatever the policy
    /// says, and has no manifest to report.
    pub registered: bool,
    /// One entry per capability the tool's manifest declares, by name.
    pub capabilities: Vec<CapabilityGrant>,
}

/// How far one capability reaches under the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityGrant {
    /// The capability, as `domain.action`.
    pub capability: String,
    /// What the policy allows it.
    pub reach: Reach,
}

impl ToolGrant {
    /// The most this tool can do without anyone being asked, across its
    /// capabilities: a call needs every capability it plans, so a tool is only
    /// as reachable as its least reachable capability.
    #[must_use]
    pub fn weakest(&self) -> Reach {
        self.capabilities
            .iter()
            .map(|grant| grant.reach)
            .max()
            .unwrap_or(Reach::Denied)
    }
}

impl Runtime {
    /// For each tool the agent has been given, how far its policy lets each
    /// capability the tool declares reach.
    ///
    /// Capabilities are listed by [`agentos_core::tool::ToolMetadata::capability_names`],
    /// the names a policy author writes rules against, so a tool that needs
    /// more than its own name says — `browser.screenshot` needs
    /// `browser.vision` — reports that too. An agent with no stored policy is
    /// denied everything, as its runs are.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the agent does not exist, and
    /// [`RuntimeError::Policy`] if its stored policy no longer compiles.
    pub async fn grant_report(&self, agent_id: AgentId) -> Result<Vec<ToolGrant>, RuntimeError> {
        let agent = self.database.agents().get(agent_id).await?;
        let policy = self.policy_for(agent.id).await?;

        Ok(agent
            .enabled_tools
            .iter()
            .map(|name| {
                let Some(tool) = self.registry.get(name) else {
                    return ToolGrant {
                        tool: name.clone(),
                        registered: false,
                        capabilities: Vec::new(),
                    };
                };
                let metadata = tool.metadata();
                let capabilities = metadata
                    .capability_names()
                    .into_iter()
                    .map(|capability| {
                        let reach = match (
                            &policy,
                            declared(&metadata.required_capabilities, &capability),
                        ) {
                            (Some(policy), Some(declared)) => policy.reach(declared, metadata.risk),
                            _ => Reach::Denied,
                        };
                        CapabilityGrant { capability, reach }
                    })
                    .collect();
                ToolGrant {
                    tool: name.clone(),
                    registered: true,
                    capabilities,
                }
            })
            .collect())
    }
}

/// The declared capability a manifest name stands for.
///
/// Looked up rather than parsed back out of the name, so a domain or action
/// containing a dot cannot be split in the wrong place.
fn declared<'a>(declared: &'a [Capability], name: &str) -> Option<&'a Capability> {
    declared
        .iter()
        .find(|capability| capability.qualified_name() == name)
}
