//! Terminal rendering.
//!
//! Kept apart from the commands so that formatting decisions do not end up
//! interleaved with runtime calls, and so the approval card has one definition.

use std::collections::HashMap;

use agentos_core::approval::ApprovalRequest;
use agentos_core::delegation::{Delegation, DelegationState};
use agentos_core::ids::ToolExecutionId;
use agentos_core::permission::Effect;
use agentos_core::risk::RiskLevel;
use agentos_core::task::{TaskRun, TaskState};
use agentos_runtime::delegation::TOOL_NAME as DELEGATE_TOOL;
use agentos_runtime::{DelegationNode, RunTrace};

/// Terminal width used for rules and boxes.
const WIDTH: usize = 76;

/// ANSI colour, applied only when the output is a terminal.
pub struct Style {
    enabled: bool,
}

impl Style {
    /// Detect whether to colour output.
    ///
    /// Honours `NO_COLOR`, which is the convention users expect.
    #[must_use]
    pub fn detect() -> Self {
        let disabled = std::env::var_os("NO_COLOR").is_some()
            || std::env::var("TERM").is_ok_and(|term| term == "dumb");
        Self { enabled: !disabled }
    }

    fn wrap(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_owned()
        }
    }

    /// Dimmed text.
    #[must_use]
    pub fn dim(&self, text: &str) -> String {
        self.wrap("2", text)
    }

    /// Bold text.
    #[must_use]
    pub fn bold(&self, text: &str) -> String {
        self.wrap("1", text)
    }

    /// Green text.
    #[must_use]
    pub fn green(&self, text: &str) -> String {
        self.wrap("32", text)
    }

    /// Red text.
    #[must_use]
    pub fn red(&self, text: &str) -> String {
        self.wrap("31", text)
    }

    /// Yellow text.
    #[must_use]
    pub fn yellow(&self, text: &str) -> String {
        self.wrap("33", text)
    }

    /// Colour a risk level by severity.
    #[must_use]
    pub fn risk(&self, risk: RiskLevel) -> String {
        let text = risk.as_str().to_uppercase();
        match risk {
            RiskLevel::None | RiskLevel::Low => self.dim(&text),
            RiskLevel::Medium => self.yellow(&text),
            RiskLevel::High | RiskLevel::Critical => self.red(&text),
        }
    }

    /// Colour a run state by disposition.
    #[must_use]
    pub fn state(&self, state: TaskState) -> String {
        match state {
            TaskState::Completed => self.green(state.as_str()),
            TaskState::Failed => self.red(state.as_str()),
            TaskState::Cancelled => self.yellow(state.as_str()),
            _ => self.dim(state.as_str()),
        }
    }

    /// Colour a delegation's state with the same dispositions as a run's.
    #[must_use]
    pub fn delegation_state(&self, state: DelegationState) -> String {
        match state {
            DelegationState::Completed => self.green(state.as_str()),
            DelegationState::Failed | DelegationState::Refused => self.red(state.as_str()),
            DelegationState::Cancelled => self.yellow(state.as_str()),
            DelegationState::Requested | DelegationState::Running => self.dim(state.as_str()),
        }
    }
}

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

/// Left-pad a possibly-coloured string to a column width.
///
/// `{:<width}` counts ANSI escape bytes, so a coloured cell silently shifts
/// every column after it. This measures visible characters instead.
#[must_use]
pub fn pad(text: &str, width: usize) -> String {
    let visible = visible_width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(visible)))
}

/// A horizontal rule.
#[must_use]
pub fn rule() -> String {
    "─".repeat(WIDTH)
}

