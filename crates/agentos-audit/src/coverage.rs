//! Whether the log holds the whole of a run, not only an unbroken chain.
//!
//! [`verify_chain`](crate::verify_chain) proves that no record was altered,
//! removed or reordered. It cannot prove that a record was ever written: a
//! tool execution whose ending was never recorded, or a run whose records were
//! never sealed at all, leaves a chain that verifies perfectly. Coverage is the
//! other half — that what the records say happened is accounted for by the
//! records — and it is checked here from the hashed payloads alone, so it needs
//! no table that could have been edited since.
//!
//! Three things are checked for a run and every run it delegated to:
//!
//! * every `tool.execution.started` has a matching completed or failed record;
//! * every `approval.granted` follows the `approval.requested` it answers;
//! * every child run named by an `agent.delegation.started` payload has
//!   records of its own, once the run that delegated to it has ended.
//!
//! The delegation edge is found through that payload, which names the parent
//! run and the child run inside the hash. No field was added to the record
//! format for it: the chain already totally orders every agent's events, so a
//! delegation tree needs no ordering of its own, only edges a verifier can
//! trust, and a payload is as trusted as anything the chain holds.

use std::collections::{HashMap, HashSet};

use agentos_core::event::AgentEvent;
use agentos_core::ids::{ApprovalId, DelegationId, TaskRunId, ToolExecutionId};

use crate::record::AuditRecord;

/// Something the records say happened that the records do not account for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoverageGap {
    /// A tool began and nothing records it finishing.
    ///
    /// Reported only for a run that has ended. While a run is live, a tool
    /// still executing is not a gap; once it has ended, the log cannot say
    /// what that tool did.
    #[error(
        "run {run_id}: `{tool}` started at record {sequence} (execution {execution_id}) and \
         nothing records it finishing"
    )]
    UnfinishedExecution {
        /// The run.
        run_id: TaskRunId,
        /// The execution.
        execution_id: ToolExecutionId,
        /// The tool.
        tool: String,
        /// Where it started.
        sequence: u64,
    },

    /// An approval was granted that the run's records never requested.
    #[error(
        "run {run_id}: approval {approval_id} was granted at record {sequence} and the run \
         never recorded asking for it"
    )]
    UnrequestedApproval {
        /// The run.
        run_id: TaskRunId,
        /// The approval.
        approval_id: ApprovalId,
        /// Where it was granted.
        sequence: u64,
    },

    /// A run delegated work to a child run that has no records.
    ///
    /// Reported only once the delegating run has ended. The parent seals the
    /// edge before its child writes anything, so while the parent is live a
    /// child with no records yet is one about to begin; once the parent has
    /// ended, every child it waited for has ended too, and one with no
    /// records never left any.
    #[error(
        "run {parent_run_id} delegated to run {child_run_id} (delegation {delegation_id}), \
         which has no records"
    )]
    MissingDelegatedRun {
        /// The delegation.
        delegation_id: DelegationId,
        /// The run that delegated.
        parent_run_id: TaskRunId,
        /// The run that should have records.
        child_run_id: TaskRunId,
    },

    /// The run asked about has no records at all.
    #[error("run {run_id} has no records")]
    MissingRun {
        /// The run.
        run_id: TaskRunId,
    },

    /// A record of a kind coverage depends on whose payload does not read as
    /// that kind, so what it accounts for cannot be told.
    #[error("record {sequence} is a `{kind}` whose payload cannot be read")]
    UnreadableRecord {
        /// Its position.
        sequence: u64,
        /// Its kind.
        kind: String,
    },
}

/// What [`verify_coverage`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageReport {
    /// The runs checked: the run asked about first, then the runs it
    /// delegated to, in the order the records name them.
    pub runs: Vec<TaskRunId>,
    /// The runs checked that the records do not show ending. Their coverage is
    /// as complete as it can be so far, not final.
    pub open_runs: Vec<TaskRunId>,
    /// Everything unaccounted for. Empty means the records cover the runs.
    pub gaps: Vec<CoverageGap>,
}

impl CoverageReport {
    /// Whether nothing was found unaccounted for.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.gaps.is_empty()
    }
}

/// Check that `records` account for `run` and every run it delegated to,
/// however deep.
///
/// `records` must be in ascending sequence order, as the log is read: an
/// approval is matched only to a request recorded before it. Pass the whole
/// log, or at least every record of the tree; a slice that leaves out a child's
/// records reports that child as missing, which is the point.
#[must_use]
pub fn verify_coverage(records: &[AuditRecord], run: TaskRunId) -> CoverageReport {
    let index = Index::build(records);
    let mut report = CoverageReport::default();
    if !index.by_run.contains_key(&run) {
        report.gaps.push(CoverageGap::MissingRun { run_id: run });
    }
    index.cover(&[run], &mut report);
    report
}

