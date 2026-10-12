//! `agentos integration` — accounts the runtime may act as at a service.
//!
//! An account is a label, an API host and a token. The token is a network
//! credential stored for the host's origin under the label, so it is spent,
//! recorded and redacted exactly as `network.request`'s credentials are; the
//! label, the host and whether that host may be on a private network are a row
//! in the database, and the row is what a tool resolves. Nothing a model writes
//! can name a host: it can only name an account the operator bound here.
//!
//! The token is read from a prompt that does not echo, or from standard input,
//! and never from the command line, for the reason `agentos credential` gives.
//! Nothing here prints it, or a hint of it.

use std::io::IsTerminal;

use agentos_runtime::integrations::{BoundAccount, CheckOutcome, INTEGRATION_IDS, binding_target};
use agentos_runtime::{Runtime, RuntimeConfig};
use agentos_secrets::{KeychainStatus, KeyringStore};
use agentos_tools::Secret;
use anyhow::{Context, Result};
use clap::Subcommand;

use crate::render::{Style, pad};

/// The label an account is bound under when none is given.
const DEFAULT_LABEL: &str = "default";

/// Integration subcommands.
#[derive(Debug, Subcommand)]
pub enum IntegrationCommand {
    /// List bound accounts, and whether a token is actually stored behind each.
    List,

    /// Bind an account, reading its token from a prompt or standard input.
    Add {
        /// The integration.
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(INTEGRATION_IDS))]
        integration: String,

        /// The account's label: lower-case letters, digits or `-`.
        #[arg(long, default_value = DEFAULT_LABEL)]
        label: String,

        /// The API base URL, for an Enterprise server, e.g.
        /// `https://ghe.example/api/v3`. Defaults to the service's own.
        #[arg(long)]
        host: Option<String>,

        /// Let this account's requests, and its token, reach a private-network
        /// address (RFC 1918, unique-local or CGNAT), for an Enterprise server
        /// on your own network; refused with the service's own public host.
        /// Loopback, link-local and cloud metadata addresses stay refused.
        #[arg(long)]
        allow_private_network: bool,

        /// A note of what the token can do, such as `repo, read:org`: lower-case
        /// letters, digits, spaces and `:_,./-`. A note only: the policy decides
        /// what an agent may do with it.
        #[arg(long)]
        scopes: Option<String>,

        /// Read the token from standard input instead of prompting.
        #[arg(long)]
        stdin: bool,
    },

    /// Unbind an account and remove its token.
    Remove {
        /// The integration.
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(INTEGRATION_IDS))]
        integration: String,

        /// The account's label; may be left out when only one is bound.
        #[arg(long)]
        label: Option<String>,
    },

    /// Make one authenticated read as an account, and say what came back.
    Test {
        /// The integration.
        #[arg(value_parser = clap::builder::PossibleValuesParser::new(INTEGRATION_IDS))]
        integration: String,

        /// The account's label; may be left out when only one is bound.
        #[arg(long)]
        label: Option<String>,
    },
}

/// Dispatch.
///
/// Binding and unbinding go through the runtime, which records each as an
/// `operator.integration.*` record beside the `operator.credential.*` record
/// for its token: the integration, label, host and address policy, never the
/// token.
pub async fn run(command: IntegrationCommand, config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();

    match command {
        IntegrationCommand::List => {
            let bound = super::open(config).await?.list_integrations().await?;
            if bound.is_empty() {
                println!(
                    "No integration accounts are bound. Bind one with `agentos integration add \
                     github`."
                );
                return Ok(());
            }
            list(&bound, &style);
        }

        IntegrationCommand::Add {
            integration,
            label,
            host,
            allow_private_network,
            scopes,
            stdin,
        } => {
            // Everything that can refuse is checked before a token is asked
            // for, so nobody pastes one only to be told it cannot be kept.
            binding_target(
                &integration,
                &label,
                host.as_deref(),
                allow_private_network,
                scopes.as_deref(),
            )?;
            if let KeychainStatus::Unavailable { reason } = KeyringStore::status() {
                anyhow::bail!(
                    "this machine has no usable keychain ({}), so there is nowhere secure to \
                     store the token. Integration tokens are network credentials, and those \
                     are not read from the environment: a variable name cannot keep two \
                     origins apart.",
                    reason.lines().next().unwrap_or(&reason)
                );
            }
            let runtime = super::open(config).await?;
            // The same check the runtime makes when it binds, made here as
            // well because this one is made before the token is asked for:
            // an account already under the label, or a network credential
            // already stored where the token would go.
            let target = runtime
                .check_binding(
                    &integration,
                    &label,
                    host.as_deref(),
                    allow_private_network,
                    scopes.as_deref(),
                )
                .await?;

            let token = if stdin || !std::io::stdin().is_terminal() {
                let mut buffer = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                    .context("reading the token from standard input")?;
                buffer
            } else {
                rpassword::prompt_password(format!(
                    "Token for {integration} `{label}` at {}: ",
                    target.origin
                ))
                .context("reading the token")?
            };

            let bound = runtime
                .bind_integration(
                    &integration,
                    &label,
                    host.as_deref(),
                    allow_private_network,
                    scopes.as_deref(),
                    Secret::new(token),
                )
                .await
                .context("binding the account")?;
            println!(
                "{} {integration} `{}` at {}{}",
                style.green("Bound"),
                bound.label,
                bound.host,
                if bound.private_network {
                    style.yellow(" (private network allowed)")
                } else {
                    String::new()
                }
            );
            println!(
                "{}",
                style.dim(&format!(
                    "Check it with `agentos integration test {integration} --label {}`.",
                    bound.label
                ))
            );
        }

        IntegrationCommand::Remove { integration, label } => {
            let runtime = super::open(config).await?;
            let account = choose(&runtime, &integration, label.as_deref()).await?;
            let removed = runtime.unbind_integration(account.account.id).await?;
            println!(
                "{} {integration} `{}` and its token",
                style.green("Removed"),
                removed.label
            );
        }

        IntegrationCommand::Test { integration, label } => {
            let runtime = super::open(config).await?;
            let account = choose(&runtime, &integration, label.as_deref()).await?;
            let check = runtime.test_integration(account.account.id).await?;
            let headline = match check.outcome {
                CheckOutcome::Reachable => style.green("reachable"),
                CheckOutcome::Unauthorised => style.red("unauthorised"),
                CheckOutcome::WrongHost => style.red("wrong host"),
                CheckOutcome::Unreachable => style.red("unreachable"),
            };
            println!(
                "{integration} `{}` at {}: {headline}",
                account.account.label, account.account.host
            );
            println!("{}", check.detail);
            // A failed check fails the command, so a script can use it the way
            // it would use `agentos doctor`.
            anyhow::ensure!(
                check.outcome == CheckOutcome::Reachable,
                "the account cannot be used as it stands"
            );
        }
    }

    Ok(())
}

