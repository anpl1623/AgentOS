//! Listing and answering approval requests.

use agentos_core::ids::ApprovalId;
use agentos_tools::ApprovalOutcome;
use tauri::State;

use super::{Answer, approval_views, parse_id};
use crate::dto::{ApprovalDecisionInput, ApprovalView};
use crate::state::AppState;

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
