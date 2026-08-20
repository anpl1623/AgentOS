//! The AgentOS command-line interface.
//!
//! A client of `agentos-runtime`, on equal footing with the desktop
//! application. Everything here is argument parsing, terminal rendering and
//! calls into the runtime; no agent behaviour lives in this crate, and none
//! should.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "this is a command-line program; printing is its output"
)]
#![allow(
    unreachable_pub,
    reason = "a binary crate has no external surface; `pub` here means crate-internal"
)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod commands;
mod gate;
mod render;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Run business operations with AI agents, on your own computer.
#[derive(Debug, Parser)]
#[command(name = "agentos", version, about, long_about = None)]
struct Cli {
    /// Use a different data directory instead of `~/.agentos`.
    #[arg(long, global = true, env = agentos_runtime::config::HOME_ENV)]
    home: Option<std::path::PathBuf>,

    /// Increase log detail. Repeat for more.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check that the installation is usable and report what is missing.
    Doctor,

    /// Create and inspect agents.
    #[command(subcommand)]
    Agent(commands::agent::AgentCommand),

    /// Inspect and edit permission policies.
    #[command(subcommand)]
    Policy(commands::policy::PolicyCommand),

    /// Run and inspect tasks.
    #[command(subcommand)]
    Task(commands::task::TaskCommand),

    /// Read and verify the audit log.
    #[command(subcommand)]
    Audit(commands::audit::AuditCommand),

    /// Configure model providers.
    #[command(subcommand)]
    Provider(commands::provider::ProviderCommand),

    /// List the tools the runtime can offer an agent.
    Tools,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let config = match &cli.home {
        Some(home) => agentos_runtime::RuntimeConfig::rooted_at(home),
        None => agentos_runtime::RuntimeConfig::discover()?,
    };

    match cli.command {
        Command::Doctor => commands::doctor::run(&config).await,
        Command::Agent(command) => commands::agent::run(command, &config).await,
        Command::Policy(command) => commands::policy::run(command, &config).await,
        Command::Task(command) => commands::task::run(command, &config).await,
        Command::Audit(command) => commands::audit::run(command, &config).await,
        Command::Provider(command) => commands::provider::run(command, &config).await,
        Command::Tools => commands::tools::run(&config).await,
    }
}

/// Logs go to stderr so that piping stdout stays useful.
fn init_tracing(verbose: u8) {
    let filter = match verbose {
        0 => "warn",
        1 => "info,agentos=debug",
        2 => "debug",
        _ => "trace",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_writer(std::io::stderr)
        .try_init();
}
