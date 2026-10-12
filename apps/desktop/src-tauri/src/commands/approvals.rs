//! Listing and answering approval requests.
//!
//! Each request is answered on its own. There is no bulk approval, no
//! select-all and no "always allow" here, deliberately: a person approving
//! twenty cards with one click has read none of them, and widening what an
//! agent may do without asking is an edit to its policy in Settings, which is
//! meant to be slower than clicking a card.

use agentos_core::approval::{MAX_DECISION_NOTE_CHARS, clean_decision_note};
use agentos_core::ids::ApprovalId;
use agentos_tools::ApprovalOutcome;
use tauri::State;

use super::{Answer, DesktopError, approval_views, parse_id};
use crate::dto::{ApprovalDecisionInput, ApprovalView};
use crate::state::AppState;

/// Approvals waiting on a human.
#[tauri::command]
pub async fn list_pending_approvals(state: State<'_, AppState>) -> Answer<Vec<ApprovalView>> {
    let pending = state.runtime.database().approvals().list_pending().await?;
    approval_views(&state.runtime, pending).await
}

/// Approvals already answered, most recently decided first.
#[tauri::command]
pub async fn list_recent_approvals(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Answer<Vec<ApprovalView>> {
    let recent = state
        .runtime
        .database()
        .approvals()
        .list_recent(limit.unwrap_or(20))
        .await?;
    approval_views(&state.runtime, recent).await
}

/// Answer an approval request.
///
/// Returns `false` when nothing was waiting on that request any more.
#[tauri::command]
pub async fn resolve_approval(
    state: State<'_, AppState>,
    input: ApprovalDecisionInput,
) -> Answer<bool> {
    let (id, outcome) = decision(input)?;
    Ok(state.approvals.resolve(id, outcome).await)
}

/// Turn a person's answer into the outcome the waiting run receives.
///
/// The note travels with an approval as well as a denial, and a note that is
/// only whitespace is no note. A note over the limit is refused rather than
/// cut, so the person learns it before the answer is sealed into the audit
/// chain; control characters are removed, as the run's gate removes them for
/// every client.
pub(crate) fn decision(input: ApprovalDecisionInput) -> Answer<(ApprovalId, ApprovalOutcome)> {
    let id: ApprovalId = parse_id("approval", &input.approval_id)?;
    if input
        .note
        .as_ref()
        .is_some_and(|note| note.trim().chars().count() > MAX_DECISION_NOTE_CHARS)
    {
        return Err(DesktopError::Rejected(format!(
            "a note can be at most {MAX_DECISION_NOTE_CHARS} characters"
        )));
    }
    let note = input.note.as_deref().and_then(clean_decision_note);
    let outcome = if input.approved {
        ApprovalOutcome::Approved { note }
    } else {
        ApprovalOutcome::Denied { note }
    };
    Ok((id, outcome))
}
