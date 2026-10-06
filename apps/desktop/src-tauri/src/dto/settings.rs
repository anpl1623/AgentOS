//! Tools, providers and the settings screen.

use agentos_core::tool::ToolMetadata;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A tool, as the catalogue and the agent editor show it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ToolView {
    /// Fully-qualified name.
    pub name: String,
    /// Domain half of the name.
    pub domain: String,
    /// What it does, in the words the model sees.
    pub description: String,
    /// Baseline risk.
    pub risk: String,
    /// Whether its output can be attacker-controlled.
    ///
    /// The single most useful thing to know when deciding what to grant, so it
    /// is surfaced rather than buried in the description.
    pub returns_untrusted_data: bool,
}

impl From<&ToolMetadata> for ToolView {
    fn from(metadata: &ToolMetadata) -> Self {
        Self {
            name: metadata.name.clone(),
            domain: metadata.domain().to_owned(),
            description: metadata.description.clone(),
            risk: metadata.risk.as_str().to_owned(),
            returns_untrusted_data: metadata.returns_untrusted_data,
        }
    }
}

/// A model provider and whether it can be used.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ProviderView {
    /// Identifier.
    pub id: String,
    /// Whether a credential is available.
    pub configured: bool,
    /// A redacted hint, never the key.
    pub hint: Option<String>,
    /// Which store supplied it.
    pub source: Option<String>,
    /// Guidance when it is not configured.
    pub note: String,
}

/// The settings screen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SettingsView {
    /// Where AgentOS keeps its state.
    pub data_dir: String,
    /// Agent workspaces.
    pub workspace: String,
    /// The database file.
    pub database: String,
    /// Whether the OS keychain is usable here.
    pub keychain_available: bool,
    /// Why not, when it is not.
    pub keychain_reason: Option<String>,
    /// Providers and their credential status.
    pub providers: Vec<ProviderView>,
    /// The browser executable that will be driven, if one was found.
    pub browser_path: Option<String>,
    /// Guidance when no browser is installed.
    pub browser_hint: Option<String>,
    /// Every tool an agent can be granted.
    pub tools: Vec<ToolView>,
}