/// Check every run that appears in `records`, as [`verify_coverage`] does for
/// one, in a single pass over the log.
///
/// What a full audit check wants: calling [`verify_coverage`] once per run
/// would read the log once per run.
#[must_use]
pub fn verify_log_coverage(records: &[AuditRecord]) -> CoverageReport {
    let index = Index::build(records);
    let mut report = CoverageReport::default();
    index.cover(&index.runs_in_order, &mut report);
    report
}

/// What one record contributes to coverage.
enum Fact {
    ExecutionStarted(ToolExecutionId, String),
    ExecutionEnded(ToolExecutionId),
    ApprovalRequested(ApprovalId),
    ApprovalGranted(ApprovalId),
    Delegated {
        delegation_id: DelegationId,
        parent_run_id: TaskRunId,
        child_run_id: TaskRunId,
    },
    RunEnded,
    Unreadable,
    Irrelevant,
}

impl Fact {
    /// Read a record. Only the kinds coverage depends on are deserialised, so
    /// a log of routine records costs nothing beyond the scan.
    fn of(record: &AuditRecord) -> Self {
        const RELEVANT: &[&str] = &[
            "tool.execution.started",
            "tool.execution.completed",
            "tool.execution.failed",
            "approval.requested",
            "approval.granted",
            "agent.delegation.started",
            "agent.task.completed",
            "agent.task.failed",
            "agent.task.cancelled",
            "agent.state.transitioned",
        ];
        if !RELEVANT.contains(&record.kind.as_str()) {
            return Self::Irrelevant;
        }
        let Ok(event) = serde_json::from_value::<AgentEvent>(record.payload.clone()) else {
            return Self::Unreadable;
        };
        if event.kind() != record.kind {
            return Self::Unreadable;
        }
        match event {
            AgentEvent::ToolExecutionStarted {
                execution_id, tool, ..
            } => Self::ExecutionStarted(execution_id, tool),
            AgentEvent::ToolExecutionCompleted { execution_id, .. }
            | AgentEvent::ToolExecutionFailed { execution_id, .. } => {
                Self::ExecutionEnded(execution_id)
            }
            AgentEvent::ApprovalRequested { approval_id, .. } => {
                Self::ApprovalRequested(approval_id)
            }
            AgentEvent::ApprovalGranted { approval_id, .. } => Self::ApprovalGranted(approval_id),
            AgentEvent::AgentDelegated {
                delegation_id,
                parent_run_id,
                child_run_id,
                ..
            } => Self::Delegated {
                delegation_id,
                parent_run_id,
                child_run_id,
            },
            AgentEvent::TaskCompleted { .. }
            | AgentEvent::TaskFailed { .. }
            | AgentEvent::TaskCancelled { .. } => Self::RunEnded,
            // A run the process abandoned is ended by the reaper with a
            // transition and no task event, and has to count as ended too, or
            // the execution it left open would never be reported.
            AgentEvent::StateTransitioned { to, .. } if to.is_terminal() => Self::RunEnded,
            _ => Self::Irrelevant,
        }
    }
}

/// The log, read once and grouped by run.
struct Index<'a> {
    records: &'a [AuditRecord],
    facts: Vec<Fact>,
    /// Each run's records, by position in `records`, in log order.
    by_run: HashMap<TaskRunId, Vec<usize>>,
    /// Every run with a record, in the order it first appears.
    runs_in_order: Vec<TaskRunId>,
    /// The delegations each run made, as its records name them.
    children: HashMap<TaskRunId, Vec<(DelegationId, TaskRunId)>>,
}

impl<'a> Index<'a> {
    fn build(records: &'a [AuditRecord]) -> Self {
        let mut index = Self {
            records,
            facts: Vec::with_capacity(records.len()),
            by_run: HashMap::new(),
            runs_in_order: Vec::new(),
            children: HashMap::new(),
        };
        for (position, record) in records.iter().enumerate() {
            let fact = Fact::of(record);
            if let Some(run) = record.run_id {
                let positions = index.by_run.entry(run).or_insert_with(|| {
                    index.runs_in_order.push(run);
                    Vec::new()
                });
                positions.push(position);
            }
            // Keyed on the parent the payload names, not on the record's run
            // column: the edge a verifier trusts is the one sealed in the
            // payload.
            if let Fact::Delegated {
                delegation_id,
                parent_run_id,
                child_run_id,
            } = &fact
            {
                index
                    .children
                    .entry(*parent_run_id)
                    .or_default()
                    .push((*delegation_id, *child_run_id));
            }
            index.facts.push(fact);
        }
        index
    }

