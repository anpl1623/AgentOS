//! The typed contract between the runtime and the user interface.
//!
//! These are view models, not domain types. They exist for three reasons:
//!
//! * The interface wants denormalised, display-ready data — a task carries its
//!   agent's name, an execution carries the decision that let it run — and
//!   reshaping in the browser means every screen reinventing the same joins.
//! * `agentos-core` should not grow a TypeScript-binding dependency. A frontend
//!   concern has no business in the domain crate.
//! * A view model can change to suit a screen without touching anything the
//!   permission engine or the audit chain depends on.
//!
//! Every type here derives [`ts_rs::TS`] and exports into `src/bindings`, so the
//! TypeScript definitions are generated from these declarations rather than
//! written by hand. `cargo test -p agentos-desktop` regenerates them; a shape
//! that drifts fails to compile on the other side.
//!
//! Identifiers and timestamps cross as strings. The interface formats them
//! anyway, and it keeps the wire format explicit rather than dependent on how
//! two serialisation libraries happen to agree.

use agentos_core::task::{Task, TaskRun};

mod agent;
mod approval;
mod event;
mod settings;
mod task;

pub use agent::*;
pub use approval::*;
pub use event::*;
pub use settings::*;
pub use task::*;

/// Render a timestamp for the interface.
fn at(value: &agentos_core::Timestamp) -> String {
    agentos_core::format_timestamp(value)
}

/// Render an optional timestamp.
fn maybe_at(value: Option<&agentos_core::Timestamp>) -> Option<String> {
    value.map(at)
}

/// Build a one-line summary of an audit event payload.
///
/// Mirrors what the CLI shows, so the two clients describe the same event the
/// same way.
#[must_use]
pub fn summarise_event(payload: &serde_json::Value) -> String {
    if let (Some(from), Some(to)) = (
        payload.get("from").and_then(serde_json::Value::as_str),
        payload.get("to").and_then(serde_json::Value::as_str),
    ) {
        return format!("{from} → {to}");
    }

    // A manifest drift names the tool and what it planned beyond its manifest;
    // the tool alone would read like a routine execution.
    if let Some(undeclared) = undeclared_capabilities(payload) {
        let tool = payload
            .get("tool")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return format!("{tool} planned undeclared {undeclared}");
    }

    for name in ["tool", "objective", "reason", "error", "summary"] {
        if let Some(value) = payload.get(name).and_then(serde_json::Value::as_str)
            && !value.is_empty()
        {
            return value.to_owned();
        }
    }

    if let Some(model) = payload.get("model").and_then(serde_json::Value::as_str) {
        let provider = payload
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return format!("{provider}/{model}");
    }

    String::new()
}

/// The `undeclared` list of a `tool.manifest_exceeded` payload, comma-joined.
fn undeclared_capabilities(payload: &serde_json::Value) -> Option<String> {
    let names: Vec<&str> = payload
        .get("undeclared")?
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// Build a [`TaskSummary`] from its parts.
#[must_use]
pub fn task_summary(task: &Task, agent_name: &str, latest_run: Option<&TaskRun>) -> TaskSummary {
    TaskSummary {
        id: task.id.to_string(),
        objective: task.objective.clone(),
        status: task.status.as_str().to_owned(),
        agent_name: agent_name.to_owned(),
        agent_id: task.agent_id.to_string(),
        created_at: at(&task.created_at),
        completed_at: maybe_at(task.completed_at.as_ref()),
        latest_run: latest_run.map(RunSummary::from),
    }
}

#[cfg(test)]
mod tests {
    use agentos_persistence::ToolExecutionRecord;

    use super::*;

    #[test]
    fn event_summaries_prefer_the_most_specific_field() {
        let transition = serde_json::json!({"from": "idle", "to": "planning"});
        assert_eq!(summarise_event(&transition), "idle → planning");

        let tool = serde_json::json!({"tool": "browser.navigate", "risk": "medium"});
        assert_eq!(summarise_event(&tool), "browser.navigate");

        let model = serde_json::json!({"provider": "anthropic", "model": "claude-opus-5"});
        assert_eq!(summarise_event(&model), "anthropic/claude-opus-5");

        let drift = serde_json::json!({
            "tool": "terminal.exec",
            "undeclared": ["filesystem.read", "network.request"],
        });
        assert_eq!(
            summarise_event(&drift),
            "terminal.exec planned undeclared filesystem.read, network.request"
        );

        assert_eq!(summarise_event(&serde_json::json!({})), "");
    }

    #[test]
    fn an_execution_view_says_whether_it_actually_ran() {
        use agentos_core::ids::{TaskRunId, ToolExecutionId};
        use agentos_core::permission::Effect;
        use agentos_core::risk::RiskLevel;
        use agentos_core::tool::ToolOutcome;

        let record = ToolExecutionRecord {
            id: ToolExecutionId::new(),
            run_id: TaskRunId::new(),
            tool: "terminal.exec".into(),
            call_id: "c1".into(),
            arguments: serde_json::json!({"program": "curl"}),
            outcome: ToolOutcome::Denied,
            effect: Effect::Deny,
            risk: RiskLevel::High,
            tainted: true,
            approval_id: None,
            output_bytes: 0,
            error: Some("permission denied".into()),
            duration_ms: 0,
            started_at: agentos_core::now(),
            completed_at: None,
        };

        let view = ExecutionView::from(&record);
        assert!(!view.executed, "a denied call must not read as executed");
        assert_eq!(view.outcome, "denied");
        assert_eq!(view.effect, "deny");
        assert!(view.tainted);
    }
}
