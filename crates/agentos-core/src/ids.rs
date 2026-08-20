//! Strongly typed identifiers.
//!
//! Every entity gets its own newtype so that a `TaskId` can never be passed
//! where an `AgentId` is expected. All of them wrap a v4 UUID.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CoreError;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a fresh random identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// The identifier family name, used in error messages and audit rows.
            #[must_use]
            pub const fn kind() -> &'static str {
                $kind
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = CoreError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|_| CoreError::InvalidId { kind: $kind, value: s.to_owned() })
            }
        }
    };
}

define_id!(
    /// Identifies a persistent agent.
    AgentId,
    "agent"
);
define_id!(
    /// Identifies a task (an objective), independent of any execution of it.
    TaskId,
    "task"
);
define_id!(
    /// Identifies a single execution of a task.
    TaskRunId,
    "task_run"
);
define_id!(
    /// Identifies one tool invocation inside a run.
    ToolExecutionId,
    "tool_execution"
);
define_id!(
    /// Identifies a human approval request.
    ApprovalId,
    "approval"
);
define_id!(
    /// Identifies a stored memory.
    MemoryId,
    "memory"
);
define_id!(
    /// Identifies an audit event.
    EventId,
    "event"
);
define_id!(
    /// Identifies a single step within a run (a planning turn, a tool call, ...).
    TaskStepId,
    "task_step"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_string() {
        let id = AgentId::new();
        let parsed: AgentId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn rejects_non_uuid() {
        let err = "not-a-uuid".parse::<TaskId>().unwrap_err();
        assert!(matches!(err, CoreError::InvalidId { kind: "task", .. }));
    }

    #[test]
    fn serialises_transparently() {
        let id = TaskId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
    }
}
