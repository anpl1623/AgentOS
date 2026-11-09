//! One sample of every [`AgentEvent`] variant, for the tests that claim to
//! cover every event, and the index that proves none is missing.

use super::AgentEvent;
use crate::ids::{
    ApprovalId, DelegationId, MemoryId, ScheduleId, TaskId, TaskRunId, ToolExecutionId,
};
use crate::permission::{Capability, Effect};
use crate::risk::RiskLevel;
use crate::task::{TaskState, TaskTrigger};
use crate::trust::DataSource;

/// How many variants [`AgentEvent`] has.
///
/// Kept beside [`variant_index`] because the two change together.
pub(super) const VARIANT_COUNT: usize = 50;

/// A dense index per variant, in declaration order.
///
/// Exhaustive with no wildcard, so a new variant does not compile until it
/// is given an index here — and once it has one, the sampling test below
/// fails until it is also in [`sample_events`]. Rust offers no way to count
/// an enum's variants directly; this is the next best thing.
pub(super) const fn variant_index(event: &AgentEvent) -> usize {
    match event {
        AgentEvent::TaskStarted { .. } => 0,
        AgentEvent::TaskCompleted { .. } => 1,
        AgentEvent::TaskFailed { .. } => 2,
        AgentEvent::TaskCancelled { .. } => 3,
        AgentEvent::StateTransitioned { .. } => 4,
        AgentEvent::ModelRequestStarted { .. } => 5,
        AgentEvent::ModelRequestCompleted { .. } => 6,
        AgentEvent::ModelRequestFailed { .. } => 7,
        AgentEvent::ReasoningCompleted { .. } => 8,
        AgentEvent::PermissionRequested { .. } => 9,
        AgentEvent::PermissionGranted { .. } => 10,
        AgentEvent::PermissionDenied { .. } => 11,
        AgentEvent::PermissionEscalatedByTaint { .. } => 12,
        AgentEvent::TaintRaised { .. } => 13,
        AgentEvent::ApprovalRequested { .. } => 14,
        AgentEvent::ApprovalGranted { .. } => 15,
        AgentEvent::ApprovalDenied { .. } => 16,
        AgentEvent::ToolExecutionStarted { .. } => 17,
        AgentEvent::ToolExecutionCompleted { .. } => 18,
        AgentEvent::ToolExecutionFailed { .. } => 19,
        AgentEvent::ToolArgumentsRejected { .. } => 20,
        AgentEvent::UnknownToolRequested { .. } => 21,
        AgentEvent::ToolManifestExceeded { .. } => 22,
        AgentEvent::MemoryRecorded { .. } => 23,
        AgentEvent::ScheduleFired { .. } => 24,
        AgentEvent::TaskAbandoned { .. } => 25,
        AgentEvent::PolicyChanged { .. } => 26,
        AgentEvent::AgentCreated { .. } => 27,
        AgentEvent::AgentEnabledChanged { .. } => 28,
        AgentEvent::ProviderKeySet { .. } => 29,
        AgentEvent::ProviderKeyRemoved { .. } => 30,
        AgentEvent::MemoryRemembered { .. } => 31,
        AgentEvent::MemoryRevised { .. } => 32,
        AgentEvent::MemoryForgotten { .. } => 33,
        AgentEvent::ScheduleCreated { .. } => 34,
        AgentEvent::SchedulePaused { .. } => 35,
        AgentEvent::ScheduleResumed { .. } => 36,
        AgentEvent::ScheduleDeleted { .. } => 37,
        AgentEvent::TaskCreated { .. } => 38,
        AgentEvent::TaskDependencyAdded { .. } => 39,
        AgentEvent::SchedulerStarted { .. } => 40,
        AgentEvent::SchedulerStopped { .. } => 41,
        AgentEvent::CredentialSet { .. } => 42,
        AgentEvent::CredentialRemoved { .. } => 43,
        AgentEvent::CredentialUsed { .. } => 44,
        AgentEvent::IntegrationBound { .. } => 45,
        AgentEvent::IntegrationUnbound { .. } => 46,
        AgentEvent::AgentDelegated { .. } => 47,
        AgentEvent::DelegationCompleted { .. } => 48,
        AgentEvent::DelegationRefused { .. } => 49,
    }
}

