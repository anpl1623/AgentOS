//! Agent definition.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::AgentId;

/// Whether an agent may be given work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Available.
    #[default]
    Enabled,
    /// Configured but not accepting work.
    Disabled,
}

impl AgentStatus {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

/// Which model an agent talks to and how.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Provider identifier, e.g. `anthropic`, `openai`, `mock`.
    pub provider: String,
    /// Model identifier as the provider names it.
    pub model: String,
    /// Sampling temperature, when the provider supports it.
    pub temperature: Option<f32>,
    /// Output token ceiling per turn.
    pub max_output_tokens: Option<u32>,
    /// Base URL override, for OpenAI-compatible endpoints such as Ollama.
    pub base_url: Option<String>,
}

impl ModelConfig {
    /// Build a configuration with provider defaults.
    #[must_use]
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            temperature: None,
            max_output_tokens: None,
            base_url: None,
        }
    }
}

/// A persistent agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    /// Identity.
    pub id: AgentId,
    /// Human-readable name, unique per installation.
    pub name: String,
    /// Trusted control-plane instructions. Operator-authored.
    pub instructions: String,
    /// Model configuration.
    pub model: ModelConfig,
    /// Names of the tools this agent may use.
    ///
    /// This is a convenience filter for the model's tool list. It is **not** a
    /// security control — the policy engine is. An agent whose tool list
    /// includes `terminal.exec` still cannot run anything its policy denies.
    pub enabled_tools: Vec<String>,
    /// Whether the agent accepts work.
    pub status: AgentStatus,
    /// Maximum model turns per run.
    pub max_steps: u32,
    /// Free-form metadata for plugins and UI.
    pub metadata: serde_json::Value,
    /// When created.
    pub created_at: Timestamp,
    /// When last modified.
    pub updated_at: Timestamp,
}

/// Default per-run model turn budget.
pub const DEFAULT_MAX_STEPS: u32 = 24;

impl Agent {
    /// Create an enabled agent with default budgets.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        instructions: impl Into<String>,
        model: ModelConfig,
    ) -> Self {
        let now = crate::now();
        Self {
            id: AgentId::new(),
            name: name.into(),
            instructions: instructions.into(),
            model,
            enabled_tools: Vec::new(),
            status: AgentStatus::Enabled,
            max_steps: DEFAULT_MAX_STEPS,
            metadata: serde_json::Value::Null,
            created_at: now,
            updated_at: now,
        }
    }

    /// Grant the agent a set of tools.
    #[must_use]
    pub fn with_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.enabled_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Whether a tool is on this agent's list.
    #[must_use]
    pub fn allows_tool(&self, name: &str) -> bool {
        self.enabled_tools.iter().any(|t| t == name)
    }

    /// Whether the agent may be given work.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.status, AgentStatus::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_agents_are_enabled_with_no_tools() {
        let agent = Agent::new("sales", "Be helpful.", ModelConfig::new("mock", "m"));
        assert!(agent.is_enabled());
        assert!(agent.enabled_tools.is_empty());
        assert!(!agent.allows_tool("filesystem.read"));
    }

    #[test]
    fn tool_list_is_a_filter_not_a_grant() {
        let agent = Agent::new("sales", "", ModelConfig::new("mock", "m"))
            .with_tools(["filesystem.read", "terminal.exec"]);
        assert!(agent.allows_tool("terminal.exec"));
        assert!(!agent.allows_tool("browser.navigate"));
    }
}
