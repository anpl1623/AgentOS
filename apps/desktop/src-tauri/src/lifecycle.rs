//! Closing the window, and what the process does on its way out.
//!
//! Closing the application stops everything it is driving: runs started from
//! the window, runs the scheduler started, and the scheduler itself. None of
//! that is visible from the window's close button, so a close while anything
//! is live is held and the interface is asked to say what it will stop. Once
//! the process is exiting, however it came to, the exit hook stops that work
//! in an orderly way, so each run is recorded as cancelled by the process that
//! drove it rather than reaped as abandoned by the next one.
//!
//! The guard fails open. A close it holds stays held only while the interface
//! acknowledges the question; a window whose interface has crashed, is
//! reloading or never loaded closes when asked, because a window that cannot
//! be closed is worse than a question that could not be put.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Window, WindowEvent};
use tokio::sync::Notify;

use crate::dto::CloseGuard;
use crate::state::AppState;

/// The label of the window the guard protects.
pub const MAIN_WINDOW: &str = "main";

/// Event emitted when a close is held, carrying a [`CloseGuard`].
pub const CLOSE_REQUESTED: &str = "agentos://close-requested";

/// How long the exit hook waits for cancelled runs to record that they
/// stopped.
///
/// A run notices cancellation at its next await on the model or a tool, which
/// is normally well under a second. One that has not finished by then is left;
/// the next launch reaps it, recording that the process exited during it.
pub const RUN_STOP_GRACE: Duration = Duration::from_secs(10);

/// How long a held close waits for the interface to acknowledge the question
/// before closing anyway.
///
/// The acknowledgement is sent as the event arrives, before the dialog is
/// drawn, so a live interface answers in milliseconds; this only has to
/// outlast a busy moment.
pub const ACKNOWLEDGE_WITHIN: Duration = Duration::from_secs(3);

/// How often the exit hook looks to see whether the cancelled runs have
/// finished.
const RUN_STOP_POLL: Duration = Duration::from_millis(50);

/// What closing now would stop.
pub async fn close_guard(state: &AppState) -> CloseGuard {
    CloseGuard {
        live_runs: count(state.runtime.running_runs().await.len()),
        scheduler_running: state.scheduler.is_running().await,
        pending_approvals: count(state.approvals.waiting_ids().await.len()),
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Hold a close of the main window while anything is live.
///
/// Every close is held at first, because what is live can only be read
/// asynchronously and this callback runs on the event loop, which must not
/// block on the runtime's locks. The check then closes the window itself when
/// nothing would be stopped, asks the interface when something would, and
/// closes anyway when nothing acknowledges the question. A close the operator
/// confirmed passes straight through.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN_WINDOW {
        return;
    }
    let Some(state) = window.try_state::<AppState>() else {
        // Setup did not finish, so nothing can be live.
        return;
    };
    if state.closing.load(Ordering::SeqCst) {
        return;
    }

    api.prevent_close();
    guard_close(window.app_handle().clone());
}

/// Hold a request to quit the application while anything is live.
///
/// Quitting, with Cmd+Q on macOS or from the application menu, never asks the
/// window to close, so without this the dialog that says what quitting stops
/// would be skipped by the most ordinary way of leaving. An exit the process
/// asked for itself carries a code and passes, as does every exit once the
/// window may close.
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        RunEvent::ExitRequested { api, code, .. } => {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if code.is_some() || state.closing.load(Ordering::SeqCst) {
                return;
            }
            api.prevent_exit();
            guard_close(app.clone());
        }
        RunEvent::Exit => on_exit(app),
        _ => {}
    }
}

/// Close the main window if nothing would be stopped, ask the interface if
/// something would, and close anyway if the interface does not acknowledge
/// the question.
fn guard_close(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let guard = close_guard(&state).await;
        if guard.is_quiet() {
            close_main_window(&app, &state);
            return;
        }
        let asked = interface_answered(&state.close_acknowledged, ACKNOWLEDGE_WITHIN, || match app
            .emit(CLOSE_REQUESTED, guard)
        {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "could not ask the interface to confirm closing");
                false
            }
        })
        .await;
        if !asked {
            tracing::warn!("nothing acknowledged the close; closing without asking");
            close_main_window(&app, &state);
        }
    });
}

