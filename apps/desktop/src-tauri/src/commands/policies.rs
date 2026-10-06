//! Checking and installing policy documents.

use agentos_core::ids::AgentId;
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
