//! `agentos provider` — configure model providers.
//!
//! Keys go straight into the OS keychain. They are never written to the
//! database, never printed, and never included in log or error output.

use std::io::IsTerminal;

use agentos_providers::provider_ids;
use agentos_runtime::RuntimeConfig;
use agentos_secrets::{
    ChainSecretStore, EnvSecretStore, KeychainStatus, KeyringStore, SecretStore, provider_key,
};
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

    match command {
        ProviderCommand::List => {
            let secrets = ChainSecretStore::standard();
            println!(
                "{}{}{}",
                pad(&style.dim("PROVIDER"), 14),
                pad(&style.dim("KEY"), 11),
                style.dim("NOTES")
            );
            for provider in provider_ids::ALL {
                let (status, note) = match secrets.locate(&provider_key(provider)) {
                    Some((store, secret)) => {
                        (style.green("set"), format!("{} via {store}", secret.hint()))
                    }
                    None => (
                        style.dim("not set"),
                        match *provider {
                            provider_ids::OLLAMA => "local; usually needs no key".to_owned(),
                            provider_ids::MOCK => "built in; no key needed".to_owned(),
                            _ => EnvSecretStore::variables_for(&provider_key(provider))
                                .last()
                                .map(|name| format!("or export {name}"))
                                .unwrap_or_default(),
                        },
                    ),
                };
                println!("{}{}{note}", pad(provider, 14), pad(&status, 11));
            }

            if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
                println!();
                println!(
                    "{}",
                    style.yellow(&format!(
                        "This machine has no usable keychain ({}), so keys must come from the \
                         environment.",
                        reason.lines().next().unwrap_or(&reason)
                    ))
                );
            }
        }

        ProviderCommand::SetKey { provider, stdin } => {
            // Fail before asking for a secret we cannot store.
            if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
                let variable = EnvSecretStore::variables_for(&provider_key(&provider))
                    .last()
                    .cloned()
                    .unwrap_or_default();
                anyhow::bail!(
                    "this machine has no usable keychain ({}), so there is nowhere secure to \
                     store the key.\n\nSet it in the environment instead:\n  export \
                     {variable}=…\n\nAgents cannot read it back: `terminal.exec` passes child \
                     processes an allowlist that excludes it.",
                    reason.lines().next().unwrap_or(&reason)
                );
            }

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

            let keyring = KeyringStore::new();
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
            KeyringStore::new().delete(&provider_key(&provider))?;
            println!("{} the {provider} key", style.green("Removed"));
        }
    }

    Ok(())
}
