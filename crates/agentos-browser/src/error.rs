//! Browser errors.

use agentos_permissions::OriginError;
use agentos_tools::ToolError;
use thiserror::Error;

/// Something the browser layer could not do.
#[derive(Debug, Error)]
pub enum BrowserError {
    /// No Chromium-family browser is installed.
    #[error("{0}")]
    NotFound(String),

    /// The browser would not start.
    #[error("could not start the browser: {message}")]
    Launch {
        /// Detail.
        message: String,
    },

    /// A CDP command failed.
    #[error("{operation} failed: {message}")]
    Command {
        /// What was being attempted.
        operation: String,
        /// Detail.
        message: String,
    },

    /// A selector matched nothing.
    #[error("no element matches `{selector}` on {url}")]
    NoSuchElement {
        /// The selector.
        selector: String,
        /// The page it was tried on.
        url: String,
    },

    /// A wait expired.
    #[error("`{selector}` did not appear within {seconds}s")]
    WaitTimeout {
        /// The selector.
        selector: String,
        /// The budget.
        seconds: u64,
    },

    /// A tool was used before navigating anywhere.
    #[error("the browser has not navigated anywhere yet; call `browser.navigate` first")]
    NoPage,

    /// A URL could not be reduced to an origin: not http(s), no host,
    /// credentials, or a host or port outside the canonical form.
    ///
    /// Carries the permission crate's error rather than a URL string so the
    /// agent is told which of those it was, and so there is one parser and one
    /// set of reasons for every tool that scopes by origin.
    #[error(transparent)]
    InvalidUrl(#[from] OriginError),
}

impl From<BrowserError> for ToolError {
    fn from(error: BrowserError) -> Self {
        match &error {
            // A missing browser is a setup problem, not a tool malfunction, but
            // both come back to the agent as something it cannot do.
            BrowserError::NotFound(_) | BrowserError::Launch { .. } => {
                Self::Failed(error.to_string())
            }
            BrowserError::InvalidUrl { .. } | BrowserError::NoPage => Self::InvalidArguments {
                tool: "browser".to_owned(),
                message: error.to_string(),
            },
            _ => Self::Failed(error.to_string()),
        }
    }
}