/// One of every variant.
///
/// Every test that claims to cover "every event" iterates this, so an
/// omission here is an omission from all of them; the sampling test is what
/// stops that happening quietly.
pub(super) fn sample_events() -> Vec<AgentEvent> {
    vec![
        AgentEvent::TaskStarted {
            objective: "o".into(),
            attempt: 1,
        },
        AgentEvent::TaskCompleted {
            steps: 3,
            duration_ms: 10,
        },
        AgentEvent::TaskFailed {
            reason: "r".into(),
            steps: 1,
        },
        AgentEvent::TaskCancelled { steps: 1 },
        AgentEvent::StateTransitioned {
            from: TaskState::Idle,
            to: TaskState::Planning,
            trigger: TaskTrigger::Start,
        },
        AgentEvent::ModelRequestStarted {
            provider: "mock".into(),
            model: "m".into(),
            message_count: 1,
            tool_count: 0,
        },
        AgentEvent::ModelRequestCompleted {
            provider: "mock".into(),
            model: "m".into(),
            duration_ms: 1,
            input_tokens: None,
            output_tokens: None,
            tool_calls: 0,
        },
        AgentEvent::ModelRequestFailed {
            provider: "mock".into(),
            error: "e".into(),
        },
        AgentEvent::ReasoningCompleted {
            summary: "s".into(),
            tool_calls: 0,
        },
        AgentEvent::PermissionRequested {
            tool: "t".into(),
            capability: Capability::new("filesystem", "read"),
            risk: RiskLevel::Low,
            tainted: false,
        },
        AgentEvent::PermissionGranted {
            tool: "t".into(),
            capability: Capability::new("filesystem", "read"),
            matched_rule: None,
        },
        AgentEvent::PermissionDenied {
            tool: "t".into(),
            capability: Capability::new("filesystem", "read"),
            reason: "r".into(),
            matched_rule: None,
        },
        AgentEvent::PermissionEscalatedByTaint {
            tool: "t".into(),
            original: Effect::Allow,
            escalated: Effect::Ask,
        },
        AgentEvent::TaintRaised {
            source: DataSource::User,
            tool: "t".into(),
        },
        AgentEvent::ApprovalRequested {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            risk: RiskLevel::High,
        },
        AgentEvent::ApprovalGranted {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            waited_ms: 5,
            note: Some("checked the recipient".into()),
        },
        AgentEvent::ApprovalDenied {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            note: None,
            over_budget: false,
        },
        AgentEvent::ToolExecutionStarted {
            execution_id: ToolExecutionId::new(),
            tool: "t".into(),
            arguments: serde_json::Value::Null,
        },
        AgentEvent::ToolExecutionCompleted {
            execution_id: ToolExecutionId::new(),
            tool: "t".into(),
            duration_ms: 1,
            success: true,
            output_bytes: 0,
        },
        AgentEvent::ToolExecutionFailed {
            execution_id: ToolExecutionId::new(),
            tool: "t".into(),
            duration_ms: 1,
            error: "e".into(),
        },
        AgentEvent::ToolArgumentsRejected {
            tool: "t".into(),
            error: "e".into(),
        },
        AgentEvent::UnknownToolRequested { tool: "t".into() },
        AgentEvent::ToolManifestExceeded {
            tool: "t".into(),
            undeclared: vec!["filesystem.write".into()],
        },
        AgentEvent::MemoryRecorded {
            kind: "fact".into(),
            source: DataSource::User,
        },
        AgentEvent::ScheduleFired {
            schedule_id: ScheduleId::new(),
            name: "n".into(),
            task_id: TaskId::new(),
        },
        AgentEvent::TaskAbandoned {
            task_id: TaskId::new(),
            blocked_by: TaskId::new(),
            reason: "failed".into(),
        },
        AgentEvent::PolicyChanged {
            agent: "a".into(),
            version: 2,
            document: "default: deny\n".into(),
        },
        AgentEvent::AgentCreated {
            agent: "a".into(),
            provider: "mock".into(),
            model: "m".into(),
            tools: vec!["filesystem.read".into()],
        },
        AgentEvent::AgentEnabledChanged {
            agent: "a".into(),
            enabled: false,
        },
        AgentEvent::ProviderKeySet {
            provider: "anthropic".into(),
        },
        AgentEvent::ProviderKeyRemoved {
            provider: "anthropic".into(),
        },
        AgentEvent::MemoryRemembered {
            agent: "a".into(),
            memory_id: MemoryId::new(),
            kind: "preference".into(),
            content: "Reply in British English.".into(),
        },
        AgentEvent::MemoryRevised {
            agent: "a".into(),
            memory_id: MemoryId::new(),
            content: "Reply in plain English.".into(),
        },
        AgentEvent::MemoryForgotten {
            agent: "a".into(),
            memory_id: MemoryId::new(),
            kind: "fact".into(),
            content: "The office is in Leeds.".into(),
        },
        AgentEvent::ScheduleCreated {
            schedule_id: ScheduleId::new(),
            name: "weekly".into(),
            objective: "o".into(),
            cadence: crate::schedule::Cadence::Every { seconds: 3600 },
            next_run_at: Some(crate::now()),
        },
        AgentEvent::SchedulePaused {
            schedule_id: ScheduleId::new(),
            name: "weekly".into(),
        },
        AgentEvent::ScheduleResumed {
            schedule_id: ScheduleId::new(),
            name: "weekly".into(),
            next_run_at: None,
        },
        AgentEvent::ScheduleDeleted {
            schedule_id: ScheduleId::new(),
            name: "weekly".into(),
        },
        AgentEvent::TaskCreated {
            objective: "o".into(),
            depends_on: vec![TaskId::new()],
            scheduled_for: None,
        },
        AgentEvent::TaskDependencyAdded {
            task_id: TaskId::new(),
            depends_on: TaskId::new(),
        },
        AgentEvent::SchedulerStarted {
            tick_seconds: 30,
            max_concurrent_runs: 1,
            on_launch: true,
        },
        AgentEvent::SchedulerStopped {
            tick_seconds: 30,
            max_concurrent_runs: 1,
        },
        AgentEvent::CredentialSet {
            origin: "https://api.example.com".into(),
            name: "default".into(),
        },
        AgentEvent::CredentialRemoved {
            origin: "https://api.example.com".into(),
            name: "default".into(),
        },
        AgentEvent::CredentialUsed {
            origin: "https://api.example.com".into(),
            name: "default".into(),
            tool: "network.request".into(),
        },
        AgentEvent::IntegrationBound {
            integration: "github".into(),
            label: "work".into(),
            host: "https://api.github.com".into(),
            private_network: false,
        },
        AgentEvent::IntegrationUnbound {
            integration: "github".into(),
            label: "work".into(),
            host: "https://api.github.com".into(),
            private_network: false,
        },
        AgentEvent::AgentDelegated {
            delegation_id: DelegationId::new(),
            parent_run_id: TaskRunId::new(),
            child_agent: "researcher".into(),
            child_task_id: TaskId::new(),
            child_run_id: TaskRunId::new(),
            objective: "o".into(),
            depth: 1,
        },
        AgentEvent::DelegationCompleted {
            delegation_id: DelegationId::new(),
            child_run_id: TaskRunId::new(),
            state: TaskState::Failed,
            steps: 3,
            tainted: true,
        },
        AgentEvent::DelegationRefused {
            child_agent: "sales".into(),
            reason: "that delegation would close a cycle: sales -> researcher -> sales".into(),
        },
    ]
}
