//! `agentos tools` — list what the runtime can offer an agent.

use agentos_runtime::RuntimeConfig;
use anyhow::Result;

use crate::render::{Style, pad};

/// Print the tool catalogue.
///
/// Reads the same registry the runtime hands to an agent, so a tool that can be
/// granted is a tool that appears here. Listing them separately is how
/// `browser.navigate` ended up usable but undiscoverable.
pub async fn run(config: &RuntimeConfig) -> Result<()> {
    let style = Style::detect();
    let registry = agentos_runtime::build_registry(config);

    println!(
        "{}{}{}{}",
        pad(&style.dim("TOOL"), 24),
        pad(&style.dim("RISK"), 10),
        pad(&style.dim("DATA"), 10),
        style.dim("DESCRIPTION")
    );

    for metadata in registry.all_metadata() {
        // Whether a tool reads the outside world is the single most useful thing
        // to know when choosing what to grant. The column is the tool's own
        // description of itself; the pipeline taints from what a call actually
        // returned, so a wrong entry here misleads the reader and nothing else.
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
            "`external` marks tools that read the outside world, so their output can be \
             attacker-controlled.\nOnce an agent reads such output, later consequential \
             actions require approval."
        )
    );
    Ok(())
}

fn first_sentence(text: &str) -> String {
    text.split_once(". ")
        .map_or_else(|| text.to_owned(), |(first, _)| format!("{first}."))
}
