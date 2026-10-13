//! Delegation: one run asking another agent to do part of its work.
//!
//! A delegation is an edge between two runs, and the interface reads it from
//! both ends: a supervisor's trace lists the work it handed out, and a worker's
//! trace says who handed it in. Neither end is complete without the other,
//! because a worker's run read on its own looks like a task nobody asked for.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One delegation, as either of the two runs it joins shows it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DelegationView {
    /// Identity.
    pub id: String,
    /// The run that asked.
    pub parent_run_id: String,
    /// The agent that asked.
    ///
    /// A worker's trace names its supervisor with this, so the run that
    /// started it is a link and not an identifier to look up.
    pub parent_agent: String,
    /// The agent that was asked.
    pub child_agent: String,
    /// What it was asked to do, in the asking agent's words.
    pub objective: String,
    /// Where the delegation is: `requested`, `running`, or how it ended.
    pub state: String,
    /// The run doing the work, once one was prepared.
    ///
    /// Absent for a delegation refused or failed before its run existed. The
    /// row is written first, so such a delegation is still on record.
    pub child_run_id: Option<String>,
    /// Model turns the child run has taken.
    ///
    /// Spent from the same budget as the asking run's, which is why the two
    /// are shown side by side.
    pub steps: u32,
    /// Whether the child run has read untrusted data.
    ///
    /// The tracker is shared between the two runs, so this is also taint the
    /// asking run now carries.
    pub tainted: bool,
    /// How far below the root run the child is: 1 for a direct delegate.
    pub depth: u8,
    /// When the delegation was recorded.
    pub requested_at: String,
    /// How long it took, once it ended.
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

/// A run and every run delegated beneath it, depth first.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DelegationTreeView {
    /// The run at this node.
    pub run_id: String,
    /// The agent performing it.
    pub agent: String,
    /// What it was asked to do.
    pub objective: String,
    /// Where the run is in the state machine.
    pub state: String,
    /// Whether it has read untrusted data.
    pub tainted: bool,
    /// Model turns it has taken.
    pub steps: u32,
    /// The runs it delegated to, in the order it asked.
    pub children: Vec<DelegationTreeView>,
}
