//! [`DataSource`]: where a piece of untrusted content came from.
//!
//! Provenance is what the taint tracker, the audit log and the approval card
//! all read, so whether a source can carry attacker-written bytes is decided
//! here, once, and not by any tool.

use serde::{Deserialize, Serialize};

/// Where a piece of untrusted content came from.
///
/// Recorded so that the audit log, the approval UI and the taint tracker can all
/// answer "who supplied this text?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataSource {
    /// Text typed by the human operator. Untrusted for injection purposes only
    /// in the sense that it is not a system instruction.
    User,
    /// Produced by the runtime itself (error text, notices).
    Runtime,
    /// The output of a tool that has no more specific source.
    ///
    /// Also the provenance the pipeline records when a tool planned to read the
    /// outside world but labelled what it returned as [`Self::User`] or
    /// [`Self::Runtime`]: the label is the tool's claim, and a reading plan is
    /// enough to disbelieve it.
    Tool {
        /// Fully-qualified tool name, e.g. `filesystem.read`.
        tool: String,
    },
    /// Content fetched from a web origin.
    Web {
        /// The URL the content was read from.
        url: String,
    },
    /// Content read from the local filesystem.
    File {
        /// Absolute path the content was read from.
        path: String,
    },
    /// Output captured from a subprocess.
    Terminal {
        /// The program that produced the output.
        program: String,
    },
    /// A capture of the screen.
    ///
    /// Pixels are content like any other. A hostile page rendered in a window is
    /// as attacker-controlled as the same page read through the DOM, and unlike
    /// text there is no envelope to put it in.
    Screen {
        /// What was captured — an application name, or a display.
        target: String,
    },
    /// A response from a SaaS API, read as an operator-bound account.
    ///
    /// Attacker-controlled in exactly the way a webpage is: an issue body is a
    /// text field a stranger can type into. It is not labelled [`Self::Web`],
    /// because the approval card for the write that follows has to say whose
    /// account read it and from which endpoint. "The agent read something from
    /// GitHub" is not enough to decide on that write.
    ///
    /// `endpoint` is the path only, never the query, which can carry a search
    /// term the operator would not want written into the audit chain. Build it
    /// with [`DataSource::integration`], which drops the query for the caller.
    Integration {
        /// The integration, e.g. `github`.
        integration: String,
        /// The label of the bound account the call acted as.
        account: String,
        /// The API path that was read, e.g. `/repos/acme/widgets/issues/412`.
        endpoint: String,
    },
    /// The report of another agent this run delegated to.
    ///
    /// A model's prose about whatever it read on the way, so it is as
    /// attacker-influenced as the most hostile thing the child touched. Treating
    /// it as clean would launder every taint the child picked up through one
    /// sentence of summary. The child's taint already reaches the parent through
    /// the tracker they share; labelling the report as well is what lets the
    /// approval card say which agent the parent heard it from.
    Agent {
        /// The name of the agent that wrote the report.
        agent: String,
    },
}

impl DataSource {
    /// An [`Self::Integration`] source, with anything after the path dropped.
    ///
    /// A query or fragment in `endpoint` is cut off here, so that a caller
    /// handing over the request target it built does not put a search term
    /// into every record that names this source.
    #[must_use]
    pub fn integration(
        integration: impl Into<String>,
        account: impl Into<String>,
        endpoint: &str,
    ) -> Self {
        let path = endpoint
            .split_once(['?', '#'])
            .map_or(endpoint, |(path, _)| path);
        Self::Integration {
            integration: integration.into(),
            account: account.into(),
            endpoint: path.to_owned(),
        }
    }

    /// Short human- and model-readable description, used in the envelope header.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::User => "user".to_owned(),
            Self::Runtime => "runtime".to_owned(),
            Self::Tool { tool } => format!("tool:{tool}"),
            Self::Web { url } => format!("web:{url}"),
            Self::File { path } => format!("file:{path}"),
            Self::Terminal { program } => format!("terminal:{program}"),
            Self::Screen { target } => format!("screen:{target}"),
            Self::Integration {
                integration,
                account,
                endpoint,
            } => format!("{integration}:{account}@{endpoint}"),
            Self::Agent { agent } => format!("agent:{agent}"),
        }
    }

    /// Whether content from this source could plausibly be attacker-controlled.
    ///
    /// Used by the taint tracker to decide whether a run has ingested content
    /// that warrants raising the approval floor. The tracker consults this and
    /// nothing a tool says about itself, so a new variant belongs in the `true`
    /// arm unless the runtime or the operator wrote every byte it can carry.
    #[must_use]
    pub const fn is_externally_influenced(&self) -> bool {
        match self {
            Self::User | Self::Runtime => false,
            Self::Tool { .. }
            | Self::Web { .. }
            | Self::File { .. }
            | Self::Terminal { .. }
            | Self::Screen { .. }
            | Self::Integration { .. }
            | Self::Agent { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_sources_are_flagged_for_taint() {
        assert!(
            DataSource::Web {
                url: "https://x".into()
            }
            .is_externally_influenced()
        );
        assert!(
            DataSource::File {
                path: "/tmp/x".into()
            }
            .is_externally_influenced()
        );
        // A screen capture is the broadest read in the system: whatever is on
        // the display, including windows the agent was never granted.
        let screen = DataSource::Screen {
            target: "Mail".into(),
        };
        assert!(screen.is_externally_influenced());
        assert_eq!(screen.label(), "screen:Mail");
        // A delegate's report is a model summarising what it read, so it taints
        // exactly as what it read would have.
        let report = DataSource::Agent {
            agent: "researcher".into(),
        };
        assert!(report.is_externally_influenced());
        assert_eq!(report.label(), "agent:researcher");
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({"kind": "agent", "agent": "researcher"})
        );
        assert!(!DataSource::User.is_externally_influenced());
        assert!(!DataSource::Runtime.is_externally_influenced());
    }

    #[test]
    fn an_integration_source_names_the_account_and_the_path_and_taints() {
        let source = DataSource::integration("github", "work", "/repos/acme/widgets/issues/412");
        assert!(source.is_externally_influenced());
        assert_eq!(source.label(), "github:work@/repos/acme/widgets/issues/412");

        // The query is the part an operator would not want in the chain.
        let searched = DataSource::integration(
            "github",
            "work",
            "/repos/acme/widgets/issues?labels=security&q=cve#top",
        );
        assert_eq!(
            searched,
            DataSource::Integration {
                integration: "github".into(),
                account: "work".into(),
                endpoint: "/repos/acme/widgets/issues".into(),
            }
        );
        assert_eq!(
            serde_json::to_value(&searched).unwrap(),
            serde_json::json!({
                "kind": "integration",
                "integration": "github",
                "account": "work",
                "endpoint": "/repos/acme/widgets/issues",
            })
        );
    }
}
