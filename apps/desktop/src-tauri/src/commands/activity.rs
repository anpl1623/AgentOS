//! The activity feed.

use tauri::State;

use super::{Answer, recent_events};
use crate::dto::EventView;
use crate::state::AppState;

/// Recent audit events.
///
/// `security_only` filters on the runtime's side, so a feed of refusals and
/// escalations is the most recent of those, not whichever of them happen to
/// sit among the most recent records of every kind.
#[tauri::command]
pub async fn activity(
    state: State<'_, AppState>,
    limit: Option<i64>,
    security_only: Option<bool>,
) -> Answer<Vec<EventView>> {
    recent_events(
        &state.runtime,
        limit.unwrap_or(200),
        security_only.unwrap_or(false),
    )
    .await
}
