//! `agentos doctor` — check the installation and report what is missing.

use agentos_providers::provider_ids;
use agentos_runtime::RuntimeConfig;
use agentos_secrets::{
    ChainSecretStore, EnvSecretStore, KeychainStatus, KeyringStore, provider_key,
};
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

    // Credential storage.
    //
    // No keychain is a fact about the machine — a headless server, a container,
    // CI — and not a failure. It only becomes a problem if there is also no
    // credential in the environment, and even then the fix is a sentence away.
    let keychain = KeyringStore::status();
    match &keychain {
        KeychainStatus::Available => {
            println!("  {} keychain       available", style.green("ok"));
        }
        KeychainStatus::Unavailable { reason } => {
            println!(
                "  {} keychain       unavailable — {}",
                style.yellow("--"),
                first_line(reason)
            );
        }
    }

    let secrets = ChainSecretStore::standard();
    let mut configured = Vec::new();
    for provider in [provider_ids::ANTHROPIC, provider_ids::OPENAI] {
        if let Some((store, secret)) = secrets.locate(&provider_key(provider)) {
            configured.push(format!("{provider} ({}, via {store})", secret.hint()));
        }
    }

    if configured.is_empty() {
        println!("  {} providers      none configured", style.yellow("--"));
    } else {
        println!(
            "  {} providers      {}",
            style.green("ok"),
            configured.join(", ")
        );
    }

    // Tools. Counted from the registry an agent is actually given, not from a
    // separate list that can drift away from it.
    let registry = agentos_runtime::build_registry(config);
    let browser = agentos_browser::locate(None);
    println!(
        "  {} tools          {} available — run `agentos tools`",
        style.green("ok"),
        registry.len(),
    );
    match &browser {
        Some(path) => println!("  {} browser        {}", style.green("ok"), path.display()),
        None => println!(
            "  {} browser        none found — browser tools will fail until one is installed",
            style.yellow("--")
        ),
    }

    println!();
    if problems == 0 {
        println!("{}", style.green("Everything checks out."));
    } else {
        println!(
            "{}",
            style.red(&format!("{problems} problem(s) need attention."))
        );
    }

    // Whatever is missing, say exactly what to do about it.
    if configured.is_empty() {
        println!();
        if keychain.is_available() {
            println!("{}", style.dim("Next: agentos provider set-key anthropic"));
        } else {
            println!(
                "{}",
                style.dim("This machine has no keychain, so set a credential in the environment:")
            );
            println!(
                "{}",
                style.dim(&format!(
                    "  export {}=…",
                    EnvSecretStore::variables_for(&provider_key(provider_ids::ANTHROPIC))
                        .last()
                        .cloned()
                        .unwrap_or_default()
                ))
            );
            println!(
                "{}",
                style.dim(
                    "Agents cannot read it back: `terminal.exec` gives child processes an \
                     allowlist that excludes it."
                )
            );
        }
        println!(
            "{}",
            style.dim("Or try it with no credential at all: agentos demo --scripted")
        );
    }

    Ok(())
}

/// Keychain errors are often multi-line; the first line is the useful part.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}
