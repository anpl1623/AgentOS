//! Delegation: one agent asking another to do part of its work.
//!
//! An agent delegates by calling `agent.delegate`, a tool like any other: it
//! is planned, authorised against the policy and, where the policy says so,
//! put to a person before anything happens. The capability is
//! `runtime.delegate`, scoped to the name of the agent asked, so a policy
//! names which agents may be asked. What the agent asked may then do is not
//! that grant's to say.
//!
//! # What a child is given
//!
//! Nothing its delegator did not have. A child run is evaluated by the
//! intersection of its own policy and every policy above it, so it can do only
//! what all of them permit and the strictest answer governs each call. It
//! shares its delegator's taint tracker, so a child of a run that has read a
//! hostile page starts tainted and a page the child reads taints the parent.
//! It spends from the one step budget the root run was granted. Its approvals
//! go to the same operator gate its delegator's do, wrapped for the child's
//! own run and naming the chain of agents that asked, so under the
//! scheduler's deny-all gate a child is refused exactly what its parent would
//! be. Its cancellation token is a child of its delegator's, so stopping a run
//! stops everything it started.
//!
//! # Why it lives in the runtime
//!
//! The tool has to prepare and drive a run, which only [`Runtime`] can do, and
//! the runtime depends on the tools crate rather than the reverse. It is
//! registered with the other tools all the same, and like the browser tools it
//! holds a pool keyed by run, the [`DelegationBroker`], which the runtime fills
//! as each run is prepared and empties as each run ends.
//!
//! # Order of events
//!
//! Every refusal is decided while the call is planned, before authorisation
//! and before any side effect. Once authorised, each child is recorded before
//! it starts: its task, then its `delegations` row, then its run, and only
//! then the audit record that seals the edge between the two runs. A
//! delegation that cannot be recorded is not performed. A child that fails is
//! a result its delegator reads and re-plans around, exactly as it reads a
//! denied approval, never an error that ends the delegator's run.
//!
//! [`Runtime`]: crate::Runtime

mod broker;
mod child;
mod tool;

pub use broker::{DelegationBroker, ParentRunHandle};
pub use tool::{DelegateArgs, DelegateRequest, DelegateTool};

/// The tool's name.
pub const TOOL_NAME: &str = "agent.delegate";

/// The capability action a delegation needs, in the `runtime` domain.
pub const DELEGATE: &str = "delegate";
