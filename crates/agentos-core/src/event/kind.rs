//! An event's kind, the dotted name the audit table files it under, and which
//! kinds are security-relevant.
//!
//! The live classification, [`AgentEvent::is_security_relevant`], and the
//! stored one, [`is_security_kind`], sit together because they must agree for
//! every variant, and a test holds them to it.

use super::AgentEvent;

impl AgentEvent {
    /// The dotted event name, matching the `serde` rename.
    ///
    /// Used as the `kind` column in the audit table so events can be filtered
    /// without deserialising the payload.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::TaskStarted { .. } => "agent.task.started",
            Self::TaskCompleted { .. } => "agent.task.completed",
            Self::TaskFailed { .. } => "agent.task.failed",
            Self::TaskCancelled { .. } => "agent.task.cancelled",
            Self::StateTransitioned { .. } => "agent.state.transitioned",
            Self::ModelRequestStarted { .. } => "agent.model.request.started",
            Self::ModelRequestCompleted { .. } => "agent.model.request.completed",
            Self::ModelRequestFailed { .. } => "agent.model.request.failed",
            Self::ReasoningCompleted { .. } => "agent.reasoning.completed",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionGranted { .. } => "permission.granted",
            Self::PermissionDenied { .. } => "permission.denied",
            Self::PermissionEscalatedByTaint { .. } => "permission.escalated_by_taint",
            Self::TaintRaised { .. } => "agent.taint.raised",
            Self::ApprovalRequested { .. } => "approval.requested",
            Self::ApprovalGranted { .. } => "approval.granted",
            Self::ApprovalDenied { .. } => "approval.denied",
            Self::ToolExecutionStarted { .. } => "tool.execution.started",
            Self::ToolExecutionCompleted { .. } => "tool.execution.completed",
            Self::ToolExecutionFailed { .. } => "tool.execution.failed",
            Self::ToolArgumentsRejected { .. } => "tool.arguments.rejected",
            Self::UnknownToolRequested { .. } => "tool.unknown",
            Self::ToolManifestExceeded { .. } => "tool.manifest_exceeded",
            Self::MemoryRecorded { .. } => "agent.memory.recorded",
            Self::ScheduleFired { .. } => "schedule.fired",
            Self::TaskAbandoned { .. } => "agent.task.abandoned",
            Self::AgentDelegated { .. } => "agent.delegation.started",
            Self::DelegationCompleted { .. } => "agent.delegation.completed",
            Self::DelegationRefused { .. } => "agent.delegation.refused",
            Self::PolicyChanged { .. } => "operator.policy.changed",
            Self::AgentCreated { .. } => "operator.agent.created",
            Self::AgentEnabledChanged { .. } => "operator.agent.enabled_changed",
            Self::ProviderKeySet { .. } => "operator.provider_key.set",
            Self::ProviderKeyRemoved { .. } => "operator.provider_key.removed",
            Self::CredentialSet { .. } => "operator.credential.set",
            Self::CredentialRemoved { .. } => "operator.credential.removed",
            Self::IntegrationBound { .. } => "operator.integration.bound",
            Self::IntegrationUnbound { .. } => "operator.integration.unbound",
            Self::MemoryRemembered { .. } => "operator.memory.recorded",
            Self::MemoryRevised { .. } => "operator.memory.revised",
            Self::MemoryForgotten { .. } => "operator.memory.forgotten",
            Self::ScheduleCreated { .. } => "operator.schedule.created",
            Self::SchedulePaused { .. } => "operator.schedule.paused",
            Self::ScheduleResumed { .. } => "operator.schedule.resumed",
            Self::ScheduleDeleted { .. } => "operator.schedule.deleted",
            Self::CredentialUsed { .. } => "network.credential.used",
            Self::TaskCreated { .. } => "operator.task.created",
            Self::TaskDependencyAdded { .. } => "operator.task.dependency_added",
            Self::SchedulerStarted { .. } => "operator.scheduler.started",
            Self::SchedulerStopped { .. } => "operator.scheduler.stopped",
        }
    }

    /// Whether this event records a security-relevant refusal or escalation.
    ///
    /// The dashboard surfaces these separately from routine activity.
    ///
    /// A `match` rather than a `matches!` so that an arm can compute its answer
    /// from the event rather than return a constant: an egress event, for one,
    /// matters only when the run was tainted. It is exhaustive so that a new
    /// variant cannot be added without somebody deciding which side it is on.
    /// A stored record is classified by [`is_security_kind`] instead, which must
    /// agree with this for every variant whose answer does not depend on its
    /// fields.
    #[must_use]
    pub fn is_security_relevant(&self) -> bool {
        match self {
            Self::PermissionDenied { .. } => true,
            Self::PermissionEscalatedByTaint { .. } => true,
            Self::ApprovalDenied { .. } => true,
            Self::ToolArgumentsRejected { .. } => true,
            Self::UnknownToolRequested { .. } => true,
            Self::TaintRaised { .. } => true,
            Self::ToolManifestExceeded { .. } => true,
            Self::DelegationRefused { .. } => true,
            // What an agent may do is decided by these. A grant widened before
            // a bad action is part of the explanation of that action.
            Self::PolicyChanged { .. }
            | Self::AgentCreated { .. }
            | Self::AgentEnabledChanged { .. }
            | Self::ProviderKeySet { .. }
            | Self::ProviderKeyRemoved { .. }
            | Self::CredentialSet { .. }
            | Self::CredentialRemoved { .. }
            | Self::IntegrationBound { .. }
            | Self::IntegrationUnbound { .. } => true,
            // The runtime acting as the operator, at a remote service. Whether
            // the run was tainted is in the permission records beside it; the
            // spend matters either way.
            Self::CredentialUsed { .. } => true,
            // What an agent is told before it plans, and what starts work with
            // nobody present, shape what it does as much as its grants do.
            Self::MemoryRemembered { .. }
            | Self::MemoryRevised { .. }
            | Self::MemoryForgotten { .. }
            | Self::ScheduleCreated { .. }
            | Self::SchedulePaused { .. }
            | Self::ScheduleResumed { .. }
            | Self::ScheduleDeleted { .. }
            | Self::TaskCreated { .. }
            | Self::TaskDependencyAdded { .. }
            | Self::SchedulerStarted { .. }
            | Self::SchedulerStopped { .. } => true,
            Self::TaskStarted { .. }
            | Self::TaskCompleted { .. }
            | Self::TaskFailed { .. }
            | Self::TaskCancelled { .. }
            | Self::StateTransitioned { .. }
            | Self::ModelRequestStarted { .. }
            | Self::ModelRequestCompleted { .. }
            | Self::ModelRequestFailed { .. }
            | Self::ReasoningCompleted { .. }
            | Self::PermissionRequested { .. }
            | Self::PermissionGranted { .. }
            | Self::ApprovalRequested { .. }
            | Self::ApprovalGranted { .. }
            | Self::ToolExecutionStarted { .. }
            | Self::ToolExecutionCompleted { .. }
            | Self::ToolExecutionFailed { .. }
            | Self::MemoryRecorded { .. }
            | Self::ScheduleFired { .. }
            | Self::TaskAbandoned { .. }
            // A delegation that was allowed is the policy working. Whatever the
            // child then does is recorded, and classified, on its own.
            | Self::AgentDelegated { .. }
            | Self::DelegationCompleted { .. } => false,
        }
    }
}

