//! Computer-control errors.

use agentos_tools::ToolError;
use thiserror::Error;

/// Something the computer layer could not do.
#[derive(Debug, Error)]
pub enum ComputerError {
    /// This platform has no backend.
    #[error("computer control is not available on this platform")]
    Unsupported,

    /// The operating system has not granted the process the right to act.
    #[error("{permission} has not been granted to AgentOS — {remedy}")]
    NotPermitted {
        /// Which operating-system permission is missing.
        permission: &'static str,
        /// Where the operator grants it.
        remedy: &'static str,
    },

    /// Nothing on the desktop currently holds keyboard focus.
    ///
    /// A refusal rather than a fallback: input has to be scoped to *something*,
    /// and "whatever happens to be in front" is not a scope.
    #[error(
        "no application is in front, so there is nothing to scope this to; \
         focus the application you want the agent to work in"
    )]
    NoFocusedApplication,

    /// The named application is not the one in front.
    #[error(
        "`{requested}` is not in front — `{actual}` is; \
         call `computer.inspect` to see what has focus"
    )]
    NotInFront {
        /// The application the call named.
        requested: String,
        /// What is actually in front.
        actual: String,
    },

    /// Focus moved between authorisation and execution.
    #[error(
        "focus moved from `{expected}` to `{actual}` part-way through; \
         {delivered} of {total} event(s) had already been sent"
    )]
    FocusChanged {
        /// The application the decision was made about.
        expected: String,
        /// What is in front now.
        actual: String,
        /// How many events were delivered before the change was noticed.
        delivered: usize,
        /// How many the action would have sent in total.
        total: usize,
    },

    /// The target is AgentOS itself.
    #[error(
        "AgentOS is in front, and an agent may not send input to the program \
         that is asking you to approve its actions"
    )]
    SelfTargeted,

    /// A coordinate is not on any display.
    #[error("({x}, {y}) is not on any display")]
    OffScreen {
        /// The x coordinate, in points.
        x: i32,
        /// The y coordinate, in points.
        y: i32,
    },

    /// The run was cancelled part-way through.
    #[error("cancelled after {delivered} of {total} event(s)")]
    Cancelled {
        /// How many events had been sent.
        delivered: usize,
        /// How many the action would have sent.
        total: usize,
    },

    /// The backend refused or failed.
    #[error("{operation} failed: {message}")]
    Backend {
        /// What was being attempted.
        operation: String,
        /// Detail from the platform.
        message: String,
    },
}

impl ComputerError {
    /// Build a backend failure.
    pub fn backend(operation: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Backend {
            operation: operation.into(),
            message: message.to_string(),
        }
    }
}

impl From<ComputerError> for ToolError {
    fn from(error: ComputerError) -> Self {
        match &error {
            // Coordinates that are off-screen, or a target that is not in front,
            // are the model getting the arguments wrong — it can usefully
            // re-plan from either.
            ComputerError::OffScreen { .. } | ComputerError::NotInFront { .. } => {
                Self::InvalidArguments {
                    tool: "computer".to_owned(),
                    message: error.to_string(),
                }
            }
            // Cancelling is not a failure; the pipeline records it as one of
            // its own outcomes.
            ComputerError::Cancelled { .. } => Self::Cancelled,
            _ => Self::Failed(error.to_string()),
        }
    }
}
