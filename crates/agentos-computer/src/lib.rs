//! Screen, mouse and keyboard control for AgentOS.
//!
//! This is the crate that reaches the physical machine, and it is the least
//! containable capability in the system. The others have a resource the runtime
//! can resolve and then check: a path is canonicalised, an origin comes from a
//! browser AgentOS launched itself. A keystroke has no such thing. It goes
//! wherever focus is, and what it means there depends on what is under it.
//!
//! So the scope on offer is the application in front, as a
//! [`ResourceRef::Application`](agentos_core::permission::ResourceRef), named by
//! the caller and checked against reality before every event. That is a real
//! control — it stops an agent authorised for Mail from typing into Slack — and
//! it is honestly a narrow one. It binds *who receives* an event. It cannot bind
//! *what the event does*: at the policy layer, Return on a focused dialogue and
//! the letter `a` are the same capability on the same resource.
//!
//! The gaps that cannot be engineered away are written down in `SECURITY.md`
//! rather than left for a reader to discover. The reasoning behind the shape of
//! the crate is in [ADR 6](../../../docs/adr/0006-computer-control.md).
//!
//! # Layout
//!
//! [`Desktop`] is the whole of the platform boundary. Everything above it — how
//! a call is scoped, which refusals are unconditional, what an approval card
//! says — is ordinary Rust that compiles and is tested everywhere, including on
//! the platforms that have no backend at all.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod desktop;
pub mod error;
pub mod input;
pub mod platform;
pub mod tools;

use std::sync::Arc;

pub use desktop::testing::RecordingDesktop;
pub use desktop::{
    Capture, Desktop, DisplayInfo, FocusedApplication, Grant, Preflight, WindowInfo,
};
pub use error::ComputerError;
pub use input::{Axis, Button, InputAction, Key, Modifier, Point};
pub use platform::current;

/// Build the computer tools for this platform.
///
/// Cheap and side-effect free: it does not talk to the window server, so
/// listing the tool catalogue does not ask macOS for the Accessibility
/// permission. The connection is opened per action and dropped after it.
#[must_use]
pub fn build() -> Vec<Arc<dyn agentos_tools::Tool>> {
    tools::all(platform::current())
}

/// What the operating system has granted, for `agentos doctor`.
#[must_use]
pub fn preflight() -> Preflight {
    platform::current().preflight()
}

/// Names of every computer tool, for policy documents and `--tool` flags.
pub const TOOL_NAMES: &[&str] = &[
    "computer.inspect",
    "computer.screenshot",
    "computer.move",
    "computer.click",
    "computer.drag",
    "computer.scroll",
    "computer.type",
    "computer.key",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_tool_is_built() {
        let names: Vec<String> = build()
            .iter()
            .map(|tool| tool.metadata().name.clone())
            .collect();
        assert_eq!(names.len(), TOOL_NAMES.len());
        for expected in TOOL_NAMES {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn every_tool_declares_a_computer_capability() {
        for tool in build() {
            let metadata = tool.metadata();
            assert!(
                metadata
                    .required_capabilities
                    .iter()
                    .any(|capability| capability.domain == "computer"),
                "`{}` declares no computer capability, so a policy could not scope it",
                metadata.name
            );
        }
    }

    #[test]
    fn tools_that_read_the_screen_are_marked_untrusted() {
        // A window title and a screenshot are both content somebody else wrote.
        // If either stopped raising taint, a run could read the screen and then
        // act on what it found without the approval bar ever moving.
        for tool in build() {
            let metadata = tool.metadata();
            let reads = matches!(
                metadata.name.as_str(),
                "computer.inspect" | "computer.screenshot"
            );
            assert_eq!(
                metadata.returns_untrusted_data, reads,
                "`{}` reports the wrong taint disposition",
                metadata.name
            );
        }
    }
}
