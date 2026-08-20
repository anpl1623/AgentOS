//! Tool errors.

use agentos_core::tool::ToolOutcome;
use thiserror::Error;

/// Something a tool could not do.
///
/// Each variant maps onto a [`ToolOutcome`] so the pipeline can record the right
/// disposition without every tool having to think about it.
#[derive(Debug, Error)]
pub enum ToolError {
    /// The model's arguments did not match the tool's schema.
    #[error("invalid arguments for `{tool}`: {message}")]
    InvalidArguments {
        /// The tool.
        tool: String,
        /// What was wrong.
        message: String,
    },

    /// A path could not be resolved, or escaped its sandbox.
    #[error("{0}")]
    Path(#[from] agentos_permissions::PathError),

    /// The policy engine refused.
    #[error("permission denied: {reason}")]
    Denied {
        /// Why.
        reason: String,
    },

    /// A human declined.
    #[error("approval denied{}", note.as_ref().map(|n| format!(": {n}")).unwrap_or_default())]
    ApprovalDenied {
        /// The human's note, if any.
        note: Option<String>,
    },

    /// The run was cancelled.
    #[error("cancelled")]
    Cancelled,

    /// The tool exceeded its time budget.
    #[error("`{tool}` timed out after {seconds}s")]
    TimedOut {
        /// The tool.
        tool: String,
        /// The budget.
        seconds: u64,
    },

    /// The filesystem, a subprocess or the network failed.
    #[error("{operation} failed: {source}")]
    Io {
        /// What was being attempted.
        operation: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// The tool ran and failed for a reason of its own.
    #[error("{0}")]
    Failed(String),

    /// No such tool, or it is not enabled for this agent.
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
}

impl ToolError {
    /// The outcome this error should be recorded as.
    #[must_use]
    pub const fn outcome(&self) -> ToolOutcome {
        match self {
            Self::InvalidArguments { .. } | Self::UnknownTool(_) => ToolOutcome::InvalidArguments,
            // A sandbox escape is a policy failure, not a tool malfunction.
            Self::Denied { .. } | Self::Path(_) => ToolOutcome::Denied,
            Self::ApprovalDenied { .. } => ToolOutcome::ApprovalDenied,
            Self::Cancelled => ToolOutcome::Cancelled,
            Self::TimedOut { .. } => ToolOutcome::TimedOut,
            Self::Io { .. } | Self::Failed(_) => ToolOutcome::Failed,
        }
    }

    /// Whether the agent could plausibly recover by trying something else.
    ///
    /// A denial is recoverable in this sense: the agent should be told and
    /// allowed to re-plan, not have the whole run torn down.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        !matches!(self, Self::Cancelled)
    }

    /// Build an I/O error with context.
    pub fn io(operation: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            operation: operation.into(),
            source,
        }
    }

    /// Build an invalid-arguments error.
    pub fn invalid(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidArguments {
            tool: tool.into(),
            message: message.into(),
        }
    }
}
