//! Core domain types for AgentOS.
//!
//! This crate is the vocabulary every other crate speaks. It deliberately has no
//! I/O, no async runtime and no knowledge of storage, models or tools — only the
//! shapes those things exchange.
//!
//! The most important thing defined here is the **trust boundary** ([`trust`]):
//! the type-level distinction between the trusted control plane (operator
//! instructions) and the untrusted data plane (anything that came from a
//! webpage, a file, a terminal or a model). Every other security property in
//! AgentOS is built on top of that distinction.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agent;
pub mod approval;
pub mod error;
pub mod event;
pub mod ids;
pub mod memory;
pub mod permission;
pub mod risk;
pub mod task;
pub mod tool;
pub mod trust;

pub use agent::{Agent, AgentStatus, ModelConfig};
pub use approval::{ApprovalDecision, ApprovalRequest, ApprovalStatus};
pub use error::CoreError;
pub use event::{AgentEvent, Event};
pub use ids::{AgentId, ApprovalId, EventId, MemoryId, TaskId, TaskRunId, ToolExecutionId};
pub use memory::{Memory, MemoryKind, MemoryQuery};
pub use permission::{
    Capability, Effect, PermissionDecision, PermissionRequest, ResourceRef, permission_domains,
};
pub use risk::RiskLevel;
pub use task::{Task, TaskRun, TaskState, TaskStatus, TaskStep, TaskStepKind};
pub use tool::{ToolCall, ToolMetadata, ToolOutcome, ToolResult};
pub use trust::{
    Content, ControlOrigin, DataSource, Message, Role, Trust, UNTRUSTED_TAG, UntrustedContent,
};

/// Wall-clock timestamp type used across the whole system.
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// Current wall-clock time.
///
/// Centralised so that tests and replay tooling have a single seam to control.
#[must_use]
pub fn now() -> Timestamp {
    chrono::Utc::now()
}
