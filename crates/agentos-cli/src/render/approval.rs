//! The approval card: what is being done, to what, why a person is being
//! asked, and whether the agent has been reading untrusted text, in one box
//! that can be answered without reading anything else.

use agentos_core::approval::ApprovalRequest;
use agentos_core::permission::Effect;

use super::{Style, WIDTH, visible_width, wrap};

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

fn boxed_line(content: &str, _style: &Style) -> String {
    let visible = visible_width(content);
    let padding = (WIDTH - 4).saturating_sub(visible);
    format!("│ {content}{} │\n", " ".repeat(padding))
}

#[cfg(test)]
mod tests {
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
    use agentos_core::permission::Capability;
    use agentos_core::risk::RiskLevel;

    use super::*;
    use crate::render::test_support::plain;

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
    fn the_box_stays_square_around_wide_text() {
        // The explanation can carry a model-supplied title in Korean,
        // Japanese or with emoji, two columns a character. Wrapped and padded
        // by characters, those lines ran far past the right-hand border of
        // the card the operator reads before approving. Measured here with
        // `unicode_width` itself, not the function under test.
        use unicode_width::UnicodeWidthStr;

        let mut wide = request(true);
        wide.explanation =
            "이슈 제목: 승인 카드의 테두리가 깨지지 않아야 합니다 🙂 日本語のタイトル ".repeat(6);
        wide.delegation_chain = vec!["감독자".into(), "연구원".into()];
        let card = approval_card(&wide, &plain());
        let widths: Vec<usize> = card
            .lines()
            .filter(|line| line.starts_with('│'))
            .map(UnicodeWidthStr::width)
            .collect();
        assert!(widths.len() > 10, "{card}");
        assert!(
            widths.iter().all(|width| *width == WIDTH),
            "ragged box: {widths:?}\n{card}"
        );
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
}
