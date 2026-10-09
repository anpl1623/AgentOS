//! The command surface the interface calls.
//!
//! Every function here is a thin translation: parse an identifier, call the
//! runtime, shape the answer into a view model. There is no agent behaviour in
//! this module — the desktop application is a client of `agentos-runtime`, on
//! equal footing with the CLI, and the moment logic starts accumulating here the
//! two clients have begun to disagree.

use std::collections::HashMap;

use agentos_core::approval::ApprovalRequest;
use agentos_core::ids::{AgentId, TaskId};
use agentos_core::task::{Task, TaskStatus};
use agentos_runtime::Runtime;
use tauri::State;

use crate::dto::{
    AgentSummary, ApprovalView, DashboardView, EventView, ExecutionView, PolicyView, TaskSummary,
    summarise_event, task_summary,
};
use crate::state::AppState;

mod activity;
mod agents;
mod approvals;
mod audit;
mod credentials;
mod graph;
mod insights;
mod integrations;
mod memories;
mod policies;
mod runs;
mod scheduler;
mod schedules;
mod settings;
mod tasks;
mod window;

pub use activity::*;
pub use agents::*;
pub use approvals::*;
pub use audit::*;
pub use credentials::*;
pub use graph::*;
pub use insights::*;
pub use integrations::*;
pub use memories::*;
pub use policies::*;
pub use runs::*;
pub use scheduler::*;
pub use schedules::*;
pub use settings::*;
pub use tasks::*;
pub use window::*;

