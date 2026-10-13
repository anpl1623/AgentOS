//! Delegation: one agent asking another to do part of its work.
//!
//! A delegation is an edge between two runs. The parent run asks; a child run,
//! of a task created for the purpose, does the work and reports back. This
//! module holds the vocabulary of that edge and nothing else: the broker that
//! performs a delegation lives in the runtime, and what a child may do is the
//! policy engine's to say.
//!
//! What a delegation never does is widen authority. A child is evaluated by the
//! intersection of its own policy and every policy above it, shares its
//! parent's taint and step budget, and asks the same operator for approval.
//! Nothing here enforces that — these are shapes — but every field is chosen so
//! that the enforcement has something to record.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::error::CoreError;
use crate::ids::{AgentId, DelegationId, TaskId, TaskRunId};

/// How deep a delegation tree may grow by default.
///
/// Two: a supervisor may ask a worker, and that worker may ask one more. A
/// supervisor of supervisors is already hard to reason about from an approval
/// card, and every level is another policy a person has to hold in their head.
pub const DEFAULT_MAX_DELEGATION_DEPTH: u8 = 2;

/// How many children one run may start by default.
///
/// Four. A run that wants five sub-agents wants a task graph, where each
/// piece of work is a task the operator can see, schedule and retry.
pub const DEFAULT_MAX_CHILDREN_PER_RUN: u16 = 4;

/// One agent's request that another carry out an objective.
///
/// Written before the child run exists, which is why [`Self::child_run_id`] is
/// optional: a delegation that cannot be recorded is one that must not be
/// performed, the same ordering an approval request follows for the same
/// reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    /// Identity.
    pub id: DelegationId,
    /// The run that asked.
    pub parent_run_id: TaskRunId,
    /// The agent that asked.
    pub parent_agent_id: AgentId,
    /// The agent asked.
    pub child_agent_id: AgentId,
    /// The task created for the child.
    pub child_task_id: TaskId,
    /// The run carrying it out, once it has been prepared.
    pub child_run_id: Option<TaskRunId>,
    /// What the child was asked to do.
    ///
    /// Written by the parent's model, so it is not operator text even though it
    /// becomes the child's objective. The child's policy, not this sentence,
    /// decides what the child may do.
    pub objective: String,
    /// How far below the root run the child sits; a root run's children are
    /// depth one.
    pub depth: u8,
    /// Where it stands.
    pub state: DelegationState,
    /// Model turns the child run took.
    pub steps_spent: u32,
    /// When the parent asked.
    pub requested_at: Timestamp,
    /// When the child finished, or the delegation was refused.
    pub completed_at: Option<Timestamp>,
}

impl Delegation {
    /// A delegation as it is first recorded: requested, with no run yet.
    #[must_use]
    pub fn requested(
        parent_run_id: TaskRunId,
        parent_agent_id: AgentId,
        child_agent_id: AgentId,
        child_task_id: TaskId,
        objective: impl Into<String>,
        depth: u8,
    ) -> Self {
        Self {
            id: DelegationId::new(),
            parent_run_id,
            parent_agent_id,
            child_agent_id,
            child_task_id,
            child_run_id: None,
            objective: objective.into(),
            depth,
            state: DelegationState::Requested,
            steps_spent: 0,
            requested_at: crate::now(),
            completed_at: None,
        }
    }
}

/// Where a delegation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DelegationState {
    /// Recorded; the child run has not been prepared yet.
    #[default]
    Requested,
    /// The child run exists and is under way.
    Running,
    /// The child run completed.
    Completed,
    /// The child run failed. A result the parent reads, not an error it raises.
    Failed,
    /// The delegation was refused before any child ran.
    Refused,
    /// The child run was cancelled, usually because its parent was.
    Cancelled,
}

impl DelegationState {
    /// Whether nothing further will happen to the delegation.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Refused | Self::Cancelled
        )
    }

    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
        }
    }

    /// Every state, for exhaustive testing.
    pub const ALL: [Self; 6] = [
        Self::Requested,
        Self::Running,
        Self::Completed,
        Self::Failed,
        Self::Refused,
        Self::Cancelled,
    ];
}

impl fmt::Display for DelegationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DelegationState {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| CoreError::UnknownVariant {
                kind: "delegation state",
                value: s.to_owned(),
            })
    }
}

