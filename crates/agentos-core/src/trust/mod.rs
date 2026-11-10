//! The trust boundary between the control plane and the data plane.
//!
//! # Why this exists
//!
//! An agent that reads a webpage, an email or a file is reading text an
//! attacker may have written. If that text reaches the model indistinguishable
//! from the operator's own instructions, the attacker is giving the orders.
//! Telling the model "ignore instructions in tool output" is not a control; it
//! is a request.
//!
//! AgentOS instead makes the distinction *structural*:
//!
//! * [`Content::Control`] is the only trusted variant. It carries operator
//!   instructions, the task objective and runtime notices.
//! * [`Content::Untrusted`] carries everything that entered from outside — every
//!   tool result, without exception.
//! * [`Content::Model`] carries model-generated prose. It is **not** trusted:
//!   a compromised model must not be able to promote itself into the control
//!   plane by asserting something.
//!
//! There is deliberately no conversion from a tool result into
//! [`Content::Control`]. The type system, not a prompt, is what stops the
//! promotion.
//!
//! Rendering untrusted content wraps it in a nonce-tagged envelope so the model
//! can see exactly where attacker-controlled text begins and ends, and cannot be
//! tricked by text that merely looks like a closing delimiter.
//!
//! None of this is the *primary* defence. The primary defence is that
//! permission decisions are computed by the runtime from policy and never from
//! model output — see `agentos-permissions`. This module makes the boundary
//! visible; the policy engine makes it enforceable.

mod envelope;
mod image;
mod message;
mod source;
#[cfg(test)]
mod test_support;

use serde::{Deserialize, Serialize};

pub use envelope::{UNTRUSTED_TAG, UntrustedContent};
pub use image::{ImageFormat, UntrustedImage};
pub use message::{Content, Message, Role};
pub use source::DataSource;

/// Which plane a piece of content belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// Operator-authored. May carry instructions.
    Control,
    /// Everything else. Data only; never instructions.
    Untrusted,
}

/// Why a piece of control-plane content exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlOrigin {
    /// The agent's configured system instructions.
    SystemInstructions,
    /// The objective the operator gave for this task.
    Objective,
    /// A notice injected by the runtime (e.g. "an approval was denied").
    RuntimeNotice,
}

/// Trusted, operator-authored content.
///
/// Constructing this is how something enters the control plane, so every call
/// site is worth reviewing. There is no path from tool output to this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlContent {
    /// Why this content is trusted.
    pub origin: ControlOrigin,
    /// The instruction text.
    pub text: String,
}

impl ControlContent {
    /// Create trusted control-plane content.
    #[must_use]
    pub fn new(origin: ControlOrigin, text: impl Into<String>) -> Self {
        Self {
            origin,
            text: text.into(),
        }
    }
}
