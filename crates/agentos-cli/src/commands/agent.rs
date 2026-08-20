//! `agentos agent` — create and inspect agents.

use agentos_core::agent::{AgentStatus, ModelConfig};
use agentos_providers::provider_ids;
use agentos_runtime::RuntimeConfig;
use anyhow::{Context, Result};
use clap::Subcommand;

use crate::render::{Style, pad, rule};

/// Agent subcommands.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Create an agent with a deny-by-default policy.
    Create {
        /// Unique name.
        #[arg(long)]
        name: String,

        /// System instructions for the agent.
        #[arg(
            long,
            default_value = "Complete the operator's objective carefully and report back."
        )]
        instructions: String,

        /// Model provider.
        #[arg(long, default_value = provider_ids::ANTHROPIC, value_parser = clap::builder::PossibleValuesParser::new(provider_ids::ALL))]
        provider: String,

        /// Model identifier as the provider names it.
        #[arg(long, default_value = "claude-opus-5")]
        model: String,

        /// Base URL override, for OpenAI-compatible endpoints.
        #[arg(long)]
        base_url: Option<String>,

        /// Tools to offer the agent. Repeatable. Defaults to the read-only set.
        #[arg(long = "tool")]
        tools: Vec<String>,
    },

    /// List agents.
    List,

    /// Show one agent and its policy.
    Show {
        /// Agent name.
        name: String,
    },

    /// Enable or disable an agent.
    Set {
        /// Agent name.
        name: String,

        /// Whether the agent may be given work.
        #[arg(long)]
        enabled: bool,
    },
}

/// Tools a new agent gets unless told otherwise.
///
/// Read-only. Anything that changes the world is an explicit choice.
const DEFAULT_TOOLS: &[&str] = &["filesystem.read", "filesystem.list"];

/// Dispatch.
pub async fn run(command: AgentCommand, config: &RuntimeConfig) -> Result<()> {
    let runtime = super::open(config).await?;
    let style = Style::detect();

    match command {
        AgentCommand::Create {
            name,
            instructions,
            provider,
            model,
            base_url,
            tools,
        } => {
            let tools = if tools.is_empty() {
                DEFAULT_TOOLS.iter().map(|t| (*t).to_owned()).collect()
            } else {
                tools
            };

            let known = runtime.registry().names();
            for tool in &tools {
                anyhow::ensure!(
                    known.contains(tool),
                    "unknown tool `{tool}`; run `agentos tools` to see what is available"
                );
            }

            let mut model_config = ModelConfig::new(&provider, &model);
            model_config.base_url = base_url;

            let agent = runtime
                .create_agent(&name, &instructions, model_config, tools)
                .await
                .with_context(|| format!("creating agent `{name}`"))?;

            println!(
                "{} agent {}",
                style.green("Created"),
                style.bold(&agent.name)
            );
            println!("  id         {}", agent.id);
            println!(
                "  model      {}/{}",
                agent.model.provider, agent.model.model
            );
            println!("  tools      {}", agent.enabled_tools.join(", "));
            println!(
                "  workspace  {}",
                config.workspace_for(&agent.name).display()
            );
            println!();
            println!(
                "{}",
                style.dim(
                    "Its policy denies everything except reading inside its own workspace.\n\
                     Review it with `agentos policy show` and widen it deliberately."
                )
            );
        }

        AgentCommand::List => {
            let agents = runtime.database().agents().list().await?;
            if agents.is_empty() {
                println!(
                    "{}",
                    style
                        .dim("No agents yet. Create one with `agentos agent create --name sales`.")
                );
                return Ok(());
            }
            println!(
                "{}{}{}{}",
                pad(&style.dim("NAME"), 22),
                pad(&style.dim("STATUS"), 11),
                pad(&style.dim("MODEL"), 30),
                style.dim("TOOLS")
            );
            for agent in agents {
                let status = if agent.is_enabled() {
                    style.green("enabled")
                } else {
                    style.yellow("disabled")
                };
                println!(
                    "{}{}{}{}",
                    pad(&agent.name, 22),
                    pad(&status, 11),
                    pad(
                        &format!("{}/{}", agent.model.provider, agent.model.model),
                        30
                    ),
                    agent.enabled_tools.len()
                );
            }
        }

        AgentCommand::Show { name } => {
            let agent = runtime.agent_by_name(&name).await?;
            println!("{}", style.bold(&agent.name));
            println!("{}", rule());
            println!("  id            {}", agent.id);
            println!("  status        {}", agent.status.as_str());
            println!(
                "  model         {}/{}",
                agent.model.provider, agent.model.model
            );
            if let Some(base_url) = &agent.model.base_url {
                println!("  base url      {base_url}");
            }
            println!("  max steps     {}", agent.max_steps);
            println!("  tools         {}", agent.enabled_tools.join(", "));
            println!(
                "  workspace     {}",
                config.workspace_for(&agent.name).display()
            );
            println!();
            println!("{}", style.dim("Instructions"));
            println!("{}", agent.instructions);

            let tasks = runtime
                .database()
                .tasks()
                .list_for_agent(agent.id, 5)
                .await?;
            if !tasks.is_empty() {
                println!();
                println!("{}", style.dim("Recent tasks"));
                for task in tasks {
                    println!("  {}  {}", task.status.as_str(), task.objective);
                }
            }
        }

        AgentCommand::Set { name, enabled } => {
            let mut agent = runtime.agent_by_name(&name).await?;
            agent.status = if enabled {
                AgentStatus::Enabled
            } else {
                AgentStatus::Disabled
            };
            runtime.database().agents().update(&agent).await?;
            println!(
                "{} {} is now {}",
                style.green("Updated"),
                style.bold(&agent.name),
                agent.status.as_str()
            );
        }
    }

    Ok(())
}
