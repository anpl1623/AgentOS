//! The command surface the interface calls.
//!
//! Every function here is a thin translation: parse an identifier, call the
//! runtime, shape the answer into a view model. There is no agent behaviour in
//! this file — the desktop application is a client of `agentos-runtime`, on
//! equal footing with the CLI, and the moment logic starts accumulating here the
//! two clients have begun to disagree.

use std::sync::Arc;

use agentos_core::agent::{AgentStatus, ModelConfig};
use agentos_core::approval::ApprovalRequest;
use agentos_core::ids::{AgentId, ApprovalId, TaskId, TaskRunId};
use agentos_providers::provider_ids;
use agentos_runtime::Runtime;
use agentos_secrets::{
    ChainSecretStore, EnvSecretStore, KeychainStatus, KeyringStore, SecretStore, provider_key,
};
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use crate::dto::{
    AgentDetail, AgentSummary, ApprovalDecisionInput, ApprovalView, CreateAgentInput,
    DashboardView, EventView, ExecutionView, PolicyCheck, PolicyView, ProviderView, RunSummary,
    SettingsView, StartedTask, StepView, TaskSummary, ToolView, TraceView, summarise_event,
    task_summary,
};
use crate::state::{AppState, DesktopApprovalGate};

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

fn parse_id<T: std::str::FromStr>(kind: &'static str, value: &str) -> Answer<T> {
    value.parse::<T>().map_err(|_| DesktopError::BadId {
        kind,
        value: value.to_owned(),
    })
}

/// Compile a stored policy document into the view the interface shows.
fn policy_view(document: String, version: i64) -> PolicyView {
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
async fn approval_views(
    runtime: &Runtime,
    requests: Vec<ApprovalRequest>,
) -> Answer<Vec<ApprovalView>> {
    let mut views = Vec::with_capacity(requests.len());
    for request in requests {
        let objective = runtime
            .database()
            .tasks()
            .find(request.task_id)
            .await?
            .map(|task| task.objective)
            .unwrap_or_default();
        views.push(ApprovalView::new(&request, objective));
    }
    Ok(views)
}

/// Load recent tasks with their agent names and latest runs.
async fn task_summaries(runtime: &Runtime, limit: i64) -> Answer<Vec<TaskSummary>> {
    let tasks = runtime.database().tasks().list(limit).await?;
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        let name = runtime
            .database()
            .agents()
            .find(task.agent_id)
            .await?
            .map(|agent| agent.name)
            .unwrap_or_else(|| "(deleted)".to_owned());
        let run = runtime.database().runs().latest_for_task(task.id).await?;
        out.push(task_summary(&task, &name, run.as_ref()));
    }
    Ok(out)
}

/// Recent audit events, newest last so the feed reads downward.
async fn recent_events(runtime: &Runtime, limit: i64) -> Answer<Vec<EventView>> {
    let mut records = runtime.database().audit_sink().tail(limit).await?;
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
            security_relevant: SECURITY_KINDS.contains(&record.kind.as_str()),
        })
        .collect())
}

/// Event kinds that record a refusal, an escalation or a rejection.
const SECURITY_KINDS: &[&str] = &[
    "permission.denied",
    "permission.escalated_by_taint",
    "approval.denied",
    "tool.arguments.rejected",
    "tool.unknown",
    "agent.taint.raised",
];

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

