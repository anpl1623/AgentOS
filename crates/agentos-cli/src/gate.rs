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
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        if let Some(ceiling) = self.auto_approve_below
            && request.risk <= ceiling
        {
            return ApprovalOutcome::Approved;
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
                Ok(true) => ApprovalOutcome::Approved,
                Ok(false) => ApprovalOutcome::Denied {
                    note: Some("declined at the terminal".to_owned()),
                },
                Err(_) => ApprovalOutcome::Cancelled,
            },
        }
    }
}
