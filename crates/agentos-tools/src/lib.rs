//! Tools, and the pipeline every tool call passes through.
//!
//! A tool is the only way an agent affects anything. There is no side channel:
//! the model emits a tool call, the pipeline validates it, the policy engine
//! authorises it, a human approves it if required, and only then does anything
//! happen. See [`pipeline`] for the full sequence and the reasoning behind it.
//!
//! Adding a capability to AgentOS means writing a [`Tool`], not editing the
//! agent loop.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod approval;
pub mod error;
pub mod filesystem;
pub mod pipeline;
pub mod taint;
pub mod terminal;
pub mod tool;
pub mod vision;

use std::sync::Arc;

pub use approval::{ApprovalGate, ApprovalOutcome, DenyAllGate, RecordingGate};
pub use error::ToolError;
pub use pipeline::{ExecutionReport, ToolPipeline};
pub use taint::TaintTracker;
pub use tool::{
    DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT, Tool, ToolContext, ToolOutput, ToolPlan,
    ToolRegistry, metadata_for, parse_arguments,
};
pub use vision::{
    DEFAULT_MAX_IMAGE_BYTES, DEFAULT_MAX_IMAGE_EDGE, PreparedImage, VisionError, prepare,
};

/// A registry with every tool the runtime ships.
///
/// Registration is not authorisation: a registered tool an agent has not been
/// given is not offered to the model, and a tool the model does call is still
/// subject to the policy engine.
#[must_use]
pub fn standard_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool in filesystem::all().into_iter().chain(terminal::all()) {
        registry.register(tool);
    }
    registry
}

/// The standard registry behind an [`Arc`].
#[must_use]
pub fn shared_standard_registry() -> Arc<ToolRegistry> {
    Arc::new(standard_registry())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_registry_holds_every_shipped_tool() {
        let registry = standard_registry();
        assert_eq!(
            registry.names(),
            vec![
                "filesystem.copy",
                "filesystem.delete",
                "filesystem.list",
                "filesystem.move",
                "filesystem.read",
                "filesystem.write",
                "terminal.exec",
            ]
        );
    }

    #[test]
    fn every_tool_declares_a_qualified_name_and_a_capability() {
        for metadata in standard_registry().all_metadata() {
            assert!(
                metadata.name.contains('.'),
                "`{}` should be named domain.action",
                metadata.name
            );
            assert!(
                !metadata.required_capabilities.is_empty(),
                "`{}` declares no capabilities, so the policy could not scope it",
                metadata.name
            );
            assert!(
                !metadata.description.is_empty(),
                "`{}` has no description for the model",
                metadata.name
            );
        }
    }

    #[test]
    fn tools_that_read_the_outside_world_are_marked_untrusted() {
        let registry = standard_registry();
        for name in ["filesystem.read", "filesystem.list", "terminal.exec"] {
            let tool = registry.get(name).unwrap();
            assert!(
                tool.metadata().returns_untrusted_data,
                "`{name}` returns external data and must raise taint"
            );
        }
    }
}
