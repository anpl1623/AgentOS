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

    /// An operator asked for something that cannot be done as asked.
    #[error("{0}")]
    Rejected(String),

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

    /// Another scheduler holds this data directory's scheduler lease.
    ///
    /// Two schedulers on one installation would each fire every due schedule
    /// and start every runnable task. The claims underneath refuse the second
    /// attempt at each, but the operator asked for one scheduler, and finding
    /// out there were two from a log of lost claims is finding out too late.
    #[error(
        "the scheduler is already running in another process on this installation; \
         stop that one first, or leave it running"
    )]
    SchedulerAlreadyRunning,

    /// A task was no longer as the caller read it when the caller tried to
    /// start it.
    ///
    /// Most often another client claimed it first and is running it. It may
    /// also have ended, been cancelled, or been given something to wait for
    /// since it was read. None of these is a failure of the task: a caller
    /// that loses the claim starts nothing and must not mark the task failed.
    #[error(
        "task {0} was not started: since it was read it has been started elsewhere, has ended, \
         or now waits for something"
    )]
    TaskAlreadyClaimed(agentos_core::ids::TaskId),

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
