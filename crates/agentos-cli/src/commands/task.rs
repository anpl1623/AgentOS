//! `agentos task` — run and inspect tasks.

use std::sync::Arc;

use agentos_core::ids::{TaskId, TaskRunId};
use agentos_core::risk::RiskLevel;
use agentos_runtime::RuntimeConfig;
use agentos_tools::{ApprovalGate, DenyAllGate};
use anyhow::{Context, Result};
use clap::Subcommand;
use tokio_util::sync::CancellationToken;

use crate::gate::InteractiveGate;
use crate::render::{Style, pad, rule, trace as render_trace};

/// Task subcommands.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Give an agent an objective and watch it work.
    Run {
        /// What to do.
        objective: String,

        /// Which agent. Defaults to the only one, if there is only one.
        #[arg(long)]
        agent: Option<String>,

        /// Approve anything at or below this risk without prompting.
        ///
        /// This cannot widen the policy; it only skips prompts for actions the
        /// policy already routed to approval.
        #[arg(long, value_parser = parse_risk)]
        auto_approve_up_to: Option<RiskLevel>,

        /// Refuse every approval instead of prompting. For unattended runs.
        #[arg(long, conflicts_with = "auto_approve_up_to")]
        unattended: bool,
    },

    /// Create a task without running it, for a scheduler to pick up.
    ///
    /// This is how a task graph is built: create the tasks that go first, then
    /// create the ones that wait for them with `--depends-on`. Nothing starts
    /// until `agentos schedule run` is going.
    Create {
        /// What to do.
        objective: String,

        /// Which agent. Defaults to the only one, if there is only one.
        #[arg(long)]
        agent: Option<String>,

        /// Task this one waits for. Repeatable; every one must succeed first.
        #[arg(long = "depends-on")]
        depends_on: Vec<String>,

        /// Hold the task until this RFC 3339 time.
        #[arg(long, conflicts_with = "depends_on")]
        at: Option<String>,
    },

    /// List recent tasks.
    List {
        /// How many to show.
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },

    /// Show a task's most recent execution trace.
    Show {
        /// Task identifier.
        task: String,
    },

    /// Stop a run that is currently executing.
    Cancel {
        /// Run identifier.
        run: String,
    },
}

fn parse_risk(value: &str) -> Result<RiskLevel, String> {
    value
        .parse::<RiskLevel>()
        .map_err(|error| error.to_string())
}

