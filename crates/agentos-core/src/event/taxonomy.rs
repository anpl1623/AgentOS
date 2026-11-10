//! [`AgentEvent`]: every kind of thing the runtime records.
//!
//! One enum, kept whole: its serde tags are the names the audit table stores,
//! and the methods that name and classify an event match on it exhaustively,
//! so a new variant does not compile until somebody has decided what it is
//! called and whether it is security-relevant.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{ApprovalId, DelegationId, MemoryId, TaskId, TaskRunId, ToolExecutionId};
use crate::permission::{Capability, Effect};
use crate::risk::RiskLevel;
use crate::task::{TaskState, TaskTrigger};
use crate::trust::DataSource;

/// What happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A run began.
    #[serde(rename = "agent.task.started")]
    TaskStarted {
        /// The objective being attempted.
        objective: String,
        /// Attempt number.
        attempt: u32,
    },

    /// A run finished successfully.
    #[serde(rename = "agent.task.completed")]
    TaskCompleted {
        /// Model turns consumed.
        steps: u32,
        /// Wall-clock duration.
        duration_ms: u64,
    },

    /// A run failed.
    #[serde(rename = "agent.task.failed")]
    TaskFailed {
        /// Why.
        reason: String,
        /// Model turns consumed.
        steps: u32,
    },

    /// A run was stopped by the operator.
    #[serde(rename = "agent.task.cancelled")]
    TaskCancelled {
        /// Model turns consumed before stopping.
        steps: u32,
    },

    /// The state machine moved.
    #[serde(rename = "agent.state.transitioned")]
    StateTransitioned {
        /// Previous state.
        from: TaskState,
        /// New state.
        to: TaskState,
        /// What caused it.
        trigger: TaskTrigger,
    },

    /// A model turn began.
    #[serde(rename = "agent.model.request.started")]
    ModelRequestStarted {
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Messages sent.
        message_count: usize,
        /// Tools advertised.
        tool_count: usize,
    },

    /// A model turn finished.
    #[serde(rename = "agent.model.request.completed")]
    ModelRequestCompleted {
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// Latency.
        duration_ms: u64,
        /// Input tokens, when reported.
        input_tokens: Option<u64>,
        /// Output tokens, when reported.
        output_tokens: Option<u64>,
        /// Tool calls the model requested.
        tool_calls: usize,
    },

    /// A model turn errored.
    #[serde(rename = "agent.model.request.failed")]
    ModelRequestFailed {
        /// Provider identifier.
        provider: String,
        /// Detail.
        error: String,
    },

    /// The model finished reasoning about what to do next.
    #[serde(rename = "agent.reasoning.completed")]
    ReasoningCompleted {
        /// The model's prose, truncated for the log.
        summary: String,
        /// Tool calls it decided on.
        tool_calls: usize,
    },

    /// The policy engine was consulted.
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        /// The tool.
        tool: String,
        /// The capability requested.
        capability: Capability,
        /// Assessed risk.
        risk: RiskLevel,
        /// Whether the run was tainted at the time.
        tainted: bool,
    },

    /// The policy engine allowed an action.
    #[serde(rename = "permission.granted")]
    PermissionGranted {
        /// The tool.
        tool: String,
        /// The capability.
        capability: Capability,
        /// The rule that matched, if any.
        matched_rule: Option<String>,
    },

    /// The policy engine refused an action.
    #[serde(rename = "permission.denied")]
    PermissionDenied {
        /// The tool.
        tool: String,
        /// The capability.
        capability: Capability,
        /// Why.
        reason: String,
        /// The rule that matched, if any.
        matched_rule: Option<String>,
    },

    /// Taint escalation changed a decision.
    #[serde(rename = "permission.escalated_by_taint")]
    PermissionEscalatedByTaint {
        /// The tool.
        tool: String,
        /// What the policy alone would have said.
        original: Effect,
        /// What the runtime decided instead.
        escalated: Effect,
    },

    /// The run ingested data from outside the trust boundary.
    #[serde(rename = "agent.taint.raised")]
    TaintRaised {
        /// Where the data came from.
        source: DataSource,
        /// The tool that brought it in.
        tool: String,
    },

    /// A human was asked to decide.
    #[serde(rename = "approval.requested")]
    ApprovalRequested {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// Assessed risk.
        risk: RiskLevel,
    },

    /// A human approved.
    #[serde(rename = "approval.granted")]
    ApprovalGranted {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// How long the human took.
        waited_ms: u64,
        /// Why they said yes, if they wrote it down.
        ///
        /// The chain can already prove that a person allowed a high-risk call
        /// on a tainted run; this is what lets it say why. Defaulted so that
        /// records written before the field existed still read.
        #[serde(default)]
        note: Option<String>,
    },

    /// A human declined, or the run's approval budget refused without asking.
    #[serde(rename = "approval.denied")]
    ApprovalDenied {
        /// The request.
        approval_id: ApprovalId,
        /// The tool.
        tool: String,
        /// Their note, if any.
        note: Option<String>,
        /// Set when nobody was asked because the run had spent its approval
        /// budget, so the chain says who refused without anyone parsing the
        /// note. Omitted when false, which keeps a person's denial recorded in
        /// the shape it always had, and defaulted so that records written
        /// before the field existed still read.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        over_budget: bool,
    },

    /// A tool invocation began.
    #[serde(rename = "tool.execution.started")]
    ToolExecutionStarted {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Validated arguments.
        arguments: serde_json::Value,
    },

    /// A tool invocation finished.
    #[serde(rename = "tool.execution.completed")]
    ToolExecutionCompleted {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Latency.
        duration_ms: u64,
        /// Whether it succeeded.
        success: bool,
        /// Bytes of output produced.
        output_bytes: usize,
    },

    /// A tool invocation failed.
    #[serde(rename = "tool.execution.failed")]
    ToolExecutionFailed {
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Latency before failing.
        duration_ms: u64,
        /// Detail.
        error: String,
    },

    /// The model asked for a tool that failed schema validation.
    #[serde(rename = "tool.arguments.rejected")]
    ToolArgumentsRejected {
        /// The tool.
        tool: String,
        /// Why the arguments were rejected.
        error: String,
    },

    /// The model asked for a tool that does not exist or is not enabled.
    #[serde(rename = "tool.unknown")]
    UnknownToolRequested {
        /// What it asked for.
        tool: String,
    },

    /// A tool planned a capability its manifest does not declare.
    ///
    /// The call was not failed for it: the policy engine evaluated the plan the
    /// tool actually produced, so the decision stands on what would really
    /// happen rather than on what the tool said it would do. The discrepancy is
    /// recorded because a manifest that understates a tool's reach is exactly
    /// how an operator comes to grant more than they meant to.
    #[serde(rename = "tool.manifest_exceeded")]
    ToolManifestExceeded {
        /// The tool.
        tool: String,
        /// The planned capabilities absent from its manifest.
        undeclared: Vec<String>,
    },

    /// A memory was written.
    #[serde(rename = "agent.memory.recorded")]
    MemoryRecorded {
        /// Kind of memory.
        kind: String,
        /// Where the claim came from.
        source: DataSource,
    },

    /// A schedule came due and produced a task.
    ///
    /// The alias reads records written before the tag was brought into line
    /// with [`AgentEvent::kind`]; the log is append-only, so those records are
    /// never rewritten and must go on deserialising.
    #[serde(rename = "schedule.fired", alias = "schedule_fired")]
    ScheduleFired {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name, so a deleted schedule's history still reads.
        name: String,
        /// What it created.
        task_id: TaskId,
    },

    /// A task was given up on because something it depended on will not
    /// succeed.
    ///
    /// Recorded rather than left implicit: a task that silently waits forever is
    /// indistinguishable from one nobody has got to yet. The alias reads records
    /// written under the tag this variant carried before it matched its kind.
    #[serde(rename = "agent.task.abandoned", alias = "task_abandoned")]
    TaskAbandoned {
        /// The task.
        task_id: TaskId,
        /// The dependency that ended it.
        blocked_by: TaskId,
        /// What happened to that dependency.
        reason: String,
    },

    /// A run delegated part of its work to another agent, and the child run
    /// that will carry it out has been prepared.
    ///
    /// Recorded once the child run exists, so the record can name it: the
    /// edge between the two runs is sealed in this payload, inside the hash,
    /// rather than reconstructed later from a table that can be edited. The
    /// `delegations` row was written earlier still, before anything started,
    /// so a delegation that could not be recorded was never performed.
    #[serde(rename = "agent.delegation.started")]
    AgentDelegated {
        /// The delegation.
        delegation_id: DelegationId,
        /// The run that asked.
        parent_run_id: TaskRunId,
        /// The agent asked, by name, so the record reads after it is deleted.
        child_agent: String,
        /// The task created for the child.
        child_task_id: TaskId,
        /// The run carrying it out.
        child_run_id: TaskRunId,
        /// What the child was asked to do.
        objective: String,
        /// How far below the root run the child sits.
        depth: u8,
    },

    /// A delegated run ended, however it ended.
    ///
    /// A failed child is recorded here as well as a successful one: it is a
    /// result the parent reads, not an error it raises.
    #[serde(rename = "agent.delegation.completed")]
    DelegationCompleted {
        /// The delegation.
        delegation_id: DelegationId,
        /// The run that carried it out.
        child_run_id: TaskRunId,
        /// The state the child run ended in.
        state: TaskState,
        /// Model turns the child run took.
        steps: u32,
        /// Whether the child had read untrusted data by the time it finished.
        tainted: bool,
    },

    /// A run asked for a delegation it may not make.
    ///
    /// Security-relevant for the reason an unknown tool is: an agent reaching
    /// past what it was given, whether to an agent that does not exist, to one
    /// already on its own chain, or beyond the depth or width the operator set.
    #[serde(rename = "agent.delegation.refused")]
    DelegationRefused {
        /// The agent it asked for, by the name it used.
        child_agent: String,
        /// Why it was refused.
        reason: String,
    },

    /// An operator installed a policy for an agent.
    ///
    /// The document is carried whole. The policies table keeps only the
    /// current version, so without it a policy widened shortly before a bad
    /// action and narrowed again afterwards would leave no trace of what it
    /// said while the action ran.
    #[serde(rename = "operator.policy.changed")]
    PolicyChanged {
        /// The agent's name, so the record reads after the agent is deleted.
        agent: String,
        /// The version the policy now has.
        version: i64,
        /// The YAML as installed.
        document: String,
    },

    /// An operator created an agent.
    #[serde(rename = "operator.agent.created")]
    AgentCreated {
        /// Its name.
        agent: String,
        /// Provider identifier.
        provider: String,
        /// Model identifier.
        model: String,
        /// The tools it was given.
        tools: Vec<String>,
    },

    /// An operator switched an agent on or off.
    #[serde(rename = "operator.agent.enabled_changed")]
    AgentEnabledChanged {
        /// Its name.
        agent: String,
        /// Whether it may now run.
        enabled: bool,
    },

    /// An operator stored a credential for a model provider.
    ///
    /// Names the provider and nothing else. Neither the key nor any part of it
    /// is recorded: the log is evidence, and evidence that holds a credential
    /// is one more place to steal it from.
    #[serde(rename = "operator.provider_key.set")]
    ProviderKeySet {
        /// Provider identifier.
        provider: String,
    },

    /// An operator removed a stored provider credential.
    #[serde(rename = "operator.provider_key.removed")]
    ProviderKeyRemoved {
        /// Provider identifier.
        provider: String,
    },

    /// An operator stored a credential for a network origin.
    ///
    /// Names the origin and the credential and nothing else, for the reason the
    /// provider events give. The origin is canonical, as it is in the key the
    /// credential is stored under.
    #[serde(rename = "operator.credential.set")]
    CredentialSet {
        /// The canonical origin the credential is bound to.
        origin: String,
        /// Its name.
        name: String,
    },

    /// An operator removed a stored network credential.
    #[serde(rename = "operator.credential.removed")]
    CredentialRemoved {
        /// The canonical origin it was bound to.
        origin: String,
        /// Its name.
        name: String,
    },

    /// An operator bound an account of an integration.
    ///
    /// The token is a network credential, stored first and recorded by its own
    /// `operator.credential.set`; this records the account row that points at
    /// it. `private_network` is here because it is the one setting that widens
    /// where the account's requests may go, and only the operator can set it.
    #[serde(rename = "operator.integration.bound")]
    IntegrationBound {
        /// The integration, e.g. `github`.
        integration: String,
        /// The account's label, which is also its credential's name.
        label: String,
        /// The API base URL every request for this account goes to.
        host: String,
        /// Whether the account may reach private-network addresses.
        private_network: bool,
    },

    /// An operator unbound an account of an integration.
    ///
    /// Recorded when the row is gone. The credential behind it is removed after
    /// and recorded by its own `operator.credential.removed`.
    #[serde(rename = "operator.integration.unbound")]
    IntegrationUnbound {
        /// The integration.
        integration: String,
        /// The account's label.
        label: String,
        /// The API base URL it was bound to.
        host: String,
        /// Whether it could reach private-network addresses.
        private_network: bool,
    },

    /// An operator wrote a memory for an agent.
    ///
    /// The content is carried whole, as a policy's document is. Memories are
    /// retrieved into the conversation before planning, so what one said while a
    /// run planned is part of the explanation of that run, and the row may since
    /// have been revised or forgotten.
    #[serde(rename = "operator.memory.recorded")]
    MemoryRemembered {
        /// The agent's name, so the record reads after the agent is deleted.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// Kind of memory.
        kind: String,
        /// What it says.
        content: String,
    },

    /// An operator rewrote a memory.
    #[serde(rename = "operator.memory.revised")]
    MemoryRevised {
        /// The agent's name.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// What it says now.
        content: String,
    },

    /// An operator deleted a memory.
    ///
    /// Carries what was forgotten: the row is gone, and a record that says only
    /// that something was removed cannot explain a run that read it earlier.
    #[serde(rename = "operator.memory.forgotten")]
    MemoryForgotten {
        /// The agent's name.
        agent: String,
        /// The memory.
        memory_id: MemoryId,
        /// Kind of memory.
        kind: String,
        /// What it said.
        content: String,
    },

    /// An operator created a schedule.
    ///
    /// A schedule is standing permission for work to start with nobody present,
    /// so its objective and cadence are kept as they were when it was created.
    #[serde(rename = "operator.schedule.created")]
    ScheduleCreated {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
        /// The objective each firing gets.
        objective: String,
        /// How often it fires.
        cadence: crate::schedule::Cadence,
        /// Its first occurrence.
        next_run_at: Option<Timestamp>,
    },

    /// An operator stopped a schedule firing.
    #[serde(rename = "operator.schedule.paused")]
    SchedulePaused {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
    },

    /// An operator started a paused schedule firing again.
    #[serde(rename = "operator.schedule.resumed")]
    ScheduleResumed {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name.
        name: String,
        /// When it fires next, computed forward from the moment of resuming;
        /// `None` for a one-shot whose moment passed while it was paused.
        next_run_at: Option<Timestamp>,
    },

    /// An operator deleted a schedule. The tasks it created are kept.
    #[serde(rename = "operator.schedule.deleted")]
    ScheduleDeleted {
        /// The schedule.
        schedule_id: crate::ids::ScheduleId,
        /// Its name, which is all a reader has once the row is gone.
        name: String,
    },

    /// A tool was given a stored credential to send.
    ///
    /// Recorded by the pipeline from what the credential store released to the
    /// call, not from what the tool reports, so a tool cannot spend a credential
    /// without it appearing here. It is written at the moment of release,
    /// before the tool holds the value, and the value is withheld if it cannot
    /// be written: a request still in flight when the process exits has
    /// already left this behind. It therefore says the value left the store,
    /// not that a server received it. Like the operator events it names the
    /// origin and the credential, never the value.
    #[serde(rename = "network.credential.used")]
    CredentialUsed {
        /// The canonical origin the credential is bound to.
        origin: String,
        /// Its name.
        name: String,
        /// The tool that was given it.
        tool: String,
    },

    /// An operator created a task.
    ///
    /// Whether it runs at once or waits, the objective is trusted control-plane
    /// text, and the record is what ties it to the person who typed it.
    #[serde(rename = "operator.task.created")]
    TaskCreated {
        /// What the agent is asked to do.
        objective: String,
        /// The tasks it waits for.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        depends_on: Vec<TaskId>,
        /// The earliest moment it may start.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scheduled_for: Option<Timestamp>,
    },

    /// An operator made a task wait for another.
    #[serde(rename = "operator.task.dependency_added")]
    TaskDependencyAdded {
        /// The task that now waits.
        task_id: TaskId,
        /// The task it waits for.
        depends_on: TaskId,
    },

    /// A scheduler began acting on schedules and queued tasks.
    ///
    /// From here until the matching stop, work starts with nobody present.
    /// The options are recorded because they bound how much of it.
    #[serde(rename = "operator.scheduler.started")]
    SchedulerStarted {
        /// Seconds between ticks.
        tick_seconds: u64,
        /// How many runs it may have in flight at once.
        max_concurrent_runs: u32,
        /// Started by a preference saved earlier, as the application opened,
        /// rather than by somebody asking for it then.
        on_launch: bool,
    },

    /// A scheduler stopped.
    #[serde(rename = "operator.scheduler.stopped")]
    SchedulerStopped {
        /// Seconds between ticks.
        tick_seconds: u64,
        /// How many runs it could have in flight at once.
        max_concurrent_runs: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::samples::{VARIANT_COUNT, sample_events, variant_index};
    use crate::ids::ScheduleId;

    #[test]
    fn every_variant_is_sampled() {
        // Sorted indices equal to 0..VARIANT_COUNT means every variant appears
        // exactly once: a missing one leaves a gap, a duplicate repeats an
        // index, and a variant added without bumping the count overshoots.
        let mut indices: Vec<usize> = sample_events().iter().map(variant_index).collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..VARIANT_COUNT).collect::<Vec<_>>());
    }

    #[test]
    fn a_refusal_by_budget_is_told_apart_from_a_person_saying_no() {
        let by_budget = AgentEvent::ApprovalDenied {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            note: Some("over budget".into()),
            over_budget: true,
        };
        let json = serde_json::to_value(&by_budget).unwrap();
        assert_eq!(json["over_budget"], serde_json::Value::Bool(true));
        assert_eq!(
            serde_json::from_value::<AgentEvent>(json).unwrap(),
            by_budget
        );

        // A person's denial keeps the shape it had before the field existed,
        // and a record written then still reads as a person's denial.
        let by_person = AgentEvent::ApprovalDenied {
            approval_id: ApprovalId::new(),
            tool: "t".into(),
            note: None,
            over_budget: false,
        };
        let json = serde_json::to_value(&by_person).unwrap();
        assert!(json.get("over_budget").is_none(), "{json}");
        assert_eq!(
            serde_json::from_value::<AgentEvent>(json).unwrap(),
            by_person
        );
    }

    #[test]
    fn events_round_trip_through_json() {
        for event in sample_events() {
            let json = serde_json::to_string(&event).unwrap();
            let back: AgentEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event);
        }
    }

    #[test]
    fn records_written_under_the_old_tags_still_deserialise() {
        // These two once serialised under snake_case tags that disagreed with
        // their kind. The log is append-only, so those records are still there.
        let schedule_id = ScheduleId::new();
        let task_id = TaskId::new();
        let fired: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "schedule_fired",
            "schedule_id": schedule_id,
            "name": "hourly",
            "task_id": task_id,
        }))
        .unwrap();
        assert_eq!(
            fired,
            AgentEvent::ScheduleFired {
                schedule_id,
                name: "hourly".into(),
                task_id,
            }
        );

        let blocked_by = TaskId::new();
        let abandoned: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "task_abandoned",
            "task_id": task_id,
            "blocked_by": blocked_by,
            "reason": "failed",
        }))
        .unwrap();
        assert_eq!(
            abandoned,
            AgentEvent::TaskAbandoned {
                task_id,
                blocked_by,
                reason: "failed".into(),
            }
        );

        // And they are written back under the tag that matches the kind.
        let tag = serde_json::to_value(&abandoned).unwrap()["event"].clone();
        assert_eq!(tag, "agent.task.abandoned");
    }

    #[test]
    fn an_approval_recorded_before_notes_existed_still_reads() {
        let approval_id = ApprovalId::new();
        let granted: AgentEvent = serde_json::from_value(serde_json::json!({
            "event": "approval.granted",
            "approval_id": approval_id,
            "tool": "email.send",
            "waited_ms": 1200,
        }))
        .unwrap();
        assert_eq!(
            granted,
            AgentEvent::ApprovalGranted {
                approval_id,
                tool: "email.send".into(),
                waited_ms: 1200,
                note: None,
            }
        );
    }

    #[test]
    fn credential_events_carry_no_secret_material() {
        // A network credential passes near five events. Each is pinned to the
        // fields that name the credential, so a value cannot be added to one
        // without changing this list.
        for (event, expected) in [
            (
                AgentEvent::CredentialSet {
                    origin: "https://api.example.com".into(),
                    name: "default".into(),
                },
                vec!["event", "name", "origin"],
            ),
            (
                AgentEvent::CredentialRemoved {
                    origin: "https://api.example.com".into(),
                    name: "default".into(),
                },
                vec!["event", "name", "origin"],
            ),
            (
                AgentEvent::CredentialUsed {
                    origin: "https://api.example.com".into(),
                    name: "default".into(),
                    tool: "network.request".into(),
                },
                vec!["event", "name", "origin", "tool"],
            ),
            // The account rows point at a credential by its label; the token
            // is never theirs to carry.
            (
                AgentEvent::IntegrationBound {
                    integration: "github".into(),
                    label: "work".into(),
                    host: "https://ghe.example/api/v3".into(),
                    private_network: true,
                },
                vec!["event", "host", "integration", "label", "private_network"],
            ),
            (
                AgentEvent::IntegrationUnbound {
                    integration: "github".into(),
                    label: "work".into(),
                    host: "https://ghe.example/api/v3".into(),
                    private_network: true,
                },
                vec!["event", "host", "integration", "label", "private_network"],
            ),
        ] {
            let json = serde_json::to_value(&event).unwrap();
            let mut fields: Vec<&str> = json
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            fields.sort_unstable();
            assert_eq!(fields, expected, "{}", event.kind());
        }
    }

    #[test]
    fn operator_events_carry_no_key_material() {
        // The provider events are the only ones a credential passes near. Their
        // serialised form is pinned to the tag and the provider id, so a field
        // added later has to come through this test to get into the log.
        for event in [
            AgentEvent::ProviderKeySet {
                provider: "anthropic".into(),
            },
            AgentEvent::ProviderKeyRemoved {
                provider: "anthropic".into(),
            },
        ] {
            let json = serde_json::to_value(&event).unwrap();
            let mut fields: Vec<&str> = json
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            fields.sort_unstable();
            assert_eq!(fields, vec!["event", "provider"]);
        }
    }

    #[test]
    fn a_delegation_record_seals_both_ends_of_the_edge() {
        // Coverage verification finds a child run through this payload alone,
        // so the fields it reads are pinned: a rename here would leave every
        // delegated run unreachable from its parent's records.
        let parent_run_id = TaskRunId::new();
        let child_run_id = TaskRunId::new();
        let event = AgentEvent::AgentDelegated {
            delegation_id: DelegationId::new(),
            parent_run_id,
            child_agent: "researcher".into(),
            child_task_id: TaskId::new(),
            child_run_id,
            objective: "o".into(),
            depth: 1,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "agent.delegation.started");
        assert_eq!(json["parent_run_id"], parent_run_id.to_string());
        assert_eq!(json["child_run_id"], child_run_id.to_string());
    }
}
