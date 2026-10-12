//! Listing, inspecting, creating and enabling agents.

use agentos_core::agent::ModelConfig;
use agentos_runtime::Runtime;
use tauri::State;

use super::{Answer, DesktopError, parse_id, policy_view};
use crate::dto::{AgentDetail, AgentSummary, CreateAgentInput, ToolGrantView, task_summary};
use crate::state::AppState;

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

/// For each tool an agent has been given, how far its policy lets each of the
/// tool's capabilities reach.
///
/// Read by the permission engine from the compiled policy, so the agent editor
/// can show what a grant actually permits without interpreting rule text.
#[tauri::command]
pub async fn grant_report(
    state: State<'_, AppState>,
    agent_id: String,
) -> Answer<Vec<ToolGrantView>> {
    let id = parse_id("agent", &agent_id)?;
    Ok(state
        .runtime
        .grant_report(id)
        .await?
        .iter()
        .map(ToolGrantView::from)
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
///
/// Through the runtime, which records the creation and the starter policy in
/// the audit chain.
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
///
/// Through the runtime, which records the change in the audit chain.
#[tauri::command]
pub async fn set_agent_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Answer<AgentSummary> {
    enable_agent(&state.runtime, &name, enabled).await
}

/// Enable or disable the agent with this name.
pub(crate) async fn enable_agent(
    runtime: &Runtime,
    name: &str,
    enabled: bool,
) -> Answer<AgentSummary> {
    let agent = runtime.agent_by_name(name).await?;
    let agent = runtime.set_agent_enabled(agent.id, enabled).await?;
    Ok(AgentSummary::from(&agent))
}
