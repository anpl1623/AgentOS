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
pub mod schedule;
pub mod task;
pub mod tool;
pub mod trust;

pub use agent::{Agent, AgentStatus, ModelConfig};
pub use approval::{ApprovalDecision, ApprovalRequest, ApprovalStatus};
pub use error::CoreError;
pub use event::{AgentEvent, Event};
pub use ids::{
    AgentId, ApprovalId, EventId, MemoryId, ScheduleId, TaskId, TaskRunId, ToolExecutionId,
};
pub use memory::{Memory, MemoryKind, MemoryQuery};
pub use permission::{
    Capability, Effect, PermissionDecision, PermissionRequest, ResourceRef, permission_domains,
};
pub use risk::RiskLevel;
pub use schedule::{Cadence, Clock, Schedule, ScheduleStatus};
pub use task::{Task, TaskRun, TaskState, TaskStatus, TaskStep, TaskStepKind};
pub use tool::{ToolCall, ToolMetadata, ToolOutcome, ToolResult};
pub use trust::{
    Content, ControlOrigin, DataSource, Message, Role, Trust, UNTRUSTED_TAG, UntrustedContent,
};

/// Wall-clock timestamp type used across the whole system.
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// The precision every timestamp is normalised to.
///
/// Microseconds, because that is what survives a round trip everywhere AgentOS
/// runs. Linux clocks report nanoseconds; macOS reports microseconds. A value
/// that is hashed at one precision and stored at another is not the same value
/// when it comes back, and the audit chain notices — it hashes the timestamp, so
/// losing three digits on the way to disk makes every record read as tampered.
///
/// Normalising here rather than at the storage layer means there is exactly one
/// representation of a timestamp in the system, so no future storage backend can
/// reintroduce the mismatch.
pub const TIMESTAMP_PRECISION: chrono::SecondsFormat = chrono::SecondsFormat::Micros;

/// The canonical text form of a timestamp.
///
/// Used by both the audit hash chain and the database. They must agree, so they
/// share this function rather than each choosing a format.
#[must_use]
pub fn format_timestamp(value: &Timestamp) -> String {
    value.to_rfc3339_opts(TIMESTAMP_PRECISION, true)
}

/// Current wall-clock time, truncated to [`TIMESTAMP_PRECISION`].
///
/// Centralised so that tests and replay tooling have a single seam to control,
/// and truncated so an in-memory value equals the one that comes back from disk.
#[must_use]
pub fn now() -> Timestamp {
    use chrono::Timelike;
    let now = chrono::Utc::now();
    let nanos = now.nanosecond();
    // A leap second is represented as nanosecond >= 1_000_000_000; leave those
    // alone rather than mangling them.
    if nanos >= 1_000_000_000 {
        return now;
    }
    now.with_nanosecond(nanos - (nanos % 1_000)).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_stable_through_the_canonical_format() {
        // The property the audit chain depends on: what is hashed in memory and
        // what is written to disk are the same string.
        for _ in 0..1_000 {
            let value = now();
            let text = format_timestamp(&value);
            let parsed = chrono::DateTime::parse_from_rfc3339(&text)
                .expect("the canonical format must be valid RFC3339")
                .with_timezone(&chrono::Utc);
            assert_eq!(parsed, value, "round trip lost precision: {text}");
        }
    }

    #[test]
    fn formatting_truncates_rather_than_rounds() {
        use chrono::TimeZone;
        // A nanosecond-resolution clock, as Linux provides.
        let value = chrono::Utc
            .timestamp_opt(1_700_000_000, 772_812_948)
            .single()
            .expect("valid timestamp");
        assert_eq!(format_timestamp(&value), "2023-11-14T22:13:20.772812Z");
    }
}
