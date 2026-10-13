//! The interactive approval gate.
//!
//! The CLI's answer to "a human must decide". It prints the approval card and
//! blocks until the operator types y or n — or until the run is cancelled, in
//! which case it stops waiting rather than holding a run open on a prompt
//! nobody is going to answer.
//!
//! One card is on the terminal at a time. A run that delegates has several
//! runs asking at once, all through this one gate, and two cards printed
//! together with two readers waiting on stdin would hand the operator's answer
//! to whichever reader took the line, not to the card they had just read.

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use agentos_core::approval::ApprovalRequest;
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::render::{Style, approval_card};

/// Shown under a prompt whose run stopped before it was answered. The read
/// may still be waiting for a line, and whatever is typed goes nowhere.
const WITHDRAWN: &str =
    "Withdrawn: the run that asked was stopped. Anything typed at this prompt is ignored.";

/// Where the gate shows a card and reads the answer to it.
///
/// The process's own terminal, except under test.
trait Console: Send + Sync + std::fmt::Debug {
    /// Whether anybody is there to answer.
    fn attached(&self) -> bool;
    /// Show `text` as it is, adding nothing, and make sure it is seen.
    fn show(&self, text: &str);
    /// Block until a line is typed; `None` once nothing more can be read.
    fn read_line(&self) -> Option<String>;
}

/// Standard input and output.
#[derive(Debug)]
struct Stdio;

impl Console for Stdio {
    fn attached(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn show(&self, text: &str) {
        print!("{text}");
        let _ = std::io::stdout().flush();
    }

    fn read_line(&self) -> Option<String> {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }
}

/// Prompts the operator on the terminal.
#[derive(Debug)]
pub struct InteractiveGate {
    /// Approve anything at or below this risk without asking.
    ///
    /// A convenience for long unattended-ish runs. It cannot widen what the
    /// policy allows — it only skips the prompt for things the policy already
    /// said `ask` about.
    auto_approve_below: Option<agentos_core::risk::RiskLevel>,
    console: Arc<dyn Console>,
    /// The terminal's one turn: held from printing a card until the line
    /// answering it has been read.
    turn: Arc<Mutex<()>>,
}

impl InteractiveGate {
    /// A gate that asks about everything.
    #[must_use]
    pub fn new() -> Self {
        Self::on(None, Arc::new(Stdio))
    }

    /// Skip the prompt for anything at or below `risk`.
    #[must_use]
    pub fn auto_approving_up_to(risk: agentos_core::risk::RiskLevel) -> Self {
        Self::on(Some(risk), Arc::new(Stdio))
    }