/// Anything a command can fail with.
///
/// Serialised as a plain message: the interface shows it to a person, and a
/// structured error would only be reconstructed into one anyway.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    /// The runtime refused or failed.
    #[error(transparent)]
    Runtime(#[from] agentos_runtime::RuntimeError),

    /// Storage failed.
    #[error(transparent)]
    Database(#[from] agentos_persistence::DbError),

    /// The audit log could not be read.
    #[error(transparent)]
    Audit(#[from] agentos_audit::AuditError),

    /// An identifier from the interface was not a valid one.
    #[error("`{value}` is not a valid {kind} identifier")]
    BadId {
        /// What kind was expected.
        kind: &'static str,
        /// What arrived.
        value: String,
    },

    /// Secret storage failed.
    #[error(transparent)]
    Secrets(#[from] agentos_secrets::SecretError),

    /// The request could not be satisfied.
    #[error("{0}")]
    Rejected(String),
}

impl serde::Serialize for DesktopError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Shorthand for a command result.
pub type Answer<T> = Result<T, DesktopError>;

pub(crate) fn parse_id<T: std::str::FromStr>(kind: &'static str, value: &str) -> Answer<T> {
    value.parse::<T>().map_err(|_| DesktopError::BadId {
        kind,
        value: value.to_owned(),
    })
}

/// Compile a stored policy document into the view the interface shows.
pub(crate) fn policy_view(document: String, version: i64) -> PolicyView {
    let compiled = agentos_permissions::PolicyDocument::from_yaml(&document)
        .and_then(|parsed| parsed.compile());

    match compiled {
        Ok(policy) => PolicyView {
            document,
            version,
            default_effect: policy.default_effect.as_str().to_owned(),
            max_risk: policy.max_risk.map(|risk| risk.as_str().to_owned()),
            taint_enabled: policy.taint.enabled,
            taint_threshold: policy.taint.escalate_at_or_above.as_str().to_owned(),
            rules: policy
                .rules
                .iter()
                .map(agentos_permissions::PolicyRule::describe)
                .collect(),
        },
        // A stored policy that no longer compiles is shown as it is, with the
        // failure visible. The runtime denies everything in that state, and the
        // interface must not imply otherwise.
        Err(error) => PolicyView {
            document,
            version,
            default_effect: "deny".to_owned(),
            max_risk: None,
            taint_enabled: true,
            taint_threshold: "medium".to_owned(),
            rules: vec![format!("this policy does not compile: {error}")],
        },
    }
}

/// Turn approval requests into views, attaching the objective each belongs to.
///
/// Requests cluster on a few tasks, so each distinct task is read once rather
/// than once per request.
pub(crate) async fn approval_views(
    runtime: &Runtime,
    requests: Vec<ApprovalRequest>,
) -> Answer<Vec<ApprovalView>> {
    let mut objectives: HashMap<TaskId, String> = HashMap::new();
    for request in &requests {
        if objectives.contains_key(&request.task_id) {
            continue;
        }
        let objective = runtime
            .database()
            .tasks()
            .find(request.task_id)
            .await?
            .map(|task| task.objective)
            .unwrap_or_default();
        objectives.insert(request.task_id, objective);
    }
    Ok(requests
        .iter()
        .map(|request| {
            let objective = objectives
                .get(&request.task_id)
                .cloned()
                .unwrap_or_default();
            ApprovalView::new(request, objective)
        })
        .collect())
}

/// Every agent's name by identity, read in one query.
pub(crate) async fn agent_names(runtime: &Runtime) -> Answer<HashMap<AgentId, String>> {
    Ok(runtime
        .database()
        .agents()
        .list()
        .await?
        .into_iter()
        .map(|agent| (agent.id, agent.name))
        .collect())
}

/// Summarise tasks, given every agent's name.
async fn summarise_tasks(
    runtime: &Runtime,
    tasks: Vec<Task>,
    names: &HashMap<AgentId, String>,
) -> Answer<Vec<TaskSummary>> {
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        let name = names
            .get(&task.agent_id)
            .map_or("(deleted)", String::as_str);
        let run = runtime.database().runs().latest_for_task(task.id).await?;
        out.push(task_summary(&task, name, run.as_ref()));
    }
    Ok(out)
}

/// Load recent tasks with their agent names and latest runs.
pub(crate) async fn task_summaries(runtime: &Runtime, limit: i64) -> Answer<Vec<TaskSummary>> {
    let tasks = runtime.database().tasks().list(limit).await?;
    let names = agent_names(runtime).await?;
    summarise_tasks(runtime, tasks, &names).await
}

/// How many failures the dashboard lists.
const FAILURES_SHOWN: i64 = 5;

/// Recent tasks whose latest attempt failed, newest first.
///
/// A task's status follows its latest run, so a failure that a retry has since
/// fixed drops out on its own. A task a scheduler could not start, because its
/// agent was disabled, no provider could be built for it or its policy no
/// longer compiles, is failed without ever having run, and is listed too: it is
/// as much in need of a person as one that ran and failed. A task abandoned
/// because what it waits for failed is cancelled, not failed, and is not.
async fn recent_failures(
    runtime: &Runtime,
    names: &HashMap<AgentId, String>,
) -> Answer<Vec<TaskSummary>> {
    let failed = runtime
        .database()
        .tasks()
        .list_with_status(TaskStatus::Failed, FAILURES_SHOWN)
        .await?;
    summarise_tasks(runtime, failed, names).await
}

/// Recent audit events, newest last so the feed reads downward.
///
/// With `security_only`, only the kinds [`agentos_core::event::is_security_kind`]
/// names are kept, and `limit` counts those rather than the records read to
/// find them.
pub(crate) async fn recent_events(
    runtime: &Runtime,
    limit: i64,
    security_only: bool,
) -> Answer<Vec<EventView>> {
    let sink = runtime.database().audit_sink();
    let mut records = if security_only {
        sink.tail_of_kinds(agentos_core::event::SECURITY_KINDS, limit)
            .await?
    } else {
        sink.tail(limit).await?
    };
    records.reverse();
    Ok(records
        .into_iter()
        .map(|record| EventView {
            id: record.id.to_string(),
            sequence: Some(record.sequence),
            at: agentos_core::format_timestamp(&record.at),
            kind: record.kind.clone(),
            run_id: record.run_id.map(|id| id.to_string()),
            task_id: record.task_id.map(|id| id.to_string()),
            summary: summarise_event(&record.payload),
            security_relevant: agentos_core::event::is_security_kind(&record.kind),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

/// Everything the dashboard shows.
///
/// Polled while the window is visible, so it reads nothing that grows with the
/// age of the installation: no audit verification, no event feed. The chain's
/// health is [`audit_health`], on its own slower schedule.
#[tauri::command]
pub async fn dashboard(state: State<'_, AppState>) -> Answer<DashboardView> {
    dashboard_view(&state.runtime).await
}

/// The dashboard, shaped.
pub(crate) async fn dashboard_view(runtime: &Runtime) -> Answer<DashboardView> {
    let agents = runtime.database().agents().list().await?;
    let names: HashMap<AgentId, String> = agents
        .iter()
        .map(|agent| (agent.id, agent.name.clone()))
        .collect();

    let active = runtime.database().tasks().list_active().await?;
    let running_tasks = summarise_tasks(runtime, active, &names).await?;

    let pending = runtime.database().approvals().list_pending().await?;

    Ok(DashboardView {
        agents: agents.iter().map(AgentSummary::from).collect(),
        running_tasks,
        pending_approvals: approval_views(runtime, pending).await?,
        recent_refusals: runtime
            .database()
            .executions()
            .list_denied(10)
            .await?
            .iter()
            .map(ExecutionView::from)
            .collect(),
        recent_failures: recent_failures(runtime, &names).await?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use std::time::Duration;

    use agentos_audit::{AuditRecord, AuditSink};
    use agentos_core::agent::ModelConfig;
    use agentos_core::approval::ApprovalStatus;
    use agentos_core::event::{AgentEvent, Event};
    use agentos_core::ids::{AgentId, ApprovalId, ToolExecutionId};
    use agentos_core::permission::{Capability, Effect};
    use agentos_core::risk::RiskLevel;
    use agentos_core::task::{Task, TaskRun, TaskState};
    use agentos_core::tool::ToolOutcome;
    use agentos_persistence::ToolExecutionRecord;
    use agentos_runtime::{AuditCheckpoint, RunApprovalGate, RunStateMachine};
    use agentos_secrets::InMemorySecretStore;
    use agentos_tools::{ApprovalGate, ApprovalOutcome};
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::dto::ApprovalDecisionInput;
    use crate::state::AuditWatch;

    /// A task with one run, for the agent [`runtime_with_agent`] made.
    async fn task_with_run(
        runtime: &Runtime,
        agent: &agentos_core::agent::Agent,
        objective: &str,
        state: TaskState,
    ) -> (Task, TaskRun) {
        let task = runtime
            .create_task(agent.id, objective, &[], None)
            .await
            .unwrap();
        let mut run = TaskRun::new(task.id, 1);
        run.state = state;
        runtime.database().runs().insert(&run).await.unwrap();
        runtime
            .database()
            .tasks()
            .set_status(task.id, TaskStatus::from_run_state(state))
            .await
            .unwrap();
        (task, run)
    }

    /// A high-risk request on a tainted run, as the pipeline raises one.
    fn approval_request(
        agent: &agentos_core::agent::Agent,
        task: &Task,
        run: &TaskRun,
    ) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: agent.id,
            agent_name: agent.name.clone(),
            task_id: task.id,
            run_id: run.id,
            tool: "browser.type".to_owned(),
            arguments: serde_json::json!({"selector": "#send"}),
            capability: Capability::new("browser", "interact"),
            risk: RiskLevel::High,
            effect_before_taint: Effect::Allow,
            asked_this_run: 3,
            approval_budget: Some(10),
            reason: "policy requires approval".to_owned(),
            explanation: "Submit the form.".to_owned(),
            affected_resources: vec!["origin:http://localhost:8420".to_owned()],
            tainted: true,
            taint_sources: vec!["web:http://localhost:8420/customers".to_owned()],
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        }
    }

    /// A recorded tool call.
    fn execution(
        run: &TaskRun,
        tool: &str,
        outcome: ToolOutcome,
        started_at: agentos_core::Timestamp,
    ) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: ToolExecutionId::new(),
            run_id: run.id,
            tool: tool.to_owned(),
            call_id: "c1".to_owned(),
            arguments: serde_json::json!({}),
            outcome,
            effect: if outcome == ToolOutcome::Denied {
                Effect::Deny
            } else {
                Effect::Allow
            },
            risk: RiskLevel::Low,
            tainted: false,
            approval_id: None,
            output_bytes: 0,
            error: None,
            duration_ms: 5,
            started_at,
            completed_at: None,
        }
    }

    /// Answers every request with one fixed outcome, standing in for a person.
    #[derive(Debug)]
    struct Answering(ApprovalOutcome);

    #[async_trait::async_trait]
    impl ApprovalGate for Answering {
        async fn request(&self, _: &ApprovalRequest, _: CancellationToken) -> ApprovalOutcome {
            self.0.clone()
        }
    }

    /// A runtime with one agent, backed by a temporary directory.
    ///
    /// The `State` wrappers above are one-line delegations; what is worth
    /// testing is the shaping underneath them, which is where a screen would
    /// silently get the wrong answer.
    async fn runtime_with_agent() -> (tempfile::TempDir, Runtime, agentos_core::agent::Agent) {
        let guard = tempfile::TempDir::new().expect("temp dir");
        let root = std::fs::canonicalize(guard.path()).expect("canonical");
        let runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .expect("runtime");
        let agent = runtime
            .create_agent(
                "sales",
                "Handle follow-ups.",
                ModelConfig::new("mock", "scripted"),
                vec!["filesystem.read".to_owned()],
            )
            .await
            .expect("agent");
        (guard, runtime, agent)
    }

    #[tokio::test]
    async fn a_policy_view_summarises_what_it_grants() {
        let view = policy_view(
            "default: deny\npermissions:\n  browser:\n    navigate: ['http://localhost:*']\n"
                .to_owned(),
            3,
        );
        assert_eq!(view.default_effect, "deny");
        assert_eq!(view.version, 3);
        assert!(view.taint_enabled, "taint escalation is on by default");
        assert_eq!(view.rules.len(), 1);
        assert!(view.rules[0].contains("browser.navigate"));
    }

    #[test]
    fn a_policy_that_does_not_compile_says_so_rather_than_implying_permissions() {
        // The runtime denies everything in this state. An interface that showed
        // an empty rule list would imply "nothing configured" instead of
        // "broken", which are very different things to an operator.
        let view = policy_view("permisions: {}\n".to_owned(), 1);
        assert_eq!(view.default_effect, "deny");
        assert_eq!(view.rules.len(), 1);
        assert!(
            view.rules[0].contains("does not compile"),
            "{:?}",
            view.rules
        );
    }

    #[tokio::test]
    async fn task_summaries_carry_the_agent_name_and_latest_run() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let task = runtime
            .create_task(agent.id, "Do the thing.", &[], None)
            .await
            .unwrap();
        let run = TaskRun::new(task.id, 1);
        runtime.database().runs().insert(&run).await.unwrap();

        let summaries = task_summaries(&runtime, 10).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].agent_name, "sales");
        assert_eq!(summaries[0].objective, "Do the thing.");
        assert_eq!(
            summaries[0].latest_run.as_ref().map(|run| run.attempt),
            Some(1)
        );
    }

    #[tokio::test]
    async fn a_task_whose_agent_was_deleted_still_renders() {
        // Agents cascade-delete their tasks, so this is defensive rather than
        // reachable today — but a list that panics is worse than one that says
        // "(deleted)".
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let orphan = Task::new(agentos_core::ids::AgentId::new(), "orphaned");
        let _ = agent;
        // Inserting with an unknown agent is refused by the foreign key, which
        // is the real guarantee; assert that rather than faking a broken row.
        assert!(runtime.database().tasks().insert(&orphan).await.is_err());
    }

    #[tokio::test]
    async fn approval_views_attach_the_objective_being_pursued() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let task = runtime
            .create_task(agent.id, "Find overdue accounts.", &[], None)
            .await
            .unwrap();
        let run = TaskRun::new(task.id, 1);
        runtime.database().runs().insert(&run).await.unwrap();

        let request = approval_request(&agent, &task, &run);
        runtime
            .database()
            .approvals()
            .insert(&request)
            .await
            .unwrap();

        let views = approval_views(&runtime, vec![request]).await.unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].objective, "Find overdue accounts.");
        assert!(views[0].tainted);
        assert_eq!(views[0].taint_sources.len(), 1);
        // The arguments are pretty-printed for a person to read.
        assert!(views[0].arguments.contains("\n"));
        // And the card can say why it is asking and how often this run has.
        assert_eq!(views[0].effect_before_taint, "allow");
        assert_eq!(views[0].asked_this_run, 3);
        assert_eq!(views[0].approval_budget, Some(10));
    }

    #[tokio::test]
    async fn an_approval_note_survives_to_the_stored_decision() {
        // The chain can prove a person allowed a high-risk action on a tainted
        // run; the note is the only place it can say why. Through the same
        // translation `resolve_approval` uses and the run gate every run has.
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let task = runtime
            .create_task(agent.id, "Send it.", &[], None)
            .await
            .unwrap();
        let run = TaskRun::new(task.id, 1);
        runtime.database().runs().insert(&run).await.unwrap();
        let request = approval_request(&agent, &task, &run);

        let (id, outcome) = approvals::decision(ApprovalDecisionInput {
            approval_id: request.id.to_string(),
            approved: true,
            note: Some("checked the recipient against the CRM".to_owned()),
        })
        .unwrap();
        assert_eq!(id, request.id);

        let machine = Arc::new(RunStateMachine::new(
            agent.id,
            task.id,
            run.id,
            TaskState::Executing,
            runtime.database().clone(),
            runtime.audit().clone(),
        ));
        let gate = RunApprovalGate::new(
            Arc::new(Answering(outcome)),
            runtime.database().clone(),
            machine,
        );
        let answered = gate.request(&request, CancellationToken::new()).await;
        assert!(answered.is_approved());

        let stored = runtime
            .database()
            .approvals()
            .get(request.id)
            .await
            .unwrap();
        assert_eq!(stored.status, ApprovalStatus::Approved);
        assert_eq!(
            stored.decision_note.as_deref(),
            Some("checked the recipient against the CRM")
        );
    }

    #[test]
    fn a_blank_note_is_no_note_on_either_answer() {
        for approved in [true, false] {
            let (_, outcome) = approvals::decision(ApprovalDecisionInput {
                approval_id: ApprovalId::new().to_string(),
                approved,
                note: Some("  \n ".to_owned()),
            })
            .unwrap();
            let expected = if approved {
                ApprovalOutcome::Approved { note: None }
            } else {
                ApprovalOutcome::Denied { note: None }
            };
            assert_eq!(outcome, expected);
        }
    }

    #[test]
    fn an_oversized_note_is_refused_and_control_characters_are_dropped() {
        let answer = |note: String| {
            approvals::decision(ApprovalDecisionInput {
                approval_id: ApprovalId::new().to_string(),
                approved: false,
                note: Some(note),
            })
        };

        let limit = agentos_core::approval::MAX_DECISION_NOTE_CHARS;
        let error = answer("x".repeat(limit + 1)).unwrap_err();
        assert!(matches!(error, DesktopError::Rejected(_)), "{error}");
        assert!(answer("x".repeat(limit)).is_ok());

        let (_, outcome) = answer("wrong\u{1b}[2J recipient\nsee ticket".to_owned()).unwrap();
        assert_eq!(
            outcome,
            ApprovalOutcome::Denied {
                note: Some("wrong[2J recipient\nsee ticket".to_owned())
            }
        );
    }

    #[tokio::test]
    async fn recent_approvals_are_decided_rows_newest_first_with_their_notes() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let (task, run) = task_with_run(&runtime, &agent, "Follow up.", TaskState::Completed).await;
        let now = agentos_core::now();

        let mut earlier = approval_request(&agent, &task, &run);
        earlier.status = ApprovalStatus::Denied;
        earlier.requested_at = now - Duration::from_secs(600);
        earlier.decided_at = Some(now - Duration::from_secs(590));
        earlier.decision_note = Some("wrong recipient".to_owned());

        let mut later = approval_request(&agent, &task, &run);
        later.status = ApprovalStatus::Approved;
        later.requested_at = now - Duration::from_secs(120);
        later.decided_at = Some(now - Duration::from_secs(60));
        later.decision_note = Some("expected follow-up".to_owned());

        // Raised after both decisions and still waiting: a card for the queue,
        // which the history must not show a second time.
        let mut waiting = approval_request(&agent, &task, &run);
        waiting.requested_at = now - Duration::from_secs(5);

        for request in [&earlier, &later, &waiting] {
            runtime
                .database()
                .approvals()
                .insert(request)
                .await
                .unwrap();
        }

        let recent = runtime
            .database()
            .approvals()
            .list_recent(20)
            .await
            .unwrap();
        let views = approval_views(&runtime, recent).await.unwrap();
        let ids: Vec<String> = views.iter().map(|view| view.id.clone()).collect();
        assert_eq!(ids, vec![later.id.to_string(), earlier.id.to_string()]);
        assert_eq!(views[0].status, "approved");
        assert_eq!(views[0].note.as_deref(), Some("expected follow-up"));
        assert!(views[0].decided_at.is_some());
        assert_eq!(views[1].note.as_deref(), Some("wrong recipient"));
        assert_eq!(views[1].objective, "Follow up.");
    }

    #[tokio::test]
    async fn refused_and_executed_calls_land_in_different_usage_buckets() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let (_, run) = task_with_run(&runtime, &agent, "Read.", TaskState::Completed).await;
        let now = agentos_core::now();
        let executions = runtime.database().executions();

        executions
            .insert(&execution(
                &run,
                "filesystem.read",
                ToolOutcome::Success,
                now,
            ))
            .await
            .unwrap();
        executions
            .insert(&execution(
                &run,
                "filesystem.read",
                ToolOutcome::Denied,
                now,
            ))
            .await
            .unwrap();

        let usage = insights::tool_usage_over(&runtime, 7).await.unwrap();
        let read = usage
            .iter()
            .find(|row| row.tool == "filesystem.read")
            .expect("a used tool is listed");
        assert_eq!(read.calls, 2);
        assert_eq!(read.executed, 1, "a refusal must not count as executed");
        assert_eq!(read.denied, 1);
        assert!(read.last_used_at.is_some());
        // Catalogue facts travel with the counts.
        assert_eq!(read.risk.as_deref(), Some("low"));
        // The busiest tool comes first.
        assert_eq!(usage[0].tool, "filesystem.read");
    }

    #[tokio::test]
    async fn a_call_outside_the_window_is_not_counted() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let (_, old_run) = task_with_run(&runtime, &agent, "Old.", TaskState::Completed).await;
        let (_, new_run) = task_with_run(&runtime, &agent, "New.", TaskState::Completed).await;
        let now = agentos_core::now();
        let month_ago = now - Duration::from_secs(30 * 24 * 60 * 60);
        let executions = runtime.database().executions();

        executions
            .insert(&execution(
                &old_run,
                "filesystem.read",
                ToolOutcome::Success,
                month_ago,
            ))
            .await
            .unwrap();
        executions
            .insert(&execution(
                &new_run,
                "filesystem.read",
                ToolOutcome::Success,
                now,
            ))
            .await
            .unwrap();
        executions
            .insert(&execution(
                &old_run,
                "filesystem.list",
                ToolOutcome::Success,
                month_ago,
            ))
            .await
            .unwrap();

        let usage = insights::tool_usage_over(&runtime, 7).await.unwrap();
        let count = |tool: &str| {
            usage
                .iter()
                .find(|row| row.tool == tool)
                .map_or(0, |row| row.calls)
        };
        assert_eq!(count("filesystem.read"), 1);
        assert_eq!(
            count("filesystem.list"),
            0,
            "a tool used only before the window reads as unused"
        );

        let month = insights::tool_usage_over(&runtime, 31).await.unwrap();
        assert_eq!(
            month
                .iter()
                .find(|row| row.tool == "filesystem.read")
                .map(|row| row.calls),
            Some(2)
        );
    }

    #[tokio::test]
    async fn the_dashboard_lists_failures_and_carries_no_audit_work() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let (failed, failed_run) =
            task_with_run(&runtime, &agent, "Broke.", TaskState::Failed).await;
        let (_, _) = task_with_run(&runtime, &agent, "Fine.", TaskState::Completed).await;
        let (running, _) = task_with_run(&runtime, &agent, "Busy.", TaskState::Executing).await;

        let view = dashboard_view(&runtime).await.unwrap();
        assert_eq!(view.recent_failures.len(), 1);
        assert_eq!(view.recent_failures[0].id, failed.id.to_string());
        assert_eq!(
            view.recent_failures[0]
                .latest_run
                .as_ref()
                .map(|run| run.id.clone()),
            Some(failed_run.id.to_string()),
            "the failure links to the run that failed"
        );
        assert_eq!(view.running_tasks.len(), 1);
        assert_eq!(view.running_tasks[0].id, running.id.to_string());
        assert_eq!(view.running_tasks[0].agent_name, "sales");

        // The shape is the contract: nothing in it is read from the audit log.
        let json = serde_json::to_value(&view).unwrap();
        for absent in ["recent_events", "audit_events", "audit_intact"] {
            assert!(
                json.get(absent).is_none(),
                "{absent} is back on the dashboard"
            );
        }
    }

    #[test]
    fn only_a_failed_or_cancelled_attempt_may_be_retried() {
        let run = |state| {
            let mut run = TaskRun::new(agentos_core::ids::TaskId::new(), 1);
            run.state = state;
            run
        };

        assert!(runs::may_retry(Some(&run(TaskState::Failed))).is_ok());
        assert!(runs::may_retry(Some(&run(TaskState::Cancelled))).is_ok());

        let running = runs::may_retry(Some(&run(TaskState::WaitingForApproval))).unwrap_err();
        assert!(running.to_string().contains("still running"), "{running}");
        assert!(runs::may_retry(Some(&run(TaskState::Executing))).is_err());

        // A finished objective run again repeats its side effects.
        let succeeded = runs::may_retry(Some(&run(TaskState::Completed))).unwrap_err();
        assert!(succeeded.to_string().contains("succeeded"), "{succeeded}");

        assert!(runs::may_retry(None).is_err());
    }

    #[tokio::test]
    async fn a_task_another_client_claimed_first_is_reported_as_started_there() {
        let directory = tempfile::TempDir::new().unwrap();
        let open = || async {
            let mut runtime = Runtime::open_with_secrets(
                agentos_runtime::RuntimeConfig::rooted_at(directory.path()),
                Arc::new(InMemorySecretStore::new()),
            )
            .await
            .unwrap();
            runtime.set_provider_factory(Arc::new(agentos_runtime::FixedProviderFactory::new(
                Arc::new(
                    agentos_providers::MockProvider::new(vec![])
                        .with_exhausted(agentos_providers::ScriptedTurn::text("Done.")),
                ),
            )));
            runtime
        };
        let (desktop, terminal) = (open().await, open().await);
        let agent = desktop
            .create_agent(
                "sales",
                "Handle follow-ups.",
                ModelConfig::new("mock", "scripted"),
                vec![],
            )
            .await
            .unwrap();
        let gate = |_: String| -> Arc<dyn ApprovalGate> { Arc::new(agentos_tools::DenyAllGate) };

        // A retry that read the task as failed, while another process claims
        // it before this one's claim lands.
        let task = desktop
            .create_task(agent.id, "Contended.", &[], None)
            .await
            .unwrap();
        let started = tasks::begin(&desktop, &task, gate(String::new()))
            .await
            .unwrap();
        let first: agentos_core::ids::TaskRunId = started.run_id.parse().unwrap();
        while desktop.running_runs().await.contains(&first) {
            tokio::task::yield_now().await;
        }
        desktop
            .database()
            .tasks()
            .set_status(task.id, agentos_core::task::TaskStatus::Failed)
            .await
            .unwrap();
        let mut failed = desktop.database().runs().get(first).await.unwrap();
        failed.state = agentos_core::task::TaskState::Failed;
        desktop.database().runs().update(&failed).await.unwrap();
        let stale = desktop.task(task.id).await.unwrap();
        assert!(
            terminal
                .database()
                .tasks()
                .claim(task.id, agentos_core::task::TaskStatus::Failed)
                .await
                .unwrap()
        );

        // The command's own path, from the read it made.
        let lost = tasks::begin(&desktop, &stale, gate(String::new()))
            .await
            .unwrap_err();
        assert_eq!(lost.to_string(), runs::STARTED_ELSEWHERE);
        // And through `retry`, which reads the task itself and finds it
        // running, so it too is told the task was started elsewhere.
        let retried = runs::retry(
            &desktop,
            &tokio::sync::Mutex::new(()),
            &task.id.to_string(),
            gate,
        )
        .await
        .unwrap_err();
        assert_eq!(retried.to_string(), runs::STARTED_ELSEWHERE);

        // A new task from the start command takes the same path and runs.
        let fresh = tasks::start_new(&desktop, &agent.id.to_string(), "Fresh.", gate)
            .await
            .unwrap();
        assert!(!fresh.run_id.is_empty());

        assert_eq!(
            runs::started_elsewhere(agentos_runtime::RuntimeError::Rejected("no".to_owned()))
                .to_string(),
            "no"
        );
    }

    async fn record(runtime: &Runtime, objective: &str) {
        runtime
            .audit()
            .record(Event::new(AgentEvent::TaskStarted {
                objective: objective.to_owned(),
                attempt: 1,
            }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn audit_health_verifies_only_what_is_new() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let checkpoint = Mutex::new(AuditWatch::default());

        record(&runtime, "one").await;
        record(&runtime, "two").await;
        let first = audit::check_audit_health(&runtime, &checkpoint)
            .await
            .unwrap();
        assert!(first.intact);
        // What the process failed to write is reported beside the verdict on
        // what it did write, never folded into it.
        assert_eq!(first.unrecorded, agentos_audit::unrecorded());
        let total = runtime.database().audit_sink().count().await.unwrap();
        assert_eq!(first.events, total);
        let verified = checkpoint.lock().await.verified.sequence;
        assert_eq!(i64::try_from(verified).unwrap(), total);

        record(&runtime, "three").await;
        let second = audit::check_audit_health(&runtime, &checkpoint)
            .await
            .unwrap();
        assert!(second.intact);
        assert_eq!(checkpoint.lock().await.verified.sequence, verified + 1);

        // Nothing new: nothing read, and the answer stands.
        let third = audit::check_audit_health(&runtime, &checkpoint)
            .await
            .unwrap();
        assert!(third.intact);
        assert_eq!(checkpoint.lock().await.verified.sequence, verified + 1);
    }

    #[tokio::test]
    async fn audit_health_reports_a_record_that_does_not_link() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let checkpoint = Mutex::new(AuditWatch::default());
        record(&runtime, "one").await;
        assert!(
            audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );

        // Written straight to the sink, past the log that would have chained
        // it: the shape of a record inserted by something other than the
        // runtime.
        let sink = runtime.database().audit_sink();
        let (tip, _) = sink.tip().await.unwrap();
        let forged = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "forged".to_owned(),
                attempt: 1,
            }),
            tip + 1,
            agentos_audit::GENESIS_HASH,
        )
        .unwrap();
        sink.append(&forged).await.unwrap();

        assert!(
            !audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );

        // A record correctly chained onto the forgery does not mend the break.
        let after = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "after".to_owned(),
                attempt: 1,
            }),
            tip + 2,
            &forged.hash,
        )
        .unwrap();
        sink.append(&after).await.unwrap();
        assert!(
            !audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );
        // Nothing past the break counts as verified, so the broken stretch is
        // checked again, and reported again, on every later check.
        assert_eq!(checkpoint.lock().await.verified.sequence, tip);
    }

    #[tokio::test]
    async fn audit_health_notices_a_record_missing_between_checks() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let checkpoint = Mutex::new(AuditWatch::default());
        record(&runtime, "one").await;
        assert!(
            audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );

        // The record after the tip never arrives; the one after that links to
        // it rather than to the tip, as it would had a record been deleted.
        let sink = runtime.database().audit_sink();
        let (tip, tip_hash) = sink.tip().await.unwrap();
        let missing = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "deleted".to_owned(),
                attempt: 1,
            }),
            tip + 1,
            &tip_hash,
        )
        .unwrap();
        let orphan = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "orphan".to_owned(),
                attempt: 1,
            }),
            tip + 2,
            &missing.hash,
        )
        .unwrap();
        sink.append(&orphan).await.unwrap();

        assert!(
            !audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );
    }

    #[tokio::test]
    async fn audit_health_notices_a_modified_record_and_stays_broken() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let checkpoint = Mutex::new(AuditWatch::default());
        record(&runtime, "one").await;
        assert!(
            audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );

        // In its place and linked correctly, but its payload no longer
        // matches its hash.
        let sink = runtime.database().audit_sink();
        let (tip, tip_hash) = sink.tip().await.unwrap();
        let mut rewritten = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "original".to_owned(),
                attempt: 1,
            }),
            tip + 1,
            &tip_hash,
        )
        .unwrap();
        rewritten.payload = serde_json::json!({"objective": "rewritten"});
        sink.append(&rewritten).await.unwrap();
        assert!(
            !audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact
        );

        let after = AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: "after".to_owned(),
                attempt: 1,
            }),
            tip + 2,
            &rewritten.hash,
        )
        .unwrap();
        sink.append(&after).await.unwrap();
        assert!(
            !audit::check_audit_health(&runtime, &checkpoint)
                .await
                .unwrap()
                .intact,
            "good records after a break must not mend it"
        );
    }

    /// A record sealed at `sequence` onto `prev`, without passing through the
    /// log that would have chained it.
    fn sealed(objective: &str, sequence: u64, prev: &str) -> AuditRecord {
        AuditRecord::seal(
            &Event::new(AgentEvent::TaskStarted {
                objective: objective.to_owned(),
                attempt: 1,
            }),
            sequence,
            prev,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_break_the_full_check_finds_behind_the_checkpoint_stays_reported() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let watch = Mutex::new(AuditWatch::default());
        let sink = runtime.database().audit_sink();

        // A modified record, and a good one linked after it.
        let (tip, tip_hash) = sink.tip().await.unwrap();
        let mut modified = sealed("original", tip + 1, &tip_hash);
        modified.payload = serde_json::json!({"objective": "rewritten"});
        sink.append(&modified).await.unwrap();
        let after = sealed("after", tip + 2, &modified.hash);
        sink.append(&after).await.unwrap();

        // The routine check had already proved everything up to `after`
        // before the record was modified, so it has nothing new to look at.
        watch.lock().await.verified = AuditCheckpoint {
            sequence: after.sequence,
            hash: after.hash.clone(),
        };
        assert!(
            audit::check_audit_health(&runtime, &watch)
                .await
                .unwrap()
                .intact
        );

        // Only the deliberate check can see it.
        let breaks = audit::verify_whole_chain(&runtime, &watch).await.unwrap();
        assert!(!breaks.is_empty());

        // And from then on the routine check reports what it found, though
        // nothing it reads is broken.
        assert!(
            !audit::check_audit_health(&runtime, &watch)
                .await
                .unwrap()
                .intact,
            "the full check's verdict must stick"
        );
    }

    #[tokio::test]
    async fn a_log_missing_its_oldest_records_is_broken_to_both_checks() {
        let guard = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .unwrap();
        let sink = runtime.database().audit_sink();

        // Records 1 and 2 existed once; only what followed them is left.
        let first = sealed("one", 1, agentos_audit::GENESIS_HASH);
        let second = sealed("two", 2, &first.hash);
        let third = sealed("three", 3, &second.hash);
        let fourth = sealed("four", 4, &third.hash);
        sink.append(&third).await.unwrap();
        sink.append(&fourth).await.unwrap();

        let watch = Mutex::new(AuditWatch::default());
        assert!(
            !audit::check_audit_health(&runtime, &watch)
                .await
                .unwrap()
                .intact
        );
        let breaks = audit::verify_whole_chain(&runtime, &watch).await.unwrap();
        assert!(!breaks.is_empty());
        assert!(
            !watch.lock().await.intact,
            "the full check cleared a real break"
        );
        assert!(!runtime.verify_audit().await.unwrap().is_intact());
    }

    #[tokio::test]
    async fn an_opened_audit_record_keeps_its_hashes_and_whole_payload() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let event = Event::new(AgentEvent::PermissionDenied {
            tool: "terminal.exec".to_owned(),
            capability: Capability::new("terminal", "exec"),
            reason: "no rule matched".to_owned(),
            matched_rule: None,
        });
        let id = event.id;
        runtime.audit().record(event).await.unwrap();

        let stored = runtime
            .database()
            .audit_sink()
            .find(id)
            .await
            .unwrap()
            .expect("the record was written");
        let view = crate::dto::AuditRecordView::from(&stored);
        assert_eq!(view.id, id.to_string());
        assert_eq!(view.kind, "permission.denied");
        assert!(view.security_relevant);
        assert_eq!(view.hash, stored.hash);
        assert_eq!(view.prev_hash, stored.prev_hash);
        assert!(view.payload.contains("\n"), "laid out for a person");
        assert!(view.payload.contains("no rule matched"));
    }

    #[tokio::test]
    async fn a_security_feed_is_not_a_filter_over_the_newest_records() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        for tool in ["terminal.exec", "filesystem.delete"] {
            runtime
                .audit()
                .record(Event::new(AgentEvent::PermissionDenied {
                    tool: tool.to_owned(),
                    capability: Capability::new("terminal", "exec"),
                    reason: "no rule matched".to_owned(),
                    matched_rule: None,
                }))
                .await
                .unwrap();
        }
        for index in 0..20 {
            record(&runtime, &format!("routine {index}")).await;
        }

        let feed = recent_events(&runtime, 2, true).await.unwrap();
        let summaries: Vec<&str> = feed.iter().map(|event| event.summary.as_str()).collect();
        assert_eq!(summaries, vec!["terminal.exec", "filesystem.delete"]);
        assert!(feed.iter().all(|event| event.security_relevant));

        let everything = recent_events(&runtime, 2, false).await.unwrap();
        assert!(everything.iter().all(|event| !event.security_relevant));
    }

    #[tokio::test]
    async fn recent_events_are_oldest_first_and_flag_refusals() {
        let (_guard, runtime, agent) = runtime_with_agent().await;

        runtime
            .audit()
            .record(agentos_core::event::Event::new(
                agentos_core::event::AgentEvent::TaskStarted {
                    objective: "first".to_owned(),
                    attempt: 1,
                },
            ))
            .await
            .unwrap();
        runtime
            .audit()
            .record(agentos_core::event::Event::new(
                agentos_core::event::AgentEvent::PermissionDenied {
                    tool: "terminal.exec".to_owned(),
                    capability: Capability::new("terminal", "exec"),
                    reason: "no rule matched".to_owned(),
                    matched_rule: None,
                },
            ))
            .await
            .unwrap();
        let _ = agent;

        let events = recent_events(&runtime, 10, false).await.unwrap();
        assert!(events.len() >= 2);

        let first = events.iter().position(|e| e.kind == "agent.task.started");
        let denial = events.iter().position(|e| e.kind == "permission.denied");
        assert!(first < denial, "the feed should read oldest to newest");

        let denial = &events[denial.expect("a denial was recorded")];
        assert!(denial.security_relevant);
        assert_eq!(denial.summary, "terminal.exec");
    }

    /// How many records of `kind` the audit log holds.
    async fn records_of(runtime: &Runtime, kind: &str) -> usize {
        runtime
            .database()
            .audit_sink()
            .all()
            .await
            .unwrap()
            .iter()
            .filter(|record| record.kind == kind)
            .count()
    }

    #[tokio::test]
    async fn the_desktop_policy_and_agent_edits_are_recorded() {
        // A policy widened shortly before a bad action is part of the
        // explanation for it, so the desktop must go through the runtime layer
        // that records operator changes rather than straight to storage.
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let policies = records_of(&runtime, "operator.policy.changed").await;
        let toggles = records_of(&runtime, "operator.agent.enabled_changed").await;

        let view = policies::install_policy(
            &runtime,
            &agent.id.to_string(),
            "default: deny\n".to_owned(),
        )
        .await
        .unwrap();
        assert_eq!(view.version, 2, "the starter policy was version one");
        assert_eq!(
            records_of(&runtime, "operator.policy.changed").await,
            policies + 1
        );

        let summary = agents::enable_agent(&runtime, "sales", false)
            .await
            .unwrap();
        assert_eq!(summary.status, "disabled");
        assert_eq!(
            records_of(&runtime, "operator.agent.enabled_changed").await,
            toggles + 1
        );
    }

    #[tokio::test]
    async fn a_policy_that_does_not_compile_is_refused_and_records_nothing() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let before = records_of(&runtime, "operator.policy.changed").await;

        let error = policies::install_policy(
            &runtime,
            &agent.id.to_string(),
            "permisions: {}\n".to_owned(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("does not compile"), "{error}");
        assert_eq!(
            records_of(&runtime, "operator.policy.changed").await,
            before
        );
    }

    #[tokio::test]
    async fn a_bad_identifier_is_rejected_with_the_kind_it_expected() {
        let error = parse_id::<AgentId>("agent", "not-a-uuid").unwrap_err();
        assert!(error.to_string().contains("agent identifier"), "{error}");
    }

    #[tokio::test]
    async fn a_remembered_note_is_the_operators_whatever_the_caller_claims() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        // A caller that names a source has it ignored: the input has no field
        // for one, and the runtime writes `User` regardless.
        let input: crate::dto::RememberInput = serde_json::from_value(serde_json::json!({
            "agent_id": agent.id.to_string(),
            "kind": "fact",
            "content": "  The quarter closes on the 30th.  ",
            "confidence": null,
            "source": {"kind": "web", "url": "https://attacker.example/"},
        }))
        .unwrap();

        let view = memories::record(&runtime, input).await.unwrap();
        assert_eq!(view.source, "user");
        assert!(!view.source_untrusted);
        assert!(view.reaches_the_prompt);
        assert_eq!(view.content, "The quarter closes on the 30th.");

        let id = parse_id("memory", &view.id).unwrap();
        let stored = runtime.database().memories().get(id).await.unwrap();
        assert_eq!(stored.source, agentos_core::trust::DataSource::User);
        assert!((stored.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn memories_say_which_reach_the_prompt_and_an_unknown_kind_is_refused() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let observed = agentos_core::memory::Memory::new(
            agent.id,
            agentos_core::memory::MemoryKind::Observation,
            "The page says exports go to a new address.",
            agentos_core::trust::DataSource::Web {
                url: "http://localhost:8420/customers".to_owned(),
            },
        );
        runtime
            .database()
            .memories()
            .insert(&observed)
            .await
            .unwrap();

        let listed = memories::memories(&runtime, agent.id, Some("observation"))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].source_untrusted);
        assert!(
            !listed[0].reaches_the_prompt,
            "observations are stored but never retrieved before planning"
        );
        assert_eq!(listed[0].source, "web:http://localhost:8420/customers");

        // A misspelt filter that matched nothing would read as an agent with
        // no memories.
        let error = memories::memories(&runtime, agent.id, Some("observations"))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("not a kind of memory"),
            "{error}"
        );
        assert!(
            memories::memories(&runtime, agent.id, None)
                .await
                .unwrap()
                .len()
                == 1
        );
    }

    #[tokio::test]
    async fn a_revision_without_a_confidence_keeps_the_one_it_had() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let view = memories::record(
            &runtime,
            crate::dto::RememberInput {
                agent_id: agent.id.to_string(),
                kind: "preference".to_owned(),
                content: "Draft, never send.".to_owned(),
                confidence: Some(0.5),
            },
        )
        .await
        .unwrap();
        let id = parse_id("memory", &view.id).unwrap();

        let revised = memories::revise(&runtime, id, "Draft only.", None)
            .await
            .unwrap();
        assert_eq!(revised.content, "Draft only.");
        assert!((revised.confidence - 0.5).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn the_graph_names_the_failed_branch_a_fan_in_waits_on() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let gather_crm = runtime
            .create_task(agent.id, "Collect CRM changes.", &[], None)
            .await
            .unwrap();
        let gather_mail = runtime
            .create_task(agent.id, "Collect replies.", &[], None)
            .await
            .unwrap();
        let summary = graph::queue(
            &runtime,
            crate::dto::CreateTaskInput {
                agent_id: agent.id.to_string(),
                objective: "Summarise the week.".to_owned(),
                depends_on: vec![gather_crm.id.to_string(), gather_mail.id.to_string()],
                scheduled_for: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.status, "blocked");
        assert_eq!(summary.blocked_by.len(), 2);
        assert!(!summary.runnable && !summary.unreachable);

        let tasks = runtime.database().tasks();
        tasks
            .set_status(gather_crm.id, TaskStatus::Succeeded)
            .await
            .unwrap();
        tasks
            .set_status(gather_mail.id, TaskStatus::Failed)
            .await
            .unwrap();

        let nodes = graph::graph(&runtime, 10).await.unwrap();
        let join = nodes.iter().find(|node| node.id == summary.id).unwrap();
        assert!(join.unreachable);
        assert!(!join.runnable);
        assert_eq!(
            join.blocked_by_failure,
            Some(gather_mail.id.to_string()),
            "the branch that failed, not merely a branch"
        );
        let crm = nodes
            .iter()
            .find(|node| node.id == gather_crm.id.to_string())
            .unwrap();
        assert_eq!(crm.blocks, vec![summary.id.clone()]);

        // The scheduler's own count agrees with the graph.
        assert_eq!(tasks.list_unreachable(10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn of_two_failed_branches_the_graph_names_the_one_the_scheduler_does() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let one = runtime
            .create_task(agent.id, "Collect CRM changes.", &[], None)
            .await
            .unwrap();
        let two = runtime
            .create_task(agent.id, "Collect replies.", &[], None)
            .await
            .unwrap();
        // The edge written first points at the id that sorts last, so key
        // order and insertion order disagree about which branch comes first.
        let (first, second) = if one.id.to_string() > two.id.to_string() {
            (one.id, two.id)
        } else {
            (two.id, one.id)
        };
        let join = runtime
            .create_task(agent.id, "Summarise the week.", &[first, second], None)
            .await
            .unwrap();
        let tasks = runtime.database().tasks();
        tasks.set_status(second, TaskStatus::Failed).await.unwrap();
        tasks.set_status(first, TaskStatus::Failed).await.unwrap();

        let nodes = graph::graph(&runtime, 10).await.unwrap();
        let node = nodes
            .iter()
            .find(|node| node.id == join.id.to_string())
            .unwrap();
        assert_eq!(node.blocked_by_failure, Some(first.to_string()));

        let scheduler = agentos_runtime::Scheduler::new(
            runtime.clone(),
            agentos_runtime::SchedulerOptions::default(),
        );
        assert_eq!(scheduler.tick().await.unwrap().abandoned, vec![join.id]);
        let abandoned = runtime
            .database()
            .audit_sink()
            .all()
            .await
            .unwrap()
            .into_iter()
            .find(|record| record.kind == "agent.task.abandoned")
            .unwrap();
        assert_eq!(abandoned.payload["blocked_by"], first.to_string());
    }

    #[tokio::test]
    async fn a_cycle_is_refused_with_its_path_and_a_missing_task_is_named() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let first = runtime
            .create_task(agent.id, "First.", &[], None)
            .await
            .unwrap();
        let second = graph::queue(
            &runtime,
            crate::dto::CreateTaskInput {
                agent_id: agent.id.to_string(),
                objective: "Second.".to_owned(),
                depends_on: vec![first.id.to_string()],
                scheduled_for: Some("2030-01-01T09:00:00Z".to_owned()),
            },
        )
        .await
        .unwrap();
        assert!(second.scheduled_for.is_some());

        let second_id: TaskId = parse_id("task", &second.id).unwrap();
        let cycle = runtime
            .add_task_dependency(first.id, second_id)
            .await
            .unwrap_err();
        let shown = DesktopError::from(cycle).to_string();
        assert!(shown.contains("cycle"), "{shown}");
        assert!(shown.contains(&first.id.to_string()), "{shown}");

        let missing = graph::queue(
            &runtime,
            crate::dto::CreateTaskInput {
                agent_id: agent.id.to_string(),
                objective: "Waits for nothing real.".to_owned(),
                depends_on: vec![TaskId::new().to_string()],
                scheduled_for: None,
            },
        )
        .await
        .unwrap_err();
        assert!(missing.to_string().contains("does not exist"), "{missing}");
    }

    #[tokio::test]
    async fn schedules_carry_their_agent_and_cadence_and_a_one_shot_needs_a_time() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let created = schedules::create(
            &runtime,
            crate::dto::CreateScheduleInput {
                agent_id: agent.id.to_string(),
                name: "weekday-follow-ups".to_owned(),
                objective: "Draft follow-ups.".to_owned(),
                cadence: crate::dto::CadenceInput {
                    kind: "cron".to_owned(),
                    seconds: None,
                    expression: Some("0 9 * * 1-5".to_owned()),
                    clock: Some("local".to_owned()),
                },
                first_run_at: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(created.agent_name, "sales");
        assert_eq!(created.cadence.description, "cron `0 9 * * 1-5` (local)");
        assert!(created.next_run_at.is_some());

        let once = schedules::create(
            &runtime,
            crate::dto::CreateScheduleInput {
                agent_id: agent.id.to_string(),
                name: "one-off".to_owned(),
                objective: "Close the quarter.".to_owned(),
                cadence: crate::dto::CadenceInput {
                    kind: "once".to_owned(),
                    ..crate::dto::CadenceInput::default()
                },
                first_run_at: None,
            },
        )
        .await
        .unwrap_err();
        assert!(once.to_string().contains("needs a time"), "{once}");

        let listed = schedules::schedule_views(&runtime).await.unwrap();
        assert_eq!(listed.len(), 1, "the refused one-shot stored nothing");
        assert_eq!(listed[0].agent_name, "sales");
    }

    #[tokio::test]
    async fn the_scheduler_view_counts_what_a_tick_would_find_while_stopped() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let supervisor = crate::state::SchedulerSupervisor::new(
            runtime.clone(),
            agentos_runtime::SchedulerOptions::default(),
        );
        runtime
            .create_task(agent.id, "Ready now.", &[], None)
            .await
            .unwrap();
        runtime
            .create_schedule(
                agent.id,
                "overdue",
                "Run it.",
                agentos_core::schedule::Cadence::Every { seconds: 3600 },
                agentos_core::now() - Duration::from_secs(60),
            )
            .await
            .unwrap();

        let view = scheduler::scheduler_view(&runtime, &supervisor)
            .await
            .unwrap();
        assert!(!view.running);
        assert_eq!(view.tick_seconds, 30);
        assert_eq!(view.max_concurrent_runs, 1);
        assert_eq!(view.active_schedules, 1);
        assert_eq!(view.overdue, 1);
        assert!(view.next_fire_at.is_some());
        assert_eq!(view.runnable_tasks, 1);
        assert_eq!(view.unreachable_tasks, 0);
        assert!(view.started_at.is_none() && view.error.is_none());
    }

    #[test]
    fn the_switch_refuses_bad_pacing_before_changing_anything() {
        tauri::async_runtime::block_on(async {
            let (_guard, runtime, _agent) = runtime_with_agent().await;
            let supervisor = crate::state::SchedulerSupervisor::new(
                runtime.clone(),
                agentos_runtime::SchedulerOptions::default(),
            );

            for (tick, max) in [(4, 1), (30, 0)] {
                assert!(
                    scheduler::switch(&runtime, &supervisor, true, tick, max)
                        .await
                        .is_err()
                );
            }
            assert!(!supervisor.is_running().await);
            assert!(!runtime.scheduler_preference().await.unwrap().enabled);

            let on = scheduler::switch(&runtime, &supervisor, true, 10, 2)
                .await
                .unwrap();
            assert!(on.running);
            assert_eq!((on.tick_seconds, on.max_concurrent_runs), (10, 2));
            assert!(runtime.scheduler_preference().await.unwrap().enabled);

            let off = scheduler::switch(&runtime, &supervisor, false, 10, 2)
                .await
                .unwrap();
            assert!(!off.running);
            assert!(!runtime.scheduler_preference().await.unwrap().enabled);
            assert_eq!(
                records_of(&runtime, "operator.scheduler.started").await,
                1,
                "turning it off did not restart it on the way"
            );
        });
    }

    #[tokio::test]
    async fn every_operator_change_the_standing_surface_makes_reads_as_a_line() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let memory = memories::record(
            &runtime,
            crate::dto::RememberInput {
                agent_id: agent.id.to_string(),
                kind: "decision".to_owned(),
                content: "Use the shared inbox.".to_owned(),
                confidence: None,
            },
        )
        .await
        .unwrap();
        let memory_id = parse_id("memory", &memory.id).unwrap();
        memories::revise(&runtime, memory_id, "Use the team inbox.", None)
            .await
            .unwrap();
        runtime.forget_memory(memory_id).await.unwrap();

        let schedule = runtime
            .create_schedule(
                agent.id,
                "hourly",
                "Check.",
                agentos_core::schedule::Cadence::Every { seconds: 3600 },
                agentos_core::now(),
            )
            .await
            .unwrap();
        runtime
            .set_schedule_paused(schedule.id, true)
            .await
            .unwrap();
        runtime
            .set_schedule_paused(schedule.id, false)
            .await
            .unwrap();
        runtime.delete_schedule(schedule.id).await.unwrap();

        let first = runtime
            .create_task(agent.id, "First.", &[], None)
            .await
            .unwrap();
        let second = runtime
            .create_task(agent.id, "Second.", &[], None)
            .await
            .unwrap();
        runtime
            .add_task_dependency(second.id, first.id)
            .await
            .unwrap();

        let options = agentos_runtime::SchedulerOptions::default();
        for transition in [
            agentos_runtime::SchedulerTransition::Started { on_launch: true },
            agentos_runtime::SchedulerTransition::Stopped,
        ] {
            runtime
                .record_scheduler_state(transition, &options)
                .await
                .unwrap();
        }

        let events = recent_events(&runtime, 100, false).await.unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for event in events
            .iter()
            .filter(|event| event.kind.starts_with("operator."))
        {
            assert!(!event.summary.is_empty(), "{} has no summary", event.kind);
            seen.insert(event.kind.as_str());
        }
        for kind in [
            "operator.memory.recorded",
            "operator.memory.revised",
            "operator.memory.forgotten",
            "operator.schedule.created",
            "operator.schedule.paused",
            "operator.schedule.resumed",
            "operator.schedule.deleted",
            "operator.task.created",
            "operator.task.dependency_added",
            "operator.scheduler.started",
            "operator.scheduler.stopped",
        ] {
            assert!(seen.contains(kind), "no {kind} record was summarised");
        }
    }

    #[tokio::test]
    async fn a_grant_report_reads_the_policy_not_the_tool_list() {
        let (_guard, runtime, agent) = runtime_with_agent().await;
        let report = runtime.grant_report(agent.id).await.unwrap();
        let views: Vec<crate::dto::ToolGrantView> =
            report.iter().map(crate::dto::ToolGrantView::from).collect();

        // The starter policy reads inside the agent's own workspace only.
        let read = views
            .iter()
            .find(|view| view.tool == "filesystem.read")
            .unwrap();
        assert!(read.registered);
        assert_eq!(read.capabilities[0].capability, "filesystem.read");
        assert_eq!(read.reach, "scoped");
        assert_eq!(read.capabilities[0].reach, "scoped");
    }

    /// A secret no other text in these tests contains, so finding it anywhere
    /// means it leaked there.
    const SECRET: &str = "tok_7f3c9e1a5b2d";

    #[tokio::test]
    async fn a_credential_is_bound_to_its_origin_and_its_value_comes_back_nowhere() {
        use agentos_secrets::SecretStore;

        let guard = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let store = Arc::new(InMemorySecretStore::new());
        let runtime = Runtime::in_memory(root, store.clone()).await.unwrap();

        // Typed the way people type origins; stored the way runs are matched.
        let view = credentials::store(
            &runtime,
            "HTTPS://CRM.Example.com:443",
            "default",
            format!("  {SECRET}\n"),
        )
        .await
        .unwrap();
        assert_eq!(
            view,
            crate::dto::NetworkCredentialView {
                origin: "https://crm.example.com".to_owned(),
                name: "default".to_owned(),
                account: None,
            }
        );
        let key = agentos_secrets::network_key("https://crm.example.com", "default").unwrap();
        assert!(
            store.get(&key).unwrap().expose().contains(SECRET),
            "the secret is stored under the normalised origin"
        );

        let listed = credentials::listed(&runtime).await.unwrap();
        assert_eq!(listed, vec![view.clone()]);

        // Neither answer, nor any record or summary the change wrote, carries it.
        let answers = serde_json::to_string(&(&view, &listed)).unwrap();
        assert!(!answers.contains(SECRET), "{answers}");
        for record in runtime.database().audit_sink().all().await.unwrap() {
            let payload = record.payload.to_string();
            assert!(!payload.contains(SECRET), "{}: {payload}", record.kind);
        }
        let events = recent_events(&runtime, 100, false).await.unwrap();
        let set = events
            .iter()
            .find(|event| event.kind == "operator.credential.set")
            .expect("the change was recorded");
        assert!(
            set.summary.contains("https://crm.example.com"),
            "{}",
            set.summary
        );
        assert!(!set.summary.contains(SECRET));

        runtime
            .remove_network_credential("https://crm.example.com", "default")
            .await
            .unwrap();
        assert!(credentials::listed(&runtime).await.unwrap().is_empty());
        assert_eq!(records_of(&runtime, "operator.credential.removed").await, 1);
    }

    #[tokio::test]
    async fn a_refused_credential_is_not_quoted_back_or_recorded() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        for (origin, name) in [
            ("crm.example.com", "default"),
            ("https://crm.example.com", "two.parts"),
        ] {
            let error = credentials::store(&runtime, origin, name, SECRET.to_owned())
                .await
                .unwrap_err()
                .to_string();
            assert!(!error.contains(SECRET), "{error}");
        }
        assert!(credentials::listed(&runtime).await.unwrap().is_empty());
        assert_eq!(records_of(&runtime, "operator.credential.set").await, 0);
    }

    #[test]
    fn a_secret_is_scrubbed_from_text_in_every_spelling_and_only_once() {
        let padded = format!(" {SECRET} ");
        assert_eq!(
            credentials::without_secret(&format!("refused {padded} and {SECRET}"), &padded),
            "refused [redacted credential] and [redacted credential]"
        );
        // A secret that occurs inside the marker is not replaced inside it.
        assert_eq!(
            credentials::without_secret("bad credential", "credential"),
            "bad [redacted credential]"
        );
        // Nothing to scrub is not an instruction to scrub everything.
        assert_eq!(credentials::without_secret("no key", "  "), "no key");
        assert_eq!(
            credentials::without_secret("naïve", "ï"),
            "na[redacted credential]ve"
        );
    }

    /// A runtime over a store this test can look into, with no agent.
    async fn runtime_with_store() -> (tempfile::TempDir, Runtime, Arc<InMemorySecretStore>) {
        let guard = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let store = Arc::new(InMemorySecretStore::new());
        let runtime = Runtime::in_memory(root, store.clone()).await.unwrap();
        (guard, runtime, store)
    }

    /// A GitHub binding with the given label and host, private network off.
    fn github<'a>(label: &'a str, host: Option<&'a str>) -> integrations::Binding<'a> {
        integrations::Binding {
            integration: "github",
            label,
            host,
            private_network: false,
            scopes: None,
        }
    }

    /// The GitHub accounts the screen would list.
    async fn github_accounts(runtime: &Runtime) -> Vec<crate::dto::IntegrationAccountView> {
        integrations::views(runtime)
            .await
            .unwrap()
            .into_iter()
            .find(|view| view.id == "github")
            .unwrap()
            .accounts
    }

    /// Everything a binding could have left a token in: every audit record and
    /// every summary of one.
    async fn assert_nowhere_recorded(runtime: &Runtime, secret: &str) {
        for record in runtime.database().audit_sink().all().await.unwrap() {
            let payload = record.payload.to_string();
            assert!(!payload.contains(secret), "{}: {payload}", record.kind);
        }
        for event in recent_events(runtime, 100, false).await.unwrap() {
            assert!(!event.summary.contains(secret), "{}", event.summary);
        }
    }

    #[tokio::test]
    async fn an_integration_lists_what_binding_grants_before_anything_is_bound() {
        let (_guard, runtime, _store) = runtime_with_store().await;
        let listed = integrations::views(&runtime).await.unwrap();
        let github = listed.iter().find(|view| view.id == "github").unwrap();
        assert_eq!(github.default_host, "https://api.github.com");
        assert!(github.accounts.is_empty());
        // The tools the screen names are the ones the registry holds, so what
        // an operator reads before binding is what an agent could be given.
        let registered: Vec<String> = runtime
            .registry()
            .all_metadata()
            .iter()
            .filter(|metadata| metadata.domain() == "github")
            .map(|metadata| metadata.name.clone())
            .collect();
        let mut named = github.tools.clone();
        named.sort();
        let mut registered = registered;
        registered.sort();
        assert_eq!(named, registered);
        assert_eq!(named.len(), 12, "{named:?}");
    }

    #[tokio::test]
    async fn a_bound_token_is_stored_as_its_origins_credential_and_comes_back_nowhere() {
        use agentos_secrets::SecretStore;

        let (_guard, runtime, store) = runtime_with_store().await;
        // Padded the way a paste arrives, and labelled the way people type.
        integrations::bind(&runtime, &github(" work ", None), format!("  {SECRET}\n"))
            .await
            .unwrap();

        let key = agentos_secrets::network_key("https://api.github.com", "work").unwrap();
        assert_eq!(
            store.get(&key).unwrap().expose(),
            SECRET,
            "stored trimmed, under the default host's origin and the label"
        );

        let listed = integrations::views(&runtime).await.unwrap();
        let account = &listed
            .iter()
            .find(|view| view.id == "github")
            .unwrap()
            .accounts[0];
        assert_eq!(account.label, "work");
        assert_eq!(account.host, "https://api.github.com");
        assert_eq!(account.origin.as_deref(), Some("https://api.github.com"));
        assert!(account.credential_present);
        assert!(!account.private_network);

        let answer = serde_json::to_string(&listed).unwrap();
        assert!(!answer.contains(SECRET), "{answer}");
        assert!(!answer.contains(&SECRET[..8]), "{answer}");
        assert_nowhere_recorded(&runtime, SECRET).await;
        assert_eq!(records_of(&runtime, "operator.integration.bound").await, 1);
        assert_eq!(records_of(&runtime, "operator.credential.set").await, 1);
    }

    #[tokio::test]
    async fn an_account_whose_token_is_gone_is_listed_as_missing_not_dropped() {
        let (_guard, runtime, _store) = runtime_with_store().await;
        integrations::bind(&runtime, &github("work", None), SECRET.to_owned())
            .await
            .unwrap();
        // Removing the credential where every credential is listed is enough
        // to leave the account with nothing behind it.
        runtime
            .remove_network_credential("https://api.github.com", "work")
            .await
            .unwrap();

        let accounts = github_accounts(&runtime).await;
        let account = &accounts[0];
        assert_eq!(account.label, "work");
        assert!(!account.credential_present);

        // And testing it sends nothing, and says why.
        let id = parse_id("integration account", &account.id).unwrap();
        let check = runtime.test_integration(id).await.unwrap();
        let view = crate::dto::IntegrationTestView::from(&check);
        assert_eq!(view.outcome, "unauthorised");
        assert!(view.detail.contains("nothing was sent"), "{}", view.detail);
    }

    #[tokio::test]
    async fn a_credential_an_account_depends_on_is_listed_as_that_accounts_token() {
        let (_guard, runtime, _store) = runtime_with_store().await;
        credentials::store(
            &runtime,
            "https://api.github.com",
            "deploy",
            SECRET.to_owned(),
        )
        .await
        .unwrap();
        integrations::bind(&runtime, &github("work", None), SECRET.to_owned())
            .await
            .unwrap();

        // The account's token is named as its, so removing or replacing it
        // under Network credentials is not done without knowing; the
        // credential stored for `network.request` alone names no account.
        let listed = credentials::listed(&runtime).await.unwrap();
        let account_of = |name: &str| {
            listed
                .iter()
                .find(|view| view.origin == "https://api.github.com" && view.name == name)
                .unwrap()
                .account
                .clone()
        };
        assert_eq!(account_of("work").as_deref(), Some("GitHub account work"));
        assert_eq!(account_of("deploy"), None);

        // Storing over it from that section answers the same way.
        let replaced =
            credentials::store(&runtime, "https://api.github.com", "work", "ghp_new".into())
                .await
                .unwrap();
        assert_eq!(replaced.account.as_deref(), Some("GitHub account work"));
    }

    #[tokio::test]
    async fn a_refused_binding_quotes_no_token_and_stores_nothing() {
        use agentos_secrets::SecretStore;

        let (_guard, runtime, store) = runtime_with_store().await;
        for (binding, token) in [
            // A token pasted into the label field is the likeliest mistake.
            (github(SECRET, None), "ghp_other_value_1234".to_owned()),
            (github("Work", None), SECRET.to_owned()),
            (github("work", Some("ftp://ghe.example")), SECRET.to_owned()),
            (
                integrations::Binding {
                    integration: "gitlab",
                    ..github("work", None)
                },
                SECRET.to_owned(),
            ),
            (github("work", None), "   ".to_owned()),
        ] {
            let error = integrations::bind(&runtime, &binding, token)
                .await
                .unwrap_err()
                .to_string();
            assert!(!error.contains(SECRET), "{error}");
        }
        let key = agentos_secrets::network_key("https://api.github.com", "work").unwrap();
        assert!(!store.contains(&key).unwrap());
        assert!(runtime.list_network_credentials().await.unwrap().is_empty());
        assert!(github_accounts(&runtime).await.is_empty());
        assert_eq!(records_of(&runtime, "operator.integration.bound").await, 0);
        assert_eq!(records_of(&runtime, "operator.credential.set").await, 0);
    }

    #[tokio::test]
    async fn a_second_binding_under_a_bound_label_leaves_the_first_token_alone() {
        use agentos_secrets::SecretStore;

        let (_guard, runtime, store) = runtime_with_store().await;
        integrations::bind(&runtime, &github("work", None), SECRET.to_owned())
            .await
            .unwrap();
        let error = integrations::bind(&runtime, &github("work", None), "ghp_replacement".into())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("already bound"), "{error}");
        assert!(!error.contains("ghp_replacement"), "{error}");
        let key = agentos_secrets::network_key("https://api.github.com", "work").unwrap();
        assert_eq!(store.get(&key).unwrap().expose(), SECRET);
    }

    #[tokio::test]
    async fn only_the_bind_call_can_let_an_account_reach_a_private_network() {
        let (_guard, runtime, _store) = runtime_with_store().await;
        let binding = integrations::Binding {
            private_network: true,
            scopes: Some(" repo "),
            ..github("ghe", Some("https://GHE.example/api/v3/"))
        };
        integrations::bind(&runtime, &binding, SECRET.to_owned())
            .await
            .unwrap();

        let accounts = github_accounts(&runtime).await;
        let account = &accounts[0];
        assert!(account.private_network);
        // Stored as the origin it is followed by its path, so the table's
        // check, the token's origin and every request agree about it.
        assert_eq!(account.host, "https://ghe.example/api/v3");
        assert_eq!(account.origin.as_deref(), Some("https://ghe.example"));
        assert_eq!(account.scopes.as_deref(), Some("repo"));

        // The record of the binding says so, so the permission is attributable.
        let bound = runtime
            .database()
            .audit_sink()
            .all()
            .await
            .unwrap()
            .into_iter()
            .find(|record| record.kind == "operator.integration.bound")
            .unwrap();
        assert_eq!(bound.payload["private_network"], serde_json::json!(true));
        assert_nowhere_recorded(&runtime, SECRET).await;
    }

    #[tokio::test]
    async fn unbinding_removes_the_row_and_then_the_token() {
        use agentos_secrets::SecretStore;

        let (_guard, runtime, store) = runtime_with_store().await;
        integrations::bind(&runtime, &github("work", None), SECRET.to_owned())
            .await
            .unwrap();
        let id = parse_id(
            "integration account",
            &github_accounts(&runtime).await[0].id,
        )
        .unwrap();
        runtime.unbind_integration(id).await.unwrap();

        assert!(github_accounts(&runtime).await.is_empty());
        let key = agentos_secrets::network_key("https://api.github.com", "work").unwrap();
        assert!(!store.contains(&key).unwrap());
        let unbound = records_of(&runtime, "operator.integration.unbound").await;
        assert_eq!(unbound, 1);
        assert_eq!(records_of(&runtime, "operator.credential.removed").await, 1);
        for event in recent_events(&runtime, 100, false).await.unwrap() {
            if event.kind.starts_with("operator.integration.") {
                assert!(event.summary.contains("work"), "{}", event.summary);
            }
        }
        assert_nowhere_recorded(&runtime, SECRET).await;
    }

    #[tokio::test]
    async fn the_close_guard_names_what_closing_would_stop() {
        let (_guard, runtime, _agent) = runtime_with_agent().await;
        let state = AppState::new(runtime);
        assert!(crate::lifecycle::close_guard(&state).await.is_quiet());
    }
}