/// Render the approval card an operator sees before deciding.
///
/// The design goal is that someone can answer without needing to go and read
/// anything else: what is being done, to what, why they are being asked, and —
/// prominently — whether the agent has been reading attacker-controllable text.
#[must_use]
pub fn approval_card(request: &ApprovalRequest, style: &Style) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!("┌{}┐\n", "─".repeat(WIDTH - 2)));
    out.push_str(&boxed_line(&style.bold("Approval required"), style));
    out.push_str(&format!("├{}┤\n", "─".repeat(WIDTH - 2)));

    out.push_str(&boxed_line(
        &format!(
            "{} wants to run {}",
            style.bold(&request.agent_name),
            style.bold(&request.tool)
        ),
        style,
    ));
    // A delegated agent asks on somebody else's behalf, and a card that named
    // only the agent asking would hide which chain of agents put it up to it.
    if let Some(chain) = request.attribution() {
        for line in wrap(&chain, WIDTH - 6) {
            out.push_str(&boxed_line(&style.yellow(&line), style));
        }
    }
    out.push_str(&boxed_line("", style));

    for line in wrap(&request.explanation, WIDTH - 6) {
        out.push_str(&boxed_line(&line, style));
    }

    if !request.affected_resources.is_empty() {
        out.push_str(&boxed_line("", style));
        out.push_str(&boxed_line(&style.dim("Affects"), style));
        for resource in &request.affected_resources {
            out.push_str(&boxed_line(&format!("  {resource}"), style));
        }
    }

    out.push_str(&boxed_line("", style));
    out.push_str(&boxed_line(
        &format!("{} {}", style.dim("Risk"), style.risk(request.risk)),
        style,
    ));
    out.push_str(&boxed_line(
        &format!("{} {}", style.dim("Because"), request.reason),
        style,
    ));
    // How often this run has asked is what tells a policy that asks twice from
    // one that asks fifty times; from the card alone the two look the same.
    if request.asked_this_run > 0 {
        let of = request
            .approval_budget
            .map(|budget| format!(" of {budget}"))
            .unwrap_or_default();
        out.push_str(&boxed_line(
            &format!(
                "{} {}{of} in this run",
                style.dim("Request"),
                request.asked_this_run
            ),
            style,
        ));
    }

    if request.tainted {
        out.push_str(&boxed_line("", style));
        out.push_str(&boxed_line(
            &style.yellow("⚠ This agent has read untrusted data during this run."),
            style,
        ));
        if request.effect_before_taint == Effect::Allow {
            out.push_str(&boxed_line(
                &style.yellow("  Its policy alone would allow this; that reading is why it asks."),
                style,
            ));
        }
        // Naming the sources is what makes the warning actionable: "it read a
        // webpage" is a different decision from "it read a file you wrote".
        for source in &request.taint_sources {
            for line in wrap(source, WIDTH - 8) {
                out.push_str(&boxed_line(&style.dim(&format!("   {line}")), style));
            }
        }
    }

    out.push_str(&format!("└{}┘\n", "─".repeat(WIDTH - 2)));
    out
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

fn summary_line(agent: &str, state: &str, steps: u32, tainted: bool, style: &Style) -> String {
    let noun = if steps == 1 { "step" } else { "steps" };
    let mut line = format!("{agent} · {state} · {steps} {noun}");
    if tainted {
        line.push_str(&format!(" · {}", style.yellow("tainted")));
    }
    line
}

/// Render a delegation tree, depth first, one node per run.
///
/// Each node is the agent and how its run stands, then what it was asked and
/// the task to `show` for that run's own trace. The tree says who asked whom;
/// what any one of them did is the flat trace's job.
#[must_use]
pub fn delegation_tree(root: &DelegationNode, style: &Style) -> String {
    let mut out = String::new();
    tree_node(root, "", "", style, &mut out);
    out
}

/// One node and everything beneath it.
///
/// `lead` prefixes the node's own line and `rest` every line after it, so a
/// branch that has further siblings below keeps its vertical rule running past
/// this node's children.
fn tree_node(node: &DelegationNode, lead: &str, rest: &str, style: &Style, out: &mut String) {
    let run = &node.run;
    out.push_str(&format!(
        "{}{}\n",
        style.dim(lead),
        summary_line(
            &node.agent_name,
            &style.state(run.state),
            run.steps_taken,
            run.tainted,
            style
        )
    ));

    let body = if node.children.is_empty() {
        format!("{rest}   ")
    } else {
        format!("{rest}│  ")
    };
    let room = WIDTH.saturating_sub(body.chars().count());
    out.push_str(&format!(
        "{}{}\n",
        style.dim(&body),
        truncate(&one_line(&node.objective), room)
    ));
    out.push_str(&format!(
        "{}{}\n",
        style.dim(&body),
        style.dim(&format!("task {}", run.task_id))
    ));

    for (index, child) in node.children.iter().enumerate() {
        let (branch, under) = if index + 1 == node.children.len() {
            ("└─ ", "   ")
        } else {
            ("├─ ", "│  ")
        };
        tree_node(
            child,
            &format!("{rest}{branch}"),
            &format!("{rest}{under}"),
            style,
            out,
        );
    }
}