    fn on(
        auto_approve_below: Option<agentos_core::risk::RiskLevel>,
        console: Arc<dyn Console>,
    ) -> Self {
        Self {
            auto_approve_below,
            console,
            turn: Arc::new(Mutex::new(())),
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

        // Wait for the card before this one to be answered. A run stopped
        // meanwhile never shows its card at all.
        let turn = tokio::select! {
            () = cancel.cancelled() => return ApprovalOutcome::Cancelled,
            turn = Arc::clone(&self.turn).lock_owned() => turn,
        };

        if !self.console.attached() {
            // Nothing is attached to answer. Denying is the only safe reading of
            // silence: the alternative is that piping a command into AgentOS
            // silently grants everything it asks for.
            self.console.show(&card);
            self.console.show(&format!(
                "{}\n",
                style.red("Denied: no terminal is attached to approve this.")
            ));
            return ApprovalOutcome::Denied {
                note: Some("no interactive terminal was available".to_owned()),
            };
        }

        self.console.show(&card);

        // The read blocks a thread, so it goes on the blocking pool; the select
        // means cancelling the run does not wait for the operator to look up.
        // The turn goes with the read rather than staying here: a read that
        // cancellation abandoned still takes the next line typed, and that
        // line must not be mistaken for the answer to the next card.
        let console = Arc::clone(&self.console);
        let prompt = tokio::task::spawn_blocking(move || {
            let _turn = turn;
            loop {
                console.show("Approve? [y/N] ");
                let Some(answer) = console.read_line() else {
                    return false;
                };
                match answer.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => return true,
                    "" | "n" | "no" => return false,
                    _ => console.show("Please answer y or n.\n"),
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
                self.console.show(&format!("\n{}\n", style.dim(WITHDRAWN)));
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
            delegation_chain: vec![],
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

    /// A terminal whose output is kept and whose input is typed by the test.
    #[derive(Debug)]
    struct Typed {
        shown: std::sync::Mutex<String>,
        typed: std::sync::Mutex<std::sync::mpsc::Receiver<String>>,
    }

    impl Console for Typed {
        fn attached(&self) -> bool {
            true
        }

        fn show(&self, text: &str) {
            self.shown.lock().unwrap().push_str(text);
        }

        fn read_line(&self) -> Option<String> {
            self.typed.lock().unwrap().recv().ok()
        }
    }

    impl Typed {
        fn new() -> (Arc<Self>, std::sync::mpsc::Sender<String>) {
            let (keyboard, typed) = std::sync::mpsc::channel();
            let console = Arc::new(Self {
                shown: std::sync::Mutex::new(String::new()),
                typed: std::sync::Mutex::new(typed),
            });
            (console, keyboard)
        }

        fn shown(&self) -> String {
            self.shown.lock().unwrap().clone()
        }

        /// Wait until `count` prompts have been shown.
        async fn prompts(&self, count: usize) -> String {
            let wait = async {
                loop {
                    let shown = self.shown();
                    if shown.matches("Approve? [y/N]").count() >= count {
                        return shown;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(10), wait)
                .await
                .expect("the prompt never appeared")
        }
    }

    fn asking(explanation: &str) -> ApprovalRequest {
        ApprovalRequest {
            explanation: explanation.into(),
            ..request(RiskLevel::High)
        }
    }

    #[tokio::test]
    async fn the_second_card_waits_until_the_first_is_answered() {
        // Two delegated runs asking at once. The operator must see one card,
        // answer it, and only then see the next, so that every answer is to
        // the card on the screen.
        let (console, keyboard) = Typed::new();
        let gate = Arc::new(InteractiveGate::on(None, console.clone()));
        let cards = ["CARD-A writes the notes", "CARD-B deletes the workspace"];
        let pending: Vec<_> = cards
            .iter()
            .map(|card| {
                let gate = Arc::clone(&gate);
                let request = asking(card);
                tokio::spawn(async move { gate.request(&request, CancellationToken::new()).await })
            })
            .collect();

        let shown = console.prompts(1).await;
        // Room for the other card to be printed, were anything to print it.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let shown_first = console.shown();
        assert_eq!(
            shown_first, shown,
            "nothing may follow an unanswered prompt"
        );
        let on_screen: Vec<usize> = (0..2).filter(|i| shown.contains(cards[*i])).collect();
        let [first] = on_screen.as_slice() else {
            panic!("expected exactly one card on screen: {shown}");
        };
        let second = 1 - first;

        keyboard.send("n\n".into()).unwrap();
        let shown = console.prompts(2).await;
        assert!(shown.contains(cards[second]), "{shown}");
        keyboard.send("y\n".into()).unwrap();

        let mut outcomes = Vec::new();
        for task in pending {
            outcomes.push(task.await.unwrap());
        }
        assert!(
            matches!(outcomes[*first], ApprovalOutcome::Denied { .. }),
            "the no was typed under {}: {outcomes:?}",
            cards[*first]
        );
        assert_eq!(outcomes[second], ApprovalOutcome::Approved { note: None });
    }

    #[tokio::test]
    async fn a_run_stopped_while_waiting_its_turn_never_shows_its_card() {
        let (console, keyboard) = Typed::new();
        let gate = Arc::new(InteractiveGate::on(None, console.clone()));
        let holding = {
            let gate = Arc::clone(&gate);
            tokio::spawn(async move {
                gate.request(&asking("CARD-A"), CancellationToken::new())
                    .await
            })
        };
        console.prompts(1).await;

        let stopped = CancellationToken::new();
        stopped.cancel();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            gate.request(&asking("CARD-B"), stopped),
        )
        .await
        .expect("a stopped run waited for the terminal");
        assert_eq!(outcome, ApprovalOutcome::Cancelled);
        assert!(!console.shown().contains("CARD-B"));

        keyboard.send("y\n".into()).unwrap();
        assert_eq!(
            holding.await.unwrap(),
            ApprovalOutcome::Approved { note: None }
        );
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
