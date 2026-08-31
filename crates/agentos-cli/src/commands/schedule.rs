//! `agentos schedule` — standing instructions, and the loop that acts on them.

use std::time::Duration;

use agentos_core::schedule::{Cadence, Clock, ScheduleStatus};
use agentos_runtime::{RuntimeConfig, Scheduler, SchedulerOptions};
use anyhow::{Context, Result, bail};
use clap::Subcommand;

use crate::render::{Style, pad, rule};

/// Schedule subcommands.
#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Create a schedule.
    ///
    /// Exactly one of `--cron`, `--every` or `--once` says how often it fires.
    Create {
        /// Unique name for the schedule.
        name: String,

        /// What the agent is asked to do, each time it fires.
        objective: String,

        /// Which agent. Defaults to the only one, if there is only one.
        #[arg(long)]
        agent: Option<String>,

        /// A cron expression: five fields, or six with leading seconds.
        #[arg(long, group = "cadence")]
        cron: Option<String>,

        /// Read `--cron` against the host's local time rather than UTC.
        #[arg(long, requires = "cron")]
        local: bool,

        /// Fire every N seconds. Minimum 60.
        #[arg(long, group = "cadence")]
        every: Option<u64>,

        /// Fire once and then finish.
        #[arg(long, group = "cadence")]
        once: bool,

        /// When the first firing happens, as RFC 3339. Defaults to now for a
        /// cron or interval schedule, and is required for `--once`.
        #[arg(long)]
        at: Option<String>,
    },

    /// List schedules.
    List,

    /// Stop a schedule firing, without deleting it.
    Pause {
        /// Schedule name.
        name: String,
    },

    /// Start a paused schedule firing again.
    ///
    /// The next occurrence is computed forward from now: a schedule paused over
    /// a weekend does not wake up owing a backlog of runs.
    Resume {
        /// Schedule name.
        name: String,
    },

    /// Delete a schedule. The tasks it already created are kept.
    Delete {
        /// Schedule name.
        name: String,
    },

    /// Run the scheduler in the foreground until stopped.
    ///
    /// Nobody is watching a scheduled run, so every approval it would have asked
    /// for is refused with a note the agent can read and re-plan around. There
    /// is no flag that changes this: an agent that needs a person to say yes
    /// needs a person.
    Run {
        /// Seconds between ticks.
        #[arg(long, default_value_t = 30)]
        tick: u64,

        /// How many runs may be in flight at once.
        #[arg(long, default_value_t = 1)]
        concurrency: usize,

        /// Do a single pass and exit, reporting what it would have done.
        #[arg(long)]
        once: bool,
    },
}

