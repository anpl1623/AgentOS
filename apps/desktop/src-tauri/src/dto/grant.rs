//! What an agent's policy actually lets its granted tools do.

use agentos_runtime::{CapabilityGrant, ToolGrant};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One granted tool, and how far its policy reaches for each capability the
/// tool declares.
///
/// Granting a tool and permitting what it does are separate acts, and an agent
/// editor that shows only the first invites the belief that a ticked box is a
/// permission. This is the second, computed by the permission engine from the
/// compiled policy, honouring each rule's effect, rather than by matching rule
/// text in the interface.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ToolGrantView {
    /// Fully-qualified tool name.
    pub tool: String,
    /// Whether this installation has the tool. A tool granted by name and
    /// missing from the registry can never be called, whatever the policy
    /// says, and has no capabilities to report.
    pub registered: bool,
    /// The least of its capabilities' reaches: a call needs every capability
    /// it plans, so a tool reaches no further than its weakest one.
    pub reach: String,
    /// One entry per capability the tool's manifest declares.
    pub capabilities: Vec<CapabilityReachView>,
}

impl From<&ToolGrant> for ToolGrantView {
    fn from(grant: &ToolGrant) -> Self {
        Self {
            tool: grant.tool.clone(),
            registered: grant.registered,
            reach: grant.weakest().as_str().to_owned(),
            capabilities: grant
                .capabilities
                .iter()
                .map(CapabilityReachView::from)
                .collect(),
        }
    }
}

/// How far a policy lets one capability reach.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CapabilityReachView {
    /// The capability, as a policy rule names it: `browser.vision`, not the
    /// tool that plans it.
    pub capability: String,
    /// `allowed` for every resource; `scoped`, allowed or asked only for some;
    /// `asks`, never allowed outright; `denied`, no rule can allow or ask.
    pub reach: String,
}

impl From<&CapabilityGrant> for CapabilityReachView {
    fn from(grant: &CapabilityGrant) -> Self {
        Self {
            capability: grant.capability.clone(),
            reach: grant.reach.as_str().to_owned(),
        }
    }
}
