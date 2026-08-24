//! Terminal rendering.
//!
//! Kept apart from the commands so that formatting decisions do not end up
//! interleaved with runtime calls, and so the approval card has one definition.

use agentos_core::approval::ApprovalRequest;
use agentos_core::risk::RiskLevel;
use agentos_core::task::TaskState;
use agentos_runtime::RunTrace;

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

    if request.tainted {
        out.push_str(&boxed_line("", style));
        out.push_str(&boxed_line(
            &style.yellow("⚠ This agent has read untrusted data during this run."),
            style,
        ));
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
#[must_use]
pub fn trace(trace: &RunTrace, style: &Style, include_header: bool) -> String {
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
    if trace.run.tainted {
        out.push_str(&format!(
            "{}\n",
            style.yellow("This run read untrusted data.")
        ));
    }

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
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
    use agentos_core::permission::Capability;

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
}