/// Whether a stored record of this kind is security-relevant.
///
/// The historical classification: what a reader of the audit table has when it
/// holds only the `kind` column and not a live [`AgentEvent`]. For every variant
/// it agrees with [`AgentEvent::is_security_relevant`], and a test holds it to
/// that.
///
/// It can only do so while the answer depends on the kind alone. An event that
/// is security-relevant only conditionally — egress, say, which matters when the
/// run was tainted and not otherwise — cannot be classified from its name after
/// the fact, and must be classified by the writer at record time and the answer
/// stored beside it. Adding such a variant means extending the record, not this
/// list.
#[must_use]
pub fn is_security_kind(kind: &str) -> bool {
    SECURITY_KINDS.contains(&kind)
}

/// Every kind [`is_security_kind`] accepts.
///
/// A list as well as a predicate so a store can select these kinds in its own
/// query. Filtering the newest records of every kind instead finds almost
/// none: security records are a small fraction of a busy log.
pub const SECURITY_KINDS: &[&str] = &[
    "permission.denied",
    "permission.escalated_by_taint",
    "approval.denied",
    "tool.arguments.rejected",
    "tool.unknown",
    "agent.taint.raised",
    "tool.manifest_exceeded",
    "agent.delegation.refused",
    "operator.policy.changed",
    "operator.agent.created",
    "operator.agent.enabled_changed",
    "operator.provider_key.set",
    "operator.provider_key.removed",
    "operator.credential.set",
    "operator.credential.removed",
    "operator.integration.bound",
    "operator.integration.unbound",
    "network.credential.used",
    "operator.memory.recorded",
    "operator.memory.revised",
    "operator.memory.forgotten",
    "operator.schedule.created",
    "operator.schedule.paused",
    "operator.schedule.resumed",
    "operator.schedule.deleted",
    "operator.task.created",
    "operator.task.dependency_added",
    "operator.scheduler.started",
    "operator.scheduler.stopped",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::samples::{VARIANT_COUNT, sample_events};
    use crate::permission::Capability;

    #[test]
    fn every_kind_is_distinct() {
        // The `kind` column is the audit table's filter; two variants sharing a
        // name would make one of them unfindable.
        let mut kinds: Vec<&str> = sample_events().iter().map(AgentEvent::kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), VARIANT_COUNT);
    }

    #[test]
    fn kind_matches_the_serialised_tag() {
        // If these ever drift, consumers filtering on the `kind` column would
        // silently miss events. Assert they cannot.
        for event in sample_events() {
            let json = serde_json::to_value(&event).unwrap();
            let tag = json
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap();
            assert_eq!(tag, event.kind(), "tag/kind mismatch for {event:?}");
        }
    }

    #[test]
    fn the_stored_classification_agrees_with_the_live_one() {
        // The desktop classifies stored records by kind and live events by
        // variant. If the two disagree, the same event is flagged in one view
        // and not the other.
        for event in sample_events() {
            assert_eq!(
                is_security_kind(event.kind()),
                event.is_security_relevant(),
                "classification mismatch for {}",
                event.kind()
            );
        }
    }

    #[test]
    fn the_security_relevant_set_is_exactly_this() {
        // Pinned, so that moving an event across the line is a decision that
        // shows up in review rather than a side effect.
        let mut flagged: Vec<&str> = sample_events()
            .iter()
            .filter(|event| event.is_security_relevant())
            .map(AgentEvent::kind)
            .collect();
        flagged.sort_unstable();
        assert_eq!(
            flagged,
            vec![
                "agent.delegation.refused",
                "agent.taint.raised",
                "approval.denied",
                "network.credential.used",
                "operator.agent.created",
                "operator.agent.enabled_changed",
                "operator.credential.removed",
                "operator.credential.set",
                "operator.integration.bound",
                "operator.integration.unbound",
                "operator.memory.forgotten",
                "operator.memory.recorded",
                "operator.memory.revised",
                "operator.policy.changed",
                "operator.provider_key.removed",
                "operator.provider_key.set",
                "operator.schedule.created",
                "operator.schedule.deleted",
                "operator.schedule.paused",
                "operator.schedule.resumed",
                "operator.scheduler.started",
                "operator.scheduler.stopped",
                "operator.task.created",
                "operator.task.dependency_added",
                "permission.denied",
                "permission.escalated_by_taint",
                "tool.arguments.rejected",
                "tool.manifest_exceeded",
                "tool.unknown",
            ]
        );
    }

    #[test]
    fn security_events_are_flagged() {
        assert!(
            AgentEvent::PermissionDenied {
                tool: "t".into(),
                capability: Capability::new("filesystem", "write"),
                reason: "r".into(),
                matched_rule: None,
            }
            .is_security_relevant()
        );
        assert!(
            !AgentEvent::TaskCompleted {
                steps: 1,
                duration_ms: 1
            }
            .is_security_relevant()
        );
    }
}