/// Dispatch.
pub async fn run(command: TaskCommand, config: &RuntimeConfig) -> Result<()> {
    let runtime = super::open(config).await?;
    let style = Style::detect();

    match command {
        TaskCommand::Run {
            objective,
            agent,
            auto_approve_up_to,
            unattended,
        } => {
            let agent = resolve_agent(&runtime, agent.as_deref()).await?;

            let gate: Arc<dyn ApprovalGate> = if unattended {
                Arc::new(DenyAllGate)
            } else {
                match auto_approve_up_to {
                    Some(risk) => Arc::new(InteractiveGate::auto_approving_up_to(risk)),
                    None => Arc::new(InteractiveGate::new()),
                }
            };

            // Ctrl-C cancels the run rather than killing the process, so the
            // agent stops cleanly and the run is recorded as cancelled.
            let cancel = CancellationToken::new();
            let signal_token = cancel.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    eprintln!("\nStopping the agent…");
                    signal_token.cancel();
                }
            });

            println!("{} {}", style.dim("Objective"), style.bold(&objective));
            println!("{} {}", style.dim("Agent    "), agent.name);
            println!("{}", rule());

            let outcome = runtime
                .run_objective(agent.id, &objective, gate, cancel)
                .await?;

            let trace = runtime.trace(outcome.run_id).await?;
            print!("{}", render_trace(&trace, &style, false));

            println!("{}", rule());
            println!(
                "{} {}   {} {} step(s)   {} in / {} out token(s)",
                style.dim("State"),
                style.state(outcome.state),
                style.dim("Took"),
                outcome.steps,
                outcome.input_tokens,
                outcome.output_tokens,
            );
            println!("{} {}", style.dim("Run  "), outcome.run_id);

            if !outcome.succeeded() {
                std::process::exit(1);
            }
        }

        TaskCommand::Create {
            objective,
            agent,
            depends_on,
            at,
        } => {
            let agent = resolve_agent(&runtime, agent.as_deref()).await?;

            let dependencies = depends_on
                .iter()
                .map(|raw| {
                    raw.parse::<TaskId>()
                        .with_context(|| format!("`{raw}` is not a task identifier"))
                })
                .collect::<Result<Vec<_>>>()?;

            let task = match at {
                Some(text) => {
                    let when = chrono::DateTime::parse_from_rfc3339(&text)
                        .with_context(|| {
                            format!("`{text}` is not an RFC 3339 time, e.g. 2026-09-01T09:00:00Z")
                        })?
                        .with_timezone(&chrono::Utc);
                    runtime.create_task_at(agent.id, &objective, when).await?
                }
                None => {
                    runtime
                        .create_task_after(agent.id, &objective, &dependencies)
                        .await?
                }
            };

            println!("{} {}", style.dim("Task     "), task.id);
            println!("{} {}", style.dim("Agent    "), agent.name);
            println!("{} {}", style.dim("Status   "), task.status.as_str());
            if !dependencies.is_empty() {
                println!(
                    "{} {}",
                    style.dim("Waits for"),
                    dependencies
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if let Some(when) = task.scheduled_for {
                println!("{} {}", style.dim("Not until"), when.to_rfc3339());
            }
            println!();
            println!(
                "Nothing starts until a scheduler is running. Start one with {}.",
                style.bold("agentos schedule run")
            );
        }

        TaskCommand::List { limit } => {
            let tasks = runtime.database().tasks().list(limit).await?;
            if tasks.is_empty() {
                println!("{}", style.dim("No tasks yet."));
                return Ok(());
            }
            println!(
                "{}{}{}",
                pad(&style.dim("TASK"), 38),
                pad(&style.dim("STATUS"), 12),
                style.dim("OBJECTIVE")
            );
            for task in tasks {
                println!(
                    "{}{}{}",
                    pad(&task.id.to_string(), 38),
                    pad(task.status.as_str(), 12),
                    truncate(&task.objective, 60)
                );
            }
        }

        TaskCommand::Show { task } => {
            let task_id: TaskId = task.parse().context("task ids are UUIDs")?;
            let task = runtime.task(task_id).await?;
            let run = runtime
                .database()
                .runs()
                .latest_for_task(task.id)
                .await?
                .context("this task has never been run")?;
            let trace = runtime.trace(run.id).await?;
            print!("{}", render_trace(&trace, &style, true));
        }

        TaskCommand::Cancel { run } => {
            let run_id: TaskRunId = run.parse().context("run ids are UUIDs")?;
            if runtime.cancel_run(run_id).await {
                println!("{} cancelling run {run_id}", style.yellow("Signalled"));
            } else {
                println!(
                    "{}",
                    style.dim("That run is not executing in this process.")
                );
            }
        }
    }

    Ok(())
}

/// Resolve which agent to use.
///
/// With exactly one agent configured, naming it every time is friction for no
/// benefit. With several, guessing would be worse than asking.
pub(crate) async fn resolve_agent(
    runtime: &agentos_runtime::Runtime,
    requested: Option<&str>,
) -> Result<agentos_core::agent::Agent> {
    if let Some(name) = requested {
        return Ok(runtime.agent_by_name(name).await?);
    }

    let agents = runtime.database().agents().list().await?;
    match agents.len() {
        0 => anyhow::bail!(
            "no agents exist yet — create one with `agentos agent create --name sales`"
        ),
        1 => Ok(agents.into_iter().next().unwrap_or_else(|| unreachable!())),
        _ => anyhow::bail!(
            "several agents exist; choose one with --agent ({})",
            agents
                .iter()
                .map(|agent| agent.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    format!("{}…", text.chars().take(max).collect::<String>())
}
