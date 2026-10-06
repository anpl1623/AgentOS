//! Listing, inspecting, creating and enabling agents.

use agentos_core::agent::{AgentStatus, ModelConfig};
use tauri::State;

use super::{Answer, DesktopError, policy_view};
use crate::dto::{AgentDetail, AgentSummary, CreateAgentInput, task_summary};
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
