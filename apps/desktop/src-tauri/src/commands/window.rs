//! The window's own lifecycle.

use tauri::{AppHandle, State};

use super::Answer;
use crate::lifecycle::close_main_window;
use crate::state::AppState;

/// Close the main window, after the operator has been told what that stops.
///
/// The close guard held the first request and asked; this is the answer
/// "close anyway". The exit hook does the stopping it described.
#[tauri::command]
pub async fn confirm_close(app: AppHandle, state: State<'_, AppState>) -> Answer<()> {
    close_main_window(&app, &state);
    Ok(())
}

/// Say that the interface has received a held close and is asking.
///
/// Sent as the question arrives. A close nobody acknowledges is not held,
/// so an interface that has crashed or not loaded cannot leave a window that
/// will not close.
#[tauri::command]
pub async fn acknowledge_close(state: State<'_, AppState>) -> Answer<()> {
    state.close_acknowledged.notify_waiters();
    Ok(())
}