/// Text collapsed onto one line, with control characters removed.
///
/// A child's objective was written by its parent's model. A newline in it
/// would break the tree's columns, and an escape sequence would be the model
/// writing to the operator's terminal.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn boxed_line(content: &str, _style: &Style) -> String {
    let visible = visible_width(content);
    let padding = (WIDTH - 4).saturating_sub(visible);
    format!("│ {content}{} │\n", " ".repeat(padding))
}

/// Width of a string ignoring ANSI escape sequences.
///
/// Without this, colouring text inside a box breaks the box.
fn visible_width(text: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for character in text.chars() {
        if in_escape {
            if character == 'm' {
                in_escape = false;
            }
        } else if character == '\u{1b}' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut cut = max;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}

#[cfg(test)]
mod tests {
    use agentos_core::Timestamp;
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId, TaskStepId};
    use agentos_core::permission::Capability;
    use agentos_core::task::{TaskStep, TaskStepKind};
    use agentos_core::tool::ToolOutcome;
    use agentos_persistence::ToolExecutionRecord;

    use super::*;

    fn plain() -> Style {
        Style { enabled: false }
    }

    fn request(tainted: bool) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: AgentId::new(),
            agent_name: "sales".into(),
            task_id: TaskId::new(),
            run_id: TaskRunId::new(),
            tool: "email.send".into(),
            arguments: serde_json::json!({}),
            capability: Capability::new("email", "send"),
            risk: RiskLevel::Medium,
            effect_before_taint: if tainted { Effect::Allow } else { Effect::Ask },
            asked_this_run: 3,
            approval_budget: Some(10),
            reason: "policy rule `email.send` requires approval".into(),
            explanation: "Send an order update to customer@example.com.".into(),
            affected_resources: vec!["customer@example.com".into()],
            tainted,
            taint_sources: if tainted {
                vec!["web:https://crm.example/customers/7".to_owned()]
            } else {
                vec![]
            },
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
            delegation_chain: vec![],
        }
    }

    #[test]
    fn the_card_shows_what_a_person_needs_to_decide() {
        let card = approval_card(&request(false), &plain());
        assert!(card.contains("Approval required"));
        assert!(card.contains("sales"));
        assert!(card.contains("email.send"));
        assert!(card.contains("customer@example.com"));
        assert!(card.contains("MEDIUM"));
        assert!(card.contains("requires approval"));
    }

    #[test]
    fn a_tainted_run_is_called_out() {
        assert!(!approval_card(&request(false), &plain()).contains("untrusted data"));
        assert!(approval_card(&request(true), &plain()).contains("read untrusted data"));
    }

    #[test]
    fn the_card_says_where_the_run_is_in_its_budget_and_why_it_asks() {
        let card = approval_card(&request(true), &plain());
        assert!(card.contains("Request 3 of 10 in this run"), "{card}");
        assert!(card.contains("policy alone would allow this"), "{card}");
        // A request the policy asks about on its own rules says nothing of the
        // kind.
        assert!(!approval_card(&request(false), &plain()).contains("policy alone"));
    }

    #[test]
    fn the_box_stays_square_when_coloured() {
        // Colour codes are zero-width; if they were counted the box would skew.
        let styled = Style { enabled: true };
        let card = approval_card(&request(true), &styled);
        let widths: Vec<usize> = card
            .lines()
            .filter(|line| line.starts_with('│'))
            .map(visible_width)
            .collect();
        assert!(!widths.is_empty());
        assert!(
            widths.iter().all(|width| *width == WIDTH),
            "ragged box: {widths:?}"
        );
    }

    #[test]
    fn wrapping_respects_the_width() {
        let wrapped = wrap(&"word ".repeat(60), 30);
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.len() <= 30));
    }

    #[test]
    fn padding_measures_visible_characters() {
        let styled = Style { enabled: true };
        let coloured = styled.green("ok");
        assert!(coloured.len() > 2, "precondition: escapes add bytes");
        assert_eq!(visible_width(&pad(&coloured, 10)), 10);
        assert_eq!(visible_width(&pad("ok", 10)), 10);
        assert_eq!(pad("toolong", 3), "toolong");
    }

    #[test]
    fn no_color_disables_escapes() {
        let plain = plain();
        assert_eq!(plain.red("danger"), "danger");
        assert!(!plain.risk(RiskLevel::Critical).contains('\u{1b}'));
    }

    #[test]
    fn the_card_names_the_chain_that_asked() {
        let mut delegated = request(false);
        delegated.agent_name = "worker".into();
        delegated.delegation_chain = vec!["supervisor".into(), "worker".into()];
        let card = approval_card(&delegated, &plain());
        assert!(card.contains("worker, asked by supervisor"), "{card}");

        // Nearest first, all the way up.
        delegated.delegation_chain = vec!["root".into(), "supervisor".into(), "worker".into()];
        let card = approval_card(&delegated, &plain());
        assert!(
            card.contains("worker, asked by supervisor, asked by root"),
            "{card}"
        );

        // An undelegated run asks for itself and the card says nothing more.
        assert!(!approval_card(&request(false), &plain()).contains("asked by"));
    }

    fn at(base: Timestamp, seconds: i64) -> Timestamp {
        base + chrono::Duration::seconds(seconds)
    }

    fn child_run(state: TaskState, steps: u32, tainted: bool) -> TaskRun {
        let mut run = TaskRun::new(TaskId::new(), 1);
        run.state = state;
        run.steps_taken = steps;
        run.tainted = tainted;
        run
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

    fn node(
        agent: &str,
        objective: &str,
        run: TaskRun,
        children: Vec<DelegationNode>,
    ) -> DelegationNode {
        DelegationNode {
            run,
            agent_name: agent.into(),
            objective: objective.into(),
            delegation: None,
            children,
        }
    }

    #[test]
    fn the_tree_nests_each_child_under_the_run_that_asked() {
        let fetcher = node(
            "fetcher",
            "Fetch",
            child_run(TaskState::Completed, 1, true),
            vec![],
        );
        let fetcher_task = fetcher.run.task_id;
        let root = node(
            "supervisor",
            "Summarise the week",
            child_run(TaskState::Completed, 4, true),
            vec![
                node(
                    "researcher",
                    "Find the orders",
                    child_run(TaskState::Completed, 3, true),
                    vec![fetcher],
                ),
                node(
                    "writer",
                    "Draft it",
                    child_run(TaskState::Failed, 1, false),
                    vec![],
                ),
            ],
        );
        let text = delegation_tree(&root, &plain());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                "supervisor · completed · 4 steps · tainted".to_owned(),
                "│  Summarise the week".to_owned(),
                format!("│  task {}", root.run.task_id),
                "├─ researcher · completed · 3 steps · tainted".to_owned(),
                "│  │  Find the orders".to_owned(),
                format!("│  │  task {}", root.children[0].run.task_id),
                "│  └─ fetcher · completed · 1 step · tainted".to_owned(),
                "│        Fetch".to_owned(),
                format!("│        task {fetcher_task}"),
                "└─ writer · failed · 1 step".to_owned(),
                "      Draft it".to_owned(),
                format!("      task {}", root.children[1].run.task_id),
            ],
            "{text}"
        );
    }

    #[test]
    fn a_model_written_objective_cannot_write_to_the_terminal() {
        let root = node(
            "supervisor",
            "line one\nline two \u{1b}[2Jcleared",
            child_run(TaskState::Completed, 1, false),
            vec![],
        );
        let text = delegation_tree(&root, &Style { enabled: true });
        assert!(!text.contains("\u{1b}[2J"), "{text:?}");
        assert!(text.contains("line one line two [2Jcleared"), "{text:?}");
        assert_eq!(text.lines().count(), 3, "{text:?}");
    }
}