/// Print the bound accounts, and then, loudly, any with no token behind them.
fn list(bound: &[BoundAccount], style: &Style) {
    println!(
        "{}{}{}{}{}",
        pad(&style.dim("INTEGRATION"), 13),
        pad(&style.dim("LABEL"), 18),
        pad(&style.dim("HOST"), 36),
        pad(&style.dim("TOKEN"), 10),
        style.dim("LAST USED")
    );
    for entry in bound {
        let account = &entry.account;
        let token = if entry.credential_present {
            style.green("stored")
        } else {
            style.red("MISSING")
        };
        let host = if account.private_network {
            format!("{} (private)", account.host)
        } else {
            account.host.clone()
        };
        println!(
            "{}{}{}{}{}",
            pad(&account.integration, 13),
            pad(&account.label, 18),
            pad(&host, 36),
            pad(&token, 10),
            account
                .last_used_at
                .map_or_else(|| "never".to_owned(), |when| when.to_rfc3339())
        );
    }

    // The failure mode worth shouting about: the account looks bound, and
    // every call made as it fails. Removing the credential under
    // `agentos credential` is enough to get here.
    let orphaned: Vec<&BoundAccount> = bound
        .iter()
        .filter(|entry| !entry.credential_present)
        .collect();
    if !orphaned.is_empty() {
        println!();
        for entry in orphaned {
            let account = &entry.account;
            println!(
                "{}",
                style.red(&format!(
                    "{} `{}` has no token stored behind it, so every call made as it fails. \
                     Remove it and bind it again: `agentos integration remove {} --label {}`, \
                     then `agentos integration add {} --label {}`.",
                    account.integration,
                    account.label,
                    account.integration,
                    account.label,
                    account.integration,
                    account.label
                ))
            );
        }
    }
}

/// The bound account a command names: by label, or the only one bound.
///
/// The same rule a tool call follows, so an operator who has one account is
/// not made to name it, and one with two is never guessed for.
async fn choose(runtime: &Runtime, integration: &str, label: Option<&str>) -> Result<BoundAccount> {
    let mut bound: Vec<BoundAccount> = runtime
        .list_integrations()
        .await?
        .into_iter()
        .filter(|entry| entry.account.integration == integration)
        .collect();
    if let Some(label) = label {
        return bound
            .into_iter()
            .find(|entry| entry.account.label == label)
            .with_context(|| {
                format!(
                    "no {integration} account is labelled `{label}`; `agentos integration list` \
                     shows the bound ones"
                )
            });
    }
    match bound.len() {
        0 => anyhow::bail!(
            "no {integration} account is bound; bind one with `agentos integration add \
             {integration}`"
        ),
        1 => Ok(bound.remove(0)),
        _ => {
            let labels: Vec<&str> = bound
                .iter()
                .map(|entry| entry.account.label.as_str())
                .collect();
            anyhow::bail!(
                "{} {integration} accounts are bound ({}); name one with `--label`",
                labels.len(),
                labels.join(", ")
            )
        }
    }
}
