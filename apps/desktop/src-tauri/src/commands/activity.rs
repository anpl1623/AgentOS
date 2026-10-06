//! The activity feed and audit verification.

use tauri::State;

use super::{Answer, recent_events};
use crate::dto::EventView;
use crate::state::AppState;

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
