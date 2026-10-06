//! The AgentOS desktop application.
//!
//! One of two clients of `agentos-runtime`, the other being the CLI. Both talk
//! to the same runtime; neither reimplements any part of it. What lives here is
//! window setup, a command surface, two bridges — approvals out to a human and
//! audit events out to the activity feed — and the supervision of the
//! scheduler, which runs inside the application so that a schedule fires
//! without a terminal left open.
//!
//! If you are looking for how an agent decides what to do, or what it is
//! allowed to do, it is not in this crate. See `agentos-runtime` and
//! `agentos-permissions`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod commands;
pub mod dto;
pub mod lifecycle;
pub mod state;

use std::sync::Arc;

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

    let app = tauri::Builder::default()
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
            let state = AppState::new(runtime);
            // After the reap, so a scheduler started now cannot have a run of
            // its own mistaken for one the last process abandoned.
            state::resume_scheduler(Arc::clone(&state.scheduler));
            app.manage(state);
            Ok(())
        })
        .on_window_event(lifecycle::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::dashboard,
            commands::list_agents,
            commands::get_agent,
            commands::create_agent,
            commands::set_agent_enabled,
            commands::grant_report,
            commands::check_policy,
            commands::set_policy,
            commands::list_tasks,
            commands::start_task,
            commands::cancel_run,
            commands::get_trace,
            commands::get_task_trace,
            commands::list_runs,
            commands::retry_task,
            commands::create_task,
            commands::add_task_dependency,
            commands::task_graph,
            commands::scheduler_status,
            commands::set_scheduler_running,
            commands::list_schedules,
            commands::create_schedule,
            commands::set_schedule_paused,
            commands::delete_schedule,
            commands::check_cadence,
            commands::list_memories,
            commands::remember,
            commands::revise_memory,
            commands::forget_memory,
            commands::list_pending_approvals,
            commands::list_recent_approvals,
            commands::resolve_approval,
            commands::activity,
            commands::verify_audit,
            commands::audit_health,
            commands::audit_record,
            commands::audit_for_run,
            commands::tool_usage,
            commands::list_tools,
            commands::settings,
            commands::set_provider_key,
            commands::remove_provider_key,
            commands::acknowledge_close,
            commands::confirm_close,
        ])
        .build(tauri::generate_context!())
        .expect("could not start the AgentOS window");

    // Built and run in two steps so the exit hook can be given to `run`: the
    // scheduler and every live run are stopped, and recorded as stopped, before
    // the process ends, rather than left for the next launch to reap.
    app.run(|handle, event| lifecycle::on_run_event(handle, &event));
}

#[cfg(test)]
mod version_tests {
    /// The version appears in three files, and a release that ships three
    /// different numbers is a support burden long outliving the minute this
    /// costs. The release workflow repeats the check against the tag; this one
    /// catches the mismatch at the commit that introduces it.
    #[test]
    fn the_three_manifests_agree_on_the_version() {
        let crate_version = env!("CARGO_PKG_VERSION");

        let tauri_conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json"))
                .expect("tauri.conf.json is valid JSON");
        let package_json: serde_json::Value =
            serde_json::from_str(include_str!("../../package.json"))
                .expect("package.json is valid JSON");

        assert_eq!(
            tauri_conf["version"].as_str(),
            Some(crate_version),
            "tauri.conf.json disagrees with the workspace version"
        );
        assert_eq!(
            package_json["version"].as_str(),
            Some(crate_version),
            "apps/desktop/package.json disagrees with the workspace version"
        );
    }

    /// A release is cut from a tag, and the workflow refuses to publish one
    /// whose version has no section here. Checking that the current version is
    /// written down before the tag exists is what makes that refusal cheap.
    #[test]
    fn the_changelog_has_a_section_for_this_version() {
        let changelog = include_str!("../../../../CHANGELOG.md");
        let heading = format!("## [{}]", env!("CARGO_PKG_VERSION"));
        assert!(
            changelog.contains(&heading),
            "CHANGELOG.md has no `{heading}` section"
        );
    }
}
