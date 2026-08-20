//! `agentos policy` — inspect and edit permission policies.

use agentos_permissions::PolicyDocument;
use agentos_runtime::RuntimeConfig;
use anyhow::{Context, Result};
use clap::Subcommand;

use crate::render::{Style, rule};

/// Policy subcommands.
#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Print an agent's policy document.
    Show {
        /// Agent name.
        agent: String,
    },

    /// Replace an agent's policy with the contents of a YAML file.
    Set {
        /// Agent name.
        agent: String,

        /// Path to the policy document.
        file: std::path::PathBuf,
    },

    /// Check that a policy document compiles, without installing it.
    Validate {
        /// Path to the policy document.
        file: std::path::PathBuf,
    },
}

/// Dispatch.
pub async fn run(command: PolicyCommand, config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();

    match command {
        PolicyCommand::Show { agent } => {
            let runtime = super::open(config).await?;
            let agent = runtime.agent_by_name(&agent).await?;
            match runtime.database().agents().policy(agent.id).await? {
                None => {
                    println!(
                        "{}",
                        style.yellow("This agent has no policy, so everything is denied.")
                    );
                }
                Some(stored) => {
                    println!("{} v{}", style.dim("policy version"), stored.version);
                    println!("{}", rule());
                    print!("{}", stored.document);
                    println!("{}", rule());
                    summarise(&stored.document, &style)?;
                }
            }
        }

        PolicyCommand::Set { agent, file } => {
            let source = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;

            // Compile before storing. A policy that does not compile would fall
            // back to denying everything, which is safe but confusing; better to
            // refuse the write and say why.
            let policy = PolicyDocument::from_yaml(&source)
                .context("parsing the policy")?
                .compile()
                .context("compiling the policy")?;

            let runtime = super::open(config).await?;
            let agent = runtime.agent_by_name(&agent).await?;
            let version = runtime
                .database()
                .agents()
                .set_policy(agent.id, &source)
                .await?;

            println!(
                "{} policy for {} (version {version}, {} rule(s), default {})",
                style.green("Installed"),
                style.bold(&agent.name),
                policy.rules.len(),
                policy.default_effect
            );
        }

        PolicyCommand::Validate { file } => {
            let source = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            summarise(&source, &style)?;
            println!("{}", style.green("Policy is valid."));
        }
    }

    Ok(())
}

/// Print what a policy actually grants, rather than only that it parsed.
fn summarise(source: &str, style: &Style) -> Result<()> {
    let policy = PolicyDocument::from_yaml(source)
        .context("parsing the policy")?
        .compile()
        .context("compiling the policy")?;

    println!(
        "{} default {}, {} rule(s){}",
        style.dim("Summary:"),
        policy.default_effect,
        policy.rules.len(),
        policy
            .max_risk
            .map(|risk| format!(", max risk {risk}"))
            .unwrap_or_default()
    );
    if policy.taint.enabled {
        println!(
            "{} once this agent reads untrusted data, actions at or above {} need approval",
            style.dim("Taint:  "),
            policy.taint.escalate_at_or_above
        );
    } else {
        println!(
            "{}",
            style.yellow(
                "Taint:   escalation is DISABLED; untrusted input will not raise the approval bar"
            )
        );
    }

    for rule in &policy.rules {
        println!("  {}", rule.describe());
    }
    Ok(())
}
