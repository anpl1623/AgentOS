//! A run's trace: its steps in order, each child it delegated to beneath the
//! call that asked for it, its tool calls, and how it ended.

use std::collections::HashMap;

use agentos_core::delegation::Delegation;
use agentos_core::ids::ToolExecutionId;
use agentos_core::task::TaskRun;
use agentos_runtime::RunTrace;
use agentos_runtime::delegation::TOOL_NAME as DELEGATE_TOOL;

use super::{Style, pad, rule, summary_line, truncate};

/// One delegation as a trace shows it, with the agent at its far end named.
///
/// The delegation row records the edge by identifier. Resolving the name and
/// the run behind it is a database read, and this module stays away from the
/// database, so the command gathers them and hands them in.
#[derive(Debug, Clone)]
pub struct DelegationEdge {
    /// The edge itself.
    pub delegation: Delegation,
    /// The agent at the other end: the child for a run's children, the parent
    /// for the edge that started it.
    pub agent: String,
    /// That agent's run, once one exists.
    pub run: Option<TaskRun>,
}

/// Where a run sits in a delegation tree: who started it, and what it started.
///
/// Empty for a run nobody delegated and that delegated nothing, which is every
/// run before delegation existed.
#[derive(Debug, Clone, Default)]
pub struct Lineage {
    /// The edge that started this run, when another agent asked for it.
    pub delegated_by: Option<DelegationEdge>,
    /// Every child this run started, in the order it asked.
    pub children: Vec<DelegationEdge>,
}

/// Render a full execution trace.
///
/// `include_header` is false when the caller has already printed the objective
/// and agent — during `task run`, that banner goes out before the work starts so
/// there is something on screen while the agent thinks.
///
/// Each child in `lineage` is printed beneath the `agent.delegate` step that
/// started it, so the trace reads in the order things happened rather than
/// collecting every child at the end where nothing says which call asked.
#[must_use]
pub fn trace(trace: &RunTrace, lineage: &Lineage, style: &Style, include_header: bool) -> String {
    let mut out = String::new();
    if include_header {
        out.push_str(&format!(
            "{} {}\n",
            style.dim("Objective"),
            style.bold(&trace.objective)
        ));
        out.push_str(&format!(
            "{} {}   {} {}   {} attempt {}\n",
            style.dim("Agent"),
            trace.agent_name,
            style.dim("State"),
            style.state(trace.run.state),
            style.dim("Run"),
            trace.run.attempt,
        ));
        out.push_str(&format!("{}\n", rule()));
    }
    if let Some(parent) = &lineage.delegated_by {
        out.push_str(&format!(
            "{} {}   {} {}\n",
            style.dim("Asked by"),
            parent.agent,
            style.dim("Run"),
            parent.delegation.parent_run_id,
        ));
    }
    if trace.run.tainted {
        out.push_str(&format!(
            "{}\n",
            style.yellow("This run read untrusted data.")
        ));
    }

    let (mut placed, unplaced) = place_children(trace, &lineage.children);
    for step in &trace.steps {
        let marker = match step.kind {
            agentos_core::task::TaskStepKind::Planning => "◆",
            agentos_core::task::TaskStepKind::ToolCall => "▶",
            agentos_core::task::TaskStepKind::Approval => "?",
            agentos_core::task::TaskStepKind::Verification => "✓",
            agentos_core::task::TaskStepKind::Recovery => "↻",
        };
        out.push_str(&format!(
            "{} {} {}\n",
            style.dim(&format!("{:>3}", step.ordinal)),
            marker,
            step.summary
        ));
        if let Some(children) = step
            .tool_execution_id
            .and_then(|execution| placed.remove(&execution))
        {
            out.push_str(&child_lines(&children, style));
        }
    }
    // A delegate call that has not returned has no step yet, and neither does
    // one whose run died under it. Its children are still this run's.
    if !unplaced.is_empty() {
        out.push_str(&format!("{STEP_INDENT}{}\n", style.dim("Delegated")));
        out.push_str(&child_lines(&unplaced, style));
    }

    if !trace.executions.is_empty() {
        out.push_str(&format!("{}\n", rule()));
        out.push_str(&format!("{}\n", style.dim("Tool calls")));
        for execution in &trace.executions {
            let outcome = if execution.outcome == agentos_core::tool::ToolOutcome::Success {
                style.green(execution.outcome.as_str())
            } else {
                style.red(execution.outcome.as_str())
            };
            out.push_str(&format!(
                "  {}{}{}{:>6}ms  {}\n",
                pad(&execution.tool, 24),
                pad(&outcome, 20),
                pad(execution.effect.as_str(), 8),
                execution.duration_ms,
                style.risk(execution.risk),
            ));
            if let Some(error) = &execution.error {
                out.push_str(&format!("    {}\n", style.dim(&truncate(error, 100))));
            }
        }
    }

    if let Some(result) = &trace.run.result {
        out.push_str(&format!("{}\n", rule()));
        out.push_str(&format!("{}\n{result}\n", style.dim("Result")));
    }
    if let Some(failure) = &trace.run.failure {
        out.push_str(&format!("{}\n", rule()));
        out.push_str(&format!("{} {failure}\n", style.red("Failed:")));
    }

    out
}

