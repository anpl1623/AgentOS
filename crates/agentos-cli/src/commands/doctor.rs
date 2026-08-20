//! `agentos doctor` — check the installation and report what is missing.

use agentos_providers::provider_ids;
use agentos_runtime::RuntimeConfig;
use agentos_secrets::{KeyringStore, SecretStore, provider_key};
use anyhow::Result;

use crate::render::{Style, rule};

/// Run the check.
pub async fn run(config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();
    let mut problems = 0usize;

    println!("{}", style.bold("AgentOS"));
    println!("{}", rule());
    println!("  data directory   {}", config.data_dir.display());
    println!("  workspace        {}", config.workspace.display());
    println!("  database         {}", config.database_path.display());
    println!();

    // Storage.
    match super::open(config).await {
        Ok(runtime) => {
            let agents = runtime.database().agents().list().await?;
            let events = runtime.database().audit_sink().count().await?;
            println!(
                "  {} storage        {} agent(s), {events} audit event(s)",
                style.green("ok"),
                agents.len()
            );

            let verification = runtime.verify_audit().await?;
            if verification.is_intact() {
                println!(
                    "  {} audit chain    {} record(s), intact",
                    style.green("ok"),
                    verification.records_checked
                );
            } else {
                problems += 1;
                println!(
                    "  {} audit chain    {} problem(s) found — run `agentos audit verify`",
                    style.red("!!"),
                    verification.breaks.len()
                );
            }

            let reaped = runtime.reap_abandoned_runs().await?;
            if reaped > 0 {
                println!(
                    "  {} runs           marked {reaped} abandoned run(s) as failed",
                    style.yellow("--")
                );
            }
        }
        Err(error) => {
            problems += 1;
            println!("  {} storage        {error}", style.red("!!"));
        }
    }

    // Credentials. The keychain is checked by attempting a read, because a
    // keychain that exists but refuses access is the interesting failure.
    let keyring = KeyringStore::new();
    let mut configured = Vec::new();
    let mut keychain_error = None;
    for provider in [provider_ids::ANTHROPIC, provider_ids::OPENAI] {
        match keyring.get(&provider_key(provider)) {
            Ok(secret) => configured.push(format!("{provider} ({})", secret.hint())),
            Err(agentos_secrets::SecretError::NotFound { .. }) => {}
            Err(error) => keychain_error = Some(error.to_string()),
        }
    }

    if let Some(error) = keychain_error {
        problems += 1;
        println!("  {} keychain       {error}", style.red("!!"));
    } else if configured.is_empty() {
        println!(
            "  {} providers      none configured — run `agentos provider set-key anthropic`",
            style.yellow("--")
        );
    } else {
        println!(
            "  {} providers      {}",
            style.green("ok"),
            configured.join(", ")
        );
    }

    // Tools.
    let registry = agentos_tools::standard_registry();
    println!(
        "  {} tools          {} registered",
        style.green("ok"),
        registry.len()
    );

    println!();
    if problems == 0 {
        println!("{}", style.green("Everything checks out."));
        if configured.is_empty() {
            println!(
                "{}",
                style.dim("Add a provider key, then: agentos agent create --name sales")
            );
        }
    } else {
        println!(
            "{}",
            style.red(&format!("{problems} problem(s) need attention."))
        );
    }

    Ok(())
}