/// Dispatch.
#[allow(clippy::too_many_lines)]
pub async fn run(command: ScheduleCommand, config: &RuntimeConfig) -> Result<()> {
    let runtime = super::open(config).await?;
    let style = Style::detect();

    match command {
        ScheduleCommand::Create {
            name,
            objective,
            agent,
            cron,
            local,
            every,
            once,
            at,
        } => {
            let agent = super::task::resolve_agent(&runtime, agent.as_deref()).await?;

            let cadence = match (cron, every, once) {
                (Some(expression), None, false) => Cadence::Cron {
                    expression,
                    clock: if local { Clock::Local } else { Clock::Utc },
                },
                (None, Some(seconds), false) => Cadence::Every { seconds },
                (None, None, true) => Cadence::Once,
                _ => bail!("choose exactly one of --cron, --every or --once"),
            };

            let first_run_at = match &at {
                Some(text) => parse_time(text)?,
                // A cron or interval schedule computes its own first occurrence;
                // a one-shot with no time would fire immediately, which is
                // almost never what somebody creating a schedule meant.
                None if matches!(cadence, Cadence::Once) => {
                    bail!("--once needs --at to say when")
                }
                None => cadence
                    .next_after(agentos_core::now())
                    .unwrap_or_else(agentos_core::now),
            };

            let schedule = runtime
                .create_schedule(agent.id, &name, &objective, cadence, first_run_at)
                .await
                .with_context(|| format!("creating schedule `{name}`"))?;

            println!("{} {}", style.dim("Schedule"), style.bold(&schedule.name));
            println!("{} {}", style.dim("Agent   "), agent.name);
            println!("{} {}", style.dim("Cadence "), schedule.cadence.describe());
            println!(
                "{} {}",
                style.dim("Next    "),
                schedule
                    .next_run_at
                    .map_or_else(|| "never".to_owned(), |next| next.to_rfc3339())
            );
            println!();
            println!(
                "Nothing fires until a scheduler is running. Start one with {}.",
                style.bold("agentos schedule run")
            );
        }

        ScheduleCommand::List => {
            let schedules = runtime.schedules().await?;
            if schedules.is_empty() {
                println!("No schedules. Create one with `agentos schedule create`.");
                return Ok(());
            }

            println!(
                "{}  {}  {}  NEXT",
                pad("NAME", 20),
                pad("STATUS", 9),
                pad("CADENCE", 34),
            );
            println!("{}", rule());
            for schedule in schedules {
                println!(
                    "{}  {}  {}  {}",
                    pad(&schedule.name, 20),
                    pad(schedule.status.as_str(), 9),
                    pad(&schedule.cadence.describe(), 34),
                    schedule
                        .next_run_at
                        .map_or_else(|| "—".to_owned(), |next| next.to_rfc3339())
                );
            }
        }

        ScheduleCommand::Pause { name } => {
            let schedule = by_name(&runtime, &name).await?;
            runtime.pause_schedule(schedule.id).await?;
            println!("Paused `{name}`. It keeps its next occurrence until resumed.");
        }

        ScheduleCommand::Resume { name } => {
            let schedule = by_name(&runtime, &name).await?;
            runtime.resume_schedule(schedule.id).await?;
            let resumed = runtime.database().schedules().get(schedule.id).await?;
            match (resumed.status, resumed.next_run_at) {
                (ScheduleStatus::Active, Some(next)) => {
                    println!("Resumed `{name}`. Next firing {}.", next.to_rfc3339());
                }
                _ => println!("`{name}` has nothing left to fire."),
            }
        }

        ScheduleCommand::Delete { name } => {
            let schedule = by_name(&runtime, &name).await?;
            runtime.delete_schedule(schedule.id).await?;
            println!("Deleted `{name}`. The tasks it already created are kept.");
        }

        ScheduleCommand::Run {
            tick,
            concurrency,
            once,
        } => {
            let scheduler = Scheduler::new(
                runtime,
                SchedulerOptions::default()
                    .with_tick(Duration::from_secs(tick.max(1)))
                    .with_max_concurrent_runs(concurrency.max(1)),
            );

            if once {
                let report = scheduler.tick().await?;
                scheduler.drain().await;
                println!(
                    "{} fired, {} started, {} abandoned",
                    report.fired.len(),
                    report.started.len(),
                    report.abandoned.len()
                );
                return Ok(());
            }

            println!("{}", style.bold("Scheduler running."));
            println!(
                "Every approval a scheduled run would have asked for is refused: nobody is \
                 watching. Ctrl-C to stop."
            );
            println!("{}", rule());

            // Ctrl-C stops the loop and lets in-flight runs finish, so nothing
            // is left recorded as running by a process that no longer exists.
            let cancel = scheduler.cancellation();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    eprintln!("\nStopping; waiting for runs already in flight…");
                    cancel.cancel();
                }
            });

            scheduler.run().await?;
        }
    }

    Ok(())
}

async fn by_name(
    runtime: &agentos_runtime::Runtime,
    name: &str,
) -> Result<agentos_core::schedule::Schedule> {
    runtime
        .database()
        .schedules()
        .find_by_name(name)
        .await?
        .with_context(|| format!("no schedule named `{name}`"))
}

fn parse_time(text: &str) -> Result<agentos_core::Timestamp> {
    Ok(chrono::DateTime::parse_from_rfc3339(text)
        .with_context(|| format!("`{text}` is not an RFC 3339 time, e.g. 2026-09-01T09:00:00Z"))?
        .with_timezone(&chrono::Utc))
}
