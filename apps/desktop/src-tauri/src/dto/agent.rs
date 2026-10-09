//! Agents, their policies, and the input that creates one.

use agentos_core::agent::Agent;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{TaskSummary, at};

/// An agent, as the list and dashboard show it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AgentSummary {
    /// Identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Provider identifier.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// `enabled` or `disabled`.
    pub status: String,
    /// Tools this agent has been given.
    pub tools: Vec<String>,
    /// Model turns allowed per run.
    pub max_steps: u32,
    /// When it was created.
    pub created_at: String,
}

impl From<&Agent> for AgentSummary {
    fn from(agent: &Agent) -> Self {
        Self {
            id: agent.id.to_string(),
            name: agent.name.clone(),
            provider: agent.model.provider.clone(),
            model: agent.model.model.clone(),
            status: agent.status.as_str().to_owned(),
            tools: agent.enabled_tools.clone(),
            max_steps: agent.max_steps,
            created_at: at(&agent.created_at),
        }
    }
}

/// An agent with everything the detail screen needs.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AgentDetail {
    /// The summary fields.
    pub summary: AgentSummary,
    /// The agent's system instructions.
    pub instructions: String,
    /// Its policy, if one is stored.
    pub policy: Option<PolicyView>,
    /// Recent tasks, newest first.
    pub recent_tasks: Vec<TaskSummary>,
    /// Where relative paths resolve for this agent.
    pub workspace: String,
}

/// A policy, summarised so the interface can show what it grants without
/// reimplementing the policy engine.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct PolicyView {
    /// The YAML source, exactly as the operator wrote it.
    pub document: String,
    /// Incremented on every save.
    #[ts(type = "number")]
    pub version: i64,
    /// What happens when no rule matches.
    pub default_effect: String,
    /// Global risk ceiling, if set.
    pub max_risk: Option<String>,
    /// Whether reading untrusted data raises the approval bar.
    pub taint_enabled: bool,
    /// The risk level at which that escalation begins.
    pub taint_threshold: String,
    /// One line per rule, in the engine's own words.
    pub rules: Vec<String>,
}

/// What the interface sends to create an agent.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CreateAgentInput {
    /// Unique name.
    pub name: String,
    /// System instructions.
    pub instructions: String,
    /// Provider identifier.
    pub provider: String,
    /// Model identifier.
    pub model: String,
    /// Base URL override, for OpenAI-compatible endpoints.
    pub base_url: Option<String>,
    /// Whether this model can be shown images. `null` takes the provider's
    /// default, which is off for a local endpoint whose model is unknown.
    pub vision: Option<bool>,
    /// Tools to grant.
    pub tools: Vec<String>,
}

/// The outcome of checking a policy document without installing it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct PolicyCheck {
    /// Whether it compiles.
    pub valid: bool,
    /// Why not, when it does not.
    pub error: Option<String>,
    /// What it grants, when it does.
    pub summary: Option<PolicyView>,
}
