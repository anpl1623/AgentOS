//! `agentos credential` — secrets bound to one network origin each.
//!
//! A credential is stored under its origin and a name, and a run spends it by
//! naming it in a `network.request` to that origin; asked for anywhere else,
//! it is not there. Spending one is a grant of its own in policy,
//! `network.credential`, separate from reaching the origin.
//!
//! The secret is read from a prompt that does not echo, or from standard
//! input, and never from the command line: an argument is in the shell's
//! history, in `ps` for every user on the machine, and in whatever records the
//! terminal. Nothing here prints it, or a hint of it.

use std::io::IsTerminal;

use agentos_permissions::normalise_origin;
use agentos_runtime::RuntimeConfig;
use agentos_secrets::{KeychainStatus, KeyringStore, MAX_CREDENTIAL_NAME_LEN, is_credential_name};
use anyhow::{Context, Result};
use clap::Subcommand;

use crate::render::{Style, pad};

/// Credential subcommands.
#[derive(Debug, Subcommand)]
pub enum CredentialCommand {
    /// List stored credentials by origin and name. Values are never shown.
    List,

    /// Store a credential for one origin, read from a prompt or standard input.
    ///
    /// A request sends it as `Authorization: Bearer <secret>`, so store the
    /// token alone, without the scheme.
    Set {
        /// The origin it is bound to, e.g. `https://api.example.com`.
        origin: String,

        /// The name a request asks for it by: letters, digits, `_` or `-`.
        name: String,

        /// Read the secret from standard input instead of prompting.
        #[arg(long)]
        stdin: bool,
    },

    /// Remove a stored credential.
    Remove {
        /// The origin it is bound to.
        origin: String,

        /// Its name.
        name: String,
    },
}

/// Dispatch.
///
/// Storing and removing go through the runtime, which records each change —
/// the origin and the name, never the value — in the audit log.
pub async fn run(command: CredentialCommand, config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();

    match command {
        CredentialCommand::List => {
            let runtime = super::open(config).await?;
            let stored = runtime.list_network_credentials().await?;
            // A credential an integration account depends on is named as its
            // token, so that `remove` and `set` on it are not done blind.
            let accounts = runtime.list_integrations().await?;
            if stored.is_empty() {
                println!(
                    "No network credentials stored. Add one with `agentos credential set \
                     <origin> <name>`."
                );
                return Ok(());
            }
            println!(
                "{}{}  {}",
                pad(&style.dim("ORIGIN"), 40),
                pad(&style.dim("NAME"), 20),
                style.dim("TOKEN OF")
            );
            for (origin, name) in stored {
                let account = accounts
                    .iter()
                    .find(|bound| bound.uses_credential(&origin, &name))
                    .map_or_else(|| "-".to_owned(), |bound| bound.describe());
                println!("{}{}  {account}", pad(&origin, 40), pad(&name, 20));
            }
        }

        CredentialCommand::Set {
            origin,
            name,
            stdin,
        } => {
            // Everything that can refuse is checked before a secret is asked
            // for, so nobody types one only to be told it cannot be kept.
            let origin = normalise_origin(origin.trim())
                .with_context(|| format!("`{origin}` is not an origin"))?;
            let name = name.trim();
            anyhow::ensure!(
                is_credential_name(name),
                "a credential name is 1 to {MAX_CREDENTIAL_NAME_LEN} letters, digits, `_` or `-`"
            );
            if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
                anyhow::bail!(
                    "this machine has no usable keychain ({}), so there is nowhere secure to \
                     store the credential. Network credentials are not read from the \
                     environment: a variable name cannot keep two origins apart.",
                    reason.lines().next().unwrap_or(&reason)
                );
            }

            let secret = if stdin || !std::io::stdin().is_terminal() {
                let mut buffer = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                    .context("reading the secret from standard input")?;
                buffer
            } else {
                rpassword::prompt_password(format!("Secret for {name} at {origin}: "))
                    .context("reading the secret")?
            };

            let bound = super::open(config)
                .await?
                .set_network_credential(&origin, name, &secret)
                .await
                .context("storing the credential in the keychain")?;
            println!(
                "{} {name} for {bound} in the system keychain",
                style.green("Stored")
            );
        }

        CredentialCommand::Remove { origin, name } => {
            super::open(config)
                .await?
                .remove_network_credential(&origin, &name)
                .await?;
            // The runtime accepted it, so it normalises.
            let origin = normalise_origin(origin.trim()).unwrap_or(origin);
            println!("{} {} for {origin}", style.green("Removed"), name.trim());
        }
    }

    Ok(())
}