/// Where a step's summary begins: the ordinal's three columns, the marker and
/// a space either side of it.
const STEP_INDENT: &str = "      ";

/// Attach each child to the `agent.delegate` call that asked for it.
///
/// A delegation row names its parent run but not the call, so the call is the
/// one under way when the row was written: started before it was requested and
/// not finished until after. Only calls a step refers to are candidates, since
/// a child placed under a call no step prints would not be printed at all.
fn place_children<'a>(
    trace: &RunTrace,
    children: &'a [DelegationEdge],
) -> (
    HashMap<ToolExecutionId, Vec<&'a DelegationEdge>>,
    Vec<&'a DelegationEdge>,
) {
    let calls: Vec<_> = trace
        .executions
        .iter()
        .filter(|execution| execution.tool == DELEGATE_TOOL)
        .filter(|execution| {
            trace
                .steps
                .iter()
                .any(|step| step.tool_execution_id == Some(execution.id))
        })
        .collect();

    let mut placed: HashMap<ToolExecutionId, Vec<&DelegationEdge>> = HashMap::new();
    let mut unplaced = Vec::new();
    for child in children {
        let asked = child.delegation.requested_at;
        let call = calls
            .iter()
            .filter(|call| {
                call.started_at <= asked && call.completed_at.is_none_or(|ended| asked <= ended)
            })
            .max_by_key(|call| call.started_at);
        match call {
            Some(call) => placed.entry(call.id).or_default().push(child),
            None => unplaced.push(child),
        }
    }
    (placed, unplaced)
}

/// One line per child, branching from beneath the step that asked.
fn child_lines(children: &[&DelegationEdge], style: &Style) -> String {
    let mut out = String::new();
    for (index, child) in children.iter().enumerate() {
        let branch = if index + 1 == children.len() {
            "└─"
        } else {
            "├─"
        };
        out.push_str(&format!(
            "{STEP_INDENT}{} {}\n",
            style.dim(branch),
            edge_summary(child, style)
        ));
    }
    out
}

/// `<agent> · <state> · <steps> · tainted`, the last only when it was.
fn edge_summary(edge: &DelegationEdge, style: &Style) -> String {
    // Once a delegation has ended, its own state is the outcome the parent was
    // given. Before that the child's run says how far it has got — and it is
    // also what tells the truth about a delegation left `running` by a process
    // that died, whose run has since been reaped as failed.
    let state = match &edge.run {
        Some(run) if !edge.delegation.state.is_terminal() => style.state(run.state),
        _ => style.delegation_state(edge.delegation.state),
    };
    let steps = edge
        .run
        .as_ref()
        .map_or(edge.delegation.steps_spent, |run| run.steps_taken);
    let tainted = edge.run.as_ref().is_some_and(|run| run.tainted);
    summary_line(&edge.agent, &state, steps, tainted, style)
}

