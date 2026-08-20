//! `agentos tools` — list what the runtime can offer an agent.

use agentos_runtime::RuntimeConfig;
use anyhow::Result;

use crate::render::{Style, pad};

/// Print the tool catalogue.
pub async fn run(_config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();
    let registry = agentos_tools::standard_registry();

    println!(
        "{}{}{}{}",
        pad(&style.dim("TOOL"), 24),
        pad(&style.dim("RISK"), 10),
        pad(&style.dim("DATA"), 10),
        style.dim("DESCRIPTION")
    );

    for metadata in registry.all_metadata() {
        // "untrusted" means results from this tool taint the run, which is the
        // single most useful thing to know when choosing what to grant.
        let data = if metadata.returns_untrusted_data {
            style.yellow("external")
        } else {
            style.dim("none")
        };
        println!(
            "{}{}{}{}",
            pad(&metadata.name, 24),
            pad(&style.risk(metadata.risk), 10),
            pad(&data, 10),
            first_sentence(&metadata.description)
        );
    }

    println!();
    println!(
        "{}",
        style.dim(
            "`external` marks tools whose output can be attacker-controlled. Once an agent \
             uses one,\nlater consequential actions require approval."
        )
    );
    Ok(())
}

fn first_sentence(text: &str) -> String {
    text.split_once(". ")
        .map_or_else(|| text.to_owned(), |(first, _)| format!("{first}."))
}
