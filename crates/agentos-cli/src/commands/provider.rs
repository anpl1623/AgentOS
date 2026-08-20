//! `agentos provider` — configure model providers.
//!
//! Keys go straight into the OS keychain. They are never written to the
//! database, never printed, and never included in log or error output.

use std::io::IsTerminal;

use agentos_providers::provider_ids;
use agentos_runtime::RuntimeConfig;
use agentos_secrets::{KeyringStore, SecretError, SecretStore, provider_key};
use anyhow::{Context, Result};
use clap::Subcommand;

use crate::render::{Style, pad};

/// Provider subcommands.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Show which providers have a credential configured.
    List,

    /// Store an API key in the operating system keychain.
    SetKey {
        /// Provider identifier.
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(provider_ids::ALL))]
        provider: String,

        /// Read the key from standard input instead of prompting.
        #[arg(long)]
        stdin: bool,
    },

    /// Remove a stored API key.
    RemoveKey {
        /// Provider identifier.
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(provider_ids::ALL))]
        provider: String,
    },
}

/// Dispatch.
pub async fn run(command: ProviderCommand, _config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();
    let keyring = KeyringStore::new();

    match command {
        ProviderCommand::List => {
            println!(
                "{}{}{}",
                pad(&style.dim("PROVIDER"), 14),
                pad(&style.dim("KEY"), 11),
                style.dim("NOTES")
            );
            for provider in provider_ids::ALL {
                let (status, note) = match keyring.get(&provider_key(provider)) {
                    Ok(secret) => (style.green("set"), secret.hint()),
                    Err(SecretError::NotFound { .. }) => (
                        style.dim("not set"),
                        match *provider {
                            provider_ids::OLLAMA => "local; usually needs no key".to_owned(),
                            provider_ids::MOCK => "built in; no key needed".to_owned(),
                            _ => String::new(),
                        },
                    ),
                    Err(error) => (style.red("error"), error.to_string()),
                };
                println!("{}{}{note}", pad(provider, 14), pad(&status, 11));
            }
        }

        ProviderCommand::SetKey { provider, stdin } => {
            let key = if stdin || !std::io::stdin().is_terminal() {
                let mut buffer = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                    .context("reading the key from standard input")?;
                buffer
            } else {
                // Terminal echo is disabled while typing, so the key does not
                // end up in the scrollback or a screen share.
                rpassword::prompt_password(format!("{provider} API key: "))
                    .context("reading the key")?
            };

            let key = key.trim();
            anyhow::ensure!(!key.is_empty(), "no key was provided");

            keyring
                .set(&provider_key(&provider), key)
                .with_context(|| format!("storing the {provider} key in the keychain"))?;

            let stored = keyring.get(&provider_key(&provider))?;
            println!(
                "{} {provider} key in the system keychain ({})",
                style.green("Stored"),
                stored.hint()
            );
        }

        ProviderCommand::RemoveKey { provider } => {
            keyring.delete(&provider_key(&provider))?;
            println!("{} the {provider} key", style.green("Removed"));
        }
    }

    Ok(())
}
