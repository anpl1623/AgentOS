//! Terminal rendering.
//!
//! Kept apart from the commands so that formatting decisions do not end up
//! interleaved with runtime calls, and so the approval card has one definition.
//!
//! Three renderings, a module each: the approval card, a run's trace and a
//! delegation tree. What they share is here: the colours, and the measuring
//! that keeps coloured text in its columns.

mod approval;
#[cfg(test)]
mod test_support;
mod trace;
mod tree;

use agentos_core::delegation::DelegationState;
use agentos_core::risk::RiskLevel;
use agentos_core::task::TaskState;

pub use approval::approval_card;
pub use trace::{DelegationEdge, Lineage, trace};
pub use tree::delegation_tree;

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

fn summary_line(agent: &str, state: &str, steps: u32, tainted: bool, style: &Style) -> String {
    let noun = if steps == 1 { "step" } else { "steps" };
    let mut line = format!("{agent} · {state} · {steps} {noun}");
    if tainted {
        line.push_str(&format!(" · {}", style.yellow("tainted")));
    }
    line
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
        // Widths are counted in characters, not bytes: an accented word is
        // one column per letter on a terminal, not two or three.
        let mut current = String::new();
        let mut current_width = 0;
        for word in paragraph.split_whitespace() {
            let word_width = visible_width(word);
            if current.is_empty() {
                current.push_str(word);
                current_width = word_width;
            } else if current_width + 1 + word_width <= width {
                current.push(' ');
                current.push_str(word);
                current_width += 1 + word_width;
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(word);
                current_width = word_width;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

/// `text` cut to at most `max` characters, with an ellipsis if anything was
/// cut. Characters, not bytes, so text outside ASCII keeps its full length.
fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        None => text.to_owned(),
        Some((cut, _)) => format!("{}…", &text[..cut]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_counts_characters_not_bytes() {
        // Each word is five characters and ten bytes. Measured in bytes, no
        // two would fit on a line of twelve.
        let wrapped = wrap("ééééé ééééé ééééé", 12);
        assert_eq!(wrapped, vec!["ééééé ééééé", "ééééé"]);
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        assert_eq!(truncate("ééééé", 5), "ééééé");
        assert_eq!(truncate("éééééé", 5), "ééééé…");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("", 0), "");
    }

    use super::test_support::plain;

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