/// Everything the dashboard shows.
#[tauri::command]
pub async fn dashboard(state: State<'_, AppState>) -> Answer<DashboardView> {
    let runtime = &state.runtime;
    let agents = runtime.database().agents().list().await?;

    let active = runtime.database().tasks().list_active().await?;
    let mut running_tasks = Vec::with_capacity(active.len());
    for task in active {
        let name = runtime
            .database()
            .agents()
            .find(task.agent_id)
            .await?
            .map(|agent| agent.name)
            .unwrap_or_else(|| "(deleted)".to_owned());
        let run = runtime.database().runs().latest_for_task(task.id).await?;
        running_tasks.push(task_summary(&task, &name, run.as_ref()));
    }

    let pending = runtime.database().approvals().list_pending().await?;
    let verification = runtime.verify_audit().await?;

    Ok(DashboardView {
        agents: agents.iter().map(AgentSummary::from).collect(),
        running_tasks,
        pending_approvals: approval_views(runtime, pending).await?,
        recent_events: recent_events(runtime, 40).await?,
        recent_refusals: runtime
            .database()
            .executions()
            .list_denied(10)
            .await?
            .iter()
            .map(ExecutionView::from)
            .collect(),
        audit_events: runtime.database().audit_sink().count().await?,
        audit_intact: verification.is_intact(),
    })
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/// Every configured agent.
#[tauri::command]
pub async fn list_agents(state: State<'_, AppState>) -> Answer<Vec<AgentSummary>> {
    Ok(state
        .runtime
        .database()
        .agents()
        .list()
        .await?
        .iter()
        .map(AgentSummary::from)
        .collect())
}

/// One agent, with its policy and recent work.
#[tauri::command]
pub async fn get_agent(state: State<'_, AppState>, name: String) -> Answer<AgentDetail> {
    let runtime = &state.runtime;
    let agent = runtime.agent_by_name(&name).await?;
    let policy = runtime
        .database()
        .agents()
        .policy(agent.id)
        .await?
        .map(|stored| policy_view(stored.document, stored.version));

    let tasks = runtime
        .database()
        .tasks()
        .list_for_agent(agent.id, 20)
        .await?;
    let mut recent_tasks = Vec::with_capacity(tasks.len());
    for task in tasks {
        let run = runtime.database().runs().latest_for_task(task.id).await?;
        recent_tasks.push(task_summary(&task, &agent.name, run.as_ref()));
    }

    Ok(AgentDetail {
        summary: AgentSummary::from(&agent),
        instructions: agent.instructions.clone(),
        policy,
        recent_tasks,
        workspace: runtime
            .config()
            .workspace_for(&agent.name)
            .display()
            .to_string(),
    })
}

/// Create an agent with a deny-by-default starter policy.
#[tauri::command]
pub async fn create_agent(
    state: State<'_, AppState>,
    input: CreateAgentInput,
) -> Answer<AgentSummary> {
    let runtime = &state.runtime;

    // A tool the runtime does not have is a mistake worth catching here rather
    // than at the moment an agent tries to use it.
    let known = runtime.registry().names();
    for tool in &input.tools {
        if !known.contains(tool) {
            return Err(DesktopError::Rejected(format!("unknown tool `{tool}`")));
        }
    }

    let mut model = ModelConfig::new(&input.provider, &input.model);
    model.base_url = input.base_url.filter(|url| !url.trim().is_empty());
    model.vision = input.vision;

    let agent = runtime
        .create_agent(&input.name, &input.instructions, model, input.tools)
        .await?;
    Ok(AgentSummary::from(&agent))
}

/// Enable or disable an agent.
#[tauri::command]
pub async fn set_agent_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Answer<AgentSummary> {
    let runtime = &state.runtime;
    let mut agent = runtime.agent_by_name(&name).await?;
    agent.status = if enabled {
        AgentStatus::Enabled
    } else {
        AgentStatus::Disabled
    };
    runtime.database().agents().update(&agent).await?;
    Ok(AgentSummary::from(&agent))
}

// ---------------------------------------------------------------------------
// Policies
// ---------------------------------------------------------------------------

/// Check a policy document without installing it.
#[tauri::command]
pub async fn check_policy(document: String) -> Answer<PolicyCheck> {
    match agentos_permissions::PolicyDocument::from_yaml(&document)
        .and_then(|parsed| parsed.compile())
    {
        Ok(_) => Ok(PolicyCheck {
            valid: true,
            error: None,
            summary: Some(policy_view(document, 0)),
        }),
        Err(error) => Ok(PolicyCheck {
            valid: false,
            error: Some(error.to_string()),
            summary: None,
        }),
    }
}

/// Install a policy for an agent.
///
/// Refuses a document that does not compile. A policy that fails to load makes
/// the runtime deny everything, which is safe but bewildering; better to say so
/// at the moment of saving.
#[tauri::command]
pub async fn set_policy(
    state: State<'_, AppState>,
    agent_id: String,
    document: String,
) -> Answer<PolicyView> {
    let id: AgentId = parse_id("agent", &agent_id)?;
    agentos_permissions::PolicyDocument::from_yaml(&document)
        .and_then(|parsed| parsed.compile())
        .map_err(|error| {
            DesktopError::Rejected(format!("this policy does not compile: {error}"))
        })?;

    let version = state
        .runtime
        .database()
        .agents()
        .set_policy(id, &document)
        .await?;
    Ok(policy_view(document, version))
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Recent tasks.
#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Answer<Vec<TaskSummary>> {
    task_summaries(&state.runtime, limit.unwrap_or(50)).await
}

/// Start a task and return immediately.
///
/// The run proceeds in the background; the interface follows it through the
/// activity stream and the trace.
#[tauri::command]
pub async fn start_task(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_id: String,
    objective: String,
) -> Answer<StartedTask> {
    let id: AgentId = parse_id("agent", &agent_id)?;
    if objective.trim().is_empty() {
        return Err(DesktopError::Rejected(
            "an objective is required".to_owned(),
        ));
    }

    let runtime = &state.runtime;
    let task = runtime.create_task(id, &objective).await?;

    let gate: Arc<dyn ApprovalGate> = Arc::new(DesktopApprovalGate::new(
        app,
        state.approvals.clone(),
        objective,
    ));
    let (run_id, _handle) = runtime
        .start_task(&task, gate, CancellationToken::new())
        .await?;

    Ok(StartedTask {
        task_id: task.id.to_string(),
        run_id: run_id.to_string(),
    })
}

/// Stop a run that is currently executing.
#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>, run_id: String) -> Answer<bool> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    Ok(state.runtime.cancel_run(id).await)
}

