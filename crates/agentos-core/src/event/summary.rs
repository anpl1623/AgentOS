//! One-line summaries of operator changes, for the activity feeds.
//!
//! Both clients summarise from here, whether they hold a live event or a
//! stored payload, so a change reads the same wherever it is shown.

use super::AgentEvent;

impl AgentEvent {
    /// A one-line description of an operator change, or `None` for any other
    /// event.
    ///
    /// The generic summaries read a tool, an objective or a reason, and an
    /// operator change carries none of them: a feed would show a policy
    /// installed as a blank line, or an agent created as its model. Both
    /// clients summarise from here so they describe the change the same way.
    /// A provider change names the provider and nothing else, as its record does.
    #[must_use]
    pub fn operator_summary(&self) -> Option<String> {
        let line = match self {
            Self::PolicyChanged { agent, version, .. } => {
                format!("{agent}: policy version {version} installed")
            }
            Self::AgentCreated {
                agent,
                provider,
                model,
                ..
            } => format!("{agent} created on {provider}/{model}"),
            Self::AgentEnabledChanged { agent, enabled } => {
                format!("{agent} {}", if *enabled { "enabled" } else { "disabled" })
            }
            Self::ProviderKeySet { provider } => format!("{provider} key stored"),
            Self::ProviderKeyRemoved { provider } => format!("{provider} key removed"),
            Self::CredentialSet { origin, name } => {
                format!("credential {name} stored for {origin}")
            }
            Self::CredentialRemoved { origin, name } => {
                format!("credential {name} removed for {origin}")
            }
            Self::IntegrationBound {
                integration,
                label,
                host,
                private_network,
            } => format!(
                "{integration} account {label} bound to {host}{}",
                if *private_network {
                    ", private network allowed"
                } else {
                    ""
                }
            ),
            Self::IntegrationUnbound {
                integration,
                label,
                host,
                ..
            } => format!("{integration} account {label} unbound from {host}"),
            Self::MemoryRemembered {
                agent,
                kind,
                content,
                ..
            } => format!("{agent}: {kind} remembered: {}", first_line(content)),
            Self::MemoryRevised { agent, content, .. } => {
                format!("{agent}: memory revised: {}", first_line(content))
            }
            Self::MemoryForgotten {
                agent,
                kind,
                content,
                ..
            } => format!("{agent}: {kind} forgotten: {}", first_line(content)),
            Self::ScheduleCreated { name, cadence, .. } => {
                format!("schedule {name} created, {}", cadence.describe())
            }
            Self::SchedulePaused { name, .. } => format!("schedule {name} paused"),
            Self::ScheduleResumed { name, .. } => format!("schedule {name} resumed"),
            Self::ScheduleDeleted { name, .. } => format!("schedule {name} deleted"),
            Self::TaskCreated {
                objective,
                depends_on,
                scheduled_for,
            } => {
                let mut line = format!("task queued: {}", first_line(objective));
                if !depends_on.is_empty() {
                    line.push_str(&format!(", after {} task(s)", depends_on.len()));
                }
                if let Some(when) = scheduled_for {
                    line.push_str(&format!(", not before {}", crate::format_timestamp(when)));
                }
                line
            }
            Self::TaskDependencyAdded { .. } => "task made to wait for another".to_owned(),
            Self::SchedulerStarted {
                tick_seconds,
                max_concurrent_runs,
                on_launch,
            } => format!(
                "scheduler started{}, every {tick_seconds}s, at most {max_concurrent_runs} run(s) at once",
                if *on_launch { " on launch" } else { "" }
            ),
            Self::SchedulerStopped { .. } => "scheduler stopped".to_owned(),
            _ => return None,
        };
        Some(line)
    }
}

/// The first line of operator-typed text, for a one-line summary.
///
/// The record keeps the whole text; a feed row that wrapped a pasted paragraph
/// would push everything after it off the screen.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

/// [`AgentEvent::operator_summary`] for a stored payload.
///
/// Only a payload tagged with an `operator.` kind is deserialised, so a feed of
/// routine records pays nothing for it. A payload that carries the tag and does
/// not read as its event gives `None`, and the caller's generic summary stands.
#[must_use]
pub fn operator_summary_of(payload: &serde_json::Value) -> Option<String> {
    let kind = payload.get("event")?.as_str()?;
    if !kind.starts_with("operator.") {
        return None;
    }
    serde_json::from_value::<AgentEvent>(payload.clone())
        .ok()?
        .operator_summary()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::samples::sample_events;

    #[test]
    fn every_operator_change_has_a_summary_and_nothing_else_does() {
        // Keyed on the kind's prefix rather than a list, so a sixth operator
        // event without a summary line fails here instead of reaching a feed
        // as a blank row.
        for event in sample_events() {
            let stored = serde_json::to_value(&event).unwrap();
            let summary = operator_summary_of(&stored);
            if event.kind().starts_with("operator.") {
                let line = summary.unwrap_or_else(|| panic!("no summary for {}", event.kind()));
                assert!(
                    !line.trim().is_empty(),
                    "blank summary for {}",
                    event.kind()
                );
            } else {
                assert_eq!(summary, None, "{} is not an operator change", event.kind());
            }
        }

        let disabled = AgentEvent::AgentEnabledChanged {
            agent: "ops".into(),
            enabled: false,
        };
        assert_eq!(disabled.operator_summary().as_deref(), Some("ops disabled"));
        let stored =
            serde_json::json!({"event": "operator.provider_key.set", "provider": "openai"});
        assert_eq!(
            operator_summary_of(&stored).as_deref(),
            Some("openai key stored")
        );
    }
}