    /// Cover `roots` and every run reachable from them through delegation,
    /// each once, appending to `report`.
    fn cover(&self, roots: &[TaskRunId], report: &mut CoverageReport) {
        let mut seen: HashSet<TaskRunId> = HashSet::new();
        let mut queue: Vec<TaskRunId> = Vec::new();
        for root in roots {
            if seen.insert(*root) {
                queue.push(*root);
            }
        }

        let mut next = 0;
        while let Some(&run) = queue.get(next) {
            next += 1;
            report.runs.push(run);
            let ended = self.cover_run(run, report);

            for (delegation_id, child_run_id) in self.children.get(&run).into_iter().flatten() {
                if ended && !self.by_run.contains_key(child_run_id) {
                    report.gaps.push(CoverageGap::MissingDelegatedRun {
                        delegation_id: *delegation_id,
                        parent_run_id: run,
                        child_run_id: *child_run_id,
                    });
                }
                // A corrupt payload naming an ancestor as a child cannot send
                // this round for ever: every run is covered once.
                if seen.insert(*child_run_id) {
                    queue.push(*child_run_id);
                }
            }
        }
    }

    /// Check one run's own records, and say whether they show it ending.
    ///
    /// A run with no records at all has not been seen to end; whatever it
    /// delegated is reported by the run that delegated to it, not here.
    fn cover_run(&self, run: TaskRunId, report: &mut CoverageReport) -> bool {
        let Some(positions) = self.by_run.get(&run) else {
            return false;
        };

        let mut open: Vec<(ToolExecutionId, String, u64)> = Vec::new();
        let mut requested: HashSet<ApprovalId> = HashSet::new();
        let mut ended = false;

        for &position in positions {
            let sequence = self.records[position].sequence;
            match &self.facts[position] {
                Fact::ExecutionStarted(execution_id, tool) => {
                    open.push((*execution_id, tool.clone(), sequence));
                }
                Fact::ExecutionEnded(execution_id) => {
                    open.retain(|(started, _, _)| started != execution_id);
                }
                Fact::ApprovalRequested(approval_id) => {
                    requested.insert(*approval_id);
                }
                Fact::ApprovalGranted(approval_id) => {
                    if !requested.contains(approval_id) {
                        report.gaps.push(CoverageGap::UnrequestedApproval {
                            run_id: run,
                            approval_id: *approval_id,
                            sequence,
                        });
                    }
                }
                Fact::RunEnded => ended = true,
                Fact::Unreadable => report.gaps.push(CoverageGap::UnreadableRecord {
                    sequence,
                    kind: self.records[position].kind.clone(),
                }),
                Fact::Delegated { .. } | Fact::Irrelevant => {}
            }
        }

        if ended {
            report
                .gaps
                .extend(open.into_iter().map(|(execution_id, tool, sequence)| {
                    CoverageGap::UnfinishedExecution {
                        run_id: run,
                        execution_id,
                        tool,
                        sequence,
                    }
                }));
        } else {
            report.open_runs.push(run);
        }
        ended
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::event::Event;
    use agentos_core::ids::TaskId;
    use agentos_core::risk::RiskLevel;
    use agentos_core::task::{TaskState, TaskTrigger};

    use super::*;
    use crate::record::GENESIS_HASH;

    /// A log under construction, sealed as the real one is.
    #[derive(Default)]
    struct Log {
        records: Vec<AuditRecord>,
    }

    impl Log {
        fn push(&mut self, run: TaskRunId, payload: AgentEvent) -> &mut Self {
            let (sequence, prev) = self.records.last().map_or_else(
                || (1, GENESIS_HASH.to_owned()),
                |last| (last.sequence + 1, last.hash.clone()),
            );
            let event = Event::new(payload).for_run(run);
            self.records
                .push(AuditRecord::seal(&event, sequence, &prev).unwrap());
            self
        }

        fn started(&mut self, run: TaskRunId, execution_id: ToolExecutionId) -> &mut Self {
            self.push(
                run,
                AgentEvent::ToolExecutionStarted {
                    execution_id,
                    tool: "filesystem.write".into(),
                    arguments: serde_json::Value::Null,
                },
            )
        }

        fn finished(&mut self, run: TaskRunId, execution_id: ToolExecutionId) -> &mut Self {
            self.push(
                run,
                AgentEvent::ToolExecutionCompleted {
                    execution_id,
                    tool: "filesystem.write".into(),
                    duration_ms: 1,
                    success: true,
                    output_bytes: 0,
                },
            )
        }

        fn requested(&mut self, run: TaskRunId, approval_id: ApprovalId) -> &mut Self {
            self.push(
                run,
                AgentEvent::ApprovalRequested {
                    approval_id,
                    tool: "email.send".into(),
                    risk: RiskLevel::High,
                },
            )
        }

        fn granted(&mut self, run: TaskRunId, approval_id: ApprovalId) -> &mut Self {
            self.push(
                run,
                AgentEvent::ApprovalGranted {
                    approval_id,
                    tool: "email.send".into(),
                    waited_ms: 1,
                    note: None,
                },
            )
        }

        fn delegated(&mut self, parent: TaskRunId, child: TaskRunId) -> &mut Self {
            self.push(
                parent,
                AgentEvent::AgentDelegated {
                    delegation_id: DelegationId::new(),
                    parent_run_id: parent,
                    child_agent: "worker".into(),
                    child_task_id: TaskId::new(),
                    child_run_id: child,
                    objective: "o".into(),
                    depth: 1,
                },
            )
        }

        fn ended(&mut self, run: TaskRunId) -> &mut Self {
            self.push(
                run,
                AgentEvent::TaskCompleted {
                    steps: 1,
                    duration_ms: 1,
                },
            )
        }
    }

    #[test]
    fn a_whole_delegation_tree_is_covered() {
        let (root, child, grandchild) = (TaskRunId::new(), TaskRunId::new(), TaskRunId::new());
        let (one, two) = (ToolExecutionId::new(), ToolExecutionId::new());
        let approval = ApprovalId::new();
        let mut log = Log::default();
        log.delegated(root, child)
            .delegated(child, grandchild)
            .started(grandchild, one)
            .requested(child, approval)
            .finished(grandchild, one)
            .granted(child, approval)
            .started(child, two)
            .finished(child, two)
            .ended(grandchild)
            .ended(child)
            .ended(root);

        let report = verify_coverage(&log.records, root);
        assert!(report.is_complete(), "{:?}", report.gaps);
        assert_eq!(report.runs, vec![root, child, grandchild]);
        assert!(report.open_runs.is_empty());
        assert_eq!(verify_log_coverage(&log.records), report);
    }

    #[test]
    fn a_tool_that_never_finished_in_an_ended_run_is_a_gap() {
        let run = TaskRunId::new();
        let execution_id = ToolExecutionId::new();
        let mut log = Log::default();
        log.started(run, execution_id);

        // Live, it is a tool still running.
        let live = verify_coverage(&log.records, run);
        assert!(live.is_complete(), "{:?}", live.gaps);
        assert_eq!(live.open_runs, vec![run]);

        // Once the run has ended — here as the reaper ends an abandoned one,
        // with a transition and no task event — nothing will ever account for
        // it.
        log.push(
            run,
            AgentEvent::StateTransitioned {
                from: TaskState::Executing,
                to: TaskState::Failed,
                trigger: TaskTrigger::UnrecoverableError,
            },
        );
        let ended = verify_coverage(&log.records, run);
        assert_eq!(
            ended.gaps,
            vec![CoverageGap::UnfinishedExecution {
                run_id: run,
                execution_id,
                tool: "filesystem.write".into(),
                sequence: 1,
            }]
        );
        assert!(ended.open_runs.is_empty());
    }

    #[test]
    fn an_approval_granted_without_being_asked_for_is_a_gap() {
        let run = TaskRunId::new();
        let (forged, late) = (ApprovalId::new(), ApprovalId::new());
        let mut log = Log::default();
        log.granted(run, forged)
            // Asked for only after it was granted: the grant answered nothing.
            .granted(run, late)
            .requested(run, late)
            .ended(run);

        let report = verify_coverage(&log.records, run);
        assert_eq!(
            report.gaps,
            vec![
                CoverageGap::UnrequestedApproval {
                    run_id: run,
                    approval_id: forged,
                    sequence: 1,
                },
                CoverageGap::UnrequestedApproval {
                    run_id: run,
                    approval_id: late,
                    sequence: 2,
                },
            ]
        );
    }

    #[test]
    fn a_request_in_another_run_does_not_cover_a_grant_in_this_one() {
        let (asking, granting) = (TaskRunId::new(), TaskRunId::new());
        let approval = ApprovalId::new();
        let mut log = Log::default();
        log.requested(asking, approval).granted(granting, approval);
        assert!(!verify_coverage(&log.records, granting).is_complete());
    }

    #[test]
    fn a_delegated_run_with_no_records_is_a_gap() {
        let (root, present, absent) = (TaskRunId::new(), TaskRunId::new(), TaskRunId::new());
        let mut log = Log::default();
        log.delegated(root, present)
            .delegated(root, absent)
            .ended(present)
            .ended(root);

        let report = verify_coverage(&log.records, root);
        assert!(matches!(
            report.gaps.as_slice(),
            [CoverageGap::MissingDelegatedRun {
                parent_run_id,
                child_run_id,
                ..
            }] if *parent_run_id == root && *child_run_id == absent
        ));
        assert!(report.gaps[0].to_string().contains("has no records"));
    }

    #[test]
    fn a_child_not_yet_begun_under_a_live_parent_is_not_a_gap() {
        // The parent seals the edge, then the child is driven: for a moment
        // there is a started edge and nothing of the child's. A full check
        // pressed in that moment must not call a healthy tree incomplete.
        let (root, child) = (TaskRunId::new(), TaskRunId::new());
        let mut log = Log::default();
        log.delegated(root, child);

        let live = verify_coverage(&log.records, root);
        assert!(live.is_complete(), "{:?}", live.gaps);
        assert_eq!(live.open_runs, vec![root]);
        assert!(verify_log_coverage(&log.records).is_complete());

        // Ended without the child ever writing: now it is a gap.
        log.ended(root);
        assert!(matches!(
            verify_coverage(&log.records, root).gaps.as_slice(),
            [CoverageGap::MissingDelegatedRun { child_run_id, .. }] if *child_run_id == child
        ));
    }

    #[test]
    fn a_gap_deep_in_the_tree_is_found_from_its_root() {
        let (root, child, grandchild) = (TaskRunId::new(), TaskRunId::new(), TaskRunId::new());
        let execution_id = ToolExecutionId::new();
        let mut log = Log::default();
        log.delegated(root, child)
            .delegated(child, grandchild)
            .started(grandchild, execution_id)
            .ended(grandchild)
            .ended(child)
            .ended(root);

        let report = verify_coverage(&log.records, root);
        assert!(matches!(
            report.gaps.as_slice(),
            [CoverageGap::UnfinishedExecution { run_id, .. }] if *run_id == grandchild
        ));
    }

    #[test]
    fn a_run_with_no_records_is_not_reported_complete() {
        let run = TaskRunId::new();
        let report = verify_coverage(&[], run);
        assert_eq!(report.gaps, vec![CoverageGap::MissingRun { run_id: run }]);
    }

    #[test]
    fn a_payload_that_does_not_read_as_its_kind_is_a_gap() {
        let run = TaskRunId::new();
        let mut log = Log::default();
        log.started(run, ToolExecutionId::new());
        // Resealed after the edit, so the chain alone would not object.
        let record = &mut log.records[0];
        record.payload = serde_json::json!({"event": "tool.execution.started"});
        record.hash = record.compute_hash();

        let report = verify_coverage(&log.records, run);
        assert_eq!(
            report.gaps,
            vec![CoverageGap::UnreadableRecord {
                sequence: 1,
                kind: "tool.execution.started".into(),
            }]
        );
    }

    #[test]
    fn a_delegation_cycle_in_corrupt_payloads_is_walked_once() {
        let (a, b) = (TaskRunId::new(), TaskRunId::new());
        let mut log = Log::default();
        log.delegated(a, b).delegated(b, a).ended(a).ended(b);
        let report = verify_coverage(&log.records, a);
        assert!(report.is_complete(), "{:?}", report.gaps);
        assert_eq!(report.runs, vec![a, b]);
    }

    #[test]
    fn the_whole_log_is_checked_in_one_pass() {
        let (first, second) = (TaskRunId::new(), TaskRunId::new());
        let mut log = Log::default();
        log.started(first, ToolExecutionId::new())
            .ended(first)
            .granted(second, ApprovalId::new())
            .ended(second);

        let report = verify_log_coverage(&log.records);
        assert_eq!(report.runs, vec![first, second]);
        assert_eq!(report.gaps.len(), 2, "{:?}", report.gaps);
    }
}