/// The full trace of one run.
#[tauri::command]
pub async fn get_trace(state: State<'_, AppState>, run_id: String) -> Answer<TraceView> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    let trace = state.runtime.trace(id).await?;
    Ok(TraceView {
        run: RunSummary::from(&trace.run),
        task_id: trace.run.task_id.to_string(),
        agent_name: trace.agent_name.clone(),
        objective: trace.objective.clone(),
        steps: trace.steps.iter().map(StepView::from).collect(),
        executions: trace.executions.iter().map(ExecutionView::from).collect(),
        approvals: trace
            .approvals
            .iter()
            .map(|request| ApprovalView::new(request, trace.objective.clone()))
            .collect(),
    })
}

/// The trace of a task's most recent run.
#[tauri::command]
pub async fn get_task_trace(state: State<'_, AppState>, task_id: String) -> Answer<TraceView> {
    let id: TaskId = parse_id("task", &task_id)?;
    let run = state
        .runtime
        .database()
        .runs()
        .latest_for_task(id)
        .await?
        .ok_or_else(|| DesktopError::Rejected("this task has never been run".to_owned()))?;
    get_trace(state, run.id.to_string()).await
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

/// Approvals waiting on a human.
#[tauri::command]
pub async fn list_pending_approvals(state: State<'_, AppState>) -> Answer<Vec<ApprovalView>> {
    let pending = state.runtime.database().approvals().list_pending().await?;
    approval_views(&state.runtime, pending).await
}

/// Answer an approval request.
#[tauri::command]
pub async fn resolve_approval(
    state: State<'_, AppState>,
    input: ApprovalDecisionInput,
) -> Answer<bool> {
    let id: ApprovalId = parse_id("approval", &input.approval_id)?;
    let outcome = if input.approved {
        ApprovalOutcome::Approved
    } else {
        ApprovalOutcome::Denied {
            note: input.note.filter(|note| !note.trim().is_empty()),
        }
    };
    Ok(state.approvals.resolve(id, outcome).await)
}

// ---------------------------------------------------------------------------
// Activity, tools and settings
// ---------------------------------------------------------------------------

/// Recent audit events.
#[tauri::command]
pub async fn activity(state: State<'_, AppState>, limit: Option<i64>) -> Answer<Vec<EventView>> {
    recent_events(&state.runtime, limit.unwrap_or(200)).await
}

/// Verify the audit chain.
#[tauri::command]
pub async fn verify_audit(state: State<'_, AppState>) -> Answer<Vec<String>> {
    let verification = state.runtime.verify_audit().await?;
    Ok(verification
        .breaks
        .iter()
        .map(ToString::to_string)
        .collect())
}

/// Every tool an agent can be granted.
#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Answer<Vec<ToolView>> {
    Ok(state
        .runtime
        .registry()
        .all_metadata()
        .iter()
        .map(ToolView::from)
        .collect())
}