/// Put a question to the interface with `ask`, and say whether it was
/// acknowledged within `within`.
///
/// The wait is registered before the question is sent, so an answer that
/// arrives at once is not missed, and only answers to this question count:
/// an acknowledgement nobody was waiting for is not kept for the next one.
pub async fn interface_answered(
    acknowledged: &Notify,
    within: Duration,
    ask: impl FnOnce() -> bool,
) -> bool {
    let answered = acknowledged.notified();
    tokio::pin!(answered);
    answered.as_mut().enable();
    if !ask() {
        return false;
    }
    tokio::time::timeout(within, answered).await.is_ok()
}

/// Close the main window without asking again.
///
/// The close goes back through [`on_window_event`], which lets it pass; the
/// exit hook then stops whatever is live. With no main window left to close,
/// the application exits instead, so a held quit is never left holding.
pub fn close_main_window(app: &AppHandle, state: &AppState) {
    state.closing.store(true, Ordering::SeqCst);
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => {
            if let Err(error) = window.close() {
                tracing::error!(%error, "could not close the main window");
            }
        }
        None => app.exit(0),
    }
}

/// Stop everything this process is driving, before it exits.
///
/// The scheduler first, because it is the thing that would start more: it is
/// cancelled, its in-flight runs drained and its task joined, and its stop is
/// recorded. Then every run started from the window is cancelled, and the hook
/// waits, within [`RUN_STOP_GRACE`], for each to record that it stopped.
pub async fn shutdown(state: &AppState) {
    state.closing.store(true, Ordering::SeqCst);
    state.scheduler.stop().await;

    let runtime = &state.runtime;
    let live = runtime.running_runs().await;
    for run in &live {
        runtime.cancel_run(*run).await;
    }

    let deadline = tokio::time::Instant::now() + RUN_STOP_GRACE;
    for run in live {
        loop {
            let ended = match runtime.database().runs().find(run).await {
                Ok(Some(stored)) => stored.state.is_terminal(),
                // Nothing to wait for, or nothing that can be read: either
                // way, waiting longer will not change the answer.
                Ok(None) | Err(_) => true,
            };
            if ended || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(RUN_STOP_POLL).await;
        }
    }
}

/// The exit hook: stop live work before the event loop ends.
///
/// Runs on the main thread after the event loop has finished dispatching, so
/// blocking here holds the process open, not the interface, which is gone.
pub fn on_exit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        tauri::async_runtime::block_on(shutdown(&state));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_close_nobody_acknowledges_is_not_held() {
        let acknowledged = Notify::new();
        let within = Duration::from_millis(50);

        // Nothing listening: the guard gives up and the window closes.
        assert!(!interface_answered(&acknowledged, within, || true).await);
        // The question could not even be sent.
        assert!(!interface_answered(&acknowledged, within, || false).await);

        // An acknowledgement sent with no question outstanding is not kept
        // for the next one, so a stale answer cannot hold a later close.
        acknowledged.notify_waiters();
        assert!(!interface_answered(&acknowledged, within, || true).await);

        // An interface that answers, even before the wait would otherwise
        // have begun, holds the close for the operator to decide.
        assert!(
            interface_answered(&acknowledged, within, || {
                acknowledged.notify_waiters();
                true
            })
            .await
        );
    }

    #[test]
    fn a_close_is_quiet_only_when_nothing_would_be_stopped() {
        assert!(CloseGuard::default().is_quiet());
        for loud in [
            CloseGuard {
                live_runs: 1,
                ..CloseGuard::default()
            },
            CloseGuard {
                scheduler_running: true,
                ..CloseGuard::default()
            },
            CloseGuard {
                pending_approvals: 1,
                ..CloseGuard::default()
            },
        ] {
            assert!(!loud.is_quiet(), "{loud:?}");
        }
    }
}
