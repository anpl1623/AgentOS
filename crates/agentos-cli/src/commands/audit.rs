//! `agentos audit` — read and verify the audit log.

use agentos_runtime::RuntimeConfig;
use anyhow::Result;
use clap::Subcommand;

use crate::render::{Style, pad, rule};

/// Audit subcommands.
#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Show the most recent events.
    Tail {
        /// How many events to show.
        #[arg(long, default_value_t = 30)]
        limit: i64,

        /// Only show security-relevant events: denials, escalations, refusals.
        #[arg(long)]
        security: bool,
    },

    /// Verify the hash chain end to end.
    Verify,
}

/// Event kinds that indicate something was refused, escalated or rejected.
const SECURITY_KINDS: &[&str] = &[
    "permission.denied",
    "permission.escalated_by_taint",
    "approval.denied",
    "tool.arguments.rejected",
    "tool.unknown",
    "agent.taint.raised",
];

/// Dispatch.
pub async fn run(command: AuditCommand, config: &RuntimeConfig) -> Result<()> {
    let runtime = super::open(config).await?;
    let style = Style::detect();

    match command {
        AuditCommand::Tail { limit, security } => {
            let mut records = runtime.database().audit_sink().tail(limit).await?;
            if security {
                records.retain(|record| SECURITY_KINDS.contains(&record.kind.as_str()));
            }
            if records.is_empty() {
                println!("{}", style.dim("Nothing recorded yet."));
                return Ok(());
            }

            records.reverse();
            for record in records {
                let kind = if SECURITY_KINDS.contains(&record.kind.as_str()) {
                    style.yellow(&record.kind)
                } else {
                    record.kind.clone()
                };
                println!(
                    "{} {:>6}  {}{}",
                    style.dim(&record.at.format("%Y-%m-%d %H:%M:%S").to_string()),
                    record.sequence,
                    pad(&kind, 34),
                    summarise(&record.payload)
                );
            }
        }

        AuditCommand::Verify => {
            let verification = runtime.verify_audit().await?;
            println!(
                "{} {} record(s)",
                style.dim("Checked"),
                verification.records_checked
            );
            println!("{}", rule());

            if verification.is_intact() {
                println!("{}", style.green("The audit chain is intact."));
            } else {
                for problem in &verification.breaks {
                    println!("  {} {problem}", style.red("!!"));
                }
                println!();
                println!(
                    "{}",
                    style.red(
                        "The audit log has been altered. Treat its contents as unreliable \
                         from the first break onwards."
                    )
                );
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// One-line summary of an event payload, without dumping raw JSON.
fn summarise(payload: &serde_json::Value) -> String {
    let field = |name: &str| {
        payload
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };

    if let (Some(from), Some(to)) = (
        payload.get("from").and_then(serde_json::Value::as_str),
        payload.get("to").and_then(serde_json::Value::as_str),
    ) {
        return format!("{from} → {to}");
    }

    for name in ["tool", "objective", "reason", "error", "summary"] {
        let value = field(name);
        if !value.is_empty() {
            return truncate(&value, 60);
        }
    }

    // Model events carry no single descriptive field, so build one.
    let model = field("model");
    if !model.is_empty() {
        let tokens = payload
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64)
            .map(|count| format!(" ({count} output token(s))"))
            .unwrap_or_default();
        return format!("{}/{model}{tokens}", field("provider"));
    }
    String::new()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    format!("{}…", text.chars().take(max).collect::<String>())
}
