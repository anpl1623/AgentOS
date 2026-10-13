//! Delegation edges, read from either end, and the tree beneath a run.

use std::collections::HashMap;

use agentos_core::delegation::Delegation;
use agentos_core::ids::{AgentId, TaskRunId};
use agentos_runtime::{DelegationNode, Runtime};
use tauri::State;

use super::{Answer, agent_names, parse_id};
use crate::dto::{DelegationTreeView, DelegationView};
use crate::state::AppState;

/// A run and every run delegated beneath it.
///
/// The runtime bounds the walk by the configured depth, so a corrupt edge
/// cannot make this recurse without end.
#[tauri::command]
pub async fn delegation_tree(
    state: State<'_, AppState>,
    run_id: String,
) -> Answer<DelegationTreeView> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    let node = state.runtime.delegation_tree(id).await?;
    Ok(tree_view(&node))
}

/// The view of one node, and of everything under it.
pub(crate) fn tree_view(node: &DelegationNode) -> DelegationTreeView {
    DelegationTreeView {
        run_id: node.run.id.to_string(),
        agent: node.agent_name.clone(),
        objective: node.objective.clone(),
        state: node.run.state.as_str().to_owned(),
        tainted: node.run.tainted,
        steps: node.run.steps_taken,
        children: node.children.iter().map(tree_view).collect(),
    }
}

/// The two ends of a run's delegations: the work it handed out, and the
/// delegation that started it.
///
/// Every agent's name is read in one query, and only when there is an edge to
/// name: most runs delegate nothing and were started by a person.
pub(crate) async fn trace_delegations(
    runtime: &Runtime,
    delegations: &[Delegation],
    delegated_by: Option<&Delegation>,
) -> Answer<(Vec<DelegationView>, Option<DelegationView>)> {
    if delegations.is_empty() && delegated_by.is_none() {
        return Ok((Vec::new(), None));
    }
    let names = agent_names(runtime).await?;
    let mut views = Vec::with_capacity(delegations.len());
    for delegation in delegations {
        views.push(delegation_view(runtime, delegation, &names).await?);
    }
    let parent = match delegated_by {
        Some(delegation) => Some(delegation_view(runtime, delegation, &names).await?),
        None => None,
    };
    Ok((views, parent))
}

/// One delegation as the interface shows it.
///
/// Steps and taint are read from the child run rather than from the row. The
/// row's count is written when the delegation ends, so for a child still
/// working it would say nothing had happened; and taint is the run's to
/// report, the delegation having no taint of its own.
///
/// The state is the row's, except where the row still says the work is under
/// way and the child run has ended. That is a delegation whose process died
/// before it could record the ending; the run has since been reaped, and its
/// ending is the truth the row never caught up with. A run's terminal states
/// are spelled as a delegation's are, so the vocabulary stays one.
async fn delegation_view(
    runtime: &Runtime,
    delegation: &Delegation,
    names: &HashMap<AgentId, String>,
) -> Answer<DelegationView> {
    let name = |id: &AgentId| {
        names
            .get(id)
            .cloned()
            .unwrap_or_else(|| "(deleted)".to_owned())
    };
    let child_run = match delegation.child_run_id {
        Some(id) => runtime.database().runs().find(id).await?,
        None => None,
    };
    let duration_ms = delegation.completed_at.map(|completed| {
        u64::try_from((completed - delegation.requested_at).num_milliseconds()).unwrap_or(0)
    });
    Ok(DelegationView {
        id: delegation.id.to_string(),
        parent_run_id: delegation.parent_run_id.to_string(),
        parent_agent: name(&delegation.parent_agent_id),
        child_agent: name(&delegation.child_agent_id),
        objective: delegation.objective.clone(),
        state: match &child_run {
            Some(run) if !delegation.state.is_terminal() && run.state.is_terminal() => {
                run.state.as_str().to_owned()
            }
            _ => delegation.state.as_str().to_owned(),
        },
        child_run_id: delegation.child_run_id.map(|id| id.to_string()),
        steps: child_run
            .as_ref()
            .map_or(delegation.steps_spent, |run| run.steps_taken),
        tainted: child_run.as_ref().is_some_and(|run| run.tainted),
        depth: delegation.depth,
        requested_at: agentos_core::format_timestamp(&delegation.requested_at),
        duration_ms,
    })
}
