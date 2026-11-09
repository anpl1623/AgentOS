//! What an operator changes, and the record that they changed it.
//!
//! A policy, an agent's existence, whether it may run, the credentials its
//! model is reached with and the ones its runs may spend at a remote service
//! decide what every later run is able to do. A policy widened five minutes
//! before a bad action and narrowed again afterwards is part of the
//! explanation of that action, so each of these changes is written to the
//! audit chain beside the actions it made possible.
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
//!
//! The same holds for what an agent is told before it plans and for what
//! starts work with nobody present: a memory, a schedule, a queued task and the
//! scheduler itself. Each method here makes exactly one record of its kind.
//!
//! An integration account is bound and unbound here too. It is two changes,
//! a credential and the row that points at it, and each is recorded as
//! itself, so the chain shows a token being stored and an account being bound
//! as the two acts they are.
//!
//! Each kind of change is a module of its own: agents and their policies,
//! credentials, integration accounts and the check of one, memories,
//! schedules and the scheduler, and tasks. What they share is the one way an
//! operator change about an agent is recorded, which is here.

mod agents;
mod credentials;
mod integration_check;
mod integrations;
mod memory;
mod schedules;
mod tasks;

use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::AgentId;

use crate::{Runtime, RuntimeError};

pub use integration_check::CHECK_TOOL;

impl Runtime {
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
