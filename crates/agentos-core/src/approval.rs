//! Human-in-the-loop approvals.
//!
//! An approval request is a first-class runtime object, not a UI concern: it is
//! persisted, audited and survives a restart. The desktop app and the CLI are
//! both just renderers of the same request.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
use crate::permission::{Capability, Effect};
use crate::risk::RiskLevel;

/// Where an approval request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// Waiting on a human.
    #[default]
    Pending,
    /// A human said yes.
    Approved,
    /// A human said no.
    Denied,
    /// Nobody answered in time.
    Expired,
    /// The run was cancelled while waiting.
    Cancelled,
}

impl ApprovalStatus {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the action may proceed.
    #[must_use]
    pub const fn is_approved(self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Everything a human needs in order to decide.
///
/// Spec §9's approval card renders directly from this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Identity.
    pub id: ApprovalId,
    /// The agent asking.
    pub agent_id: AgentId,
    /// Its name, denormalised so the UI need not join.
    pub agent_name: String,
    /// The task.
    pub task_id: TaskId,
    /// The run.
    pub run_id: TaskRunId,
    /// The tool it wants to invoke.
    pub tool: String,
    /// The validated arguments it wants to invoke it with.
    ///
    /// Already schema-checked, so the UI can render them structurally.
    pub arguments: serde_json::Value,
    /// The capability being exercised.
    pub capability: Capability,
    /// Assessed risk, after taint escalation.
    pub risk: RiskLevel,
    /// What the policy alone said, before taint escalation.
    ///
    /// `allow` here means the rules would have let the call through silently
    /// and a person is being asked only because the run has read untrusted
    /// data. That changes what the person is judging — not "is this action
    /// acceptable" but "has what this agent read steered it" — so it travels
    /// as a fact rather than as a clause buried in `reason`.
    pub effect_before_taint: Effect,
    /// Which request this is in its run, counting from one.
    ///
    /// Stamped by the run's approval gate, which every request in a run passes
    /// through and which enforces the budget against the same count. Zero
    /// means no run gate has seen the request.
    pub asked_this_run: u32,
    /// How many requests the run's policy allows before it stops asking.
    ///
    /// Stamped beside [`Self::asked_this_run`] so a card can say "3 of 10".
    /// `None` when the policy sets no budget.
    pub approval_budget: Option<u32>,
    /// Why the runtime is asking — the policy rule or escalation that triggered it.
    pub reason: String,
    /// Plain-language explanation of what will happen if approved.
    pub explanation: String,
    /// Resources the action will touch, for display.
    pub affected_resources: Vec<String>,
    /// Whether the run had ingested untrusted data before this request.
    ///
    /// Surfaced prominently: "this agent has read a webpage" changes how a human
    /// should read the request.
    pub tainted: bool,
    /// Where that untrusted data came from, most useful first.
    ///
    /// Data rather than a sentence, so each client writes its own: the terminal
    /// wants one line, a window wants a highlighted panel, and neither should be
    /// parsing prose the runtime composed for the other.
    pub taint_sources: Vec<String>,
    /// Current status.
    pub status: ApprovalStatus,
    /// When it was raised.
    pub requested_at: Timestamp,
    /// When it was answered.
    pub decided_at: Option<Timestamp>,
    /// Free-text note from the human.
    pub decision_note: Option<String>,
}

/// A human's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// The request being answered.
    pub approval_id: ApprovalId,
    /// Yes or no.
    pub approved: bool,
    /// Optional note recorded in the audit log.
    pub note: Option<String>,
}

impl ApprovalDecision {
    /// Approve.
    #[must_use]
    pub const fn approve(approval_id: ApprovalId) -> Self {
        Self {
            approval_id,
            approved: true,
            note: None,
        }
    }

    /// Deny.
    #[must_use]
    pub const fn deny(approval_id: ApprovalId) -> Self {
        Self {
            approval_id,
            approved: false,
            note: None,
        }
    }

    /// Attach a note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// The longest note a decision may carry, in characters.
///
/// A note is sealed into the append-only audit chain and the approvals table,
/// neither of which can be cleaned up afterwards, and a denial's note is also
/// handed to the model. Two thousand characters is several paragraphs of
/// reasons; a pasted log is not a reason.
pub const MAX_DECISION_NOTE_CHARS: usize = 2_000;

/// A person's note made fit to keep: control characters removed, surrounding
/// whitespace trimmed, and cut to [`MAX_DECISION_NOTE_CHARS`].
///
/// Line breaks survive because a reason can have paragraphs. Every other
/// control character goes, terminal escape sequences included: the note is
/// printed by the CLI and stored for good, and an escape sequence in it would
/// be an instruction to whatever terminal later shows it. A note left empty is
/// no note.
#[must_use]
pub fn clean_decision_note(note: &str) -> Option<String> {
    let kept: String = note
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect();
    let trimmed = kept.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut chars = trimmed.chars();
    let clipped: String = chars.by_ref().take(MAX_DECISION_NOTE_CHARS).collect();
    Some(if chars.next().is_some() {
        format!("{}…", clipped.trim_end())
    } else {
        clipped
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_approved_status_permits_action() {
        assert!(ApprovalStatus::Approved.is_approved());
        for status in [
            ApprovalStatus::Pending,
            ApprovalStatus::Denied,
            ApprovalStatus::Expired,
            ApprovalStatus::Cancelled,
        ] {
            assert!(!status.is_approved(), "{} must not permit", status.as_str());
        }
    }

    #[test]
    fn a_note_keeps_its_lines_and_loses_its_escape_sequences() {
        let note = "\u{1b}[2J\u{1b}[31mwrong\trecipient\r\nsee ticket\u{7}\u{9b}1m ";
        assert_eq!(
            clean_decision_note(note).as_deref(),
            Some("[2J[31mwrongrecipient\nsee ticket1m")
        );
        assert_eq!(clean_decision_note(" \n\u{1b} "), None);
    }

    #[test]
    fn an_oversized_note_is_cut_to_the_limit() {
        let note = "a".repeat(MAX_DECISION_NOTE_CHARS * 50);
        let kept = clean_decision_note(&note).unwrap();
        assert_eq!(kept.chars().count(), MAX_DECISION_NOTE_CHARS + 1);
        assert!(kept.ends_with('…'));

        let exact = "é".repeat(MAX_DECISION_NOTE_CHARS);
        assert_eq!(clean_decision_note(&exact).as_deref(), Some(exact.as_str()));
    }

    #[test]
    fn decisions_carry_notes() {
        let decision = ApprovalDecision::deny(ApprovalId::new()).with_note("wrong recipient");
        assert!(!decision.approved);
        assert_eq!(decision.note.as_deref(), Some("wrong recipient"));
    }
}