#[cfg(test)]
mod tests {
    use agentos_core::Timestamp;
    use agentos_core::delegation::DelegationState;
    use agentos_core::ids::{AgentId, TaskId, TaskStepId};
    use agentos_core::permission::Effect;
    use agentos_core::risk::RiskLevel;
    use agentos_core::task::{TaskState, TaskStep, TaskStepKind};
    use agentos_core::tool::ToolOutcome;
    use agentos_persistence::ToolExecutionRecord;

    use super::*;
    use crate::render::test_support::{child_run, plain};

    fn at(base: Timestamp, seconds: i64) -> Timestamp {
        base + chrono::Duration::seconds(seconds)
    }

    fn execution(run: &TaskRun, tool: &str, from: Timestamp, to: Timestamp) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: ToolExecutionId::new(),
            run_id: run.id,
            tool: tool.into(),
            call_id: "call".into(),
            arguments: serde_json::json!({}),
            outcome: ToolOutcome::Success,
            effect: Effect::Allow,
            risk: RiskLevel::High,
            tainted: false,
            approval_id: None,
            output_bytes: 0,
            error: None,
            duration_ms: 0,
            started_at: from,
            completed_at: Some(to),
        }
    }

    fn step(
        run: &TaskRun,
        ordinal: u32,
        summary: &str,
        call: Option<&ToolExecutionRecord>,
        when: Timestamp,
    ) -> TaskStep {
        TaskStep {
            id: TaskStepId::new(),
            run_id: run.id,
            ordinal,
            kind: if call.is_some() {
                TaskStepKind::ToolCall
            } else {
                TaskStepKind::Planning
            },
            state: TaskState::Executing,
            summary: summary.into(),
            tool_execution_id: call.map(|call| call.id),
            detail: None,
            at: when,
        }
    }

    fn edge(
        parent: &TaskRun,
        agent: &str,
        asked: Timestamp,
        state: DelegationState,
        run: Option<TaskRun>,
    ) -> DelegationEdge {
        let mut delegation = Delegation::requested(
            parent.id,
            AgentId::new(),
            AgentId::new(),
            run.as_ref().map_or_else(TaskId::new, |run| run.task_id),
            format!("help {agent}"),
            1,
        );
        delegation.requested_at = asked;
        delegation.state = state;
        delegation.child_run_id = run.as_ref().map(|run| run.id);
        DelegationEdge {
            delegation,
            agent: agent.into(),
            run,
        }
    }

    fn supervisor_trace(
        parent: TaskRun,
        steps: Vec<TaskStep>,
        executions: Vec<ToolExecutionRecord>,
    ) -> RunTrace {
        RunTrace {
            run: parent,
            agent_name: "supervisor".into(),
            objective: "Summarise the week".into(),
            steps,
            executions,
            approvals: vec![],
            delegations: vec![],
            delegated_by: None,
        }
    }

    fn line_of(text: &str, needle: &str) -> usize {
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no line contains {needle:?} in:\n{text}"))
    }

    #[test]
    fn a_child_is_printed_beneath_the_call_that_started_it() {
        let base = agentos_core::now();
        let parent = child_run(TaskState::Completed, 4, true);
        let reading = execution(&parent, "filesystem.read", at(base, 1), at(base, 2));
        let first = execution(&parent, DELEGATE_TOOL, at(base, 3), at(base, 10));
        let second = execution(&parent, DELEGATE_TOOL, at(base, 11), at(base, 20));
        let steps = vec![
            step(&parent, 1, "read notes", Some(&reading), at(base, 2)),
            step(&parent, 2, "ask researcher", Some(&first), at(base, 10)),
            step(&parent, 3, "ask writer", Some(&second), at(base, 20)),
            step(&parent, 4, "done", None, at(base, 21)),
        ];
        let children = vec![
            // Given out of order, to show placement is by time and not position.
            edge(
                &parent,
                "writer",
                at(base, 12),
                DelegationState::Failed,
                Some(child_run(TaskState::Failed, 1, false)),
            ),
            edge(
                &parent,
                "researcher",
                at(base, 4),
                DelegationState::Completed,
                Some(child_run(TaskState::Completed, 3, true)),
            ),
            edge(
                &parent,
                "fetcher",
                at(base, 5),
                DelegationState::Completed,
                Some(child_run(TaskState::Completed, 2, false)),
            ),
        ];
        let lineage = Lineage {
            delegated_by: None,
            children,
        };
        let trace = supervisor_trace(parent, steps, vec![reading, first, second]);
        let text = super::trace(&trace, &lineage, &plain(), true);

        let asked_researcher = line_of(&text, "ask researcher");
        let asked_writer = line_of(&text, "ask writer");
        let researcher = line_of(&text, "├─ researcher · completed · 3 steps · tainted");
        let fetcher = line_of(&text, "└─ fetcher · completed · 2 steps");
        let writer = line_of(&text, "└─ writer · failed · 1 step");
        assert!(
            asked_researcher < researcher && researcher < fetcher,
            "{text}"
        );
        assert!(fetcher < asked_writer && asked_writer < writer, "{text}");
        assert!(writer < line_of(&text, "done"), "{text}");
        // A clean child says nothing about taint; only a tainted one is marked.
        assert!(
            !text
                .lines()
                .nth(fetcher)
                .unwrap_or_default()
                .contains("tainted")
        );
        assert!(
            !text.contains("Delegated\n"),
            "every child had a call: {text}"
        );
    }

    #[test]
    fn a_child_whose_call_has_not_returned_is_still_shown() {
        // The delegate call is recorded only when it returns, so while a child
        // works there is no step to hang it from. Leaving it out would make a
        // live child invisible in its parent's trace.
        let base = agentos_core::now();
        let parent = child_run(TaskState::Executing, 1, false);
        let steps = vec![step(&parent, 1, "thinking", None, at(base, 1))];
        let live = edge(
            &parent,
            "researcher",
            at(base, 2),
            DelegationState::Running,
            Some(child_run(TaskState::Executing, 1, false)),
        );
        let lineage = Lineage {
            delegated_by: None,
            children: vec![live],
        };
        let text = super::trace(
            &supervisor_trace(parent, steps, vec![]),
            &lineage,
            &plain(),
            false,
        );
        assert!(
            line_of(&text, "Delegated") < line_of(&text, "└─ researcher · executing · 1 step"),
            "{text}"
        );
    }

    #[test]
    fn a_delegation_left_running_by_a_dead_process_reports_its_run() {
        // The row says `running` for ever once the process that would have
        // completed it is gone; the reaped run is the one that knows.
        let base = agentos_core::now();
        let parent = child_run(TaskState::Failed, 2, false);
        let stale = edge(
            &parent,
            "researcher",
            at(base, 2),
            DelegationState::Running,
            Some(child_run(TaskState::Failed, 2, false)),
        );
        let lineage = Lineage {
            delegated_by: None,
            children: vec![stale],
        };
        let text = super::trace(
            &supervisor_trace(parent, vec![], vec![]),
            &lineage,
            &plain(),
            false,
        );
        assert!(text.contains("researcher · failed · 2 steps"), "{text}");
    }

    #[test]
    fn a_delegated_run_says_who_asked_for_it() {
        let base = agentos_core::now();
        let parent = child_run(TaskState::Executing, 1, false);
        let parent_id = parent.id;
        let mut trace = supervisor_trace(child_run(TaskState::Completed, 1, false), vec![], vec![]);
        trace.agent_name = "researcher".into();
        let lineage = Lineage {
            delegated_by: Some(edge(
                &parent,
                "supervisor",
                at(base, 0),
                DelegationState::Running,
                Some(parent.clone()),
            )),
            children: vec![],
        };
        let text = super::trace(&trace, &lineage, &plain(), true);
        assert!(
            text.contains(&format!("Asked by supervisor   Run {parent_id}")),
            "{text}"
        );
        assert!(!super::trace(&trace, &Lineage::default(), &plain(), true).contains("Asked by"));
    }
}
