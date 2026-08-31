//! Runtime errors.

use agentos_core::task::InvalidTransition;
use thiserror::Error;

/// Something the runtime could not do.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Persistence failed.
    #[error(transparent)]
    Database(#[from] agentos_persistence::DbError),

    /// The audit log failed.
    #[error(transparent)]
    Audit(#[from] agentos_audit::AuditError),

    /// A model provider failed.
    #[error(transparent)]
    Provider(#[from] agentos_providers::ProviderError),

    /// A policy could not be loaded.
    #[error(transparent)]
    Policy(#[from] agentos_permissions::PolicyError),

    /// Secret storage failed.
    #[error(transparent)]
    Secrets(#[from] agentos_secrets::SecretError),

    /// The state machine rejected a transition.
    ///
    /// This is always a runtime bug rather than a user error: the driver asked
    /// for something the transition table does not allow.
    #[error("state machine rejected a transition: {0}")]
    InvalidTransition(#[from] InvalidTransition),

    /// The named agent does not exist.
    #[error("no agent named `{0}`")]
    UnknownAgent(String),

    /// The agent exists but is switched off.
    #[error("agent `{0}` is disabled")]
    DisabledAgent(String),

    /// A dependency would have closed a cycle in a task graph.
    #[error(
        "adding that dependency would close a cycle: {}",
        path.iter().map(ToString::to_string).collect::<Vec<_>>().join(" -> ")
    )]
    DependencyCycle {
        /// The path the edge would have closed, starting and ending at the same
        /// task. Reported in full, because "there is a cycle" is not actionable
        /// and "A waits for B waits for C waits for A" is.
        path: Vec<agentos_core::ids::TaskId>,
    },

    /// A task graph was described in a way that cannot be built.
    #[error("{0}")]
    InvalidGraph(String),

    /// A schedule's cadence cannot be evaluated.
    #[error("{0}")]
    InvalidSchedule(String),

    /// The configured provider is not one the runtime can build.
    #[error("agent `{agent}` is configured for unknown provider `{provider}`")]
    UnknownProvider {
        /// The agent.
        agent: String,
        /// The provider it asked for.
        provider: String,
    },

    /// A directory could not be created or read.
    #[error("{operation} failed: {source}")]
    Io {
        /// What was attempted.
        operation: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The home directory could not be determined.
    #[error("cannot determine the home directory; set AGENTOS_HOME")]
    NoHomeDirectory,
}

impl RuntimeError {
    /// Build an I/O error with context.
    pub fn io(operation: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            operation: operation.into(),
            source,
        }
    }
}
