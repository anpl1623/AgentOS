//! What each tool has been doing.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One tool's calls over a window, with the catalogue facts that frame them.
///
/// The buckets are not exclusive: `under_taint` and `approvals` cut across the
/// outcomes. `executed`, `denied`, `approval_denied` and `failed` are each a
/// count of calls that ended that way.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ToolUsageView {
    /// The tool.
    pub tool: String,
    /// Every call in the window, however it ended.
    #[ts(type = "number")]
    pub calls: u64,
    /// Calls that ran.
    #[ts(type = "number")]
    pub executed: u64,
    /// Calls the policy refused.
    #[ts(type = "number")]
    pub denied: u64,
    /// Calls refused at the approval step: by a person, or by the run's
    /// approval budget without anyone being asked.
    #[ts(type = "number")]
    pub approval_denied: u64,
    /// Calls that ran and failed.
    #[ts(type = "number")]
    pub failed: u64,
    /// Calls made after the run had read untrusted data.
    #[ts(type = "number")]
    pub under_taint: u64,
    /// Calls that waited on a person.
    #[ts(type = "number")]
    pub approvals: u64,
    /// Time spent in the tool across every call.
    #[ts(type = "number")]
    pub total_duration_ms: u64,
    /// The most recent call in the window.
    pub last_used_at: Option<String>,
    /// Baseline risk, when the tool is still in the catalogue.
    ///
    /// `None` for a tool that has calls on record but is no longer offered.
    pub risk: Option<String>,
    /// Whether the catalogue says the tool reads the world outside the runtime.
    pub returns_untrusted_data: bool,
}
