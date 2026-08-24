//! The AgentOS desktop application.
//!
//! One of two clients of `agentos-runtime`, the other being the CLI. Both talk
//! to the same runtime; neither reimplements any part of it. What lives here is
//! window setup, a command surface, and two bridges — approvals out to a human
//! and audit events out to the activity feed.
//!
//! If you are looking for how an agent decides what to do, or what it is
//! allowed to do, it is not in this crate. See `agentos-runtime` and
//! `agentos-permissions`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod commands;
pub mod dto;
pub mod state;

use agentos_runtime::{Runtime, RuntimeConfig};
use tauri::Manager;

use crate::state::AppState;

/// Start the application.
///
/// # Panics
///
/// Panics if the runtime cannot be opened or the window cannot be created.
/// There is nothing useful to do in either case — an application that cannot
/// reach its own database should say so and stop, not run in a degraded state
/// where an operator might believe their policies are being enforced.
#[allow(
    clippy::expect_used,
    reason = "startup failures are fatal and must be loud"
)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,agentos=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let config =
                RuntimeConfig::discover().expect("could not determine where to store data");
            let runtime = tauri::async_runtime::block_on(Runtime::open(config))
                .expect("could not open the AgentOS database");

            // A run that was executing when the application last closed is not
            // executing now. Leaving it marked live would misreport the state of
            // the system indefinitely.
            match tauri::async_runtime::block_on(runtime.reap_abandoned_runs()) {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "marked abandoned runs as failed"),
                Err(error) => tracing::error!(%error, "could not reap abandoned runs"),
            }

            state::stream_activity(app.handle().clone(), &runtime);
            app.manage(AppState::new(runtime));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::dashboard,
            commands::list_agents,
            commands::get_agent,
            commands::create_agent,
            commands::set_agent_enabled,
            commands::check_policy,
            commands::set_policy,
            commands::list_tasks,
            commands::start_task,
            commands::cancel_run,
            commands::get_trace,
            commands::get_task_trace,
            commands::list_pending_approvals,
            commands::resolve_approval,
            commands::activity,
            commands::verify_audit,
            commands::list_tools,
            commands::settings,
            commands::set_provider_key,
            commands::remove_provider_key,
        ])
        .run(tauri::generate_context!())
        .expect("could not start the AgentOS window");
}