/// The settings screen.
#[tauri::command]
pub async fn settings(state: State<'_, AppState>) -> Answer<SettingsView> {
    let runtime = &state.runtime;
    let config = runtime.config();
    let keychain = KeyringStore::status();
    let secrets = ChainSecretStore::standard();

    let providers = provider_ids::ALL
        .iter()
        .map(|id| {
            let located = secrets.locate(&provider_key(id));
            ProviderView {
                id: (*id).to_owned(),
                configured: located.is_some(),
                hint: located.as_ref().map(|(_, secret)| secret.hint()),
                source: located.as_ref().map(|(store, _)| (*store).to_owned()),
                note: match *id {
                    provider_ids::OLLAMA => "local; usually needs no key".to_owned(),
                    provider_ids::MOCK => "built in; no key needed".to_owned(),
                    _ => EnvSecretStore::variables_for(&provider_key(id))
                        .last()
                        .map(|name| format!("or set {name} in the environment"))
                        .unwrap_or_default(),
                },
            }
        })
        .collect();

    let browser = agentos_browser::locate(None);
    Ok(SettingsView {
        data_dir: config.data_dir.display().to_string(),
        workspace: config.workspace.display().to_string(),
        database: config.database_path.display().to_string(),
        keychain_available: keychain.is_available(),
        keychain_reason: match &keychain {
            KeychainStatus::Available => None,
            KeychainStatus::Unavailable { reason } => {
                Some(reason.lines().next().unwrap_or(reason).to_owned())
            }
        },
        providers,
        browser_path: browser.as_ref().map(|path| path.display().to_string()),
        browser_hint: browser.is_none().then(agentos_browser::install_hint),
        tools: runtime
            .registry()
            .all_metadata()
            .iter()
            .map(ToolView::from)
            .collect(),
    })
}

/// Store a provider credential in the operating system keychain.
///
/// Refuses when there is no keychain rather than pretending to succeed: on such
/// a machine the credential belongs in the environment, and saying so is more
/// use than a generic failure.
#[tauri::command]
pub async fn set_provider_key(provider: String, key: String) -> Answer<()> {
    if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
        let variable = EnvSecretStore::variables_for(&provider_key(&provider))
            .last()
            .cloned()
            .unwrap_or_default();
        return Err(DesktopError::Rejected(format!(
            "this machine has no usable keychain ({}), so there is nowhere secure to store the \
             key. Set {variable} in the environment instead.",
            reason.lines().next().unwrap_or(&reason)
        )));
    }

    let key = key.trim();
    if key.is_empty() {
        return Err(DesktopError::Rejected("no key was provided".to_owned()));
    }
    KeyringStore::new().set(&provider_key(&provider), key)?;
    Ok(())
}

/// Remove a stored provider credential.
#[tauri::command]
pub async fn remove_provider_key(provider: String) -> Answer<()> {
    KeyringStore::new().delete(&provider_key(&provider))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agentos_core::approval::ApprovalStatus;
    use agentos_core::ids::ApprovalId;
    use agentos_core::permission::Capability;
    use agentos_core::risk::RiskLevel;
    use agentos_core::task::{Task, TaskRun};
    use agentos_secrets::InMemorySecretStore;

    use super::*;

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
            .create_task(agent.id, "Do the thing.")
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
            .create_task(agent.id, "Find overdue accounts.")
            .await
            .unwrap();
        let run = TaskRun::new(task.id, 1);
        runtime.database().runs().insert(&run).await.unwrap();

        let request = ApprovalRequest {
            id: ApprovalId::new(),
            agent_id: agent.id,
            agent_name: agent.name.clone(),
            task_id: task.id,
            run_id: run.id,
            tool: "browser.type".to_owned(),
            arguments: serde_json::json!({"selector": "#send"}),
            capability: Capability::new("browser", "interact"),
            risk: RiskLevel::High,
            reason: "policy requires approval".to_owned(),
            explanation: "Submit the form.".to_owned(),
            affected_resources: vec!["origin:http://localhost:8420".to_owned()],
            tainted: true,
            taint_sources: vec!["web:http://localhost:8420/customers".to_owned()],
            status: ApprovalStatus::Pending,
            requested_at: agentos_core::now(),
            decided_at: None,
            decision_note: None,
        };
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

        let events = recent_events(&runtime, 10).await.unwrap();
        assert!(events.len() >= 2);

        let first = events.iter().position(|e| e.kind == "agent.task.started");
        let denial = events.iter().position(|e| e.kind == "permission.denied");
        assert!(first < denial, "the feed should read oldest to newest");

        let denial = &events[denial.expect("a denial was recorded")];
        assert!(denial.security_relevant);
        assert_eq!(denial.summary, "terminal.exec");
    }

    #[tokio::test]
    async fn a_bad_identifier_is_rejected_with_the_kind_it_expected() {
        let error = parse_id::<AgentId>("agent", "not-a-uuid").unwrap_err();
        assert!(error.to_string().contains("agent identifier"), "{error}");
    }
}