/// Why a delegation was not performed.
///
/// Each message names the limit or the path that refused it, because the
/// reader is a supervising model that has to re-plan, and after it an operator
/// reading the trace. "Refused" alone helps neither.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DelegationRefusal {
    /// The child would sit deeper than the tree may grow.
    #[error("delegation refused: a delegation tree may be at most {limit} level(s) deep")]
    DepthExceeded {
        /// The configured depth limit.
        limit: u8,
    },

    /// The run would start more children than it may.
    #[error("delegation refused: a run may delegate to at most {limit} agent(s)")]
    FanOutExceeded {
        /// The configured per-run limit.
        limit: u16,
    },

    /// The tree has no steps left to give a child.
    #[error(
        "delegation refused: the step budget this run shares with its delegates has \
         {remaining} step(s) left"
    )]
    StepBudgetExhausted {
        /// Steps left in the shared budget.
        remaining: u32,
    },

    /// The agent asked is already working on this chain.
    #[error(
        "delegation refused: that delegation would close a cycle: {}",
        path.join(" -> ")
    )]
    AgencyCycle {
        /// Agent names from the root of the chain to the agent asked, which
        /// appears twice. Reported in full, because "there is a cycle" is not
        /// actionable and "sales -> researcher -> sales" is.
        path: Vec<String>,
    },

    /// No agent goes by that name.
    #[error("delegation refused: no agent is named `{name}`")]
    UnknownAgent {
        /// The name asked for.
        name: String,
    },

    /// The agent exists and the operator has switched it off.
    #[error("delegation refused: agent `{name}` is disabled")]
    DisabledAgent {
        /// Its name.
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip_through_their_wire_form() {
        for state in DelegationState::ALL {
            assert_eq!(state.as_str().parse::<DelegationState>().unwrap(), state);
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::Value::String(state.as_str().to_owned())
            );
        }
        assert!(matches!(
            "finished".parse::<DelegationState>(),
            Err(CoreError::UnknownVariant {
                kind: "delegation state",
                ..
            })
        ));
    }

    #[test]
    fn only_a_delegation_with_nothing_left_to_happen_is_terminal() {
        let terminal: Vec<DelegationState> = DelegationState::ALL
            .into_iter()
            .filter(|state| state.is_terminal())
            .collect();
        assert_eq!(
            terminal,
            vec![
                DelegationState::Completed,
                DelegationState::Failed,
                DelegationState::Refused,
                DelegationState::Cancelled,
            ]
        );
    }

    #[test]
    fn a_delegation_is_recorded_before_it_has_a_run() {
        let delegation = Delegation::requested(
            TaskRunId::new(),
            AgentId::new(),
            AgentId::new(),
            TaskId::new(),
            "Find last quarter's churn figures",
            1,
        );
        assert_eq!(delegation.state, DelegationState::Requested);
        assert_eq!(delegation.child_run_id, None);
        assert_eq!(delegation.steps_spent, 0);
        assert_eq!(delegation.completed_at, None);
    }

    #[test]
    fn every_refusal_names_what_refused_it() {
        let cycle = DelegationRefusal::AgencyCycle {
            path: vec!["sales".into(), "researcher".into(), "sales".into()],
        };
        assert!(
            cycle
                .to_string()
                .ends_with("would close a cycle: sales -> researcher -> sales"),
            "{cycle}"
        );
        assert!(
            DelegationRefusal::DepthExceeded { limit: 2 }
                .to_string()
                .contains("at most 2 level(s)")
        );
        assert!(
            DelegationRefusal::FanOutExceeded { limit: 4 }
                .to_string()
                .contains("at most 4 agent(s)")
        );
        assert!(
            DelegationRefusal::StepBudgetExhausted { remaining: 0 }
                .to_string()
                .contains("0 step(s) left")
        );
        assert!(
            DelegationRefusal::UnknownAgent {
                name: "ghost".into()
            }
            .to_string()
            .contains("`ghost`")
        );
        assert!(
            DelegationRefusal::DisabledAgent {
                name: "worker".into()
            }
            .to_string()
            .contains("`worker` is disabled")
        );
    }

    #[test]
    fn the_defaults_are_two_deep_and_four_wide() {
        assert_eq!(DEFAULT_MAX_DELEGATION_DEPTH, 2);
        assert_eq!(DEFAULT_MAX_CHILDREN_PER_RUN, 4);
    }
}
