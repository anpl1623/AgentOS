//! Checking and installing policy documents.

use agentos_core::ids::AgentId;
use agentos_runtime::{Runtime, RuntimeError};
use tauri::State;

use super::{Answer, DesktopError, parse_id, policy_view};
use crate::dto::{PolicyCheck, PolicyView};
use crate::state::AppState;

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
/// Through the runtime, which refuses a document that does not compile and
/// records the new version in the audit chain. A policy that fails to load
/// makes the runtime deny everything, which is safe but bewildering; better to
/// say so at the moment of saving.
#[tauri::command]
pub async fn set_policy(
    state: State<'_, AppState>,
    agent_id: String,
    document: String,
) -> Answer<PolicyView> {
    install_policy(&state.runtime, &agent_id, document).await
}

/// Install a policy for the agent with this identity.
pub(crate) async fn install_policy(
    runtime: &Runtime,
    agent_id: &str,
    document: String,
) -> Answer<PolicyView> {
    let id: AgentId = parse_id("agent", agent_id)?;
    let version = runtime
        .set_policy(id, &document)
        .await
        .map_err(|error| match error {
            RuntimeError::Policy(error) => {
                DesktopError::Rejected(format!("this policy does not compile: {error}"))
            }
            other => DesktopError::Runtime(other),
        })?;
    Ok(policy_view(document, version))
}
