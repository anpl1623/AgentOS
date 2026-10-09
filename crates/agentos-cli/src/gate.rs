//! The interactive approval gate.
//!
//! The CLI's answer to "a human must decide". It prints the approval card and
//! blocks until the operator types y or n — or until the run is cancelled, in
//! which case it stops waiting rather than holding a run open on a prompt
//! nobody is going to answer.

use std::io::{IsTerminal, Write};

use agentos_core::approval::ApprovalRequest;
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::render::{Style, approval_card};

/// Prompts the operator on the terminal.
#[derive(Debug)]
pub struct InteractiveGate {
    /// Approve anything at or below this risk without asking.
    ///
    /// A convenience for long unattended-ish runs. It cannot widen what the
    /// policy allows — it only skips the prompt for things the policy already
    /// said `ask` about.
    auto_approve_below: Option<agentos_core::risk::RiskLevel>,
}

impl InteractiveGate {
    /// A gate that asks about everything.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            auto_approve_below: None,
        }
    }

    /// Skip the prompt for anything at or below `risk`.
    #[must_use]
    pub const fn auto_approving_up_to(risk: agentos_core::risk::RiskLevel) -> Self {
        Self {
            auto_approve_below: Some(risk),
        }
    }
}

impl Default for InteractiveGate {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApprovalGate for InteractiveGate {
    /// Everything above the auto-approval ceiling is put to the operator.
    ///
    /// Said here as well as acted on in [`Self::request`], so a run's approval
    /// budget is spent only on prompts the operator actually saw.
    fn will_ask(&self, request: &ApprovalRequest) -> bool {
        self.auto_approve_below
            .is_none_or(|ceiling| request.risk > ceiling)
    }

    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        if !self.will_ask(request) {
            return ApprovalOutcome::Approved { note: None };
        }

        let style = Style::detect();
        let card = approval_card(request, &style);

        if !std::io::stdin().is_terminal() {
            // Nothing is attached to answer. Denying is the only safe reading of
            // silence: the alternative is that piping a command into AgentOS
            // silently grants everything it asks for.
            print!("{card}");
            println!(
                "{}",
                style.red("Denied: no terminal is attached to approve this.")
            );
            return ApprovalOutcome::Denied {
                note: Some("no interactive terminal was available".to_owned()),
            };
        }

        print!("{card}");
        let _ = std::io::stdout().flush();

        // The read blocks a thread, so it goes on the blocking pool; the select
        // means cancelling the run does not wait for the operator to look up.
        let prompt = tokio::task::spawn_blocking(move || {
            loop {
                print!("Approve? [y/N] ");
                let _ = std::io::stdout().flush();
                let mut answer = String::new();
                if std::io::stdin().read_line(&mut answer).is_err() {
                    return false;
                }
                match answer.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => return true,
                    "" | "n" | "no" => return false,
                    _ => println!("Please answer y or n."),
                }
            }
        });

        // Aborting a blocking task only helps if it has not started reading yet,
        // so a cancel mid-prompt stops the *run* immediately while the read
        // itself unblocks whenever stdin next yields. That is the right trade:
        // the operator's cancel takes effect without waiting on them to type.
        let abort = prompt.abort_handle();
        tokio::select! {
            () = cancel.cancelled() => {
                abort.abort();
                ApprovalOutcome::Cancelled
            }
            result = prompt => match result {
                Ok(true) => ApprovalOutcome::Approved { note: None },
                Ok(false) => ApprovalOutcome::Denied {
                    note: Some("declined at the terminal".to_owned()),
                },
                Err(_) => ApprovalOutcome::Cancelled,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
    use agentos_core::permission::{Capability, Effect};
    use agentos_core::risk::RiskLevel;

    use super::*;

    fn request(risk: RiskLevel) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: AgentId::new(),
            agent_name: "a".into(),
            task_id: TaskId::new(),
            run_id: TaskRunId::new(),
            tool: "filesystem.write".into(),
            arguments: serde_json::json!({}),
            capability: Capability::new("filesystem", "write"),
            risk,
            effect_before_taint: Effect::Ask,
            asked_this_run: 0,
            approval_budget: None,
            reason: "policy".into(),
            explanation: "writes a file".into(),
            affected_resources: vec![],
            tainted: false,
            taint_sources: vec![],
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        }
    }

    #[test]
    fn only_what_is_above_the_ceiling_is_put_to_the_operator() {
        // The run's approval budget counts what this says, so it must agree
        // with what `request` does.
        let gate = InteractiveGate::auto_approving_up_to(RiskLevel::Medium);
        assert!(!gate.will_ask(&request(RiskLevel::Low)));
        assert!(!gate.will_ask(&request(RiskLevel::Medium)));
        assert!(gate.will_ask(&request(RiskLevel::High)));

        let asks_everything = InteractiveGate::new();
        assert!(asks_everything.will_ask(&request(RiskLevel::Low)));
    }

    #[tokio::test]
    async fn what_is_not_asked_is_approved_without_a_prompt() {
        let gate = InteractiveGate::auto_approving_up_to(RiskLevel::Medium);
        let outcome = gate
            .request(&request(RiskLevel::Low), CancellationToken::new())
            .await;
        assert_eq!(outcome, ApprovalOutcome::Approved { note: None });
    }
}
